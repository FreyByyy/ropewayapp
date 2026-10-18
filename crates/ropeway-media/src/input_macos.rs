//! Applies a controller's input on macOS with Core Graphics events. macOS
//! asks once for the Accessibility permission (System Settings → Privacy &
//! Security → Accessibility); until it is given, events are dropped.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::display::CGDisplay;
use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField, ScrollEventUnit};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use ropeway_core::InputEvent;

use crate::keymap::mac_keycode;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> bool;
}

/// Whether input may be sent; shows the system prompt the first time.
pub fn ask_accessibility() -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

const DOUBLE_CLICK: Duration = Duration::from_millis(500);

pub struct Injector {
    display: u32,
    position: CGPoint,
    buttons: HashSet<u8>,
    keys: HashSet<u16>,
    /// For double and triple clicks: macOS counts them from a field on the event.
    last_click: Option<(u8, Instant, CGPoint, i64)>,
}

fn source() -> Option<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()
}

impl Injector {
    pub fn new(display: Option<usize>) -> Self {
        if !ask_accessibility() {
            tracing::warn!("remote control needs the Accessibility permission");
        }
        let display = display.map(|d| d as u32).unwrap_or_else(|| CGDisplay::main().id);
        Self { display, position: CGPoint::new(0.0, 0.0), buttons: HashSet::new(), keys: HashSet::new(), last_click: None }
    }

    pub fn set_target(&mut self, display: Option<usize>) {
        if let Some(d) = display {
            self.display = d as u32;
        }
    }

    pub fn apply(&mut self, event: &InputEvent) {
        match event {
            InputEvent::MouseMove { x, y } => {
                // Global coordinates in points, origin at the top left of the main display.
                let b = CGDisplay::new(self.display).bounds();
                self.position = CGPoint::new(
                    b.origin.x + x.clamp(0.0, 1.0) as f64 * (b.size.width - 1.0).max(0.0),
                    b.origin.y + y.clamp(0.0, 1.0) as f64 * (b.size.height - 1.0).max(0.0),
                );
                let kind = if self.buttons.contains(&0) {
                    CGEventType::LeftMouseDragged
                } else if self.buttons.contains(&2) {
                    CGEventType::RightMouseDragged
                } else if !self.buttons.is_empty() {
                    CGEventType::OtherMouseDragged
                } else {
                    CGEventType::MouseMoved
                };
                self.mouse(kind, CGMouseButton::Left, None);
            }
            InputEvent::MouseButton { button, down } => self.button(*button, *down),
            InputEvent::Wheel { dx, dy } => {
                let (lines_y, lines_x) = (-(dy.round() as i32), -(dx.round() as i32));
                if lines_x == 0 && lines_y == 0 {
                    return;
                }
                if let Some(e) = source().and_then(|s| CGEvent::new_scroll_event(s, ScrollEventUnit::LINE, 2, lines_y, lines_x, 0).ok()) {
                    e.post(CGEventTapLocation::HID);
                }
            }
            InputEvent::Key { code, down } => {
                let Some(k) = mac_keycode(code) else { return };
                if *down {
                    self.keys.insert(k);
                } else {
                    self.keys.remove(&k);
                }
                key(k, *down);
            }
            InputEvent::Text { text } => {
                // A key event carrying the text: typed as is, whatever the layout.
                for chunk in text.chars().collect::<Vec<_>>().chunks(20) {
                    let s: String = chunk.iter().collect();
                    for down in [true, false] {
                        if let Some(e) = source().and_then(|src| CGEvent::new_keyboard_event(src, 0, down).ok()) {
                            e.set_string(&s);
                            e.post(CGEventTapLocation::HID);
                        }
                    }
                }
            }
            InputEvent::ReleaseAll => self.release_all(),
            InputEvent::Clipboard { .. } | InputEvent::SwitchMonitor { .. } | InputEvent::SetVideo { .. } => {}
        }
    }

    fn mouse(&self, kind: CGEventType, button: CGMouseButton, extra: Option<(i64, i64)>) {
        let Some(e) = source().and_then(|s| CGEvent::new_mouse_event(s, kind, self.position, button).ok()) else { return };
        if let Some((number, clicks)) = extra {
            e.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, number);
            e.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, clicks);
        }
        e.post(CGEventTapLocation::HID);
    }

    fn button(&mut self, button: u8, down: bool) {
        let (kind, cg, number) = match (button, down) {
            (0, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left, 0),
            (0, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left, 0),
            (2, true) => (CGEventType::RightMouseDown, CGMouseButton::Right, 1),
            (2, false) => (CGEventType::RightMouseUp, CGMouseButton::Right, 1),
            (1, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, 2),
            (1, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, 2),
            (3 | 4, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center, button as i64),
            (3 | 4, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center, button as i64),
            _ => return,
        };
        let clicks = if down {
            let now = Instant::now();
            let count = match self.last_click {
                Some((b, at, p, n))
                    if b == button
                        && now.duration_since(at) < DOUBLE_CLICK
                        && (p.x - self.position.x).abs() < 4.0
                        && (p.y - self.position.y).abs() < 4.0 =>
                {
                    n + 1
                }
                _ => 1,
            };
            self.last_click = Some((button, now, self.position, count));
            self.buttons.insert(button);
            count
        } else {
            self.buttons.remove(&button);
            self.last_click.map(|(_, _, _, n)| n).unwrap_or(1)
        };
        self.mouse(kind, cg, Some((number, clicks)));
    }

    pub fn release_all(&mut self) {
        for k in std::mem::take(&mut self.keys) {
            key(k, false);
        }
        for b in std::mem::take(&mut self.buttons).into_iter().collect::<Vec<_>>() {
            self.buttons.insert(b);
            self.button(b, false);
        }
    }
}

impl Drop for Injector {
    fn drop(&mut self) {
        self.release_all();
    }
}

fn key(code: u16, down: bool) {
    if let Some(e) = source().and_then(|s| CGEvent::new_keyboard_event(s, code, down).ok()) {
        e.post(CGEventTapLocation::HID);
    }
}
