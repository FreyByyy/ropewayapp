//! Local control channel for the `ropeway` terminal commands.
//!
//! The running app (window or `ropeway daemon`) listens on 127.0.0.1 and
//! writes the port and a random token to `cli.json` in its data folder,
//! readable only by the user. Each connection sends one JSON request line and
//! gets JSON lines back: one answer, or a stream of events for `watch`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ropeway_core::Node;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;

pub const FILE: &str = "cli.json";

/// What `cli.json` holds.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    /// "app" (the window) or "daemon" (no window).
    pub mode: String,
}

#[derive(Deserialize)]
pub struct Request {
    pub token: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
}

pub fn endpoint_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// Starts listening and publishes the endpoint. `on_quit` runs for `quit`.
pub async fn serve(node: Node, mode: &str, on_quit: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let endpoint = Endpoint {
        port: listener.local_addr()?.port(),
        token: format!("{}{}", random_hex(), random_hex()),
        pid: std::process::id(),
        mode: mode.to_owned(),
    };
    write_private(&endpoint_path(node.data_dir()), &serde_json::to_vec_pretty(&endpoint)?)?;
    let token = Arc::new(endpoint.token);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let (node, token, on_quit) = (node.clone(), token.clone(), on_quit.clone());
            tokio::spawn(async move {
                if let Err(e) = handle(stream, node, &token, on_quit).await {
                    tracing::debug!("terminal command: {e}");
                }
            });
        }
    });
    Ok(())
}

/// Removes `cli.json` if it is ours.
pub fn unpublish(data_dir: &Path) {
    let path = endpoint_path(data_dir);
    if let Some(e) = read_endpoint(data_dir) {
        if e.pid == std::process::id() {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn read_endpoint(data_dir: &Path) -> Option<Endpoint> {
    serde_json::from_slice(&std::fs::read(endpoint_path(data_dir)).ok()?).ok()
}

/// 128 random bits from the operating system, as hex.
fn random_hex() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut bytes).expect("system random generator");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes a file only the current user can read.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        // The data folder is in the user's profile, private by default.
        std::fs::write(path, bytes)
    }
}

async fn handle(stream: TcpStream, node: Node, token: &str, on_quit: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let Some(line) = lines.next_line().await? else { return Ok(()) };
    let request: Request = serde_json::from_str(&line)?;
    if request.token != token {
        write.write_all(b"{\"ok\":false,\"error\":\"access denied\"}\n").await?;
        return Ok(());
    }
    if request.cmd == "watch" {
        let mut rx = node.subscribe();
        write.write_all(b"{\"ok\":true,\"data\":null}\n").await?;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let mut line = serde_json::to_vec(&json!({ "event": event }))?;
                    line.push(b'\n');
                    if write.write_all(&line).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
        return Ok(());
    }
    let reply = match run(&node, &request.cmd, &request.args).await {
        Ok(data) => json!({ "ok": true, "data": data }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };
    let mut out = serde_json::to_vec(&reply)?;
    out.push(b'\n');
    write.write_all(&out).await?;
    write.flush().await?;
    if request.cmd == "quit" {
        on_quit();
    }
    Ok(())
}

fn arg<'a>(args: &'a Value, name: &str) -> anyhow::Result<&'a str> {
    args.get(name).and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing {name}"))
}

fn flag(args: &Value, name: &str) -> anyhow::Result<bool> {
    args.get(name).and_then(Value::as_bool).ok_or_else(|| anyhow::anyhow!("missing {name}"))
}

fn opt_path(args: &Value, name: &str) -> Option<PathBuf> {
    args.get(name).and_then(Value::as_str).filter(|s| !s.is_empty()).map(PathBuf::from)
}

async fn run(node: &Node, cmd: &str, a: &Value) -> anyhow::Result<Value> {
    Ok(match cmd {
        "status" => json!({
            "me": node.me(),
            "language": node.settings().language,
            "folders": node.folders(),
        }),
        "peers" => json!(node.peers()),
        "history" => json!(node.history(arg(a, "conversation")?)),
        "send_chat" => json!(node.send_chat(arg(a, "conversation")?, arg(a, "text")?)?),
        "transfers" => json!(node.transfers()),
        "send_paths" => {
            let paths: Vec<PathBuf> = a
                .get("paths")
                .and_then(Value::as_array)
                .map(|v| v.iter().filter_map(Value::as_str).map(PathBuf::from).collect())
                .unwrap_or_default();
            json!(node.send_paths(arg(a, "peer")?, paths).await?)
        }
        "accept" => json!(node.accept_transfer(arg(a, "id")?)?),
        "reject" => json!(node.reject_transfer(arg(a, "id")?)?),
        "cancel" => json!(node.cancel_transfer(arg(a, "id")?)?),
        "my_shared" => json!(node.my_shared().await),
        "peer_shared" => json!(node.peer_shared(arg(a, "peer")?).await?),
        "download_shared" => json!(node.download_shared(arg(a, "peer")?, arg(a, "path")?).await?),
        "changes" => json!(node.changes()),
        "approve_change" => json!(node.approve_change(arg(a, "id")?)?),
        "reject_change" => json!(node.reject_change(arg(a, "id")?)?),
        "set_name" => json!(node.set_device_name(arg(a, "name")?)?),
        "set_trusted" => json!(node.set_trusted(arg(a, "id")?, flag(a, "trusted")?)?),
        "set_download_dir" => json!(node.set_download_dir(opt_path(a, "path"))?),
        "set_shared_dir" => json!(node.set_shared_dir(opt_path(a, "path"))?),
        "set_sharing" => json!(node.set_sharing(flag(a, "enabled")?)?),
        "set_trusted_only" => json!(node.set_shared_trusted_only(flag(a, "enabled")?)?),
        "connect" => json!(node.connect_address(arg(a, "address")?).await?),
        "quit" => Value::Null,
        other => anyhow::bail!("unknown command: {other}"),
    })
}
