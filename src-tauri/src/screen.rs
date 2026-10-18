//! Screen sharing and remote control: commands, viewer windows, the
//! "someone controls this computer" indicator, and notifications.

use std::collections::HashMap;
use std::sync::Mutex;

use ropeway_core::screen::{VideoSettings, HEADER_LEN, PKT_CLIPBOARD};
use ropeway_core::{CaptureOptions, InputEvent, Node, Notice, ScreenView};
#[cfg(target_os = "android")]
use crate::android::SourceInfo;
#[cfg(desktop)]
use ropeway_media::clipboard::ClipboardSync;
#[cfg(desktop)]
use ropeway_media::SourceInfo;
use tauri::ipc::{Channel, InvokeResponseBody};
#[cfg(mobile)]
use tauri::Emitter;
#[cfg(desktop)]
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use crate::notify::{self, Target};
use tokio::sync::{mpsc, oneshot};

use crate::{err, CmdResult};

#[cfg(desktop)]
const INDICATOR: &str = "control-indicator";
/// Mobile: tells the main window to show a session (`{peer, session}`).
#[cfg(mobile)]
const VIEWER_EVENT: &str = "ropeway-viewer";

struct OpenViewer {
    cancel: oneshot::Sender<()>,
    input: mpsc::Sender<InputEvent>,
}

/// Open viewer windows by label: closing one stops its stream.
#[derive(Default)]
pub struct Viewers {
    open: Mutex<HashMap<String, OpenViewer>>,
}

#[tauri::command]
pub async fn list_screen_sources() -> Vec<SourceInfo> {
    #[cfg(desktop)]
    let list = ropeway_media::list_sources;
    #[cfg(target_os = "android")]
    let list = crate::android::list_sources;
    tauri::async_runtime::spawn_blocking(list).await.unwrap_or_default()
}

/// Asks for permission to capture the screen, where the system requires
/// one before every share (Android). Elsewhere always granted.
#[tauri::command]
pub async fn request_capture() -> CmdResult<bool> {
    #[cfg(target_os = "android")]
    return crate::android::request_capture().await.map_err(err);
    #[cfg(not(target_os = "android"))]
    Ok(true)
}

#[tauri::command]
pub fn get_screen(node: State<Node>) -> ScreenView {
    node.screen()
}

#[tauri::command]
/// Async and off the main thread: starting capture may wait for the system's
/// sharing dialog (Wayland), and the window must stay responsive meanwhile.
pub async fn start_screen_share(node: State<'_, Node>, options: CaptureOptions, invite: Vec<String>, public: bool) -> CmdResult<String> {
    let node = node.inner().clone();
    tauri::async_runtime::spawn_blocking(move || node.start_screen_share(options, invite, public))
        .await
        .map_err(err)?
        .map_err(err)
}

/// Remembers video settings for the next share (and for asks from others).
#[tauri::command]
pub fn set_screen_video(node: State<Node>, video: VideoSettings) -> CmdResult<()> {
    node.set_screen_video(video).map_err(err)
}

#[tauri::command]
pub fn set_trusted_view_without_asking(node: State<Node>, enabled: bool) -> CmdResult<()> {
    node.set_trusted_view_without_asking(enabled).map_err(err)
}

#[tauri::command]
pub fn stop_screen_share(node: State<Node>, id: String) {
    node.stop_screen_share(&id)
}

#[tauri::command]
pub fn ask_screen(node: State<Node>, peer: String, control: bool) -> CmdResult<()> {
    node.ask_screen(&peer, control).map_err(err)
}

#[tauri::command]
/// Accepting starts capture; see `start_screen_share`.
pub async fn answer_screen_ask(node: State<'_, Node>, id: String, accept: bool) -> CmdResult<()> {
    let node = node.inner().clone();
    tauri::async_runtime::spawn_blocking(move || node.answer_screen_ask(&id, accept))
        .await
        .map_err(err)?
        .map_err(err)
}

/// Opens (or focuses) the window that shows a session. Async: creating a
/// window from a synchronous command can deadlock on Windows.
#[tauri::command]
pub async fn open_viewer(app: AppHandle, peer: String, session: String) -> CmdResult<()> {
    open_viewer_window(&app, &peer, &session)
}

pub fn open_viewer_window(app: &AppHandle, peer: &str, session: &str) -> CmdResult<()> {
    if !is_plain_id(peer) || !is_plain_id(session) {
        return Err("invalid session".into());
    }
    show_viewer(app, peer, session)
}

#[cfg(desktop)]
fn show_viewer(app: &AppHandle, peer: &str, session: &str) -> CmdResult<()> {
    let label = format!("viewer-{session}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let url = format!("index.html#/viewer?peer={peer}&session={session}");
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Ropeway")
        .inner_size(1280.0, 760.0)
        .min_inner_size(480.0, 320.0)
        .build()
        .map_err(err)?;
    Ok(())
}

/// Phones have one window: the page shows the session over the app.
#[cfg(mobile)]
fn show_viewer(app: &AppHandle, peer: &str, session: &str) -> CmdResult<()> {
    #[derive(serde::Serialize, Clone)]
    struct Open<'a> {
        peer: &'a str,
        session: &'a str,
    }
    app.emit_to("main", VIEWER_EVENT, Open { peer, session }).map_err(err)
}

/// Streams a session's packets to the calling viewer window until the
/// session ends or the window closes. In a control session the clipboard is
/// kept in sync both ways.
#[tauri::command]
pub async fn watch_screen(
    window: WebviewWindow,
    node: State<'_, Node>,
    viewers: State<'_, Viewers>,
    peer: String,
    session: String,
    control: bool,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<()> {
    let node = node.inner().clone();
    let watch = node.watch_screen(&peer, &session).await.map_err(err)?;
    let mut packets = watch.packets;
    let input = watch.input;
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    viewers
        .open
        .lock()
        .unwrap()
        .insert(window.label().to_owned(), OpenViewer { cancel: cancel_tx, input: input.clone() });

    #[cfg(desktop)]
    let clipboard = control.then(|| {
        let input = input.clone();
        ClipboardSync::start(move |text| {
            let _ = input.try_send(InputEvent::Clipboard { text });
        })
    });
    // No clipboard sync from a phone yet.
    #[cfg(mobile)]
    let clipboard: Option<NoClipboard> = {
        let _ = control;
        None
    };

    loop {
        tokio::select! {
            packet = packets.recv() => match packet {
                Some(p) if p.first() == Some(&PKT_CLIPBOARD) => {
                    if let (Some(sync), Ok(text)) = (&clipboard, std::str::from_utf8(&p[HEADER_LEN..])) {
                        sync.set(text);
                    }
                }
                Some(p) => {
                    if channel.send(InvokeResponseBody::Raw(p)).is_err() {
                        break;
                    }
                }
                None => break,
            },
            _ = &mut cancel_rx => break,
        }
    }
    viewers.open.lock().unwrap().remove(window.label());
    drop(clipboard);
    Ok(())
}

#[cfg(mobile)]
struct NoClipboard;

#[cfg(mobile)]
impl NoClipboard {
    fn set(&self, _: &str) {}
}

/// Phone: whether remote control can work (the accessibility service is on).
/// Computers can always be controlled.
#[tauri::command]
pub fn control_enabled() -> bool {
    #[cfg(target_os = "android")]
    return crate::android::control_enabled();
    #[cfg(not(target_os = "android"))]
    true
}

/// Phone: opens the settings where remote control is turned on.
#[tauri::command]
pub fn open_control_settings() {
    #[cfg(target_os = "android")]
    crate::android::open_control_settings();
}

/// Stops the stream of the calling window. Desktop viewer windows stop by
/// closing; on a phone the viewer is part of the main window.
#[tauri::command]
pub fn stop_watch(window: WebviewWindow) {
    on_window_destroyed(window.app_handle(), window.label());
}

/// Input from a viewer window in control mode.
#[tauri::command]
pub fn send_input(window: WebviewWindow, viewers: State<Viewers>, event: InputEvent) {
    if let InputEvent::Clipboard { .. } = event {
        // Only the clipboard watcher sends clipboard text.
        return;
    }
    match viewers.open.lock().unwrap().get(window.label()) {
        Some(v) => {
            tracing::debug!("input from {}: {event:?}", window.label());
            if v.input.try_send(event).is_err() {
                tracing::debug!("input queue full, event dropped");
            }
        }
        None => tracing::debug!("input from {} without an open stream", window.label()),
    }
}

/// Called when any window is destroyed.
pub fn on_window_destroyed(app: &AppHandle, label: &str) {
    let removed = app.state::<Viewers>().open.lock().unwrap().remove(label);
    if let Some(v) = removed {
        // Let go of anything held before the stream closes.
        let _ = v.input.try_send(InputEvent::ReleaseAll);
        let _ = v.cancel.send(());
    }
}

fn is_plain_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Shows the always-on-top indicator while someone controls this computer,
/// and closes it when nobody does.
#[cfg(mobile)]
pub fn update_indicator(_app: &AppHandle, _view: &ScreenView) {}

#[cfg(desktop)]
pub fn update_indicator(app: &AppHandle, view: &ScreenView) {
    let controlled = view.sharing.iter().any(|s| !s.controllers.is_empty());
    let existing = app.get_webview_window(INDICATOR);
    match (controlled, existing) {
        (true, None) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = open_indicator(&app) {
                    tracing::warn!("control indicator: {e}");
                }
            });
        }
        (false, Some(window)) => {
            let _ = window.close();
        }
        _ => {}
    }
}

#[cfg(desktop)]
fn open_indicator(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(INDICATOR).is_some() {
        return Ok(());
    }
    let (width, height) = (460.0, 48.0);
    let x = app
        .primary_monitor()?
        .map(|m| {
            let scale = m.scale_factor();
            m.position().x as f64 / scale + (m.size().width as f64 / scale - width) / 2.0
        })
        .unwrap_or(400.0);
    WebviewWindowBuilder::new(app, INDICATOR, WebviewUrl::App("index.html#/indicator".into()))
        .title("Ropeway")
        .inner_size(width, height)
        // GTK would otherwise grow the window to its own minimum size.
        .min_inner_size(width, height)
        .max_inner_size(width, height)
        .position(x, 10.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()?;
    Ok(())
}

/// Screen-related notices. Returns `true` if handled.
pub fn handle_notice(app: &AppHandle, node: &Node, notice: &Notice) -> bool {
    let en = node.settings().language == "en";
    let (title, body, target) = match notice {
        Notice::ScreenInvite { share } => {
            if share.requested {
                // The user asked for it: show the screen right away.
                let (app, peer, session) = (app.clone(), share.peer.clone(), share.id.clone());
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = open_viewer_window(&app, &peer, &session) {
                        tracing::warn!("opening viewer: {e}");
                    }
                });
                return true;
            }
            (
                share.peer_name.clone(),
                if en { "is sharing their screen with you".to_string() } else { "показывает вам свой экран".to_string() },
                Target::Chat(&share.peer),
            )
        }
        Notice::ScreenAsked { ask } => (
            ask.peer_name.clone(),
            match (ask.control, en) {
                (true, false) if cfg!(mobile) => "хочет управлять вашим телефоном".to_string(),
                (true, true) if cfg!(mobile) => "wants to control your phone".to_string(),
                (true, false) => "хочет управлять вашим компьютером".to_string(),
                (true, true) => "wants to control your computer".to_string(),
                (false, false) => "хочет посмотреть ваш экран".to_string(),
                (false, true) => "wants to see your screen".to_string(),
            },
            Target::Chat(&ask.peer),
        ),
        Notice::ScreenAskAnswered { peer_name, accepted: false } => (
            peer_name.clone(),
            if en { "declined your request".to_string() } else { "отклонил ваш запрос".to_string() },
            Target::App,
        ),
        Notice::ScreenAskAnswered { .. } => return true,
        _ => return false,
    };
    notify::show(app, title, body, target);
    true
}
