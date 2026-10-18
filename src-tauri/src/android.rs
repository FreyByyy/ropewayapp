//! Android side of the app, reached over JNI: the Kotlin `RopewayBridge`
//! captures and encodes the phone screen (MediaProjection + MediaCodec),
//! picks and opens files, and keeps Wi-Fi broadcasts coming in.
//!
//! Kotlin hands its bridge object to [`attach`] when the activity starts;
//! Rust calls its methods. Answers that need the user (consent dialogs, the
//! file picker) come back through `onResult` with the id they were asked with.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail, Result};
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};
use ropeway_core::screen::{Quality, Resolution, StreamInfo, VideoSettings};
use ropeway_core::{BroadcastSink, CaptureOptions, InputEvent, ScreenProvider};
use serde::Serialize;
use tokio::sync::oneshot;

static VM: OnceLock<JavaVM> = OnceLock::new();
static BRIDGE: Mutex<Option<GlobalRef>> = Mutex::new(None);
static PENDING: Mutex<Option<HashMap<i64, oneshot::Sender<Option<String>>>>> = Mutex::new(None);
static NEXT_ID: AtomicI64 = AtomicI64::new(1);
static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

struct Capture {
    session: String,
    sink: BroadcastSink,
}

// ---- calls into Kotlin ------------------------------------------------------

fn with_bridge<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Result<T> {
    let vm = VM.get().ok_or_else(|| anyhow!("Android bridge is not ready"))?;
    let bridge = BRIDGE.lock().unwrap().clone().ok_or_else(|| anyhow!("Android bridge is not ready"))?;
    // Threads stay attached: input events arrive often.
    let mut env = vm.attach_current_thread_permanently()?;
    let out = f(&mut env, bridge.as_obj());
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
        bail!("Android call failed");
    }
    Ok(out?)
}

/// Calls `method(id)` on the bridge and waits for `onResult(id, value)`.
async fn ask(method: &str) -> Result<Option<String>> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    PENDING.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, tx);
    if let Err(e) = with_bridge(|env, b| env.call_method(b, method, "(J)V", &[JValue::Long(id)]).map(|_| ())) {
        PENDING.lock().unwrap().as_mut().map(|p| p.remove(&id));
        return Err(e);
    }
    Ok(rx.await.unwrap_or(None))
}

fn call_void(method: &str) {
    if let Err(e) = with_bridge(|env, b| env.call_method(b, method, "()V", &[]).map(|_| ())) {
        tracing::warn!("{method}: {e}");
    }
}

/// Asks the user to allow screen capture. Must precede every share.
pub async fn request_capture() -> Result<bool> {
    Ok(ask("requestCapture").await?.as_deref() == Some("ok"))
}

/// Lets the user pick files; they are copied into the app's cache so the
/// transfer code can read them as ordinary files.
pub async fn pick_files() -> Result<Vec<String>> {
    match ask("pickFiles").await? {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(Vec::new()),
    }
}

/// Opens a file in the app the user chooses for it.
pub fn open_file(path: &str) -> Result<()> {
    call_with_path("openFile", path)
}

/// Hands a downloaded update to the system installer.
pub fn install_apk(path: &str) -> Result<()> {
    call_with_path("installApk", path)
}

/// Calls `method(path)`, which returns an error message or null.
fn call_with_path(method: &str, path: &str) -> Result<()> {
    let error = with_bridge(|env, b| {
        let p = env.new_string(path)?;
        let out = env.call_method(b, method, "(Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&p)])?.l()?;
        if out.is_null() {
            return Ok(None);
        }
        let s: String = env.get_string(&JString::from(out))?.into();
        Ok(Some(s))
    })?;
    match error {
        Some(e) => bail!(e),
        None => Ok(()),
    }
}

/// Whether Ropeway's accessibility service, needed for remote control, is on.
pub fn control_enabled() -> bool {
    with_bridge(|env, b| env.call_method(b, "controlEnabled", "()Z", &[])?.z()).unwrap_or(false)
}

/// Opens the system's accessibility settings, where the service is turned on.
pub fn open_control_settings() {
    call_void("openControlSettings");
}

/// Colors the status and navigation bar areas; `dark` picks light icons.
pub fn set_system_bars(color: &str, dark: bool) -> Result<()> {
    with_bridge(|env, b| {
        let c = env.new_string(color)?;
        env.call_method(b, "setSystemBars", "(Ljava/lang/String;Z)V", &[JValue::Object(&c), JValue::Bool(dark as u8)])
            .map(|_| ())
    })
}

fn screen_size() -> Option<(u32, u32)> {
    let s = with_bridge(|env, b| {
        let out = env.call_method(b, "screenSize", "()Ljava/lang/String;", &[])?.l()?;
        let s: String = env.get_string(&JString::from(out))?.into();
        Ok(s)
    })
    .ok()?;
    let (w, h) = s.split_once(',')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

// ---- calls from Kotlin ------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_app_ropeway_RopewayNative_attach(env: JNIEnv, _class: JClass, bridge: JObject) {
    if VM.get().is_none() {
        if let Ok(vm) = env.get_java_vm() {
            let _ = VM.set(vm);
        }
    }
    if let Ok(global) = env.new_global_ref(bridge) {
        *BRIDGE.lock().unwrap() = Some(global);
    }
}

#[no_mangle]
pub extern "system" fn Java_app_ropeway_RopewayNative_onResult(mut env: JNIEnv, _class: JClass, id: jlong, value: JString) {
    let value = if value.is_null() { None } else { env.get_string(&value).ok().map(String::from) };
    let tx = PENDING.lock().unwrap().as_mut().and_then(|p| p.remove(&id));
    if let Some(tx) = tx {
        let _ = tx.send(value);
    }
}

/// One encoded H.264 access unit (Annex B; keyframes carry SPS/PPS).
#[no_mangle]
pub extern "system" fn Java_app_ropeway_RopewayNative_onVideo(
    env: JNIEnv,
    _class: JClass,
    data: JByteArray,
    keyframe: jboolean,
    pts_us: jlong,
) {
    let Ok(bytes) = env.convert_byte_array(&data) else { return };
    let want_key = {
        let capture = CAPTURE.lock().unwrap();
        let Some(c) = capture.as_ref() else { return };
        if c.sink.is_closed() {
            return;
        }
        c.sink.video(keyframe != 0, pts_us.max(0) as u64, &bytes);
        c.sink.take_keyframe_request()
    };
    if want_key {
        call_void("requestKeyframe");
    }
}

/// Capture ended on the phone: the user stopped it from the notification,
/// or the system revoked it.
#[no_mangle]
pub extern "system" fn Java_app_ropeway_RopewayNative_onCaptureStopped(_env: JNIEnv, _class: JClass) {
    if let Some(c) = CAPTURE.lock().unwrap().take() {
        c.sink.end();
    }
}

/// The encoder's size changed (the phone was rotated).
#[no_mangle]
pub extern "system" fn Java_app_ropeway_RopewayNative_onCaptureSize(_env: JNIEnv, _class: JClass, width: jint, height: jint) {
    if let Some(c) = CAPTURE.lock().unwrap().as_ref() {
        c.sink.info(&StreamInfo {
            width: width as u32,
            height: height as u32,
            audio: false,
            monitors: Vec::new(),
            monitor: None,
            video: VideoSettings::default(),
        });
    }
}

// ---- screen sharing -----------------------------------------------------------

/// What the share dialog lists: the phone's screen.
#[derive(Serialize, Clone, Debug)]
pub struct SourceInfo {
    pub kind: String,
    pub id: String,
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub process: String,
}

pub fn list_sources() -> Vec<SourceInfo> {
    let (width, height) = screen_size().unwrap_or((1080, 2400));
    vec![SourceInfo {
        kind: "monitor".into(),
        id: "phone".into(),
        index: 1,
        name: String::new(),
        width,
        height,
        primary: true,
        process: String::new(),
    }]
}

/// Encoded size: the short side follows the chosen resolution ("auto" is
/// 720 on a phone), rounded to multiples of 16 as hardware encoders like.
fn encoded_size(width: u32, height: u32, resolution: Resolution) -> (u32, u32) {
    let short = width.min(height);
    let target = match resolution {
        Resolution::Native => short,
        Resolution::P1440 => 1440,
        Resolution::P1080 => 1080,
        Resolution::Auto | Resolution::P720 => 720,
    }
    .min(short);
    let scale = target as f64 / short as f64;
    let round = |v: u32| (((v as f64 * scale) as u32) / 16 * 16).max(16);
    (round(width), round(height))
}

fn bitrate(width: u32, height: u32, video: &VideoSettings) -> u32 {
    let bpp = match video.quality {
        Quality::Economy => 0.06,
        Quality::Standard => 0.1,
        Quality::High => 0.18,
    };
    ((width as f64 * height as f64 * video.fps as f64 * bpp) as u32).clamp(1_000_000, 20_000_000)
}

#[derive(Default)]
pub struct AndroidScreenProvider;

impl ScreenProvider for AndroidScreenProvider {
    fn start(&self, session_id: &str, options: &CaptureOptions, sink: BroadcastSink) -> Result<()> {
        let video = options.video.clamped();
        let (sw, sh) = screen_size().unwrap_or((1080, 2400));
        let (w, h) = encoded_size(sw, sh, video.resolution);
        if let Some(old) = CAPTURE.lock().unwrap().take() {
            old.sink.end();
        }
        sink.info(&StreamInfo { width: w, height: h, audio: false, monitors: Vec::new(), monitor: None, video });
        *CAPTURE.lock().unwrap() = Some(Capture { session: session_id.to_owned(), sink: sink.clone() });
        let started = with_bridge(|env, b| {
            env.call_method(
                b,
                "startCapture",
                "(IIII)Z",
                &[
                    JValue::Int(w as i32),
                    JValue::Int(h as i32),
                    JValue::Int(video.fps as i32),
                    JValue::Int(bitrate(w, h, &video) as i32),
                ],
            )?
            .z()
        });
        if !matches!(started, Ok(true)) {
            CAPTURE.lock().unwrap().take();
            bail!("screen capture was not allowed");
        }
        Ok(())
    }

    fn stop(&self, session_id: &str) {
        let mut capture = CAPTURE.lock().unwrap();
        if capture.as_ref().is_some_and(|c| c.session == session_id) {
            capture.take();
            drop(capture);
            call_void("stopCapture");
        }
    }

    /// Taps, swipes and text from a controller, performed by the Kotlin
    /// accessibility service (when the user has turned it on).
    fn input(&self, _session_id: &str, event: &InputEvent) {
        if matches!(event, InputEvent::SetVideo { .. } | InputEvent::SwitchMonitor { .. } | InputEvent::Clipboard { .. }) {
            return;
        }
        let Ok(json) = serde_json::to_string(event) else { return };
        let result = with_bridge(|env, b| {
            let j = env.new_string(&json)?;
            env.call_method(b, "input", "(Ljava/lang/String;)V", &[JValue::Object(&j)]).map(|_| ())
        });
        if let Err(e) = result {
            tracing::debug!("input: {e}");
        }
    }
}
