//! Between a capture backend and the encoder: the backend drops the newest
//! frame into a [`Slot`], a separate encoder thread picks it up. That keeps
//! capture smooth when encoding is slow, and lets a static screen be
//! re-encoded as a keyframe when a new viewer joins.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use ropeway_core::screen::VideoSettings;
use ropeway_core::{BroadcastSink, MonitorChoice, StreamInfo};

use crate::encoder::{encoded_size, H264Encoder};

const KEYFRAME_INTERVAL: Duration = Duration::from_secs(10);

/// One captured picture, 32-bit BGRA/BGRX rows of `stride` bytes.
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub data: Vec<u8>,
}

pub struct Slot {
    /// Time between captured frames.
    pub interval: Duration,
    frame: Mutex<Option<RawFrame>>,
    ready: Condvar,
    pub stop: AtomicBool,
    /// The capture source went away (window closed, portal session ended).
    ended: AtomicBool,
}

impl Slot {
    pub fn new(fps: u32) -> Self {
        Self {
            interval: Duration::from_secs_f64(1.0 / fps.max(1) as f64),
            frame: Mutex::new(None),
            ready: Condvar::new(),
            stop: AtomicBool::new(false),
            ended: AtomicBool::new(false),
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Stores a frame, filled by `fill` into a reused buffer.
    pub fn put(&self, width: u32, height: u32, stride: u32, fill: impl FnOnce(&mut Vec<u8>)) {
        let mut slot = self.frame.lock().unwrap();
        // Reuse the previous allocation when the encoder has not taken it.
        let mut data = slot.take().map(|f| f.data).unwrap_or_default();
        data.clear();
        fill(&mut data);
        *slot = Some(RawFrame { width, height, stride, data });
        self.ready.notify_one();
    }

    pub fn end(&self) {
        self.ended.store(true, Ordering::Relaxed);
        self.ready.notify_all();
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.ready.notify_all();
    }
}

/// Encodes frames from `slot` and sends them to `sink` until stopped.
pub fn encode_loop(
    slot: &Slot,
    sink: &BroadcastSink,
    audio: bool,
    monitors: &[MonitorChoice],
    current: Option<String>,
    video: VideoSettings,
) {
    let started = Instant::now();
    let mut encoder: Option<H264Encoder> = None;
    let mut last: Option<RawFrame> = None;
    let mut want_key = true;
    let mut last_key = Instant::now();
    loop {
        let fresh = {
            let guard = slot.frame.lock().unwrap();
            let (mut guard, _) = slot
                .ready
                .wait_timeout_while(guard, Duration::from_millis(200), |f| {
                    f.is_none() && !slot.stop.load(Ordering::Relaxed) && !slot.ended.load(Ordering::Relaxed)
                })
                .unwrap();
            guard.take()
        };
        if slot.stop.load(Ordering::Relaxed) || sink.is_closed() {
            break;
        }
        if slot.ended.load(Ordering::Relaxed) {
            sink.end();
            break;
        }
        want_key |= sink.take_keyframe_request();
        // A periodic keyframe lets a viewer whose decoder reset recover.
        want_key |= last_key.elapsed() >= KEYFRAME_INTERVAL;
        let has_fresh = fresh.is_some();
        if let Some(f) = fresh {
            last = Some(f);
        }
        // Without a new frame, re-encode the last one only for a keyframe.
        if !has_fresh && !(want_key && last.is_some()) {
            continue;
        }
        if !sink.has_viewers() {
            continue;
        }
        let Some(frame) = last.as_ref() else { continue };

        let (w, h) = encoded_size(frame.width, frame.height, video.resolution);
        if w < 16 || h < 16 {
            continue;
        }
        if encoder.as_ref().map(H264Encoder::size) != Some((w, h)) {
            match H264Encoder::new(w, h, video) {
                Ok(e) => {
                    encoder = Some(e);
                    sink.info(&StreamInfo {
                        width: w,
                        height: h,
                        audio,
                        monitors: monitors.to_vec(),
                        monitor: current.clone(),
                        video,
                    });
                    want_key = true;
                }
                Err(e) => {
                    tracing::error!("{e}");
                    sink.end();
                    break;
                }
            }
        }
        let enc = encoder.as_mut().unwrap();
        match enc.encode(&frame.data, frame.width, frame.height, frame.stride, want_key) {
            Ok(Some(out)) => {
                if out.keyframe {
                    want_key = false;
                    last_key = Instant::now();
                }
                sink.video(out.keyframe, started.elapsed().as_micros() as u64, &out.data);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("{e}"),
        }
    }
}
