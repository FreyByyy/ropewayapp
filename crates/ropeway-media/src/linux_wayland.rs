//! Linux on Wayland: the desktop portal's remote desktop session with a
//! screen cast in it. The compositor asks the user which screen to share
//! (once: the choice is remembered with a restore token); frames come over
//! PipeWire, input goes back through the same session.

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use ashpd::desktop::remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::PersistMode;
use pipewire as pw;
use pw::spa;
use ropeway_core::InputEvent;
use tokio::sync::mpsc;

use crate::frames::Slot;
use crate::keymap::{evdev_code, keysym_for_char};

/// How long the user may take to answer the system's sharing dialog.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(180);

/// A running portal session: capture plus a way to send input into it.
pub struct Session {
    input: mpsc::UnboundedSender<InputEvent>,
    thread: Option<JoinHandle<()>>,
    capture: Option<JoinHandle<()>>,
    slot: Arc<Slot>,
}

impl Session {
    pub fn input(&self, event: InputEvent) {
        let _ = self.input.send(event);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.slot.request_stop();
        // Closing the channel ends the portal task, which closes the session.
        let (tx, _) = mpsc::unbounded_channel();
        drop(std::mem::replace(&mut self.input, tx));
        if let Some(t) = self.capture.take() {
            let _ = t.join();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Where the portal's restore token is kept, so the screen is not asked
/// for again next time. Viewing and control are remembered separately: a
/// choice made without remote interaction must not stand in for control.
fn token_path(control: bool) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    let name = if control { "portal-token-control" } else { "portal-token-view" };
    Some(base.join("ropeway").join(name))
}

fn save_token(control: bool, token: Option<&str>) {
    if let (Some(path), Some(token)) = (token_path(control), token) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, token);
    }
}

pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").map(|t| t == "wayland").unwrap_or(false)
}

struct Started {
    node: u32,
    fd: OwnedFd,
    size: (i32, i32),
}

/// Opens the portal session (showing the system dialog if needed) and starts
/// capturing into `slot`.
pub fn start(slot: Arc<Slot>, control: bool) -> Result<Session> {
    let (ready_tx, ready_rx) = std_mpsc::channel::<Result<Started>>();
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<InputEvent>();
    let thread = std::thread::Builder::new().name("ropeway-portal".into()).spawn(move || {
        async_io::block_on(async move {
            if !control {
                // Viewing only: a screen cast session, without the
                // remote interaction choice.
                match cast().await {
                    Ok((session, started)) => {
                        let _ = ready_tx.send(Ok(started));
                        while input_rx.recv().await.is_some() {}
                        let _ = session.close().await;
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
                return;
            }
            let opened = open().await;
            let (proxy, session, stream_node, size) = match opened {
                Ok((proxy, session, started)) => {
                    let node = started.node;
                    let size = started.size;
                    let _ = ready_tx.send(Ok(started));
                    (proxy, session, node, size)
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let mut pointer = (0.0f64, 0.0f64);
            while let Some(event) = input_rx.recv().await {
                if let Err(e) = apply(&proxy, &session, stream_node, size, &mut pointer, &event).await {
                    tracing::debug!("portal input: {e}");
                }
            }
            let _ = session.close().await;
        });
    })?;

    let started = ready_rx
        .recv_timeout(CONSENT_TIMEOUT)
        .map_err(|_| anyhow!("no answer from the screen sharing dialog"))?
        .inspect_err(|e| tracing::warn!("portal: {e:#}"))?;
    let capture_slot = slot.clone();
    let capture = std::thread::Builder::new().name("ropeway-pipewire".into()).spawn(move || {
        if let Err(e) = stream(started.node, started.fd, &capture_slot) {
            tracing::warn!("PipeWire capture: {e}");
        }
        capture_slot.end();
    })?;
    Ok(Session { input: input_tx, thread: Some(thread), capture: Some(capture), slot })
}

async fn open() -> Result<(RemoteDesktop, ashpd::desktop::Session<RemoteDesktop>, Started)> {
    tracing::info!("portal: opening a remote desktop session");
    let proxy = RemoteDesktop::new().await.context("the desktop portal is not available")?;
    let screencast = Screencast::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    tracing::info!("portal: session created");
    let token = token_path(true).and_then(|p| std::fs::read_to_string(p).ok());
    proxy
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(token.as_deref()),
        )
        .await?;
    screencast
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(Some(SourceType::Monitor.into()))
                .set_multiple(false),
        )
        .await?;
    tracing::info!("portal: devices and sources selected, asking the user");
    let request = proxy.start(&session, None, Default::default()).await?;
    tracing::info!("portal: start answered");
    let response = request.response().map_err(|e| anyhow!("screen sharing was not allowed ({e})"))?;
    save_token(true, response.restore_token());
    if !response.devices().contains(DeviceType::Pointer) {
        tracing::warn!("portal: remote interaction was not allowed; the screen is shown without control");
    }
    let stream = response.streams().first().cloned().ok_or_else(|| anyhow!("no screen was chosen"))?;
    tracing::info!("portal: {} stream(s), opening PipeWire", response.streams().len());
    let fd = screencast.open_pipe_wire_remote(&session, Default::default()).await?;
    tracing::info!("portal: PipeWire remote open");
    let started = Started { node: stream.pipe_wire_node_id(), fd, size: stream.size().unwrap_or((1920, 1080)) };
    Ok((proxy, session, started))
}

/// A screen cast session for viewing only.
async fn cast() -> Result<(ashpd::desktop::Session<Screencast>, Started)> {
    tracing::info!("portal: opening a screen cast session");
    let screencast = Screencast::new().await.context("the desktop portal is not available")?;
    let session = screencast.create_session(Default::default()).await?;
    let token = token_path(false).and_then(|p| std::fs::read_to_string(p).ok());
    screencast
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(Some(SourceType::Monitor.into()))
                .set_multiple(false)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(token.as_deref()),
        )
        .await?;
    let response = screencast
        .start(&session, None, Default::default())
        .await?
        .response()
        .map_err(|e| anyhow!("screen sharing was not allowed ({e})"))?;
    save_token(false, response.restore_token());
    let stream = response.streams().first().cloned().ok_or_else(|| anyhow!("no screen was chosen"))?;
    let fd = screencast.open_pipe_wire_remote(&session, Default::default()).await?;
    tracing::info!("portal: screen cast ready");
    Ok((session, Started { node: stream.pipe_wire_node_id(), fd, size: stream.size().unwrap_or((1920, 1080)) }))
}

const BTN_LEFT: i32 = 0x110;
const BTN_RIGHT: i32 = 0x111;
const BTN_MIDDLE: i32 = 0x112;
const BTN_SIDE: i32 = 0x113;
const BTN_EXTRA: i32 = 0x114;

async fn apply(
    proxy: &RemoteDesktop,
    session: &ashpd::desktop::Session<RemoteDesktop>,
    stream: u32,
    size: (i32, i32),
    pointer: &mut (f64, f64),
    event: &InputEvent,
) -> ashpd::Result<()> {
    let key = |down: bool| if down { KeyState::Pressed } else { KeyState::Released };
    match event {
        InputEvent::MouseMove { x, y } => {
            *pointer = (
                x.clamp(0.0, 1.0) as f64 * (size.0.max(1) - 1) as f64,
                y.clamp(0.0, 1.0) as f64 * (size.1.max(1) - 1) as f64,
            );
            proxy.notify_pointer_motion_absolute(session, stream, pointer.0, pointer.1, Default::default()).await
        }
        InputEvent::MouseButton { button, down } => {
            let code = match button {
                0 => BTN_LEFT,
                1 => BTN_MIDDLE,
                2 => BTN_RIGHT,
                3 => BTN_SIDE,
                4 => BTN_EXTRA,
                _ => return Ok(()),
            };
            proxy.notify_pointer_button(session, code, key(*down), Default::default()).await
        }
        InputEvent::Wheel { dx, dy } => {
            let (sx, sy) = (dx.round() as i32, dy.round() as i32);
            if sy != 0 {
                proxy.notify_pointer_axis_discrete(session, Axis::Vertical, sy, Default::default()).await?;
            }
            if sx != 0 {
                proxy.notify_pointer_axis_discrete(session, Axis::Horizontal, sx, Default::default()).await?;
            }
            Ok(())
        }
        InputEvent::Key { code, down } => match evdev_code(code) {
            Some(c) => proxy.notify_keyboard_keycode(session, c as i32, key(*down), Default::default()).await,
            None => Ok(()),
        },
        InputEvent::Text { text } => {
            for c in text.chars().take(256) {
                let sym = keysym_for_char(c) as i32;
                proxy.notify_keyboard_keysym(session, sym, KeyState::Pressed, Default::default()).await?;
                proxy.notify_keyboard_keysym(session, sym, KeyState::Released, Default::default()).await?;
            }
            Ok(())
        }
        // Buttons and keys go up with every event of theirs; nothing to undo.
        InputEvent::ReleaseAll => Ok(()),
        InputEvent::Clipboard { .. } | InputEvent::SwitchMonitor { .. } | InputEvent::SetVideo { .. } => Ok(()),
    }
}

/// Receives frames from the screen cast until the slot is stopped.
fn stream(node: u32, fd: OwnedFd, slot: &Arc<Slot>) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd_rc(fd, None)?;

    struct State {
        format: spa::param::video::VideoInfoRaw,
        slot: Arc<Slot>,
    }

    let stream = pw::stream::StreamRc::new(
        core,
        "ropeway-screen",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(State { format: Default::default(), slot: slot.clone() })
        .param_changed(|_, state, id, param| {
            let Some(param) = param else { return };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            if let Ok((media_type, media_subtype)) = pw::spa::param::format_utils::parse_format(param) {
                if media_type == pw::spa::param::format::MediaType::Video
                    && media_subtype == pw::spa::param::format::MediaSubtype::Raw
                {
                    let _ = state.format.parse(param);
                }
            }
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else { return };
            let size = state.format.size();
            let (w, h) = (size.width, size.height);
            let chunk = data.chunk();
            let stride = if chunk.stride() > 0 { chunk.stride() as u32 } else { w * 4 };
            let offset = chunk.offset() as usize;
            let swap = matches!(
                state.format.format(),
                spa::param::video::VideoFormat::RGBx | spa::param::video::VideoFormat::RGBA
            );
            let Some(bytes) = data.data() else { return };
            let need = stride as usize * h as usize;
            if w == 0 || h == 0 || bytes.len() < offset + need {
                return;
            }
            let pixels = &bytes[offset..offset + need];
            state.slot.put(w, h, stride, |out| {
                out.extend_from_slice(pixels);
                if swap {
                    for px in out.chunks_exact_mut(4) {
                        px.swap(0, 2);
                    }
                }
            });
        })
        .register()?;

    use spa::param::video::VideoFormat;
    let format = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(pw::spa::param::format::FormatProperties::MediaType, Id, pw::spa::param::format::MediaType::Video),
        pw::spa::pod::property!(pw::spa::param::format::FormatProperties::MediaSubtype, Id, pw::spa::param::format::MediaSubtype::Raw),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle { width: 1920, height: 1080 },
            pw::spa::utils::Rectangle { width: 1, height: 1 },
            pw::spa::utils::Rectangle { width: 8192, height: 8192 }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: 30, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );
    let bytes: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(format),
    )
    .map_err(|_| anyhow!("building the stream format"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("stream format"))?];
    stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    // Leave the loop once the share stops.
    let quit = mainloop.clone();
    let watched = slot.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if watched.stopped() {
            quit.quit();
        }
    });
    timer.update_timer(Some(Duration::from_millis(100)), Some(Duration::from_millis(100)));
    mainloop.run();
    Ok(())
}
