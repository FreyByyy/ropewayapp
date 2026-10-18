//! BGRA frames → H.264 (Annex B). Portable: used with any capture backend.

use std::ffi::c_void;

use anyhow::{anyhow, Result};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, FrameType, RateControlMode, UsageType, VuiConfig};
use openh264::formats::YUVSource;
use openh264::OpenH264API;
use openh264_sys2::{SEncParamExt, ENCODER_OPTION_SVC_ENCODE_PARAM_EXT, SM_FIXEDSLCNUM_SLICE};
use ropeway_core::screen::{Quality, Resolution, VideoSettings};
use yuv::{bgra_to_yuv420, YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange, YuvStandardMatrix};

/// In `Auto` mode, frames wider than this are scaled down…
pub const AUTO_MAX_WIDTH: u32 = 2560;
/// …to this width (4K screens become 1920 wide).
pub const AUTO_SCALED_WIDTH: u32 = 1920;

struct Planar(YuvPlanarImageMut<'static, u8>);

impl YUVSource for Planar {
    fn dimensions(&self) -> (usize, usize) {
        (self.0.width as usize, self.0.height as usize)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.0.y_stride as usize, self.0.u_stride as usize, self.0.v_stride as usize)
    }

    fn y(&self) -> &[u8] {
        self.0.y_plane.borrow()
    }

    fn u(&self) -> &[u8] {
        self.0.u_plane.borrow()
    }

    fn v(&self) -> &[u8] {
        self.0.v_plane.borrow()
    }
}

pub struct EncodedFrame {
    pub keyframe: bool,
    pub data: Vec<u8>,
}

pub struct H264Encoder {
    encoder: Encoder,
    width: u32,
    height: u32,
    settings: VideoSettings,
    planar: Planar,
    scaled: Vec<u8>,
    scaler: Option<Scaler>,
    sliced: bool,
}

/// Size actually encoded for a captured frame, rounded down to even
/// numbers as 4:2:0 requires. Never scales up.
pub fn encoded_size(width: u32, height: u32, resolution: Resolution) -> (u32, u32) {
    let target_height = match resolution {
        Resolution::Auto if width > AUTO_MAX_WIDTH => {
            (height as u64 * AUTO_SCALED_WIDTH as u64 / width as u64) as u32
        }
        Resolution::Auto | Resolution::Native => height,
        Resolution::P1440 => 1440,
        Resolution::P1080 => 1080,
        Resolution::P720 => 720,
    };
    let (w, h) = if target_height < height {
        ((width as u64 * target_height as u64 / height as u64) as u32, target_height)
    } else {
        (width, height)
    };
    // OpenH264 does not encode above 3840x2160.
    let (w, h) = if w > 3840 || h > 2160 {
        let s = (3840.0 / w as f64).min(2160.0 / h as f64);
        ((w as f64 * s) as u32, (h as f64 * s) as u32)
    } else {
        (w, h)
    };
    (w & !1, h & !1)
}

fn encoder_threads() -> u16 {
    std::thread::available_parallelism().map(|n| n.get().clamp(1, 8) as u16).unwrap_or(4)
}

fn bitrate_for(width: u32, height: u32, settings: &VideoSettings) -> u32 {
    // Screen content compresses well; bits per pixel per frame by quality.
    let bpp = match settings.quality {
        Quality::Economy => 0.05,
        Quality::Standard => 0.09,
        Quality::High => 0.16,
    };
    let bps = width as f64 * height as f64 * settings.fps as f64 * bpp;
    (bps as u32).clamp(1_500_000, 40_000_000)
}

impl H264Encoder {
    /// `width`/`height` are the encoded size (see [`encoded_size`]).
    pub fn new(width: u32, height: u32, settings: VideoSettings) -> Result<Self> {
        let settings = settings.clamped();
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_for(width, height, &settings)))
            .max_frame_rate(FrameRate::from_hz(settings.fps as f32))
            .skip_frames(true)
            .num_threads(encoder_threads())
            // Not supported for screen content; set explicitly to avoid warnings.
            .adaptive_quantization(false)
            .background_detection(false)
            .vui(VuiConfig::bt601());
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| anyhow!("H.264 encoder: {e}"))?;
        Ok(Self {
            encoder,
            width,
            height,
            settings,
            planar: Planar(YuvPlanarImageMut::alloc(width, height, YuvChromaSubsampling::Yuv420)),
            scaled: Vec::new(),
            scaler: None,
            sliced: false,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn settings(&self) -> VideoSettings {
        self.settings
    }

    /// Encodes a BGRA frame of `src_w`×`src_h` pixels with `stride` bytes per
    /// row. Returns `None` when the encoder skipped the frame.
    pub fn encode(&mut self, bgra: &[u8], src_w: u32, src_h: u32, stride: u32, force_keyframe: bool) -> Result<Option<EncodedFrame>> {
        let needs_scale = src_w & !1 != self.width || src_h & !1 != self.height;
        let (src, src_stride) = if needs_scale {
            let scaler = match &self.scaler {
                Some(s) if s.matches(src_w, src_h, self.width, self.height) => s,
                _ => self.scaler.insert(Scaler::new(src_w, src_h, self.width, self.height)),
            };
            scaler.scale(bgra, stride, &mut self.scaled);
            (&self.scaled[..], self.width * 4)
        } else {
            (bgra, stride)
        };
        bgra_to_yuv420(
            &mut self.planar.0,
            src,
            src_stride,
            YuvRange::Limited,
            YuvStandardMatrix::Bt601,
            YuvConversionMode::Balanced,
        )
        .map_err(|e| anyhow!("color conversion: {e:?}"))?;
        if force_keyframe {
            self.encoder.force_intra_frame();
        }
        let (keyframe, data) = {
            let bitstream = self.encoder.encode(&self.planar).map_err(|e| anyhow!("encode: {e}"))?;
            (matches!(bitstream.frame_type(), FrameType::IDR | FrameType::I), bitstream.to_vec())
        };
        if !self.sliced {
            self.sliced = true;
            self.enable_slices();
        }
        if data.is_empty() {
            return Ok(None);
        }
        Ok(Some(EncodedFrame { keyframe, data }))
    }

    /// The `openh264` crate always encodes a frame as one slice, which keeps
    /// the encoder on one core. After the first frame (the encoder is
    /// initialized lazily) switch to one slice per thread.
    fn enable_slices(&mut self) {
        let threads = encoder_threads() as u32;
        if threads < 2 {
            return;
        }
        unsafe {
            let api = self.encoder.raw_api();
            let mut params = SEncParamExt::default();
            if api.get_option(ENCODER_OPTION_SVC_ENCODE_PARAM_EXT, &mut params as *mut _ as *mut c_void) != 0 {
                tracing::debug!("encoder: could not read parameters, staying single-threaded");
                return;
            }
            params.iMultipleThreadIdc = threads as u16;
            params.sSpatialLayers[0].sSliceArgument.uiSliceMode = SM_FIXEDSLCNUM_SLICE;
            params.sSpatialLayers[0].sSliceArgument.uiSliceNum = threads;
            if api.set_option(ENCODER_OPTION_SVC_ENCODE_PARAM_EXT, &mut params as *mut _ as *mut c_void) != 0 {
                tracing::debug!("encoder: multi-slice mode refused, staying single-threaded");
            }
        }
    }
}

/// Bilinear BGRA downscaler with precomputed coefficients, run on several
/// threads in horizontal bands.
struct Scaler {
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    /// Per destination column: byte offsets of the two source pixels and the
    /// weight of the second (0..=256).
    cols: Vec<(u32, u32, u32)>,
    /// Per destination row: the two source rows and the weight of the second.
    rows: Vec<(u32, u32, u32)>,
}

impl Scaler {
    fn new(sw: u32, sh: u32, dw: u32, dh: u32) -> Self {
        let axis = |src: u32, dst: u32| -> Vec<(u32, u32, u32)> {
            (0..dst)
                .map(|i| {
                    // Sample at pixel centers.
                    let pos = ((i as f64 + 0.5) * src as f64 / dst as f64 - 0.5).max(0.0);
                    let p0 = (pos as u32).min(src - 1);
                    let p1 = (p0 + 1).min(src - 1);
                    let w = ((pos - p0 as f64) * 256.0).round().clamp(0.0, 256.0) as u32;
                    (p0, p1, w)
                })
                .collect()
        };
        let cols = axis(sw, dw).into_iter().map(|(a, b, w)| (a * 4, b * 4, w)).collect();
        let rows = axis(sh, dh);
        Self { sw, sh, dw, dh, cols, rows }
    }

    fn matches(&self, sw: u32, sh: u32, dw: u32, dh: u32) -> bool {
        (self.sw, self.sh, self.dw, self.dh) == (sw, sh, dw, dh)
    }

    fn scale(&self, src: &[u8], stride: u32, out: &mut Vec<u8>) {
        let row_bytes = self.dw as usize * 4;
        out.resize(row_bytes * self.dh as usize, 0);
        let threads = encoder_threads() as usize;
        let band = (self.dh as usize).div_ceil(threads).max(1);
        let stride = stride as usize;
        std::thread::scope(|scope| {
            for (b, chunk) in out.chunks_mut(band * row_bytes).enumerate() {
                let rows = &self.rows[b * band..(b * band + chunk.len() / row_bytes)];
                let cols = &self.cols;
                scope.spawn(move || {
                    for (dst, &(r0, r1, wy)) in chunk.chunks_mut(row_bytes).zip(rows) {
                        let a = &src[r0 as usize * stride..];
                        let c = &src[r1 as usize * stride..];
                        let iy = 256 - wy;
                        for (px, &(x0, x1, wx)) in dst.chunks_exact_mut(4).zip(cols) {
                            let (x0, x1) = (x0 as usize, x1 as usize);
                            let ix = 256 - wx;
                            for ch in 0..4 {
                                let top = a[x0 + ch] as u32 * ix + a[x1 + ch] as u32 * wx;
                                let bottom = c[x0 + ch] as u32 * ix + c[x1 + ch] as u32 * wx;
                                px[ch] = ((top * iy + bottom * wy + 32768) >> 16) as u8;
                            }
                        }
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::decoder::Decoder;

    fn test_frame(w: u32, h: u32, shift: u32) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                v[i] = ((x + shift) % 256) as u8;
                v[i + 1] = (y % 256) as u8;
                v[i + 2] = ((x + y) % 256) as u8;
                v[i + 3] = 255;
            }
        }
        v
    }

    fn settings(fps: u32, resolution: Resolution) -> VideoSettings {
        VideoSettings { fps, resolution, quality: Quality::Standard }
    }

    #[test]
    fn sizes() {
        assert_eq!(encoded_size(1281, 721, Resolution::Auto), (1280, 720));
        assert_eq!(encoded_size(3840, 2160, Resolution::Auto), (1920, 1080));
        assert_eq!(encoded_size(3840, 2160, Resolution::Native), (3840, 2160));
        assert_eq!(encoded_size(2560, 1440, Resolution::P1080), (1920, 1080));
        assert_eq!(encoded_size(2560, 1440, Resolution::P720), (1280, 720));
        // Never upscale.
        assert_eq!(encoded_size(1280, 720, Resolution::P1440), (1280, 720));
        // Odd results are made even.
        assert_eq!(encoded_size(1366, 768, Resolution::P720), (1280, 720));
    }

    #[test]
    fn encodes_decodable_stream_with_keyframes_on_demand() {
        let (w, h) = encoded_size(1281, 721, Resolution::Auto);
        let mut enc = H264Encoder::new(w, h, settings(30, Resolution::Auto)).unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut decoded = 0;
        for i in 0..10 {
            let frame = test_frame(1281, 721, i * 3);
            let out = enc.encode(&frame, 1281, 721, 1281 * 4, i == 5).unwrap();
            let Some(out) = out else { continue };
            if i == 0 || i == 5 {
                assert!(out.keyframe, "frame {i} should be a keyframe");
                // Annex B with SPS (NAL type 7) up front.
                assert_eq!(&out.data[..4], &[0, 0, 0, 1]);
                assert_eq!(out.data[4] & 0x1f, 7);
            }
            if let Ok(Some(yuv)) = dec.decode(&out.data) {
                assert_eq!(yuv.dimensions(), (1280, 720));
                decoded += 1;
            }
        }
        assert!(decoded >= 8, "decoded {decoded} frames");
    }

    #[test]
    fn scaled_frames_decode_at_the_target_size() {
        for (res, expect) in [(Resolution::Auto, (1920, 1080)), (Resolution::P720, (1280, 720))] {
            let (w, h) = encoded_size(3840, 2160, res);
            assert_eq!((w, h), expect);
            let mut enc = H264Encoder::new(w, h, settings(30, res)).unwrap();
            let mut dec = Decoder::new().unwrap();
            for i in 0..3 {
                let frame = test_frame(3840, 2160, i);
                // Rate control may skip a frame; the forced keyframe always comes out.
                let Some(out) = enc.encode(&frame, 3840, 2160, 3840 * 4, i == 0).unwrap() else {
                    assert!(i > 0, "keyframe was skipped");
                    continue;
                };
                let yuv = dec.decode(&out.data).unwrap().unwrap();
                assert_eq!(yuv.dimensions(), (w as usize, h as usize));
            }
        }
    }

    #[test]
    fn frames_after_the_first_use_several_slices() {
        if encoder_threads() < 2 {
            return;
        }
        let mut enc = H264Encoder::new(1280, 720, settings(30, Resolution::Auto)).unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut slices_per_frame = Vec::new();
        for i in 0..4 {
            let out = enc.encode(&test_frame(1280, 720, i * 5), 1280, 720, 1280 * 4, i == 2).unwrap().unwrap();
            // NAL types 1 and 5 are coded slices.
            let slices = out
                .data
                .windows(5)
                .filter(|w| w[..4] == [0, 0, 0, 1] && matches!(w[4] & 0x1f, 1 | 5))
                .count();
            slices_per_frame.push(slices);
            assert!(dec.decode(&out.data).unwrap().is_some());
        }
        assert!(slices_per_frame[2..].iter().all(|&n| n > 1), "{slices_per_frame:?}");
    }

    #[test]
    fn scaler_keeps_flat_color() {
        let src = [10u8, 20, 30, 255].repeat(100 * 50);
        let s = Scaler::new(100, 50, 37, 19);
        let mut out = Vec::new();
        s.scale(&src, 400, &mut out);
        assert_eq!(out.len(), 37 * 19 * 4);
        assert!(out.chunks(4).all(|p| p == [10, 20, 30, 255]));
    }

    /// `cargo test --release -p ropeway-media encoding_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn encoding_speed() {
        for (w, h, res) in [
            (1920, 1080, Resolution::Auto),
            (2560, 1440, Resolution::Auto),
            (2560, 1440, Resolution::P1080),
            (3840, 2160, Resolution::Auto),
        ] {
            let (ew, eh) = encoded_size(w, h, res);
            let mut enc = H264Encoder::new(ew, eh, settings(60, res)).unwrap();
            let n = 90;

            // Typical desktop: a small area changes between frames.
            let mut frame = test_frame(w, h, 0);
            enc.encode(&frame, w, h, w * 4, true).unwrap();
            let start = std::time::Instant::now();
            for i in 0..n {
                for y in 100..300u32 {
                    for x in 100..500u32 {
                        let p = ((y * w + x) * 4) as usize;
                        frame[p] = frame[p].wrapping_add(i as u8 * 13);
                    }
                }
                enc.encode(&frame, w, h, w * 4, false).unwrap();
            }
            let small = start.elapsed() / n as u32;

            // Worst case: every pixel changes.
            let frames: Vec<Vec<u8>> = (0..4).map(|i| test_frame(w, h, i * 7)).collect();
            let start = std::time::Instant::now();
            for i in 0..n as usize {
                enc.encode(&frames[i % 4], w, h, w * 4, false).unwrap();
            }
            let full = start.elapsed() / n as u32;
            println!("{w}x{h} -> {ew}x{eh}: {small:?}/frame small change, {full:?}/frame everything changes");
        }
    }
}
