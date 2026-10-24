//! Screen and window capture on Windows: Windows Graphics Capture, or for
//! monitors on Windows 10 builds without it (and when it fails to start),
//! DXGI Desktop Duplication.
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
use windows_capture::dxgi_duplication_api::{DxgiDuplicationApi, DxgiDuplicationFormat, Error as DxgiError};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
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

/// What delivers the frames.
enum Capture {
    Graphics(CaptureControl<Handler, BoxError>),
    Duplication(JoinHandle<()>),
}

pub struct VideoPipeline {
    slot: Arc<Slot>,
    capture: Option<Capture>,
    encoder: Option<JoinHandle<()>>,
}

impl VideoPipeline {
    /// `_control`: whether input will follow; Windows needs no permission for it.
    pub fn start(source: Option<&CaptureSource>, sink: BroadcastSink, audio: bool, video: VideoSettings, _control: bool) -> Result<Self> {
        let video = video.clamped();
        let slot = Arc::new(Slot::new(video.fps));
        // ROPEWAY_CAPTURE_DXGI: use desktop duplication even where WGC works (testing).
        let graphics = std::env::var_os("ROPEWAY_CAPTURE_DXGI").is_none() && GraphicsCaptureApi::is_supported().unwrap_or(false);
        let capture = match source {
            Some(s) if s.kind == "window" => {
                if !graphics {
                    return Err(anyhow!("showing a single window needs Windows 10 version 1903 or newer; show the whole screen instead"));
                }
                let hwnd: usize = s.id.parse().map_err(|_| anyhow!("invalid window"))?;
                let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
                if !window.is_valid() {
                    return Err(anyhow!("the window is no longer available"));
                }
                Capture::Graphics(
                    Handler::start_free_threaded(settings(window, slot.clone()))
                        .map_err(|e| anyhow!("screen capture failed to start: {e}"))?,
                )
            }
            _ => {
                let monitor = match source {
                    Some(s) => {
                        let handle: usize = s.id.parse().map_err(|_| anyhow!("invalid monitor"))?;
                        Monitor::from_raw_hmonitor(handle as *mut std::ffi::c_void)
                    }
                    None => Monitor::primary()?,
                };
                let handle = monitor.as_raw_hmonitor() as usize;
                let started = if graphics {
                    Handler::start_free_threaded(settings(monitor, slot.clone())).map_err(|e| {
                        tracing::warn!("Windows Graphics Capture failed ({e}), using desktop duplication");
                    })
                } else {
                    tracing::info!("Windows Graphics Capture is not available, using desktop duplication");
                    Err(())
                };
                match started {
                    Ok(control) => Capture::Graphics(control),
                    Err(()) => Capture::Duplication(start_duplication(handle, slot.clone())?),
                }
            }
        };

        let encoder_slot = slot.clone();
        let monitors = monitor_choices();
        let current = resolve_monitor(source).map(|h| h.to_string());
        let encoder = std::thread::Builder::new()
            .name("ropeway-encoder".into())
            .spawn(move || encode_loop(&encoder_slot, &sink, audio, &monitors, current, video))?;
        Ok(Self { slot, capture: Some(capture), encoder: Some(encoder) })
    }
}

/// Capture settings this version of Windows accepts: older Windows 10 builds
/// refuse to start a capture that asks for options they do not have.
fn settings<T: TryInto<GraphicsCaptureItemType>>(item: T, slot: Arc<Slot>) -> Settings<Arc<Slot>, T> {
    let cursor = if GraphicsCaptureApi::is_cursor_settings_supported().unwrap_or(false) {
        CursorCaptureSettings::WithCursor
    } else {
        CursorCaptureSettings::Default
    };
    // Let Windows deliver every refresh; frames are picked on our schedule.
    // Windows 10 cannot set this and delivers at its own pace.
    let interval = if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
        MinimumUpdateIntervalSettings::Custom(Duration::from_millis(4))
    } else {
        MinimumUpdateIntervalSettings::Default
    };
    Settings::new(
        item,
        cursor,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        interval,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        slot,
    )
}

/// Captures a monitor with DXGI Desktop Duplication on its own thread. The
/// first frame is awaited, so a capture that cannot start fails here.
fn start_duplication(monitor: usize, slot: Arc<Slot>) -> Result<JoinHandle<()>> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let handle = std::thread::Builder::new().name("ropeway-duplication".into()).spawn(move || {
        let open = || DxgiDuplicationApi::new_options(Monitor::from_raw_hmonitor(monitor as *mut std::ffi::c_void), &[DxgiDuplicationFormat::Bgra8]);
        let mut dup = match open() {
            Ok(d) => {
                let _ = ready_tx.send(Ok(()));
                d
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e.to_string()));
                return;
            }
        };
        let interval = slot.interval;
        let wait_ms = (interval.as_millis() as u32).clamp(5, 100);
        let mut next_due = Instant::now();
        let mut failures = 0u32;
        while !slot.stopped() {
            match dup.acquire_next_frame(wait_ms) {
                Ok(mut frame) => {
                    failures = 0;
                    // Only the mouse moved: the frame carries no picture.
                    if frame.frame_info().LastPresentTime == 0 {
                        continue;
                    }
                    let now = Instant::now();
                    if now + interval / 4 < next_due {
                        continue;
                    }
                    next_due = if now < next_due + interval { next_due + interval } else { now + interval };
                    match frame.buffer() {
                        Ok(mut buffer) => {
                            let (width, height, stride) = (buffer.width(), buffer.height(), buffer.row_pitch());
                            let raw = buffer.as_raw_buffer();
                            slot.put(width, height, stride, |data| data.extend_from_slice(raw));
                        }
                        Err(e) => tracing::debug!("desktop duplication frame: {e}"),
                    }
                }
                // Nothing changed on the screen.
                Err(DxgiError::Timeout) => {}
                // Mode change, a secure desktop (UAC, Ctrl+Alt+Del) or a
                // full-screen app: open the duplication again.
                Err(e) => {
                    failures += 1;
                    if failures > 50 {
                        tracing::warn!("desktop duplication stopped: {e}");
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    match open() {
                        Ok(d) => dup = d,
                        Err(e) => tracing::debug!("desktop duplication: {e}"),
                    }
                }
            }
        }
        slot.end();
    })?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(anyhow!("screen capture failed to start: {e}"))
        }
        Err(_) => Err(anyhow!("screen capture failed to start")),
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        self.slot.request_stop();
        match self.capture.take() {
            Some(Capture::Graphics(control)) => {
                let _ = control.stop();
            }
            Some(Capture::Duplication(thread)) => {
                let _ = thread.join();
            }
            None => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Desktop duplication delivers real frames of the primary monitor.
    /// Captures this computer's screen, so it runs only on request:
    /// `cargo test -p ropeway-media duplication -- --ignored`.
    #[test]
    #[ignore]
    fn duplication_captures_the_screen() {
        let slot = Arc::new(Slot::new(30));
        let monitor = Monitor::primary().unwrap();
        let thread = start_duplication(monitor.as_raw_hmonitor() as usize, slot.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let frame = loop {
            if let Some(f) = slot.frame.lock().unwrap().take() {
                break f;
            }
            assert!(Instant::now() < deadline, "no frame from desktop duplication");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!((frame.width, frame.height), (monitor.width().unwrap(), monitor.height().unwrap()));
        assert!(frame.stride >= frame.width * 4);
        assert!(frame.data.iter().any(|&b| b != 0), "the frame is empty");
        slot.request_stop();
        thread.join().unwrap();
    }
}
