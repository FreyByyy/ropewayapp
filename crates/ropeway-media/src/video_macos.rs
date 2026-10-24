//! Screen capture on macOS with ScreenCaptureKit (macOS 13+). The first
//! capture makes macOS ask for the Screen Recording permission.

use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use core_graphics::display::CGDisplay;
use ropeway_core::screen::VideoSettings;
use ropeway_core::{BroadcastSink, CaptureSource, MonitorChoice};
use screencapturekit::prelude::*;

use crate::frames::{encode_loop, Slot};
use crate::SourceInfo;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Whether macOS lets Ropeway record the screen. The first time it shows the
/// system prompt; afterwards only System Settings can change the answer, and
/// macOS applies it after Ropeway restarts.
pub fn screen_recording_allowed() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() }
}

/// The error text the UI recognises and explains (System Settings, restart).
const NO_PERMISSION: &str = "screen recording is not allowed";

struct Output {
    slot: Arc<Slot>,
}

impl SCStreamOutputTrait for Output {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, _kind: SCStreamOutputType) {
        if self.slot.stopped() {
            return;
        }
        // Frames without a pixel buffer only say the screen did not change.
        let Some(pixels) = sample.pixel_buffer() else { return };
        let Ok(guard) = pixels.lock_read_only() else { return };
        let (width, height, stride) = (guard.width(), guard.height(), guard.bytes_per_row());
        let base = guard.base_address();
        if base.is_null() || width == 0 || height == 0 {
            return;
        }
        let bytes = unsafe { std::slice::from_raw_parts(base, stride * height) };
        self.slot.put(width as u32, height as u32, stride as u32, |data| data.extend_from_slice(bytes));
    }
}

pub struct VideoPipeline {
    slot: Arc<Slot>,
    stream: Option<SCStream>,
    encoder: Option<JoinHandle<()>>,
}

impl VideoPipeline {
    /// `_control`: input needs the Accessibility permission, asked for by the injector.
    pub fn start(source: Option<&CaptureSource>, sink: BroadcastSink, audio: bool, video: VideoSettings, _control: bool) -> Result<Self> {
        let video = video.clamped();
        let slot = Arc::new(Slot::new(video.fps));
        if !screen_recording_allowed() {
            return Err(anyhow!(NO_PERMISSION));
        }
        let content = SCShareableContent::get().map_err(|e| anyhow!("{NO_PERMISSION} ({e:?})"))?;
        let displays = content.displays();
        let wanted = resolve_monitor(source).map(|id| id as u32);
        let display = displays
            .iter()
            .find(|d| Some(d.display_id()) == wanted)
            .or_else(|| displays.first())
            .ok_or_else(|| anyhow!("no display to capture"))?;
        let id = display.display_id();
        // Capture in pixels, not points.
        let cg = CGDisplay::new(id);
        let (width, height) = (cg.pixels_wide() as u32, cg.pixels_high() as u32);
        let filter = SCContentFilter::create()
            .with_display(display)
            .with_excluding_windows(&[])
            .build()
            .map_err(|e| anyhow!("screen capture filter: {e:?}"))?;
        let config = SCStreamConfiguration::new()
            .with_width(width.max(2))
            .with_height(height.max(2))
            .with_pixel_format(PixelFormat::BGRA)
            .with_shows_cursor(true)
            .with_queue_depth(5)
            .with_minimum_frame_interval(&CMTime::new(1, video.fps as i32));
        let mut stream = SCStream::new(&filter, &config).map_err(|e| anyhow!("screen capture: {e:?}"))?;
        stream
            .add_output_handler(Output { slot: slot.clone() }, SCStreamOutputType::Screen)
            .map_err(|e| anyhow!("screen capture: {e:?}"))?;
        stream.start_capture().map_err(|e| anyhow!("screen capture did not start ({e:?})"))?;

        let monitors = monitor_choices();
        let current = Some(id.to_string());
        let encoder_slot = slot.clone();
        let encoder = std::thread::Builder::new()
            .name("ropeway-encoder".into())
            .spawn(move || encode_loop(&encoder_slot, &sink, audio, &monitors, current, video))?;
        Ok(Self { slot, stream: Some(stream), encoder: Some(encoder) })
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        self.slot.request_stop();
        if let Some(stream) = self.stream.take() {
            let _ = stream.stop_capture();
        }
        if let Some(t) = self.encoder.take() {
            let _ = t.join();
        }
    }
}

fn main_display() -> u32 {
    CGDisplay::main().id
}

/// Display id a source shows (`None` source: the main display).
pub fn resolve_monitor(source: Option<&CaptureSource>) -> Option<usize> {
    match source {
        Some(s) if s.kind == "monitor" => s.id.parse().ok(),
        _ => Some(main_display() as usize),
    }
}

fn displays() -> Vec<u32> {
    let mut ids = CGDisplay::active_displays().unwrap_or_default();
    let main = main_display();
    ids.sort_by_key(|&id| id != main);
    ids
}

pub fn monitor_choices() -> Vec<MonitorChoice> {
    let main = main_display();
    displays()
        .into_iter()
        .enumerate()
        .map(|(i, id)| MonitorChoice { id: id.to_string(), label: (i + 1).to_string(), primary: id == main })
        .collect()
}

pub fn list_sources() -> Vec<SourceInfo> {
    let main = main_display();
    displays()
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let d = CGDisplay::new(id);
            SourceInfo {
                kind: "monitor".into(),
                id: id.to_string(),
                index: i as u32 + 1,
                name: String::new(),
                width: d.pixels_wide() as u32,
                height: d.pixels_high() as u32,
                primary: id == main,
                process: String::new(),
            }
        })
        .collect()
}
