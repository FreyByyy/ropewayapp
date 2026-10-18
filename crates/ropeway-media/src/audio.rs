//! System sound (loopback of the default output) and microphone, mixed to
//! 48 kHz stereo signed 16-bit PCM in 20 ms chunks.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use ropeway_core::BroadcastSink;

pub const SAMPLE_RATE: u32 = 48_000;
const CHUNK_FRAMES: usize = 960; // 20 ms
/// Keep at most this much buffered per input; older audio is dropped so the
/// delay never grows.
const MAX_BUFFERED_FRAMES: usize = SAMPLE_RATE as usize / 4;

/// Linear resampler to 48 kHz, streaming over arbitrary block sizes.
struct Resampler {
    ratio: f64,
    pos: f64,
    prev: [f32; 2],
}

impl Resampler {
    fn new(input_rate: u32) -> Self {
        Self { ratio: input_rate as f64 / SAMPLE_RATE as f64, pos: 0.0, prev: [0.0; 2] }
    }

    fn process(&mut self, input: &[[f32; 2]], out: &mut VecDeque<f32>) {
        if input.is_empty() {
            return;
        }
        if (self.ratio - 1.0).abs() < f64::EPSILON {
            for f in input {
                out.extend(f);
            }
            return;
        }
        // Index 0 is the last frame of the previous block, 1.. this block.
        let n = input.len();
        let at = |i: usize| if i == 0 { self.prev } else { input[i - 1] };
        while self.pos + 1.0 <= n as f64 {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push_back(a[0] + (b[0] - a[0]) * frac);
            out.push_back(a[1] + (b[1] - a[1]) * frac);
            self.pos += self.ratio;
        }
        self.pos -= n as f64;
        self.prev = input[n - 1];
    }
}

struct Input {
    buffer: VecDeque<f32>,
    resampler: Resampler,
}

type Inputs = Arc<Mutex<Vec<Input>>>;

pub struct AudioPipeline {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioPipeline {
    pub fn start(system: bool, microphone: bool, sink: BroadcastSink) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let stop_flag = stop.clone();
        // cpal streams are not `Send` on every platform: they live and die
        // on this thread.
        let thread = std::thread::Builder::new()
            .name("ropeway-audio".into())
            .spawn(move || {
                let inputs: Inputs = Arc::new(Mutex::new(Vec::new()));
                let streams = match open_streams(system, microphone, &inputs) {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                mix_loop(&inputs, &sink, &stop_flag);
                drop(streams);
            })?;
        ready_rx.recv().map_err(|_| anyhow!("audio thread failed"))??;
        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open_streams(system: bool, microphone: bool, inputs: &Inputs) -> Result<Vec<Stream>> {
    let host = cpal::default_host();
    let mut streams = Vec::new();
    if system {
        // On Windows an input stream on an output device records what it plays.
        match host.default_output_device() {
            Some(device) => match device.default_output_config() {
                Ok(config) => match open(&device, config, inputs) {
                    Ok(s) => streams.push(s),
                    Err(e) => tracing::warn!("system sound unavailable: {e}"),
                },
                Err(e) => tracing::warn!("system sound unavailable: {e}"),
            },
            None => tracing::warn!("no output device for system sound"),
        }
    }
    if microphone {
        match host.default_input_device() {
            Some(device) => match device.default_input_config() {
                Ok(config) => match open(&device, config, inputs) {
                    Ok(s) => streams.push(s),
                    Err(e) => tracing::warn!("microphone unavailable: {e}"),
                },
                Err(e) => tracing::warn!("microphone unavailable: {e}"),
            },
            None => tracing::warn!("no microphone"),
        }
    }
    if streams.is_empty() {
        return Err(anyhow!("no audio source could be opened"));
    }
    for s in &streams {
        s.play()?;
    }
    Ok(streams)
}

fn open(device: &cpal::Device, config: cpal::SupportedStreamConfig, inputs: &Inputs) -> Result<Stream> {
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let channels = config.channels as usize;
    let index = {
        let mut list = inputs.lock().unwrap();
        list.push(Input { buffer: VecDeque::new(), resampler: Resampler::new(config.sample_rate) });
        list.len() - 1
    };
    let on_error = |e: cpal::Error| tracing::warn!("audio stream error: {e}");

    macro_rules! stream {
        ($t:ty, $to_f32:expr) => {{
            let inputs = inputs.clone();
            let mut frames: Vec<[f32; 2]> = Vec::new();
            device.build_input_stream::<$t, _, _>(
                config.clone(),
                move |data: &[$t], _| {
                    frames.clear();
                    for chunk in data.chunks(channels) {
                        let l = $to_f32(chunk[0]);
                        let r = if channels > 1 { $to_f32(chunk[1]) } else { l };
                        frames.push([l, r]);
                    }
                    let mut list = inputs.lock().unwrap();
                    let input = &mut list[index];
                    input.resampler.process(&frames, &mut input.buffer);
                    let max = MAX_BUFFERED_FRAMES * 2;
                    if input.buffer.len() > max {
                        let excess = input.buffer.len() - max;
                        input.buffer.drain(..excess);
                    }
                },
                on_error,
                None,
            )?
        }};
    }

    let stream = match format {
        SampleFormat::F32 => stream!(f32, |s: f32| s),
        SampleFormat::I16 => stream!(i16, |s: i16| s as f32 / 32768.0),
        SampleFormat::I32 => stream!(i32, |s: i32| s as f32 / 2147483648.0),
        SampleFormat::U16 => stream!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        other => return Err(anyhow!("unsupported sample format {other:?}")),
    };
    Ok(stream)
}

fn mix_loop(inputs: &Inputs, sink: &BroadcastSink, stop: &AtomicBool) {
    let started = Instant::now();
    let tick = Duration::from_millis(20);
    let mut next = Instant::now() + tick;
    let mut mixed = vec![0f32; CHUNK_FRAMES * 2];
    let mut pcm = Vec::with_capacity(CHUNK_FRAMES * 4);
    while !stop.load(Ordering::Relaxed) && !sink.is_closed() {
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        next += tick;
        if Instant::now() > next + tick * 5 {
            // Fell far behind (system was busy): resynchronize.
            next = Instant::now() + tick;
        }

        mixed.iter_mut().for_each(|s| *s = 0.0);
        {
            let mut list = inputs.lock().unwrap();
            for input in list.iter_mut() {
                let n = input.buffer.len().min(mixed.len());
                for (i, s) in input.buffer.drain(..n).enumerate() {
                    mixed[i] += s;
                }
            }
        }
        if !sink.has_viewers() {
            continue;
        }
        pcm.clear();
        for s in &mixed {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            pcm.extend_from_slice(&v.to_le_bytes());
        }
        sink.audio(started.elapsed().as_micros() as u64, &pcm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_keeps_rate() {
        let mut r = Resampler::new(44_100);
        let mut out = VecDeque::new();
        // One second in uneven blocks.
        let block: Vec<[f32; 2]> = (0..441).map(|i| [i as f32 / 441.0, 0.0]).collect();
        for _ in 0..100 {
            r.process(&block, &mut out);
        }
        let frames = out.len() / 2;
        assert!((47_990..=48_010).contains(&frames), "{frames}");
    }

    #[test]
    fn same_rate_passes_through() {
        let mut r = Resampler::new(48_000);
        let mut out = VecDeque::new();
        r.process(&[[0.5, -0.5]; 10], &mut out);
        assert_eq!(out.len(), 20);
        assert_eq!(out[0], 0.5);
        assert_eq!(out[1], -0.5);
    }
}
