//! `ropeway daemon`: Ropeway without a window, for servers and terminals.
//! Chat, files and the shared folder work as usual and are driven with the
//! other `ropeway` commands. Opening the app's window later takes over.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ropeway_core::{Node, NodeOptions};

use crate::ipc;

pub fn run(dir: &Path, ru: bool) -> i32 {
    let t = |a: &'static str, b: &'static str| if ru { a } else { b };
    if let Some(e) = ipc::read_endpoint(dir).filter(running) {
        println!(
            "{}",
            if e.mode == "daemon" {
                t("Ropeway уже работает в фоне.", "Ropeway is already running in the background.")
            } else {
                t("Ropeway уже открыт; команды работают и так.", "Ropeway is already open; commands work with it.")
            }
        );
        return 0;
    }
    crate::init_logging(Some(dir.to_path_buf()));
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    runtime.block_on(async {
        let node = match Node::start(NodeOptions { data_dir: dir.to_path_buf(), port: None, discovery: true, use_recycle_bin: true }).await {
            Ok(n) => n,
            Err(e) => {
                eprintln!("{}: {e}", t("не удалось запустить", "could not start"));
                return 1;
            }
        };
        node.set_screen_provider(Arc::new(ropeway_media::DesktopScreenProvider::default()));
        let (quit_tx, mut quit_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let on_quit = Arc::new(move || {
            let _ = quit_tx.send(());
        });
        if let Err(e) = ipc::serve(node.clone(), "daemon", on_quit).await {
            eprintln!("{e}");
            return 1;
        }
        let me = node.me();
        println!(
            "{} {} ({} {}). {}",
            t("Ropeway работает без окна как", "Ropeway is running without a window as"),
            me.name,
            t("код", "code"),
            me.code,
            t("Остановить: Ctrl+C или ropeway quit", "Stop: Ctrl+C or ropeway quit")
        );
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = quit_rx.recv() => {}
        }
        node.shutdown().await;
        ipc::unpublish(node.data_dir());
        0
    })
}

fn running(e: &ipc::Endpoint) -> bool {
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], e.port)), Duration::from_millis(500)).is_ok()
}

/// Asks a running daemon to stop and waits for it, so the app can start on
/// the same data folder.
pub fn stop_running_daemon(dir: &Path) {
    use std::io::{BufRead, BufReader, Write};
    let Some(e) = ipc::read_endpoint(dir).filter(|e| e.mode == "daemon" && running(e)) else { return };
    tracing::info!("stopping the background Ropeway to open the window");
    if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", e.port)) {
        let line = serde_json::json!({ "token": e.token, "cmd": "quit" }).to_string() + "\n";
        let _ = s.write_all(line.as_bytes());
        let mut reply = String::new();
        let _ = BufReader::new(s).read_line(&mut reply);
    }
    for _ in 0..50 {
        if !running(&e) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Let the daemon release its port and files.
    std::thread::sleep(Duration::from_millis(300));
}
