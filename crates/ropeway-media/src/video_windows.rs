//! Screen and window capture on Windows (Windows Graphics Capture).
//!
//! The capture callback only copies the newest frame into a slot; a separate
//! encoder thread picks it up. That keeps capture smooth when encoding is
//! slow, and lets a static screen be re-encoded as a keyframe when a new
//! viewer joins.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use ropeway_core::screen::VideoSettings;
use ropeway_core::{BroadcastSink, CaptureSource, MonitorChoice};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, GraphicsCaptureItemType,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

use crate::frames::{encode_loop, Slot};
use crate::SourceInfo;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

struct Handler {
    slot: Arc<Slot>,
    /// When the next frame is due. Windows delivers frames at the monitor's
    /// refresh rate; taking one when it is due (with some tolerance for
    /// jitter) gives the full requested rate, where "at least one interval
    /// since the last frame" would drop frames on every jittery tick.
    next_due: Option<Instant>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Arc<Slot>;
    type Error = BoxError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { slot: ctx.flags, next_due: None })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        if self.slot.stopped() {
            control.stop();
            return Ok(());
        }
        let now = Instant::now();
        let interval = self.slot.interval;
        if let Some(due) = self.next_due {
            if now + interval / 4 < due {
                return Ok(());
            }
            // Stay on the schedule unless we fell more than a frame behind.
            self.next_due = Some(if now < due + interval { due + interval } else { now + interval });
        } else {
            self.next_due = Some(now + interval);
        }
        let mut buffer = frame.buffer()?;
        let (width, height, stride) = (buffer.width(), buffer.height(), buffer.row_pitch());
        let raw = buffer.as_raw_buffer();
        self.slot.put(width, height, stride, |data| data.extend_from_slice(raw));
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.slot.end();
        Ok(())
    }
}

pub struct VideoPipeline {
    slot: Arc<Slot>,
    control: Option<CaptureControl<Handler, BoxError>>,
    encoder: Option<JoinHandle<()>>,
}

impl VideoPipeline {
    /// `_control`: whether input will follow; Windows needs no permission for it.
    pub fn start(source: Option<&CaptureSource>, sink: BroadcastSink, audio: bool, video: VideoSettings, _control: bool) -> Result<Self> {
        let video = video.clamped();
        let slot = Arc::new(Slot::new(video.fps));
        let control = match source {
            Some(s) if s.kind == "window" => {
                let hwnd: usize = s.id.parse().map_err(|_| anyhow!("invalid window"))?;
                let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
                if !window.is_valid() {
                    return Err(anyhow!("the window is no longer available"));
                }
                Handler::start_free_threaded(settings(window, slot.clone()))
            }
            Some(s) => {
                let handle: usize = s.id.parse().map_err(|_| anyhow!("invalid monitor"))?;
                Handler::start_free_threaded(settings(Monitor::from_raw_hmonitor(handle as *mut std::ffi::c_void), slot.clone()))
            }
            None => Handler::start_free_threaded(settings(Monitor::primary()?, slot.clone())),
        }
        .map_err(|e| anyhow!("screen capture failed to start: {e}"))?;

        let encoder_slot = slot.clone();
        let monitors = monitor_choices();
        let current = resolve_monitor(source).map(|h| h.to_string());
        let encoder = std::thread::Builder::new()
            .name("ropeway-encoder".into())
            .spawn(move || encode_loop(&encoder_slot, &sink, audio, &monitors, current, video))?;
        Ok(Self { slot, control: Some(control), encoder: Some(encoder) })
    }
}

fn settings<T: TryInto<GraphicsCaptureItemType>>(item: T, slot: Arc<Slot>) -> Settings<Arc<Slot>, T> {
    Settings::new(
        item,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        // Let Windows deliver every refresh; frames are picked on our schedule.
        MinimumUpdateIntervalSettings::Custom(Duration::from_millis(4)),
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        slot,
    )
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        self.slot.request_stop();
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
        if let Some(t) = self.encoder.take() {
            let _ = t.join();
        }
    }
}

/// Monitor handle a capture source shows (`None` source: the primary monitor).
pub fn resolve_monitor(source: Option<&CaptureSource>) -> Option<usize> {
    match source {
        None => Monitor::primary().ok().map(|m| m.as_raw_hmonitor() as usize),
        Some(s) if s.kind == "monitor" => s.id.parse().ok(),
        Some(_) => None,
    }
}

/// Monitors a controller may switch between; labels are their numbers.
pub fn monitor_choices() -> Vec<MonitorChoice> {
    let primary = Monitor::primary().ok().map(|m| m.as_raw_hmonitor() as usize);
    Monitor::enumerate()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let handle = m.as_raw_hmonitor() as usize;
            MonitorChoice { id: handle.to_string(), label: (i + 1).to_string(), primary: Some(handle) == primary }
        })
        .collect()
}

pub fn list_sources() -> Vec<SourceInfo> {
    let mut out = Vec::new();
    let primary = Monitor::primary().ok().map(|m| m.as_raw_hmonitor() as usize);
    for (i, m) in Monitor::enumerate().unwrap_or_default().into_iter().enumerate() {
        let handle = m.as_raw_hmonitor() as usize;
        out.push(SourceInfo {
            kind: "monitor".into(),
            id: handle.to_string(),
            index: i as u32 + 1,
            name: m.name().unwrap_or_default(),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            primary: Some(handle) == primary,
            process: String::new(),
        });
    }
    let own_pid = std::process::id();
    for w in Window::enumerate().unwrap_or_default() {
        let title = w.title().unwrap_or_default();
        if title.trim().is_empty() || w.process_id().ok() == Some(own_pid) {
            continue;
        }
        let (width, height) = (w.width().unwrap_or(0), w.height().unwrap_or(0));
        if width < 50 || height < 50 {
            continue;
        }
        out.push(SourceInfo {
            kind: "window".into(),
            id: (w.as_raw_hwnd() as usize).to_string(),
            index: 0,
            name: title,
            width: width as u32,
            height: height as u32,
            primary: false,
            process: w.process_name().unwrap_or_default(),
        });
    }
    out
}
