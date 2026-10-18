//! Screen capture and input on Linux: X11 directly, Wayland through the
//! desktop portal. The session type decides; an X11 app under Wayland
//! (XWayland) could only see other X11 windows, so Wayland wins.

use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;

use anyhow::Result;
use ropeway_core::screen::VideoSettings;
use ropeway_core::{BroadcastSink, CaptureSource, InputEvent, MonitorChoice};

use crate::frames::{encode_loop, Slot};
use crate::linux_wayland as wayland;
use crate::linux_x11 as x11;
use crate::SourceInfo;

/// The portal session being shared, for input to go into.
static PORTAL: Mutex<Option<Weak<wayland::Session>>> = Mutex::new(None);

pub struct VideoPipeline {
    slot: Arc<Slot>,
    capture: Option<JoinHandle<()>>,
    portal: Option<Arc<wayland::Session>>,
    encoder: Option<JoinHandle<()>>,
}

impl VideoPipeline {
    /// `control`: input will follow, which Wayland asks permission for.
    pub fn start(source: Option<&CaptureSource>, sink: BroadcastSink, audio: bool, video: VideoSettings, control: bool) -> Result<Self> {
        let video = video.clamped();
        let slot = Arc::new(Slot::new(video.fps));
        let (capture, portal, monitors, current) = if wayland::is_wayland() {
            let session = Arc::new(wayland::start(slot.clone(), control)?);
            *PORTAL.lock().unwrap() = Some(Arc::downgrade(&session));
            (None, Some(session), Vec::new(), None)
        } else {
            let list = x11::monitors();
            let index = resolve_monitor(source).unwrap_or(0);
            let monitor = list.get(index).cloned().ok_or_else(|| anyhow::anyhow!("no monitor to capture"))?;
            let current = Some(monitor.name.clone());
            let handle = x11::start_capture(slot.clone(), monitor)?;
            (Some(handle), None, monitor_choices(), current)
        };
        let encoder_slot = slot.clone();
        let encoder = std::thread::Builder::new()
            .name("ropeway-encoder".into())
            .spawn(move || encode_loop(&encoder_slot, &sink, audio, &monitors, current, video))?;
        Ok(Self { slot, capture, portal, encoder: Some(encoder) })
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        self.slot.request_stop();
        if let Some(t) = self.capture.take() {
            let _ = t.join();
        }
        drop(self.portal.take());
        if let Some(t) = self.encoder.take() {
            let _ = t.join();
        }
    }
}

/// Index of the monitor a source shows (X11). `None` source: the primary.
pub fn resolve_monitor(source: Option<&CaptureSource>) -> Option<usize> {
    if wayland::is_wayland() {
        return Some(0);
    }
    let list = x11::monitors();
    match source {
        Some(s) if s.kind == "monitor" => list.iter().position(|m| m.name == s.id).or(Some(0)),
        _ => list.iter().position(|m| m.primary).or(Some(0)),
    }
}

/// Monitors a controller may switch between (X11; on Wayland the user
/// picks the screen in the system dialog).
pub fn monitor_choices() -> Vec<MonitorChoice> {
    if wayland::is_wayland() {
        return Vec::new();
    }
    x11::monitors()
        .into_iter()
        .enumerate()
        .map(|(i, m)| MonitorChoice { id: m.name, label: (i + 1).to_string(), primary: m.primary })
        .collect()
}

pub fn list_sources() -> Vec<SourceInfo> {
    if wayland::is_wayland() {
        // The compositor shows its own picker when sharing starts.
        return vec![SourceInfo {
            kind: "monitor".into(),
            id: "portal".into(),
            index: 1,
            name: String::new(),
            width: 0,
            height: 0,
            primary: true,
            process: String::new(),
        }];
    }
    x11::monitors()
        .into_iter()
        .enumerate()
        .map(|(i, m)| SourceInfo {
            kind: "monitor".into(),
            id: m.name.clone(),
            index: i as u32 + 1,
            name: m.name,
            width: m.width as u32,
            height: m.height as u32,
            primary: m.primary,
            process: String::new(),
        })
        .collect()
}

/// Applies a controller's input on this computer.
pub enum Injector {
    X11(x11::Injector),
    Wayland,
    Unavailable,
}

impl Injector {
    pub fn new(monitor: Option<usize>) -> Self {
        if wayland::is_wayland() {
            return Injector::Wayland;
        }
        let list = x11::monitors();
        let Some(target) = list.get(monitor.unwrap_or(0)).cloned() else { return Injector::Unavailable };
        match x11::Injector::new(target) {
            Ok(i) => Injector::X11(i),
            Err(e) => {
                tracing::warn!("remote control is not available: {e}");
                Injector::Unavailable
            }
        }
    }

    pub fn set_target(&mut self, monitor: Option<usize>) {
        if let Injector::X11(i) = self {
            if let Some(m) = x11::monitors().get(monitor.unwrap_or(0)).cloned() {
                i.set_target(m);
            }
        }
    }

    pub fn apply(&mut self, event: &InputEvent) {
        match self {
            Injector::X11(i) => i.apply(event),
            Injector::Wayland => {
                if let Some(session) = PORTAL.lock().unwrap().as_ref().and_then(Weak::upgrade) {
                    session.input(event.clone());
                }
            }
            Injector::Unavailable => {}
        }
    }
}
