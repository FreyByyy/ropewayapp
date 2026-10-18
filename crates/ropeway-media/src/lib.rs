//! Platform side of screen sharing: what can be captured, and the
//! [`ScreenProvider`] that captures, encodes and feeds a session.
//!
//! Encoding (H.264 via OpenH264) and audio (cpal) are portable; only screen
//! capture is per platform. Porting to another OS means adding a
//! `video_<os>.rs` with the same `VideoPipeline` and `list_sources`.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use ropeway_core::{BroadcastSink, CaptureOptions, CaptureSource, InputEvent, ScreenProvider};
use serde::Serialize;

pub mod audio;
pub mod clipboard;
pub mod encoder;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod frames;
pub mod keymap;

#[cfg(windows)]
mod input_windows;
#[cfg(windows)]
mod video_windows;
#[cfg(windows)]
use video_windows as video;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_wayland;
#[cfg(target_os = "linux")]
mod linux_x11;
#[cfg(target_os = "linux")]
use linux as video;

#[cfg(target_os = "macos")]
mod input_macos;
#[cfg(target_os = "macos")]
mod video_macos;
#[cfg(target_os = "macos")]
use video_macos as video;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod video {
    use anyhow::{bail, Result};
    use ropeway_core::{BroadcastSink, CaptureSource};

    pub struct VideoPipeline;

    impl VideoPipeline {
        pub fn start(
            _: Option<&CaptureSource>,
            _: BroadcastSink,
            _: bool,
            _: ropeway_core::screen::VideoSettings,
            _: bool,
        ) -> Result<Self> {
            bail!("screen capture is not implemented on this platform yet")
        }
    }

    pub fn list_sources() -> Vec<super::SourceInfo> {
        Vec::new()
    }

    pub fn resolve_monitor(_: Option<&CaptureSource>) -> Option<usize> {
        None
    }
}

/// Something that can be shared: a monitor or a window.
#[derive(Serialize, Clone, Debug)]
pub struct SourceInfo {
    /// "monitor" or "window".
    pub kind: String,
    pub id: String,
    /// Monitors: 1-based number.
    pub index: u32,
    /// Monitor model or window title.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    /// Windows: executable name.
    pub process: String,
}

pub fn list_sources() -> Vec<SourceInfo> {
    video::list_sources()
}

struct Running {
    video: Option<video::VideoPipeline>,
    _audio: Option<audio::AudioPipeline>,
    sink: BroadcastSink,
    has_audio: bool,
    source: Option<CaptureSource>,
    settings: ropeway_core::screen::VideoSettings,
    control: Option<Control>,
}

impl Running {
    /// Restarts capture with the current source and settings; falls back to
    /// the primary monitor if the source cannot be captured. Returns the
    /// source actually shown.
    fn restart_video(&mut self) -> Option<CaptureSource> {
        // Stop the old capture before starting the new one.
        self.video = None;
        match video::VideoPipeline::start(self.source.as_ref(), self.sink.clone(), self.has_audio, self.settings, self.control.is_some()) {
            Ok(v) => {
                self.video = Some(v);
                self.source.clone()
            }
            Err(e) => {
                tracing::warn!("restarting capture: {e}");
                self.video = video::VideoPipeline::start(None, self.sink.clone(), self.has_audio, self.settings, self.control.is_some()).ok();
                self.source = None;
                None
            }
        }
    }
}

/// Remote control state of a session.
struct Control {
    clipboard: clipboard::ClipboardSync,
    #[cfg(windows)]
    injector: input_windows::Injector,
    #[cfg(target_os = "linux")]
    injector: linux::Injector,
    #[cfg(target_os = "macos")]
    injector: input_macos::Injector,
}

impl Control {
    fn new(sink: &BroadcastSink, monitor: Option<usize>) -> Self {
        let clip_sink = sink.clone();
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        let _ = monitor;
        Self {
            clipboard: clipboard::ClipboardSync::start(move |text| clip_sink.clipboard(&text)),
            #[cfg(windows)]
            injector: input_windows::Injector::new(monitor.and_then(input_windows::monitor_target)),
            #[cfg(target_os = "linux")]
            injector: linux::Injector::new(monitor),
            #[cfg(target_os = "macos")]
            injector: input_macos::Injector::new(monitor),
        }
    }
}

/// Captures the local screen for Ropeway sessions.
#[derive(Default)]
pub struct DesktopScreenProvider {
    running: Mutex<HashMap<String, Running>>,
}

impl ScreenProvider for DesktopScreenProvider {
    fn start(&self, session_id: &str, options: &CaptureOptions, sink: BroadcastSink) -> Result<()> {
        let wants_audio = options.system_audio || options.microphone;
        let audio = if wants_audio {
            match audio::AudioPipeline::start(options.system_audio, options.microphone, sink.clone()) {
                Ok(a) => Some(a),
                Err(e) => {
                    tracing::warn!("sharing without sound: {e}");
                    None
                }
            }
        } else {
            None
        };
        let has_audio = audio.is_some();
        let video = video::VideoPipeline::start(options.source.as_ref(), sink.clone(), has_audio, options.video, options.control)?;
        let control = options
            .control
            .then(|| Control::new(&sink, video::resolve_monitor(options.source.as_ref())));
        self.running.lock().unwrap().insert(
            session_id.to_owned(),
            Running {
                video: Some(video),
                _audio: audio,
                sink,
                has_audio,
                source: options.source.clone(),
                settings: options.video,
                control,
            },
        );
        Ok(())
    }

    fn stop(&self, session_id: &str) {
        // Dropping the pipelines stops capture and joins their threads;
        // dropping the injector releases anything still held.
        let removed = self.running.lock().unwrap().remove(session_id);
        drop(removed);
    }

    fn input(&self, session_id: &str, event: &InputEvent) {
        let mut running = self.running.lock().unwrap();
        let Some(r) = running.get_mut(session_id) else {
            tracing::debug!("input for unknown session {session_id}");
            return;
        };
        // The core only forwards this from a controller or from the device
        // the share was started for.
        if let InputEvent::SetVideo { video } = event {
            r.settings = video.clamped();
            r.restart_video();
            return;
        }
        if r.control.is_none() {
            tracing::debug!("input ignored: session {session_id} is not a control session");
            return;
        }
        tracing::debug!("applying {event:?}");
        match event {
            InputEvent::SwitchMonitor { id } => {
                r.source = Some(CaptureSource { kind: "monitor".into(), id: id.clone(), label: String::new() });
                let shown = r.restart_video();
                #[cfg(windows)]
                if let Some(c) = r.control.as_mut() {
                    let handle = video::resolve_monitor(shown.as_ref());
                    c.injector.set_target(handle.and_then(input_windows::monitor_target));
                }
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                if let Some(c) = r.control.as_mut() {
                    c.injector.set_target(video::resolve_monitor(shown.as_ref()));
                }
                #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
                let _ = shown;
            }
            InputEvent::Clipboard { text } => {
                if let Some(c) = &r.control {
                    c.clipboard.set(text);
                }
            }
            #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
            other => {
                if let Some(c) = r.control.as_mut() {
                    c.injector.apply(other);
                }
            }
            #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
            _ => {}
        }
    }
}
