//! Applies a controller's input on Windows with `SendInput`.
//!
//! Every key and button pressed through here is remembered so that
//! `release_all` can let go of them if the controller disconnects mid-press.
//! Elevated windows (UAC) ignore injected input; that is by design.

use std::collections::HashSet;

use ropeway_core::InputEvent;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::keymap::scan_code;

const WHEEL_DELTA: f32 = 120.0;

/// Screen rectangle the picture shows, in desktop pixels.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

pub fn monitor_target(handle: usize) -> Option<Target> {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let ok = unsafe { GetMonitorInfoW(HMONITOR(handle as *mut _), &mut info) };
    if !ok.as_bool() {
        return None;
    }
    let RECT { left, top, right, bottom } = info.rcMonitor;
    Some(Target { left, top, width: right - left, height: bottom - top })
}

#[derive(Default)]
pub struct Injector {
    target: Option<Target>,
    keys: HashSet<(u16, bool)>,
    buttons: HashSet<u8>,
}

impl Injector {
    pub fn new(target: Option<Target>) -> Self {
        Self { target, keys: HashSet::new(), buttons: HashSet::new() }
    }

    pub fn set_target(&mut self, target: Option<Target>) {
        self.target = target;
    }

    pub fn apply(&mut self, event: &InputEvent) {
        match event {
            InputEvent::MouseMove { x, y } => self.mouse_move(*x, *y),
            InputEvent::MouseButton { button, down } => self.mouse_button(*button, *down),
            InputEvent::Wheel { dx, dy } => {
                if *dy != 0.0 {
                    send_mouse(MOUSEEVENTF_WHEEL, 0, 0, (-dy * WHEEL_DELTA).round() as i32);
                }
                if *dx != 0.0 {
                    send_mouse(MOUSEEVENTF_HWHEEL, 0, 0, (dx * WHEEL_DELTA).round() as i32);
                }
            }
            InputEvent::Key { code, down } => {
                if let Some(key) = scan_code(code) {
                    if *down {
                        self.keys.insert(key);
                    } else {
                        self.keys.remove(&key);
                    }
                    send_key(key, *down);
                }
            }
            InputEvent::Text { text } => send_text(text),
            InputEvent::ReleaseAll => self.release_all(),
            // Handled by the provider.
            InputEvent::Clipboard { .. } | InputEvent::SwitchMonitor { .. } | InputEvent::SetVideo { .. } => {}
        }
    }

    fn mouse_move(&self, x: f32, y: f32) {
        let Some(t) = self.target else { return };
        let px = t.left as f64 + x.clamp(0.0, 1.0) as f64 * (t.width - 1).max(0) as f64;
        let py = t.top as f64 + y.clamp(0.0, 1.0) as f64 * (t.height - 1).max(0) as f64;
        // Absolute coordinates span the whole virtual desktop as 0..65535.
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let ax = ((px - vx as f64) * 65535.0 / (vw - 1).max(1) as f64).round() as i32;
        let ay = ((py - vy as f64) * 65535.0 / (vh - 1).max(1) as f64).round() as i32;
        send_mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, ax, ay, 0);
    }

    fn mouse_button(&mut self, button: u8, down: bool) {
        let (flags, data) = match (button, down) {
            (0, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (0, false) => (MOUSEEVENTF_LEFTUP, 0),
            (1, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (1, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (3, true) => (MOUSEEVENTF_XDOWN, 1),
            (3, false) => (MOUSEEVENTF_XUP, 1),
            (4, true) => (MOUSEEVENTF_XDOWN, 2),
            (4, false) => (MOUSEEVENTF_XUP, 2),
            _ => return,
        };
        if down {
            self.buttons.insert(button);
        } else {
            self.buttons.remove(&button);
        }
        send_mouse(flags, 0, 0, data);
    }

    pub fn release_all(&mut self) {
        for key in std::mem::take(&mut self.keys) {
            send_key(key, false);
        }
        for button in std::mem::take(&mut self.buttons) {
            self.mouse_button(button, false);
        }
        self.buttons.clear();
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        self.release_all();
    }
}

fn send_mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    /// Moves the real cursor: `cargo test -p ropeway-media -- --ignored inject`
    #[test]
    #[ignore]
    fn inject_mouse_move_lands_on_the_monitor_point() {
        let primary = windows_capture::monitor::Monitor::primary().unwrap().as_raw_hmonitor() as usize;
        let target = monitor_target(primary).unwrap();
        assert!(target.width > 0 && target.height > 0);
        let mut before = POINT::default();
        unsafe { GetCursorPos(&mut before).unwrap() };

        let mut injector = Injector::new(Some(target));
        for (x, y) in [(0.5f32, 0.5f32), (0.25, 0.75)] {
            injector.apply(&InputEvent::MouseMove { x, y });
            std::thread::sleep(std::time::Duration::from_millis(50));
            let mut p = POINT::default();
            unsafe { GetCursorPos(&mut p).unwrap() };
            let ex = target.left + (x * (target.width - 1) as f32).round() as i32;
            let ey = target.top + (y * (target.height - 1) as f32).round() as i32;
            assert!((p.x - ex).abs() <= 2 && (p.y - ey).abs() <= 2, "cursor at {},{} expected {ex},{ey}", p.x, p.y);
        }
        unsafe { SetCursorPos(before.x, before.y).unwrap() };
    }
}

/// Types text as Unicode characters, independent of the keyboard layout.
fn send_text(text: &str) {
    let inputs: Vec<INPUT> = text
        .chars()
        .take(256)
        .flat_map(|c| {
            let mut units = [0u16; 2];
            c.encode_utf16(&mut units).to_vec()
        })
        .flat_map(|unit| {
            [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP].map(|flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: 0 },
                },
            })
        })
        .collect();
    if !inputs.is_empty() {
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

fn send_key((scan, extended): (u16, bool), down: bool) {
    let mut flags: KEYBD_EVENT_FLAGS = KEYEVENTF_SCANCODE;
    if extended {
        flags = flags | KEYEVENTF_EXTENDEDKEY;
    }
    if !down {
        flags = flags | KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}
