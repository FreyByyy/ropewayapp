//! App shell: wires the core node to the web UI and system notifications;
//! on desktop also the tray and autostart, on Android the Kotlin side.

#[cfg(target_os = "android")]
mod android;
mod appearance;
#[cfg(desktop)]
pub mod cli;
#[cfg(desktop)]
mod daemon;
#[cfg(desktop)]
mod ipc;
mod notify;
mod files;
mod screen;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ropeway_core::{ChatMessage, ConversationSummary, Event, MeView, Node, NodeOptions, PeerView, Settings, GENERAL};
#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(desktop)]
use tauri::Wry;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
#[cfg(desktop)]
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::broadcast::error::RecvError;

#[cfg(desktop)]
const MINIMIZED_ARG: &str = "--minimized";

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Default)]
struct UiState {
    /// Conversation open in the window; no notifications for it while focused.
    active_conversation: Mutex<Option<String>>,
}

#[cfg(desktop)]
struct TrayMenu {
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

// ---- commands -----------------------------------------------------------

#[tauri::command]
fn get_me(node: State<Node>) -> MeView {
    node.me()
}

#[tauri::command]
fn get_peers(node: State<Node>) -> Vec<PeerView> {
    node.peers()
}

#[tauri::command]
fn get_settings(node: State<Node>) -> Settings {
    node.settings()
}

#[tauri::command]
fn set_device_name(node: State<Node>, name: String) -> CmdResult<()> {
    node.set_device_name(&name).map_err(err)
}

#[tauri::command]
fn set_language(app: AppHandle, node: State<Node>, language: String) -> CmdResult<()> {
    node.set_language(&language).map_err(err)?;
    #[cfg(desktop)]
    if let Some(tray) = app.try_state::<TrayMenu>() {
        let (open, quit) = tray_labels(&language);
        let _ = tray.open.set_text(open);
        let _ = tray.quit.set_text(quit);
    }
    #[cfg(mobile)]
    let _ = app;
    Ok(())
}

#[tauri::command]
fn set_autostart(app: AppHandle, node: State<Node>, enabled: bool) -> CmdResult<()> {
    #[cfg(desktop)]
    apply_autostart(&app, enabled)?;
    #[cfg(mobile)]
    let _ = app;
    node.set_autostart(enabled).map_err(err)
}

#[tauri::command]
fn set_trusted(node: State<Node>, id: String, trusted: bool) -> CmdResult<()> {
    node.set_trusted(&id, trusted).map_err(err)
}

#[tauri::command]
async fn connect_address(node: State<'_, Node>, address: String) -> CmdResult<PeerView> {
    let node = node.inner().clone();
    node.connect_address(&address).await.map_err(err)
}

#[tauri::command]
fn send_chat(node: State<Node>, conversation: String, text: String) -> CmdResult<ChatMessage> {
    node.send_chat(&conversation, &text).map_err(err)
}

#[tauri::command]
fn retry_chat(node: State<Node>, conversation: String, id: String) -> CmdResult<()> {
    node.retry_chat(&conversation, &id).map_err(err)
}

#[tauri::command]
fn get_history(node: State<Node>, conversation: String) -> Vec<ChatMessage> {
    node.history(&conversation)
}

#[tauri::command]
fn get_conversations(node: State<Node>) -> Vec<ConversationSummary> {
    node.conversations()
}

#[tauri::command]
fn set_active_conversation(ui: State<UiState>, conversation: Option<String>) {
    *ui.active_conversation.lock().unwrap() = conversation;
}

// ---- app ----------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // A custom data dir means a second, independent instance (used to test
    // two devices on one PC), so the single-instance lock is skipped.
    let custom_data_dir = std::env::var_os("ROPEWAY_DATA_DIR").map(PathBuf::from);
    init_logging(custom_data_dir.clone());

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        if custom_data_dir.is_none() {
            builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)));
        }
        builder = builder.plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_ARG])));
    }

    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(UiState::default())
        .manage(screen::Viewers::default())
        .setup(move |app| {
            notify::init(app.handle());
            // Test instances keep removed files in their own data folder
            // instead of the user's Recycle Bin.
            let use_recycle_bin = custom_data_dir.is_none() && cfg!(desktop);
            let data_dir = match custom_data_dir {
                Some(dir) => dir,
                None => app.path().app_data_dir()?,
            };
            // `ropeway daemon` on this data folder gives way to the window.
            #[cfg(desktop)]
            daemon::stop_running_daemon(&data_dir);
            let node = tauri::async_runtime::block_on(Node::start(NodeOptions {
                data_dir,
                port: None,
                // ROPEWAY_NO_DISCOVERY: a test instance that neither announces
                // itself nor finds others; connect it by address instead.
                discovery: std::env::var_os("ROPEWAY_NO_DISCOVERY").is_none(),
                use_recycle_bin,
            }))?;
            let settings = node.settings();
            #[cfg(desktop)]
            node.set_screen_provider(Arc::new(ropeway_media::DesktopScreenProvider::default()));
            #[cfg(target_os = "android")]
            node.set_screen_provider(Arc::new(android::AndroidScreenProvider));

            // Dev builds must not register themselves to run at login.
            #[cfg(desktop)]
            if !cfg!(debug_assertions) {
                let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                if enabled != settings.autostart {
                    let _ = apply_autostart(app.handle(), settings.autostart);
                }
            }

            app.manage(node.clone());
            #[cfg(desktop)]
            build_tray(app, &settings.language)?;
            // Terminal commands talk to this instance.
            #[cfg(desktop)]
            {
                let handle = app.handle().clone();
                let on_quit: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let h = handle.clone();
                    let _ = handle.run_on_main_thread(move || quit(&h));
                });
                if let Err(e) = tauri::async_runtime::block_on(ipc::serve(node.clone(), "app", on_quit)) {
                    tracing::warn!("terminal commands unavailable: {e}");
                }
            }
            #[cfg(mobile)]
            let _ = settings;
            forward_events(app.handle().clone(), node);

            #[cfg(desktop)]
            if !std::env::args().any(|a| a == MINIMIZED_ARG) {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the main window keeps the app running in the tray.
            #[cfg(desktop)]
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Destroyed => screen::on_window_destroyed(window.app_handle(), window.label()),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_me,
            get_peers,
            get_settings,
            set_device_name,
            set_language,
            set_autostart,
            set_trusted,
            connect_address,
            send_chat,
            retry_chat,
            get_history,
            get_conversations,
            set_active_conversation,
            files::get_transfers,
            files::send_paths,
            files::accept_transfer,
            files::reject_transfer,
            files::cancel_transfer,
            files::retry_transfer,
            files::clear_transfers,
            files::get_folders,
            files::set_download_dir,
            files::set_shared_dir,
            files::set_sharing,
            files::set_shared_trusted_only,
            files::get_my_shared,
            files::get_peer_shared,
            files::download_shared,
            files::edit_shared,
            files::get_edits,
            files::discard_edit,
            files::get_changes,
            files::propose_replace,
            files::propose_delete,
            files::approve_change,
            files::reject_change,
            files::open_path,
            files::reveal_path,
            screen::list_screen_sources,
            screen::get_screen,
            screen::start_screen_share,
            screen::stop_screen_share,
            screen::set_screen_video,
            screen::ask_screen,
            screen::answer_screen_ask,
            screen::open_viewer,
            screen::watch_screen,
            screen::stop_watch,
            screen::request_capture,
            screen::control_enabled,
            screen::open_control_settings,
            files::pick_files,
            screen::send_input,
            appearance::list_themes,
            appearance::save_theme,
            appearance::delete_theme,
            appearance::read_theme_file,
            appearance::export_theme,
            appearance::theme_file_for_sending,
            appearance::get_appearance,
            appearance::set_appearance,
            appearance::windows_accent,
            appearance::set_backdrop,
            appearance::set_system_bars,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ropeway");
}

/// Logs to the console and to `ropeway.log` in the data folder (rewritten
/// at every start). `RUST_LOG` overrides the level, e.g. `ropeway_core=debug`.
pub(crate) fn init_logging(custom_data_dir: Option<PathBuf>) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,quinn=warn,quinn_proto=warn".into());
    let dir = custom_data_dir.or_else(|| {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("app.ropeway.desktop"))
    });
    let file = dir.and_then(|d| {
        std::fs::create_dir_all(&d).ok()?;
        std::fs::File::create(d.join("ropeway.log")).ok()
    });
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer());
    match file {
        Some(f) => registry
            .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(Mutex::new(f)))
            .init(),
        None => registry.init(),
    }
}

#[cfg(desktop)]
fn apply_autostart(app: &AppHandle, enabled: bool) -> CmdResult<()> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(err)
    } else {
        launcher.disable().map_err(err)
    }
}

#[cfg(desktop)]
pub(crate) fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // GNOME keeps a minimized window down when asked to raise it from
        // the background; mapping it again brings it up.
        #[cfg(target_os = "linux")]
        if window.is_minimized().unwrap_or(false) {
            let _ = window.hide();
        }
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn quit(app: &AppHandle) {
    let node = app.state::<Node>().inner().clone();
    tauri::async_runtime::block_on(node.shutdown());
    ipc::unpublish(node.data_dir());
    app.exit(0);
}

#[cfg(desktop)]
fn tray_labels(language: &str) -> (&'static str, &'static str) {
    match language {
        "en" => ("Open Ropeway", "Quit"),
        _ => ("Открыть Ropeway", "Выход"),
    }
}

#[cfg(desktop)]
fn build_tray(app: &tauri::App, language: &str) -> tauri::Result<()> {
    let (open_label, quit_label) = tray_labels(language);
    let open = MenuItem::with_id(app, "open", open_label, true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit_item])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Ropeway")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    app.manage(TrayMenu { open, quit: quit_item });
    Ok(())
}

/// Relays core events to the UI and raises notifications for new messages.
fn forward_events(app: AppHandle, node: Node) {
    let mut rx = node.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    match &event {
                        Event::Chat(msg) => notify_message(&app, &node, msg),
                        Event::Screen(view) => screen::update_indicator(&app, view),
                        Event::Notice(notice) => {
                            if !screen::handle_notice(&app, &node, notice) {
                                files::handle_notice(&app, &node, notice);
                            }
                        }
                        _ => {}
                    }
                    let _ = app.emit("ropeway", &event);
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn notify_message(app: &AppHandle, node: &Node, msg: &ChatMessage) {
    if msg.outgoing {
        return;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let active = app.state::<UiState>().active_conversation.lock().unwrap().clone();
    if focused && active.as_deref() == Some(msg.conversation.as_str()) {
        return;
    }
    let title = if msg.conversation == GENERAL {
        let general = if node.settings().language == "en" { "General chat" } else { "Общий чат" };
        format!("{} · {general}", msg.from_name)
    } else {
        msg.from_name.clone()
    };
    let body: String = msg.text.chars().take(240).collect();
    notify::show(app, title, body, notify::Target::Chat(&msg.conversation));
}
