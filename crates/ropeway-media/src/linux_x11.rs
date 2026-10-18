//! Linux on X11: screen capture through MIT-SHM (plain GetImage when that
//! is unavailable) with the pointer drawn in from XFixes, and input through
//! XTest.

use std::collections::HashSet;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::{anyhow, Context as _, Result};
use ropeway_core::InputEvent;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _, ImageFormat};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::frames::Slot;
use crate::keymap::{evdev_code, keysym_for_char};

/// A monitor (RandR) in root window coordinates.
#[derive(Clone, Debug)]
pub struct Monitor {
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub primary: bool,
}

fn connect() -> Result<(RustConnection, xproto::Window)> {
    let (conn, screen) = RustConnection::connect(None).context("connecting to the X server")?;
    let root = conn.setup().roots[screen].root;
    Ok((conn, root))
}

/// Monitors, primary first; the whole screen if RandR knows none.
pub fn monitors() -> Vec<Monitor> {
    let Ok((conn, root)) = connect() else { return Vec::new() };
    let mut list: Vec<Monitor> = conn
        .randr_get_monitors(root, true)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| {
            r.monitors
                .iter()
                .map(|m| Monitor {
                    name: conn
                        .get_atom_name(m.name)
                        .ok()
                        .and_then(|c| c.reply().ok())
                        .map(|a| String::from_utf8_lossy(&a.name).into_owned())
                        .unwrap_or_default(),
                    x: m.x,
                    y: m.y,
                    width: m.width,
                    height: m.height,
                    primary: m.primary,
                })
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        let s = &conn.setup().roots[0];
        list.push(Monitor {
            name: "screen".into(),
            x: 0,
            y: 0,
            width: s.width_in_pixels,
            height: s.height_in_pixels,
            primary: true,
        });
    }
    if !list.iter().any(|m| m.primary) {
        list[0].primary = true;
    }
    list.sort_by_key(|m| !m.primary);
    list
}

/// Shared memory segment the X server writes screenshots into.
struct Shm {
    seg: u32,
    addr: *mut u8,
    size: usize,
}

impl Shm {
    fn new(conn: &RustConnection, size: usize) -> Result<Self> {
        unsafe {
            let id = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600);
            if id < 0 {
                return Err(anyhow!("shmget failed"));
            }
            let addr = libc::shmat(id, std::ptr::null(), 0);
            if addr as isize == -1 {
                libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                return Err(anyhow!("shmat failed"));
            }
            let seg = conn.generate_id()?;
            let attached = conn.shm_attach(seg, id as u32, false).map_err(anyhow::Error::from).and_then(|c| Ok(c.check()?));
            // Freed once both sides detach.
            libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
            if let Err(e) = attached {
                libc::shmdt(addr);
                return Err(e);
            }
            Ok(Self { seg, addr: addr as *mut u8, size })
        }
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.addr, self.size) }
    }

    fn free(self, conn: &RustConnection) {
        let _ = conn.shm_detach(self.seg);
        let _ = conn.flush();
        unsafe {
            libc::shmdt(self.addr as *const libc::c_void);
        }
    }
}

/// Captures `monitor` into `slot` until the slot is stopped.
pub fn start_capture(slot: Arc<Slot>, monitor: Monitor) -> Result<JoinHandle<()>> {
    let (conn, root) = connect()?;
    let cursor = conn
        .xfixes_query_version(5, 0)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some();
    Ok(std::thread::Builder::new().name("ropeway-x11-capture".into()).spawn(move || {
        let (w, h) = (monitor.width, monitor.height);
        let size = w as usize * h as usize * 4;
        let shm = Shm::new(&conn, size).map_err(|e| tracing::info!("X11 capture without MIT-SHM: {e}")).ok();
        let mut next = Instant::now();
        while !slot.stopped() {
            let via_shm = shm.as_ref().is_some_and(|s| {
                conn.shm_get_image(root, monitor.x, monitor.y, w, h, !0, ImageFormat::Z_PIXMAP.into(), s.seg, 0)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some()
            });
            let fallback;
            let source: &[u8] = match &shm {
                Some(s) if via_shm => s.bytes(),
                _ => match conn
                    .get_image(ImageFormat::Z_PIXMAP, root, monitor.x, monitor.y, w, h, !0)
                    .ok()
                    .and_then(|c| c.reply().ok())
                {
                    Some(r) => {
                        fallback = r.data;
                        &fallback
                    }
                    None => {
                        tracing::warn!("X11 capture failed");
                        slot.end();
                        break;
                    }
                },
            };
            if source.len() < size {
                tracing::warn!("unexpected X11 image size (depth other than 24/32 bits?)");
                slot.end();
                break;
            }
            let pointer = if cursor { conn.xfixes_get_cursor_image().ok().and_then(|c| c.reply().ok()) } else { None };
            slot.put(w as u32, h as u32, w as u32 * 4, |data| {
                data.extend_from_slice(&source[..size]);
                if let Some(p) = &pointer {
                    draw_cursor(data, w as i32, h as i32, p, monitor.x as i32, monitor.y as i32);
                }
            });
            next += slot.interval;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
        if let Some(s) = shm {
            s.free(&conn);
        }
    })?)
}

/// Blends the pointer (premultiplied ARGB) over a BGRX frame.
fn draw_cursor(frame: &mut [u8], fw: i32, fh: i32, c: &x11rb::protocol::xfixes::GetCursorImageReply, ox: i32, oy: i32) {
    let left = c.x as i32 - c.xhot as i32 - ox;
    let top = c.y as i32 - c.yhot as i32 - oy;
    for cy in 0..c.height as i32 {
        let y = top + cy;
        if y < 0 || y >= fh {
            continue;
        }
        for cx in 0..c.width as i32 {
            let x = left + cx;
            if x < 0 || x >= fw {
                continue;
            }
            let argb = c.cursor_image[(cy * c.width as i32 + cx) as usize];
            let a = argb >> 24;
            if a == 0 {
                continue;
            }
            let i = ((y * fw + x) * 4) as usize;
            let inv = 255 - a;
            for (k, shift) in [(0usize, 0u32), (1, 8), (2, 16)] {
                let src = (argb >> shift) & 0xff;
                frame[i + k] = (src + (frame[i + k] as u32 * inv) / 255).min(255) as u8;
            }
        }
    }
}

// ---- input ----

const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
const MOTION: u8 = 6;

pub struct Injector {
    conn: RustConnection,
    root: xproto::Window,
    target: Monitor,
    keys: HashSet<u8>,
    buttons: HashSet<u8>,
}

impl Injector {
    pub fn new(target: Monitor) -> Result<Self> {
        let (conn, root) = connect()?;
        conn.xtest_get_version(2, 2)?.reply().context("the X server has no XTest extension")?;
        Ok(Self { conn, root, target, keys: HashSet::new(), buttons: HashSet::new() })
    }

    pub fn set_target(&mut self, target: Monitor) {
        self.target = target;
    }

    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) {
        let _ = self.conn.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x, y, 0);
    }

    pub fn apply(&mut self, event: &InputEvent) {
        match event {
            InputEvent::MouseMove { x, y } => {
                let t = &self.target;
                let px = t.x as f32 + x.clamp(0.0, 1.0) * (t.width.max(1) - 1) as f32;
                let py = t.y as f32 + y.clamp(0.0, 1.0) * (t.height.max(1) - 1) as f32;
                self.fake(MOTION, 0, px.round() as i16, py.round() as i16);
            }
            InputEvent::MouseButton { button, down } => {
                let Some(b) = x_button(*button) else { return };
                if *down {
                    self.buttons.insert(b);
                } else {
                    self.buttons.remove(&b);
                }
                self.fake(if *down { BUTTON_PRESS } else { BUTTON_RELEASE }, b, 0, 0);
            }
            InputEvent::Wheel { dx, dy } => {
                // Buttons 4/5 scroll up/down, 6/7 left/right.
                for (amount, neg, pos) in [(*dy, 4u8, 5u8), (*dx, 6, 7)] {
                    let steps = amount.round() as i32;
                    let b = if steps < 0 { neg } else { pos };
                    for _ in 0..steps.unsigned_abs().min(20) {
                        self.fake(BUTTON_PRESS, b, 0, 0);
                        self.fake(BUTTON_RELEASE, b, 0, 0);
                    }
                }
            }
            InputEvent::Key { code, down } => {
                let Some(k) = evdev_code(code).and_then(|c| u8::try_from(c + 8).ok()) else { return };
                if *down {
                    self.keys.insert(k);
                } else {
                    self.keys.remove(&k);
                }
                self.fake(if *down { KEY_PRESS } else { KEY_RELEASE }, k, 0, 0);
            }
            InputEvent::Text { text } => self.type_text(text),
            InputEvent::ReleaseAll => self.release_all(),
            InputEvent::Clipboard { .. } | InputEvent::SwitchMonitor { .. } | InputEvent::SetVideo { .. } => {}
        }
        let _ = self.conn.flush();
    }

    /// Types characters whatever the layout: a character missing from the
    /// keyboard map is put on a spare keycode for the moment it is typed.
    fn type_text(&mut self, text: &str) {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let Some(map) = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()
            .and_then(|c| c.reply().ok())
        else {
            return;
        };
        let per = map.keysyms_per_keycode as usize;
        let find = |sym: u32| -> Option<(u8, bool)> {
            (0..=(max - min) as usize).find_map(|i| {
                let syms = &map.keysyms[i * per..(i + 1) * per];
                if syms.first() == Some(&sym) {
                    Some((min + i as u8, false))
                } else if syms.get(1) == Some(&sym) {
                    Some((min + i as u8, true))
                } else {
                    None
                }
            })
        };
        let spare = (0..=(max - min) as usize)
            .rev()
            .find(|&i| map.keysyms[i * per..(i + 1) * per].iter().all(|&s| s == 0))
            .map(|i| min + i as u8);
        const SHIFT: u8 = 50; // Left Shift (evdev 42 + 8)
        for c in text.chars().take(256) {
            let sym = keysym_for_char(c);
            match find(sym) {
                Some((code, shifted)) => {
                    if shifted {
                        self.fake(KEY_PRESS, SHIFT, 0, 0);
                    }
                    self.fake(KEY_PRESS, code, 0, 0);
                    self.fake(KEY_RELEASE, code, 0, 0);
                    if shifted {
                        self.fake(KEY_RELEASE, SHIFT, 0, 0);
                    }
                }
                None => {
                    let Some(code) = spare else { continue };
                    let syms = vec![sym; per];
                    let _ = self.conn.change_keyboard_mapping(1, code, per as u8, &syms);
                    let _ = self.conn.sync();
                    self.fake(KEY_PRESS, code, 0, 0);
                    self.fake(KEY_RELEASE, code, 0, 0);
                    let _ = self.conn.sync();
                    let _ = self.conn.change_keyboard_mapping(1, code, per as u8, &vec![0; per]);
                }
            }
        }
        let _ = self.conn.flush();
    }

    pub fn release_all(&mut self) {
        for k in std::mem::take(&mut self.keys) {
            self.fake(KEY_RELEASE, k, 0, 0);
        }
        for b in std::mem::take(&mut self.buttons) {
            self.fake(BUTTON_RELEASE, b, 0, 0);
        }
        let _ = self.conn.flush();
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        self.release_all();
    }
}

/// `MouseEvent.button` → X button: left 1, middle 2, right 3, back 8, forward 9.
fn x_button(button: u8) -> Option<u8> {
    Some(match button {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 8,
        4 => 9,
        _ => return None,
    })
}
