//! `ropeway <command>`: Ropeway from a terminal. Talks to the running app
//! (or `ropeway daemon`) over the local channel in `ipc.rs`, starting the app
//! in the background when it is not running.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ipc::{read_endpoint, Endpoint};

/// First words that make the binary a terminal command instead of the app.
const COMMANDS: &[&str] = &[
    "help", "--help", "-h", "status", "peers", "devices", "send", "msg", "chat", "watch", "files", "transfers",
    "accept", "reject", "cancel", "shared", "changes", "approve", "deny", "name", "trust", "untrust", "trusted",
    "folders", "connect", "code", "visible", "daemon", "quit", "version", "--version",
];

pub fn is_cli(args: &[String]) -> bool {
    args.get(1).is_some_and(|a| COMMANDS.contains(&a.as_str()))
}

/// Where the app keeps its data; `ROPEWAY_DATA_DIR` overrides it, as for the app.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ROPEWAY_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("app.ropeway.desktop")
}

struct Ui {
    ru: bool,
    json: bool,
}

impl Ui {
    fn t<'a>(&self, ru: &'a str, en: &'a str) -> &'a str {
        if self.ru {
            ru
        } else {
            en
        }
    }
}

fn language(dir: &Path) -> bool {
    let from_settings = std::fs::read(dir.join("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.get("language").and_then(Value::as_str).map(str::to_owned));
    // The app starts in Russian until the user picks a language.
    from_settings.map_or(true, |l| l == "ru")
}

/// Runs a terminal command; returns the process exit code.
pub fn main(mut args: Vec<String>) -> i32 {
    #[cfg(windows)]
    attach_console();
    args.remove(0);
    let json_out = args.iter().any(|a| a == "--json");
    args.retain(|a| a != "--json");
    let dir = data_dir();
    let ui = Ui { ru: language(&dir), json: json_out };
    let cmd = args.first().cloned().unwrap_or_default();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let result = match cmd.as_str() {
        "help" | "--help" | "-h" => {
            print_help(&ui);
            Ok(())
        }
        "version" | "--version" => {
            println!("Ropeway {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "daemon" => return crate::daemon::run(&dir, ui.ru),
        _ => Client::open(&dir, &ui, cmd != "quit").and_then(|c| c.command(&cmd, &rest)),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}: {e}", ui.t("ошибка", "error"));
            1
        }
    }
}

#[cfg(windows)]
fn attach_console() {
    // The app is a GUI program; borrow the terminal it was started from.
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

type Res<T> = Result<T, String>;

struct Client<'a> {
    endpoint: Endpoint,
    ui: &'a Ui,
}

impl<'a> Client<'a> {
    /// Connects to the running app, starting it in the background if needed.
    fn open(dir: &Path, ui: &'a Ui, start: bool) -> Res<Self> {
        if let Some(endpoint) = read_endpoint(dir).filter(|e| reachable(e)) {
            return Ok(Self { endpoint, ui });
        }
        if !start {
            return Err(ui.t("Ropeway не запущен", "Ropeway is not running").into());
        }
        spawn_app().map_err(|e| format!("{}: {e}", ui.t("не удалось запустить Ropeway", "could not start Ropeway")))?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(endpoint) = read_endpoint(dir).filter(|e| reachable(e)) {
                // Give discovery a moment so devices are known.
                std::thread::sleep(Duration::from_millis(1500));
                return Ok(Self { endpoint, ui });
            }
        }
        Err(ui.t("Ropeway не ответил после запуска", "Ropeway did not answer after starting").into())
    }

    fn connect(&self, cmd: &str, args: Value) -> Res<BufReader<TcpStream>> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.endpoint.port)).map_err(|e| e.to_string())?;
        let request = json!({ "token": self.endpoint.token, "cmd": cmd, "args": args });
        let mut line = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        line.push(b'\n');
        stream.write_all(&line).map_err(|e| e.to_string())?;
        Ok(BufReader::new(stream))
    }

    fn call(&self, cmd: &str, args: Value) -> Res<Value> {
        let mut reader = self.connect(cmd, args)?;
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if reply.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(reply.get("data").cloned().unwrap_or(Value::Null))
        } else {
            Err(reply.get("error").and_then(Value::as_str).unwrap_or("?").to_owned())
        }
    }

    fn out(&self, value: &Value) {
        println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
    }

    fn command(&self, cmd: &str, rest: &[String]) -> Res<()> {
        let t = |ru, en| self.ui.t(ru, en);
        match cmd {
            "status" => {
                let s = self.call("status", Value::Null)?;
                if self.ui.json {
                    return Ok(self.out(&s));
                }
                let me = &s["me"];
                println!("{}: {}", t("Устройство", "Device"), me["name"].as_str().unwrap_or(""));
                println!("{}: {}", t("Код", "Code"), me["code"].as_str().unwrap_or(""));
                let addrs: Vec<&str> = me["addresses"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                println!("{}: {} ({} {})", t("Адреса", "Addresses"), addrs.join(", "), t("порт", "port"), me["port"]);
                println!("{}: {}", t("Полученные файлы", "Received files"), s["folders"]["download_dir"].as_str().unwrap_or(""));
                println!("{}: {}", t("Общая папка", "Shared folder"), s["folders"]["shared_dir"].as_str().unwrap_or(""));
                println!("{}: {}", t("Режим", "Mode"), if self.endpoint.mode == "daemon" { t("служба без окна", "service, no window") } else { t("приложение", "app") });
            }
            "peers" | "devices" => {
                let peers = self.call("peers", Value::Null)?;
                if self.ui.json {
                    return Ok(self.out(&peers));
                }
                let list = peers.as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    println!("{}", t("Устройств пока нет.", "No devices yet."));
                }
                for p in list {
                    let online = p["online"].as_bool() == Some(true);
                    println!(
                        "{} {:<24} {:<20} {}{}",
                        if online { "●" } else { "○" },
                        p["name"].as_str().unwrap_or(""),
                        p["code"].as_str().unwrap_or(""),
                        p["address"].as_str().unwrap_or(""),
                        if p["trusted"].as_bool() == Some(true) { t("  доверенное", "  trusted") } else { "" },
                    );
                }
            }
            "send" | "msg" => {
                let (who, words) = rest.split_first().ok_or_else(|| usage(self.ui, "send <устройство|all> <текст>", "send <device|all> <text>"))?;
                if words.is_empty() {
                    return Err(usage(self.ui, "send <устройство|all> <текст>", "send <device|all> <text>"));
                }
                let conversation = self.conversation(who)?;
                let m = self.call("send_chat", json!({ "conversation": conversation, "text": words.join(" ") }))?;
                if self.ui.json {
                    self.out(&m);
                }
            }
            "chat" => {
                let who = rest.first().ok_or_else(|| usage(self.ui, "chat <устройство|all> [-n 20]", "chat <device|all> [-n 20]"))?;
                let n = option(rest, "-n").and_then(|v| v.parse().ok()).unwrap_or(20usize);
                let conversation = self.conversation(who)?;
                let history = self.call("history", json!({ "conversation": conversation }))?;
                if self.ui.json {
                    return Ok(self.out(&history));
                }
                let list = history.as_array().cloned().unwrap_or_default();
                for m in list.iter().skip(list.len().saturating_sub(n)) {
                    print_message(self.ui, m);
                }
            }
            "watch" => self.watch()?,
            "files" => {
                let (who, paths) = rest.split_first().ok_or_else(|| usage(self.ui, "files <устройство> <файл|папка>…", "files <device> <file|folder>…"))?;
                if paths.is_empty() {
                    return Err(usage(self.ui, "files <устройство> <файл|папка>…", "files <device> <file|folder>…"));
                }
                let peer = self.peer(who)?;
                let cwd = std::env::current_dir().unwrap_or_default();
                let abs: Vec<String> = paths
                    .iter()
                    .map(|p| {
                        let p = Path::new(p);
                        if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) }.to_string_lossy().into_owned()
                    })
                    .collect();
                let tr = self.call("send_paths", json!({ "peer": peer["id"], "paths": abs }))?;
                if self.ui.json {
                    return Ok(self.out(&tr));
                }
                println!(
                    "{} {} ({}). {}",
                    t("Предложено устройству", "Offered to"),
                    peer["name"].as_str().unwrap_or(""),
                    size(tr["total"].as_u64().unwrap_or(0), self.ui.ru),
                    t("Ждём, пока примут.", "Waiting for them to accept.")
                );
            }
            "transfers" => {
                let list = self.call("transfers", Value::Null)?;
                if self.ui.json {
                    return Ok(self.out(&list));
                }
                let list = list.as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    println!("{}", t("Передач нет.", "No transfers."));
                }
                for tr in list {
                    print_transfer(self.ui, &tr);
                }
            }
            "accept" | "reject" | "cancel" => {
                let id = rest.first().ok_or_else(|| usage(self.ui, &format!("{cmd} <id>"), &format!("{cmd} <id>")))?;
                let full = self.transfer_id(id)?;
                self.call(cmd, json!({ "id": full }))?;
                println!("{}", t("Готово.", "Done."));
            }
            "shared" => self.shared(rest)?,
            "changes" => {
                let list = self.call("changes", Value::Null)?;
                if self.ui.json {
                    return Ok(self.out(&list));
                }
                let list = list.as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    println!("{}", t("Запросов на изменение нет.", "No change requests."));
                }
                for c in list {
                    println!(
                        "{}  {:<9} {:<8} {}  {}  [{}]",
                        short(c["id"].as_str().unwrap_or("")),
                        c["direction"].as_str().unwrap_or(""),
                        c["kind"].as_str().unwrap_or(""),
                        c["peer_name"].as_str().unwrap_or(""),
                        c["path"].as_str().unwrap_or(""),
                        c["status"].as_str().unwrap_or(""),
                    );
                }
            }
            "approve" | "deny" => {
                let id = rest.first().ok_or_else(|| usage(self.ui, &format!("{cmd} <id>"), &format!("{cmd} <id>")))?;
                let full = self.change_id(id)?;
                self.call(if cmd == "approve" { "approve_change" } else { "reject_change" }, json!({ "id": full }))?;
                println!("{}", t("Готово.", "Done."));
            }
            "name" => {
                if rest.is_empty() {
                    let s = self.call("status", Value::Null)?;
                    println!("{}", s["me"]["name"].as_str().unwrap_or(""));
                } else {
                    self.call("set_name", json!({ "name": rest.join(" ") }))?;
                    println!("{}", t("Имя изменено.", "Name changed."));
                }
            }
            "trust" | "untrust" => {
                let who = rest.first().ok_or_else(|| usage(self.ui, &format!("{cmd} <устройство>"), &format!("{cmd} <device>")))?;
                let peer = self.peer(who)?;
                if cmd == "trust" && !rest.iter().any(|a| a == "--yes") {
                    let me = self.call("status", Value::Null)?;
                    println!(
                        "{}\n  {}: {}\n  {}: {}\n{}",
                        t("Сверьте коды на обоих устройствах:", "Check the codes on both devices:"),
                        t("это устройство", "this device"),
                        me["me"]["code"].as_str().unwrap_or(""),
                        peer["name"].as_str().unwrap_or(""),
                        peer["code"].as_str().unwrap_or(""),
                        t("Если совпадают, повторите с --yes.", "If they match, run again with --yes."),
                    );
                    return Ok(());
                }
                self.call("set_trusted", json!({ "id": peer["id"], "trusted": cmd == "trust" }))?;
                println!("{}", t("Готово.", "Done."));
            }
            "trusted" => {
                if let Some(v) = option(rest, "--view-screen") {
                    self.call("set_trusted_view", json!({ "enabled": on(&v) }))?;
                }
                let status = self.call("status", Value::Null)?;
                let view = status["trusted_view_without_asking"].as_bool() == Some(true);
                println!(
                    "{}: {}",
                    t("Смотрят экран без вопроса", "Watch the screen without asking"),
                    if view { t("да", "yes") } else { t("нет", "no") }
                );
                let peers = self.call("peers", Value::Null)?;
                for p in peers.as_array().cloned().unwrap_or_default().into_iter().filter(|p| p["trusted"].as_bool() == Some(true)) {
                    println!("{:<24} {}", p["name"].as_str().unwrap_or(""), p["code"].as_str().unwrap_or(""));
                }
            }
            "folders" => self.folders(rest)?,
            "connect" => {
                let address = rest
                    .first()
                    .ok_or_else(|| usage(self.ui, "connect <ip[:порт] | код | ссылка>", "connect <ip[:port] | code | link>"))?;
                let p = self.call("connect", json!({ "address": address }))?;
                println!("{} {} ({})", t("Подключено:", "Connected:"), p["name"].as_str().unwrap_or(""), p["code"].as_str().unwrap_or(""));
            }
            "code" => {
                let c = self.call("pairing_code", Value::Null)?;
                let code = c["code"].as_str().unwrap_or("");
                println!(
                    "{} {} {}\n{}",
                    t("Временный код:", "Temporary code:"),
                    &code[..code.len().min(3)],
                    &code[code.len().min(3)..],
                    t(
                        "Введите его на другом устройстве в этой сети: «Добавить устройство → Ввести код» или ropeway connect <код>. Код работает 10 минут.",
                        "Enter it on another device on this network: \"Add device → Enter code\" or ropeway connect <code>. The code works for 10 minutes."
                    )
                );
            }
            "visible" => {
                let v = rest.first().ok_or_else(|| usage(self.ui, "visible on|off", "visible on|off"))?;
                self.call("set_discoverable", json!({ "enabled": on(v) }))?;
                println!(
                    "{}",
                    if on(v) {
                        t("Устройство снова видно в сети.", "This device is visible on the network again.")
                    } else {
                        t(
                            "Устройство скрыто: подключения только по IP, QR или коду.",
                            "This device is hidden: connections only by IP, QR or code.",
                        )
                    }
                );
            }
            "quit" => {
                self.call("quit", Value::Null)?;
                println!("{}", t("Ropeway остановлен.", "Ropeway stopped."));
            }
            _ => print_help(self.ui),
        }
        Ok(())
    }

    fn peers(&self) -> Res<Vec<Value>> {
        Ok(self.call("peers", Value::Null)?.as_array().cloned().unwrap_or_default())
    }

    /// A device by name, code or id (or a unique start of either).
    fn peer(&self, query: &str) -> Res<Value> {
        let peers = self.peers()?;
        let q = query.to_lowercase();
        let code = |p: &Value| p["code"].as_str().unwrap_or("").replace('-', "").to_lowercase();
        let qcode = q.replace('-', "");
        let exact: Vec<&Value> = peers
            .iter()
            .filter(|p| {
                p["id"].as_str() == Some(query)
                    || p["name"].as_str().map(str::to_lowercase).as_deref() == Some(&q)
                    || (!qcode.is_empty() && code(p) == qcode)
            })
            .collect();
        let found: Vec<&Value> = if exact.is_empty() {
            peers
                .iter()
                .filter(|p| {
                    p["name"].as_str().unwrap_or("").to_lowercase().starts_with(&q)
                        || (q.len() >= 4 && (p["id"].as_str().unwrap_or("").starts_with(&q) || code(p).starts_with(&qcode)))
                })
                .collect()
        } else {
            exact
        };
        match found.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(format!("{} «{query}». {}", self.ui.t("нет устройства", "no device"), self.ui.t("Список: ropeway peers", "List: ropeway peers"))),
            many => Err(format!(
                "{}: {}",
                self.ui.t("подходит несколько устройств, уточните (имя или код)", "several devices match, be more specific (name or code)"),
                many.iter().map(|p| format!("{} ({})", p["name"].as_str().unwrap_or(""), p["code"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// A chat: a device, or the general chat for "all".
    fn conversation(&self, who: &str) -> Res<String> {
        if ["all", "general", "все", "общий"].contains(&who.to_lowercase().as_str()) {
            return Ok(ropeway_core::GENERAL.to_owned());
        }
        Ok(self.peer(who)?["id"].as_str().unwrap_or("").to_owned())
    }

    fn by_prefix(&self, list: Value, id: &str) -> Res<String> {
        let ids: Vec<String> = list
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v["id"].as_str().map(str::to_owned))
            .filter(|full| full.starts_with(id))
            .collect();
        match ids.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(format!("{} {id}", self.ui.t("не найдено:", "not found:"))),
            _ => Err(self.ui.t("неоднозначный id, укажите больше символов", "ambiguous id, give more characters").into()),
        }
    }

    fn transfer_id(&self, id: &str) -> Res<String> {
        self.by_prefix(self.call("transfers", Value::Null)?, id)
    }

    fn change_id(&self, id: &str) -> Res<String> {
        self.by_prefix(self.call("changes", Value::Null)?, id)
    }

    /// `shared` — own listing; `shared <device> [path]`; `shared get <device> <path>`.
    fn shared(&self, rest: &[String]) -> Res<()> {
        if rest.first().map(String::as_str) == Some("get") {
            let (who, path) = match rest {
                [_, who, path, ..] => (who, path),
                _ => return Err(usage(self.ui, "shared get <устройство> <путь>", "shared get <device> <path>")),
            };
            let peer = self.peer(who)?;
            self.call("download_shared", json!({ "peer": peer["id"], "path": path }))?;
            println!("{}", self.ui.t("Скачивание началось: ropeway transfers", "Download started: ropeway transfers"));
            return Ok(());
        }
        let listing = match rest.first() {
            None => self.call("my_shared", Value::Null)?,
            Some(who) => {
                let peer = self.peer(who)?;
                self.call("peer_shared", json!({ "peer": peer["id"] }))?
            }
        };
        if self.ui.json {
            self.out(&listing);
            return Ok(());
        }
        if listing["enabled"].as_bool() == Some(false) {
            println!("{}", self.ui.t("Общая папка выключена.", "The shared folder is off."));
            return Ok(());
        }
        let under = rest.get(1).map(|p| p.trim_matches('/').to_owned()).unwrap_or_default();
        for e in listing["entries"].as_array().cloned().unwrap_or_default() {
            let path = e["path"].as_str().unwrap_or("");
            if !under.is_empty() && !path.starts_with(&format!("{under}/")) {
                continue;
            }
            if e["is_dir"].as_bool() == Some(true) {
                println!("{path}/");
            } else {
                println!("{path}  ({})", size(e["size"].as_u64().unwrap_or(0), self.ui.ru));
            }
        }
        Ok(())
    }

    /// `folders` shows them; options change them.
    fn folders(&self, rest: &[String]) -> Res<()> {
        let mut changed = false;
        if let Some(dir) = option(rest, "--downloads") {
            self.call("set_download_dir", json!({ "path": absolute(&dir) }))?;
            changed = true;
        }
        if let Some(dir) = option(rest, "--shared") {
            self.call("set_shared_dir", json!({ "path": absolute(&dir) }))?;
            changed = true;
        }
        if let Some(v) = option(rest, "--sharing") {
            self.call("set_sharing", json!({ "enabled": on(&v) }))?;
            changed = true;
        }
        if let Some(v) = option(rest, "--trusted-only") {
            self.call("set_trusted_only", json!({ "enabled": on(&v) }))?;
            changed = true;
        }
        let s = self.call("status", Value::Null)?;
        let f = &s["folders"];
        if self.ui.json {
            self.out(f);
            return Ok(());
        }
        let t = |ru, en| self.ui.t(ru, en);
        let yes = |b: &Value| if b.as_bool() == Some(true) { t("да", "yes") } else { t("нет", "no") };
        if changed {
            println!("{}", t("Сохранено.", "Saved."));
        }
        println!("{}: {}", t("Полученные файлы", "Received files"), f["download_dir"].as_str().unwrap_or(""));
        println!("{}: {}", t("Общая папка", "Shared folder"), f["shared_dir"].as_str().unwrap_or(""));
        println!("{}: {}", t("Общий доступ", "Sharing"), yes(&f["sharing_enabled"]));
        println!("{}: {}", t("Только для доверенных", "Trusted devices only"), yes(&f["shared_trusted_only"]));
        Ok(())
    }

    /// Prints messages, incoming files and requests as they happen.
    fn watch(&self) -> Res<()> {
        let mut reader = self.connect("watch", Value::Null)?;
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if !self.ui.json {
            eprintln!("{}", self.ui.t("Слежу за событиями (Ctrl+C — выход)…", "Watching (Ctrl+C to stop)…"));
        }
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err(self.ui.t("Ropeway закрылся", "Ropeway closed").into());
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let event = &v["event"];
            if self.ui.json {
                println!("{}", event);
                continue;
            }
            match event["kind"].as_str() {
                Some("chat") => print_message(self.ui, &event["data"]),
                Some("transfer") => {
                    let tr = &event["data"];
                    let status = tr["status"].as_str().unwrap_or("");
                    if tr["direction"] == "incoming" && status == "pending" && tr["kind"] == "offer" {
                        print_transfer(self.ui, tr);
                        println!("  ropeway accept {}   /   ropeway reject {}", short(tr["id"].as_str().unwrap_or("")), short(tr["id"].as_str().unwrap_or("")));
                    } else if matches!(status, "completed" | "rejected" | "cancelled" | "failed") {
                        print_transfer(self.ui, tr);
                    }
                }
                _ => {}
            }
            let _ = std::io::stdout().flush();
        }
    }
}

fn reachable(e: &Endpoint) -> bool {
    TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], e.port)), Duration::from_millis(500)).is_ok()
}

/// Starts the app minimized to the tray, detached from this terminal.
fn spawn_app() -> std::io::Result<()> {
    // From an AppImage, start the AppImage itself: the binary inside needs
    // the environment its launcher sets up.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => PathBuf::from(appimage),
        None => std::env::current_exe()?,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--minimized")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    cmd.spawn().map(|_| ())
}

fn option(rest: &[String], name: &str) -> Option<String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned()
}

fn absolute(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(p).to_string_lossy().into_owned()
    }
}

fn usage(ui: &Ui, ru: &str, en: &str) -> String {
    format!("{}: ropeway {}", ui.t("использование", "usage"), ui.t(ru, en))
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn size(bytes: u64, ru: bool) -> String {
    crate::files::fmt_size(bytes, !ru)
}

fn print_message(ui: &Ui, m: &Value) {
    let time = m["ts"]
        .as_i64()
        .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms))
        .map(|t| t.with_timezone(&chrono::Local).format("%d.%m %H:%M").to_string())
        .unwrap_or_default();
    let from = if m["outgoing"].as_bool() == Some(true) { ui.t("вы", "you") } else { m["from_name"].as_str().unwrap_or("?") };
    let place = if m["conversation"].as_str() == Some(ropeway_core::GENERAL) { ui.t(" (общий чат)", " (general chat)") } else { "" };
    println!("[{time}] {from}{place}: {}", m["text"].as_str().unwrap_or(""));
}

fn print_transfer(ui: &Ui, tr: &Value) {
    let names: Vec<&str> = tr["names"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let dir = if tr["direction"] == "incoming" { "←" } else { "→" };
    let total = tr["total"].as_u64().unwrap_or(0);
    let done = tr["transferred"].as_u64().unwrap_or(0);
    let status = match tr["status"].as_str().unwrap_or("") {
        "waiting" => ui.t("ждёт ответа", "waiting"),
        "pending" => ui.t("ждёт вашего решения", "needs your answer"),
        "active" => ui.t("идёт", "in progress"),
        "interrupted" => ui.t("прервана", "interrupted"),
        "completed" => ui.t("готово", "done"),
        "rejected" => ui.t("отклонена", "rejected"),
        "cancelled" => ui.t("отменена", "cancelled"),
        _ => ui.t("ошибка", "failed"),
    };
    let progress = if total > 0 && done < total && done > 0 { format!(" {}%", done * 100 / total) } else { String::new() };
    println!(
        "{}  {dir} {:<16} {}  ({})  {status}{progress}",
        short(tr["id"].as_str().unwrap_or("")),
        tr["peer_name"].as_str().unwrap_or(""),
        names.join(", "),
        size(total, ui.ru),
    );
}

/// "on"/"off" style answers on the command line.
fn on(v: &str) -> bool {
    matches!(v, "on" | "yes" | "1" | "true" | "вкл" | "да")
}

fn print_help(ui: &Ui) {
    let help_ru = "\
Ropeway — команды в терминале. Если приложение не запущено, оно запустится в фоне.

  ropeway status                         это устройство: имя, код, адреса, папки
  ropeway peers                          устройства в сети
  ropeway send <устройство|all> <текст>  отправить сообщение (all — общий чат)
  ropeway chat <устройство|all> [-n 20]  последние сообщения
  ropeway watch                          следить за сообщениями и входящими файлами
  ropeway files <устройство> <путь>…     отправить файлы и папки
  ropeway transfers                      передачи файлов
  ropeway accept|reject|cancel <id>      принять, отклонить или отменить передачу
  ropeway shared [<устройство> [папка]]  файлы общей папки (своей или чужой)
  ropeway shared get <устройство> <путь> скачать из чужой общей папки
  ropeway changes                        запросы на изменение общей папки
  ropeway approve|deny <id>              одобрить или отклонить изменение
  ropeway name [<новое имя>]             имя этого устройства
  ropeway trust|untrust <устройство>     отметить устройство доверенным или снять отметку
  ropeway trusted [--view-screen on|off] доверенные устройства; смотрят ли они экран без вопроса
  ropeway folders [--downloads ПАПКА] [--shared ПАПКА] [--sharing on|off] [--trusted-only on|off]
  ropeway connect <ip[:порт] | код>      добавить устройство по IP или временному коду
  ropeway code                           показать временный код для другого устройства
  ropeway visible on|off                 показывать это устройство в сети или скрыть
  ropeway daemon                         работать без окна (для серверов)
  ropeway quit                           остановить Ropeway

Устройство — имя, код или начало id. Добавьте --json для вывода в JSON.";
    let help_en = "\
Ropeway terminal commands. If the app is not running, it starts in the background.

  ropeway status                         this device: name, code, addresses, folders
  ropeway peers                          devices on the network
  ropeway send <device|all> <text>       send a message (all: the general chat)
  ropeway chat <device|all> [-n 20]      recent messages
  ropeway watch                          follow messages and incoming files
  ropeway files <device> <path>…         send files and folders
  ropeway transfers                      file transfers
  ropeway accept|reject|cancel <id>      accept, reject or cancel a transfer
  ropeway shared [<device> [folder]]     files in a shared folder (yours or theirs)
  ropeway shared get <device> <path>     download from their shared folder
  ropeway changes                        change requests for shared folders
  ropeway approve|deny <id>              approve or reject a change
  ropeway name [<new name>]              this device's name
  ropeway trust|untrust <device>         mark a device trusted, or not
  ropeway trusted [--view-screen on|off] trusted devices; whether they watch the screen without asking
  ropeway folders [--downloads DIR] [--shared DIR] [--sharing on|off] [--trusted-only on|off]
  ropeway connect <ip[:port] | code>     add a device by IP or temporary code
  ropeway code                           show a temporary code for another device
  ropeway visible on|off                 show this device on the network, or hide it
  ropeway daemon                         run without a window (servers)
  ropeway quit                           stop Ropeway

A device is a name, a code or the start of an id. Add --json for JSON output.";
    println!("{}", if ui.ru { help_ru } else { help_en });
}
