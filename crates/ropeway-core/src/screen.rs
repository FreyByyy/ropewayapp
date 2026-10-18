//! Screen sharing sessions.
//!
//! The core does not capture anything itself: a platform [`ScreenProvider`]
//! (registered by the app) captures and encodes, and pushes packets into a
//! [`BroadcastSink`]. The core fans each packet out to every viewer of the
//! session. A viewer that falls behind skips frames until the next keyframe
//! instead of slowing the others down.
//!
//! Viewers pull: `ScreenWatch` opens a stream on which the sharer writes
//! media packets until the session ends. For remote control, the viewer
//! writes [`InputEvent`] frames on the same stream; the sharer applies them
//! only for devices it allowed to control.
//!
//! Packet = kind u8 | flags u8 | timestamp µs u64 | length u32 | payload

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

use crate::node::{now_ms, Event, Node, Notice};
use crate::protocol::{read_frame, write_frame, Request, Response};
use crate::transfers::valid_wire_id;

pub const PKT_INFO: u8 = 0;
pub const PKT_VIDEO: u8 = 1;
pub const PKT_AUDIO: u8 = 2;
/// Clipboard text of the controlled device, sent to controllers only.
pub const PKT_CLIPBOARD: u8 = 3;
pub const FLAG_KEYFRAME: u8 = 1;
pub const HEADER_LEN: usize = 14;
const MAX_PACKET: usize = 16 * 1024 * 1024;
/// Packets queued per viewer before it is considered too slow.
const VIEWER_QUEUE: usize = 90;
/// Screens one device may share with this one at a time.
const MAX_INVITES_PER_PEER: usize = 8;
/// Undecided "show me your screen" asks kept at once.
const MAX_ASKS: usize = 20;
/// How long an accepted ask lets the invite that follows open the viewer.
const ACCEPTED_ASK_TTL: std::time::Duration = std::time::Duration::from_secs(30);

pub fn encode_packet(kind: u8, flags: u8, ts_us: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&ts_us.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// What to capture. `id` is meaningful only to the provider that listed it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CaptureSource {
    /// "monitor" or "window".
    pub kind: String,
    pub id: String,
    pub label: String,
}

/// Size the picture is encoded at.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Resolution {
    /// Native size; screens wider than 2560 are scaled to 1920 wide.
    #[default]
    #[serde(rename = "auto")]
    Auto,
    /// Native size, whatever it is.
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "1440")]
    P1440,
    #[serde(rename = "1080")]
    P1080,
    #[serde(rename = "720")]
    P720,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// Lower bitrate for slow networks.
    Economy,
    #[default]
    Standard,
    /// Sharper text and motion, more bandwidth.
    High,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct VideoSettings {
    /// Frames per second: 15, 30 or 60.
    pub fps: u32,
    pub resolution: Resolution,
    pub quality: Quality,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self { fps: 30, resolution: Resolution::Auto, quality: Quality::Standard }
    }
}

impl VideoSettings {
    pub fn clamped(self) -> Self {
        Self { fps: self.fps.clamp(5, 60), ..self }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CaptureOptions {
    #[serde(default)]
    pub video: VideoSettings,
    /// `None`: the primary monitor.
    pub source: Option<CaptureSource>,
    pub system_audio: bool,
    pub microphone: bool,
    /// Remote control session: input is accepted and the clipboard synced.
    #[serde(default)]
    pub control: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MonitorChoice {
    pub id: String,
    pub label: String,
    pub primary: bool,
}

/// Sent to viewers as a `PKT_INFO` packet whenever the picture changes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct StreamInfo {
    pub width: u32,
    pub height: u32,
    /// Audio is 48 kHz stereo signed 16-bit little-endian PCM.
    pub audio: bool,
    /// Monitors a controller can switch to.
    #[serde(default)]
    pub monitors: Vec<MonitorChoice>,
    /// Id of the monitor being shown, if it is a monitor.
    #[serde(default)]
    pub monitor: Option<String>,
    /// Current encoding settings.
    #[serde(default)]
    pub video: VideoSettings,
}

/// Input from a controller. Mouse coordinates are relative to the shown
/// picture (0..1); keys are `KeyboardEvent.code` values, so the controlled
/// device's own keyboard layout applies.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum InputEvent {
    MouseMove { x: f32, y: f32 },
    /// 0 left, 1 middle, 2 right, 3 back, 4 forward (as `MouseEvent.button`).
    MouseButton { button: u8, down: bool },
    /// In wheel notches; positive `dy` scrolls down.
    Wheel { dx: f32, dy: f32 },
    Key { code: String, down: bool },
    /// Typed text, for keyboards that have no physical key codes (phone
    /// on-screen keyboards). Entered as Unicode characters.
    Text { text: String },
    Clipboard { text: String },
    SwitchMonitor { id: String },
    /// Change resolution / frame rate / quality of the stream. Accepted from
    /// a controller or from the device the share was started for.
    SetVideo { video: VideoSettings },
    /// Release every key and button still held (focus lost, disconnect).
    ReleaseAll,
}

/// Implemented per platform: starts and stops capture for a session, and
/// applies controller input.
pub trait ScreenProvider: Send + Sync {
    fn start(&self, session_id: &str, options: &CaptureOptions, sink: BroadcastSink) -> Result<()>;
    fn stop(&self, session_id: &str);
    /// Called only for devices allowed to control the session.
    fn input(&self, _session_id: &str, _event: &InputEvent) {}
}

struct ViewerSlot {
    peer: String,
    tx: mpsc::Sender<Arc<Vec<u8>>>,
    waiting_for_key: bool,
    control: bool,
}

pub(crate) struct SessionShared {
    viewers: Mutex<HashMap<u64, ViewerSlot>>,
    next_viewer: AtomicU64,
    keyframe_wanted: AtomicBool,
    closed: AtomicBool,
    info: Mutex<Option<Arc<Vec<u8>>>>,
    /// Ends the session from the capture side (e.g. the window was closed).
    on_end: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// Handed to the provider; cheap to clone, safe to use from any thread.
#[derive(Clone)]
pub struct BroadcastSink {
    shared: Arc<SessionShared>,
}

impl BroadcastSink {
    pub fn info(&self, info: &StreamInfo) {
        let packet = Arc::new(encode_packet(PKT_INFO, 0, 0, &serde_json::to_vec(info).unwrap_or_default()));
        *self.shared.info.lock().unwrap() = Some(packet.clone());
        self.fan_out(packet, true);
    }

    pub fn video(&self, keyframe: bool, ts_us: u64, data: &[u8]) {
        let flags = if keyframe { FLAG_KEYFRAME } else { 0 };
        let packet = Arc::new(encode_packet(PKT_VIDEO, flags, ts_us, data));
        let mut viewers = self.shared.viewers.lock().unwrap();
        let mut gone = Vec::new();
        for (key, v) in viewers.iter_mut() {
            if v.waiting_for_key {
                if !keyframe {
                    continue;
                }
                v.waiting_for_key = false;
            }
            match v.tx.try_send(packet.clone()) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Too slow: drop frames until a fresh keyframe.
                    v.waiting_for_key = true;
                    self.shared.keyframe_wanted.store(true, Ordering::Relaxed);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => gone.push(*key),
            }
        }
        for key in gone {
            viewers.remove(&key);
        }
    }

    pub fn audio(&self, ts_us: u64, pcm: &[u8]) {
        self.fan_out(Arc::new(encode_packet(PKT_AUDIO, 0, ts_us, pcm)), false);
    }

    /// Sends clipboard text to the controllers of the session.
    pub fn clipboard(&self, text: &str) {
        let packet = Arc::new(encode_packet(PKT_CLIPBOARD, 0, 0, text.as_bytes()));
        let viewers = self.shared.viewers.lock().unwrap();
        for v in viewers.values().filter(|v| v.control) {
            let _ = v.tx.try_send(packet.clone());
        }
    }

    fn fan_out(&self, packet: Arc<Vec<u8>>, important: bool) {
        let viewers = self.shared.viewers.lock().unwrap();
        for v in viewers.values() {
            if v.tx.try_send(packet.clone()).is_err() && important {
                tracing::debug!("viewer queue full, info packet dropped");
            }
        }
    }

    /// True once after a viewer joined or fell behind: the next frame should
    /// be a keyframe.
    pub fn take_keyframe_request(&self) -> bool {
        self.shared.keyframe_wanted.swap(false, Ordering::Relaxed)
    }

    pub fn has_viewers(&self) -> bool {
        !self.shared.viewers.lock().unwrap().is_empty()
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Relaxed)
    }

    /// The source went away; ends the session for everyone.
    pub fn end(&self) {
        let f = self.shared.on_end.lock().unwrap().take();
        if let Some(f) = f {
            f();
        }
    }
}

pub(crate) struct LocalSession {
    title: String,
    options: CaptureOptions,
    invited: HashSet<String>,
    public: bool,
    started: i64,
    shared: Arc<SessionShared>,
    /// Devices allowed to send input.
    controllers: HashSet<String>,
    /// Started because this device asked; ends when it stops watching.
    requested_by: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ViewerInfo {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct OutgoingShare {
    pub id: String,
    pub title: String,
    pub system_audio: bool,
    pub microphone: bool,
    pub public: bool,
    pub invited: Vec<String>,
    pub viewers: Vec<ViewerInfo>,
    /// Connected viewers that are controlling this device.
    pub controllers: Vec<ViewerInfo>,
    pub started: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct RemoteShare {
    pub id: String,
    pub peer: String,
    pub peer_name: String,
    pub title: String,
    pub audio: bool,
    pub started: i64,
    /// This device asked to see that screen; open the viewer right away.
    pub requested: bool,
    /// This device may control the other one.
    pub control: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct ScreenAsk {
    pub id: String,
    pub peer: String,
    pub peer_name: String,
    pub created: i64,
    /// Wants to control, not only to watch.
    pub control: bool,
}

/// An open viewer: packets in, input out.
#[derive(Debug)]
pub struct ScreenWatch {
    pub packets: mpsc::Receiver<Vec<u8>>,
    pub input: mpsc::Sender<InputEvent>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ScreenView {
    /// Sessions this device is sharing.
    pub sharing: Vec<OutgoingShare>,
    /// Sessions other devices offer to this device.
    pub available: Vec<RemoteShare>,
    /// Devices asking to see this screen, waiting for a decision.
    pub asks: Vec<ScreenAsk>,
    /// Devices this device asked to see, waiting for their answer.
    pub asked: Vec<String>,
}

#[derive(Default)]
pub(crate) struct ScreenState {
    provider: Option<Arc<dyn ScreenProvider>>,
    sessions: HashMap<String, LocalSession>,
    remote: HashMap<String, RemoteShare>,
    asks: Vec<ScreenAsk>,
    /// Ask id → peer.
    asked: HashMap<String, String>,
    /// Peers that accepted an ask of this device, and when. The answer may
    /// arrive before the invite it promises.
    accepted: HashMap<String, std::time::Instant>,
}

impl Node {
    pub fn set_screen_provider(&self, provider: Arc<dyn ScreenProvider>) {
        self.inner.screen.lock().unwrap().provider = Some(provider);
    }

    pub fn screen(&self) -> ScreenView {
        let state = self.inner.screen.lock().unwrap();
        let mut sharing: Vec<OutgoingShare> = state
            .sessions
            .iter()
            .map(|(id, s)| OutgoingShare {
                id: id.clone(),
                title: s.title.clone(),
                system_audio: s.options.system_audio,
                microphone: s.options.microphone,
                public: s.public,
                invited: s.invited.iter().cloned().collect(),
                viewers: {
                    let viewers = s.shared.viewers.lock().unwrap();
                    let mut seen = HashSet::new();
                    viewers
                        .values()
                        .filter(|v| seen.insert(v.peer.clone()))
                        .map(|v| ViewerInfo { id: v.peer.clone(), name: self.peer_name(&v.peer) })
                        .collect()
                },
                controllers: {
                    let viewers = s.shared.viewers.lock().unwrap();
                    let mut seen = HashSet::new();
                    viewers
                        .values()
                        .filter(|v| v.control && seen.insert(v.peer.clone()))
                        .map(|v| ViewerInfo { id: v.peer.clone(), name: self.peer_name(&v.peer) })
                        .collect()
                },
                started: s.started,
            })
            .collect();
        sharing.sort_by_key(|s| s.started);
        let mut available: Vec<RemoteShare> = state.remote.values().cloned().collect();
        available.sort_by_key(|s| s.started);
        ScreenView {
            sharing,
            available,
            asks: state.asks.clone(),
            asked: state.asked.values().cloned().collect(),
        }
    }

    fn emit_screen(&self) {
        self.emit(Event::Screen(self.screen()));
    }

    /// Starts sharing and invites the given devices. With `public`, every
    /// device on the network is invited, including ones that appear later.
    pub fn start_screen_share(&self, mut options: CaptureOptions, invite: Vec<String>, public: bool) -> Result<String> {
        // Control is only ever granted through an accepted ask.
        options.control = false;
        self.start_share_inner(options, invite, public, None, false)
    }

    fn start_share_inner(
        &self,
        options: CaptureOptions,
        invite: Vec<String>,
        public: bool,
        requested_by: Option<String>,
        control: bool,
    ) -> Result<String> {
        let requested = requested_by.is_some();
        let provider = self
            .inner
            .screen
            .lock()
            .unwrap()
            .provider
            .clone()
            .ok_or_else(|| anyhow!("screen sharing is not available on this device"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let shared = Arc::new(SessionShared {
            viewers: Mutex::new(HashMap::new()),
            next_viewer: AtomicU64::new(1),
            keyframe_wanted: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            info: Mutex::new(None),
            on_end: Mutex::new(None),
        });
        {
            let node = self.clone();
            let sid = id.clone();
            *shared.on_end.lock().unwrap() = Some(Box::new(move || {
                let n = node.clone();
                node.spawn(async move {
                    n.stop_screen_share(&sid);
                });
            }));
        }
        provider.start(&id, &options, BroadcastSink { shared: shared.clone() })?;

        let mut invited: HashSet<String> = invite.into_iter().filter(|p| crate::identity::is_valid_id(p)).collect();
        if public {
            invited.extend(self.peers().into_iter().filter(|p| p.online).map(|p| p.id));
        }
        let title = options.source.as_ref().map(|s| s.label.clone()).unwrap_or_default();
        let audio = options.system_audio || options.microphone;
        let controllers: HashSet<String> = match (&requested_by, control) {
            (Some(peer), true) => HashSet::from([peer.clone()]),
            _ => HashSet::new(),
        };
        self.inner.screen.lock().unwrap().sessions.insert(
            id.clone(),
            LocalSession {
                title: title.clone(),
                options,
                invited: invited.clone(),
                public,
                started: now_ms(),
                shared,
                controllers: controllers.clone(),
                requested_by,
            },
        );
        for peer in invited {
            self.notify_peer(
                &peer,
                Request::ScreenInvite {
                    session: id.clone(),
                    title: title.clone(),
                    audio,
                    requested,
                    control: controllers.contains(&peer),
                },
            );
        }
        self.emit_screen();
        Ok(id)
    }

    pub fn stop_screen_share(&self, id: &str) {
        let (session, provider) = {
            let mut state = self.inner.screen.lock().unwrap();
            (state.sessions.remove(id), state.provider.clone())
        };
        let Some(session) = session else { return };
        session.shared.closed.store(true, Ordering::Relaxed);
        // Dropping the senders ends every viewer's stream.
        session.shared.viewers.lock().unwrap().clear();
        if let Some(p) = provider {
            p.stop(id);
        }
        for peer in &session.invited {
            self.notify_peer(peer, Request::ScreenEnded { session: id.to_owned() });
        }
        self.emit_screen();
    }

    pub(crate) fn stop_all_screen_shares(&self) {
        let ids: Vec<String> = self.inner.screen.lock().unwrap().sessions.keys().cloned().collect();
        for id in ids {
            self.stop_screen_share(&id);
        }
    }

    /// A device lost its trust: it stops seeing this screen. Shares started
    /// for its ask end; it is dropped from the other private shares it was
    /// invited to (public shares stay open to everyone).
    pub(crate) fn revoke_screen_access(&self, peer: &str) {
        let (requested, dropped): (Vec<String>, Vec<String>) = {
            let mut state = self.inner.screen.lock().unwrap();
            let requested = state
                .sessions
                .iter()
                .filter(|(_, s)| s.requested_by.as_deref() == Some(peer))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let mut dropped = Vec::new();
            for (id, s) in state.sessions.iter_mut() {
                if s.public || s.requested_by.as_deref() == Some(peer) || !s.invited.remove(peer) {
                    continue;
                }
                s.controllers.remove(peer);
                // Dropping a viewer's sender ends its stream.
                s.shared.viewers.lock().unwrap().retain(|_, v| v.peer != peer);
                dropped.push(id.clone());
            }
            (requested, dropped)
        };
        for id in &requested {
            self.stop_screen_share(id);
        }
        for id in dropped {
            self.notify_peer(peer, Request::ScreenEnded { session: id });
        }
        self.emit_screen();
    }

    /// Asks a device to show its screen to this device, and with `control`
    /// to let this device control it.
    pub fn ask_screen(&self, peer: &str, control: bool) -> Result<()> {
        if !crate::identity::is_valid_id(peer) {
            bail!("unknown device");
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.inner.screen.lock().unwrap().asked.insert(id.clone(), peer.to_owned());
        self.emit_screen();
        let node = self.clone();
        let peer = peer.to_owned();
        self.spawn(async move {
            let failed = match node.request(&peer, &Request::ScreenAsk { id: id.clone(), control }).await {
                Ok(Response::Ok) => None,
                Ok(Response::Error { message }) => Some(message),
                Ok(other) => Some(format!("unexpected answer: {other:?}")),
                Err(e) => Some(e.to_string()),
            };
            if let Some(error) = failed {
                tracing::info!("screen ask to {peer} failed: {error}");
                node.inner.screen.lock().unwrap().asked.remove(&id);
                node.emit_screen();
                node.emit(Event::Notice(Notice::ScreenAskAnswered { peer_name: node.peer_name(&peer), accepted: false }));
            }
        });
        Ok(())
    }

    /// Answers a device that asked to see this screen.
    pub fn answer_screen_ask(&self, id: &str, accept: bool) -> Result<()> {
        let ask = {
            let mut state = self.inner.screen.lock().unwrap();
            let pos = state.asks.iter().position(|a| a.id == id).ok_or_else(|| anyhow!("request not found"))?;
            state.asks.remove(pos)
        };
        self.emit_screen();
        if accept {
            self.accept_ask(&ask.peer, &ask.id, ask.control)
        } else {
            self.notify_peer(&ask.peer, Request::ScreenAskAnswer { id: ask.id, accepted: false });
            Ok(())
        }
    }

    fn accept_ask(&self, peer: &str, ask_id: &str, control: bool) -> Result<()> {
        let video = self.settings().screen_video;
        let options = CaptureOptions { video, source: None, system_audio: true, microphone: false, control };
        match self.start_share_inner(options, vec![peer.to_owned()], false, Some(peer.to_owned()), control) {
            Ok(_) => {
                self.notify_peer(peer, Request::ScreenAskAnswer { id: ask_id.to_owned(), accepted: true });
                Ok(())
            }
            Err(e) => {
                self.notify_peer(peer, Request::ScreenAskAnswer { id: ask_id.to_owned(), accepted: false });
                Err(e)
            }
        }
    }

    /// Starts receiving a session. Each packet includes its header; the
    /// channel closes when the session ends or the connection drops.
    /// Dropping the receiver leaves the session. Input sent on `input` is
    /// applied only if the sharer allowed this device to control.
    pub async fn watch_screen(&self, peer: &str, session: &str) -> Result<ScreenWatch> {
        let conn = self.connection(peer).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        write_frame(&mut send, &Request::ScreenWatch { session: session.to_owned() }).await?;
        let answer: Response = tokio::time::timeout(std::time::Duration::from_secs(10), read_frame(&mut recv))
            .await
            .map_err(|_| anyhow!("no answer from device"))??;
        match answer {
            Response::Ok => {}
            Response::Error { message } => bail!(message),
            other => bail!("unexpected answer: {other:?}"),
        }
        let (tx, rx) = mpsc::channel(256);
        let (input_tx, mut input_rx) = mpsc::channel::<InputEvent>(512);
        self.spawn(async move {
            while let Some(event) = input_rx.recv().await {
                if write_frame(&mut send, &event).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        self.spawn(async move {
            let mut header = [0u8; HEADER_LEN];
            loop {
                if recv.read_exact(&mut header).await.is_err() {
                    break;
                }
                let len = u32::from_be_bytes(header[10..14].try_into().unwrap()) as usize;
                if len > MAX_PACKET {
                    break;
                }
                // The buffer grows as data arrives instead of trusting the
                // announced length, as in `read_frame`.
                let mut packet = Vec::with_capacity(HEADER_LEN + len.min(64 * 1024));
                packet.extend_from_slice(&header);
                match (&mut recv).take(len as u64).read_to_end(&mut packet).await {
                    Ok(n) if n == len => {}
                    _ => break,
                }
                if tx.send(packet).await.is_err() {
                    break;
                }
            }
            let _ = recv.stop(0u32.into());
        });
        Ok(ScreenWatch { packets: rx, input: input_tx })
    }

    // ---- requests from peers ---------------------------------------------

    pub(crate) fn on_screen_invite(
        &self,
        peer: &str,
        session: String,
        title: String,
        audio: bool,
        requested: bool,
        control: bool,
    ) -> Response {
        if !valid_wire_id(&session) {
            return Response::Error { message: "invalid session".into() };
        }
        let peer_name = self.peer_name(peer);
        let share = {
            let mut state = self.inner.screen.lock().unwrap();
            if !state.remote.contains_key(&session) && state.remote.values().filter(|s| s.peer == peer).count() >= MAX_INVITES_PER_PEER {
                return Response::Error { message: "too many sessions".into() };
            }
            // "In answer to your ask" opens the viewer right away, so it is
            // believed only if this device really asked that peer.
            let recently_accepted = state.accepted.get(peer).is_some_and(|t| t.elapsed() < ACCEPTED_ASK_TTL);
            let requested = requested && (recently_accepted || state.asked.values().any(|p| p == peer));
            if requested {
                state.asked.retain(|_, p| p != peer);
                state.accepted.remove(peer);
            }
            let share = RemoteShare {
                id: session.clone(),
                peer: peer.to_owned(),
                peer_name,
                title: title.chars().take(200).collect(),
                audio,
                started: now_ms(),
                requested,
                control,
            };
            if state.remote.insert(session, share.clone()).is_some() {
                return Response::Ok;
            }
            share
        };
        self.emit_screen();
        self.emit(Event::Notice(Notice::ScreenInvite { share }));
        Response::Ok
    }

    pub(crate) fn on_screen_ended(&self, peer: &str, session: &str) {
        let removed = {
            let mut state = self.inner.screen.lock().unwrap();
            match state.remote.get(session) {
                Some(s) if s.peer == peer => state.remote.remove(session).is_some(),
                _ => false,
            }
        };
        if removed {
            self.emit_screen();
        }
    }

    pub(crate) fn on_screen_ask(&self, peer: &str, id: String, control: bool) -> Response {
        if !valid_wire_id(&id) {
            return Response::Error { message: "invalid request".into() };
        }
        if self.inner.screen.lock().unwrap().provider.is_none() {
            return Response::Error { message: "screen sharing is not available on this device".into() };
        }
        if self.is_trusted(peer) && !control && self.settings().trusted_view_without_asking {
            // The user let trusted devices watch without confirmation; the
            // sharing banner still shows who is watching. Control always asks.
            if let Err(e) = self.accept_ask(peer, &id, control) {
                tracing::warn!("could not start screen sharing: {e}");
            }
            return Response::Ok;
        }
        let ask = ScreenAsk { id, peer: peer.to_owned(), peer_name: self.peer_name(peer), created: now_ms(), control };
        {
            let mut state = self.inner.screen.lock().unwrap();
            if state.asks.iter().any(|a| a.peer == peer) {
                return Response::Ok;
            }
            if state.asks.len() >= MAX_ASKS {
                return Response::Error { message: "too many requests are waiting for an answer".into() };
            }
            state.asks.push(ask.clone());
        }
        self.emit_screen();
        self.emit(Event::Notice(Notice::ScreenAsked { ask }));
        Response::Ok
    }

    pub(crate) fn on_screen_ask_answer(&self, peer: &str, id: &str, accepted: bool) {
        let removed = {
            let mut state = self.inner.screen.lock().unwrap();
            let removed = match state.asked.get(id) {
                Some(p) if p == peer => state.asked.remove(id).is_some(),
                _ => false,
            };
            if removed && accepted {
                state.accepted.retain(|_, t| t.elapsed() < ACCEPTED_ASK_TTL);
                state.accepted.insert(peer.to_owned(), std::time::Instant::now());
            }
            removed
        };
        if removed {
            self.emit_screen();
            if !accepted {
                self.emit(Event::Notice(Notice::ScreenAskAnswered { peer_name: self.peer_name(peer), accepted }));
            }
        }
    }

    /// Serves one viewer: writes the session's packets until it ends, and
    /// applies its input if it may control.
    pub(crate) async fn serve_screen_watch(
        &self,
        peer: &str,
        session: &str,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
    ) -> Result<()> {
        let found = {
            let state = self.inner.screen.lock().unwrap();
            match state.sessions.get(session) {
                Some(s) if s.public || s.invited.contains(peer) => Some((
                    s.shared.clone(),
                    s.controllers.contains(peer),
                    s.requested_by.as_deref() == Some(peer),
                    state.provider.clone(),
                )),
                _ => None,
            }
        };
        let Some((shared, control, requester, provider)) = found else {
            write_frame(&mut send, &Response::Error { message: "this screen is not shared with you".into() }).await?;
            send.finish()?;
            return Ok(());
        };
        write_frame(&mut send, &Response::Ok).await?;

        let (tx, mut rx) = mpsc::channel(VIEWER_QUEUE);
        if let Some(info) = shared.info.lock().unwrap().clone() {
            let _ = tx.try_send(info);
        }
        let key = shared.next_viewer.fetch_add(1, Ordering::Relaxed);
        shared
            .viewers
            .lock()
            .unwrap()
            .insert(key, ViewerSlot { peer: peer.to_owned(), tx, waiting_for_key: true, control });
        shared.keyframe_wanted.store(true, Ordering::Relaxed);
        self.emit_screen();

        // Input from the viewer. Everything is read (so the stream does not
        // stall) but only a controller's events are applied; the device the
        // share was started for may also change the video settings.
        let input_task = {
            let provider = provider.clone();
            let session = session.to_owned();
            self.inner.rt.spawn(async move {
                loop {
                    match read_frame::<InputEvent>(&mut recv).await {
                        Ok(event) => {
                            tracing::debug!("input for {session} (control: {control}): {event:?}");
                            let allowed = control || (requester && matches!(event, InputEvent::SetVideo { .. }));
                            if allowed {
                                if let Some(p) = &provider {
                                    p.input(&session, &event);
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        };

        while let Some(packet) = rx.recv().await {
            if send.write_all(&packet).await.is_err() {
                break;
            }
        }
        input_task.abort();
        if control {
            // Never leave a key or button stuck down on this machine.
            if let Some(p) = &provider {
                p.input(session, &InputEvent::ReleaseAll);
            }
        }
        shared.viewers.lock().unwrap().remove(&key);
        let _ = send.finish();
        self.emit_screen();

        // A share made for someone's ask ends when they stop watching.
        let ended_for_requester = {
            let state = self.inner.screen.lock().unwrap();
            state
                .sessions
                .get(session)
                .is_some_and(|s| s.requested_by.as_deref() == Some(peer) && s.shared.viewers.lock().unwrap().is_empty())
        };
        if ended_for_requester {
            self.stop_screen_share(session);
        }
        Ok(())
    }

    /// A device (re)connected: invite it to public sessions it does not know.
    pub(crate) fn invite_to_public_shares(&self, peer: &str) {
        let invites: Vec<Request> = {
            let mut state = self.inner.screen.lock().unwrap();
            state
                .sessions
                .iter_mut()
                .filter(|(_, s)| s.public && !s.invited.contains(peer))
                .map(|(id, s)| {
                    s.invited.insert(peer.to_owned());
                    Request::ScreenInvite {
                        session: id.clone(),
                        title: s.title.clone(),
                        audio: s.options.system_audio || s.options.microphone,
                        requested: false,
                        control: false,
                    }
                })
                .collect()
        };
        for req in invites {
            self.notify_peer(peer, req);
        }
    }
}
