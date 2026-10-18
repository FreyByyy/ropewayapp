//! `KeyboardEvent.code` → PC/AT set-1 scan code. Codes name physical keys,
//! so the controlled device types with its own keyboard layout (switch
//! layouts there with its usual shortcut).

/// Returns `(scan code, extended)`.
pub fn scan_code(code: &str) -> Option<(u16, bool)> {
    let plain = |s: u16| Some((s, false));
    let ext = |s: u16| Some((s, true));
    match code {
        "Escape" => plain(0x01),
        "Digit1" => plain(0x02),
        "Digit2" => plain(0x03),
        "Digit3" => plain(0x04),
        "Digit4" => plain(0x05),
        "Digit5" => plain(0x06),
        "Digit6" => plain(0x07),
        "Digit7" => plain(0x08),
        "Digit8" => plain(0x09),
        "Digit9" => plain(0x0A),
        "Digit0" => plain(0x0B),
        "Minus" => plain(0x0C),
        "Equal" => plain(0x0D),
        "Backspace" => plain(0x0E),
        "Tab" => plain(0x0F),
        "KeyQ" => plain(0x10),
        "KeyW" => plain(0x11),
        "KeyE" => plain(0x12),
        "KeyR" => plain(0x13),
        "KeyT" => plain(0x14),
        "KeyY" => plain(0x15),
        "KeyU" => plain(0x16),
        "KeyI" => plain(0x17),
        "KeyO" => plain(0x18),
        "KeyP" => plain(0x19),
        "BracketLeft" => plain(0x1A),
        "BracketRight" => plain(0x1B),
        "Enter" => plain(0x1C),
        "ControlLeft" => plain(0x1D),
        "KeyA" => plain(0x1E),
        "KeyS" => plain(0x1F),
        "KeyD" => plain(0x20),
        "KeyF" => plain(0x21),
        "KeyG" => plain(0x22),
        "KeyH" => plain(0x23),
        "KeyJ" => plain(0x24),
        "KeyK" => plain(0x25),
        "KeyL" => plain(0x26),
        "Semicolon" => plain(0x27),
        "Quote" => plain(0x28),
        "Backquote" => plain(0x29),
        "ShiftLeft" => plain(0x2A),
        "Backslash" => plain(0x2B),
        "KeyZ" => plain(0x2C),
        "KeyX" => plain(0x2D),
        "KeyC" => plain(0x2E),
        "KeyV" => plain(0x2F),
        "KeyB" => plain(0x30),
        "KeyN" => plain(0x31),
        "KeyM" => plain(0x32),
        "Comma" => plain(0x33),
        "Period" => plain(0x34),
        "Slash" => plain(0x35),
        "ShiftRight" => plain(0x36),
        "NumpadMultiply" => plain(0x37),
        "AltLeft" => plain(0x38),
        "Space" => plain(0x39),
        "CapsLock" => plain(0x3A),
        "F1" => plain(0x3B),
        "F2" => plain(0x3C),
        "F3" => plain(0x3D),
        "F4" => plain(0x3E),
        "F5" => plain(0x3F),
        "F6" => plain(0x40),
        "F7" => plain(0x41),
        "F8" => plain(0x42),
        "F9" => plain(0x43),
        "F10" => plain(0x44),
        "NumLock" => ext(0x45),
        "ScrollLock" => plain(0x46),
        "Numpad7" => plain(0x47),
        "Numpad8" => plain(0x48),
        "Numpad9" => plain(0x49),
        "NumpadSubtract" => plain(0x4A),
        "Numpad4" => plain(0x4B),
        "Numpad5" => plain(0x4C),
        "Numpad6" => plain(0x4D),
        "NumpadAdd" => plain(0x4E),
        "Numpad1" => plain(0x4F),
        "Numpad2" => plain(0x50),
        "Numpad3" => plain(0x51),
        "Numpad0" => plain(0x52),
        "NumpadDecimal" => plain(0x53),
        "IntlBackslash" => plain(0x56),
        "F11" => plain(0x57),
        "F12" => plain(0x58),
        "NumpadEnter" => ext(0x1C),
        "ControlRight" => ext(0x1D),
        "NumpadDivide" => ext(0x35),
        "PrintScreen" => ext(0x37),
        "AltRight" => ext(0x38),
        "Home" => ext(0x47),
        "ArrowUp" => ext(0x48),
        "PageUp" => ext(0x49),
        "ArrowLeft" => ext(0x4B),
        "ArrowRight" => ext(0x4D),
        "End" => ext(0x4F),
        "ArrowDown" => ext(0x50),
        "PageDown" => ext(0x51),
        "Insert" => ext(0x52),
        "Delete" => ext(0x53),
        "MetaLeft" => ext(0x5B),
        "MetaRight" => ext(0x5C),
        "ContextMenu" => ext(0x5D),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_extended_keys() {
        assert_eq!(scan_code("KeyA"), Some((0x1E, false)));
        assert_eq!(scan_code("ArrowLeft"), Some((0x4B, true)));
        assert_eq!(scan_code("ControlRight"), Some((0x1D, true)));
        assert_eq!(scan_code("MetaLeft"), Some((0x5B, true)));
        assert_eq!(scan_code("NoSuchKey"), None);
    }

    #[test]
    fn no_two_codes_share_a_key() {
        let codes = [
            "Escape", "Digit1", "Digit0", "Minus", "Equal", "Backspace", "Tab", "KeyQ", "KeyP", "BracketLeft",
            "BracketRight", "Enter", "ControlLeft", "KeyA", "KeyL", "Semicolon", "Quote", "Backquote", "ShiftLeft",
            "Backslash", "KeyZ", "KeyM", "Comma", "Period", "Slash", "ShiftRight", "AltLeft", "Space", "CapsLock",
            "F1", "F10", "F11", "F12", "NumpadEnter", "ControlRight", "AltRight", "Home", "End", "ArrowUp",
            "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Insert", "Delete", "MetaLeft",
            "IntlBackslash", "Numpad0", "NumpadDecimal", "NumpadDivide", "NumpadMultiply",
        ];
        let mut seen = std::collections::HashSet::new();
        for c in codes {
            let k = scan_code(c).unwrap_or_else(|| panic!("{c} unmapped"));
            assert!(seen.insert(k), "{c} collides");
        }
    }
}

/// `KeyboardEvent.code` → Linux input event code (`KEY_*` in
/// linux/input-event-codes.h). Plain set-1 scan codes equal the Linux codes;
/// the extended (E0-prefixed) keys are mapped one by one. X11 keycodes are
/// these plus 8.
pub fn evdev_code(code: &str) -> Option<u16> {
    let (scan, extended) = scan_code(code)?;
    if !extended {
        return Some(scan);
    }
    Some(match scan {
        0x1C => 96,  // KEY_KPENTER
        0x45 => 69,  // KEY_NUMLOCK (extended only in Windows terms)
        0x1D => 97,  // KEY_RIGHTCTRL
        0x35 => 98,  // KEY_KPSLASH
        0x37 => 99,  // KEY_SYSRQ (Print Screen)
        0x38 => 100, // KEY_RIGHTALT
        0x47 => 102, // KEY_HOME
        0x48 => 103, // KEY_UP
        0x49 => 104, // KEY_PAGEUP
        0x4B => 105, // KEY_LEFT
        0x4D => 106, // KEY_RIGHT
        0x4F => 107, // KEY_END
        0x50 => 108, // KEY_DOWN
        0x51 => 109, // KEY_PAGEDOWN
        0x52 => 110, // KEY_INSERT
        0x53 => 111, // KEY_DELETE
        0x5B => 125, // KEY_LEFTMETA
        0x5C => 126, // KEY_RIGHTMETA
        0x5D => 127, // KEY_COMPOSE (context menu)
        _ => return None,
    })
}

/// X keysym typing a character: Latin-1 maps directly, the rest uses the
/// Unicode range.
pub fn keysym_for_char(c: char) -> u32 {
    match c {
        '\n' | '\r' => 0xff0d, // Return
        '\t' => 0xff09,        // Tab
        c if (' '..='~').contains(&c) || ('\u{a0}'..='\u{ff}').contains(&c) => c as u32,
        c => 0x0100_0000 | c as u32,
    }
}

/// `KeyboardEvent.code` → macOS virtual key code (`kVK_*` in Carbon's
/// Events.h). Physical keys, like the other tables.
pub fn mac_keycode(code: &str) -> Option<u16> {
    Some(match code {
        "KeyA" => 0x00,
        "KeyS" => 0x01,
        "KeyD" => 0x02,
        "KeyF" => 0x03,
        "KeyH" => 0x04,
        "KeyG" => 0x05,
        "KeyZ" => 0x06,
        "KeyX" => 0x07,
        "KeyC" => 0x08,
        "KeyV" => 0x09,
        "IntlBackslash" => 0x0A,
        "KeyB" => 0x0B,
        "KeyQ" => 0x0C,
        "KeyW" => 0x0D,
        "KeyE" => 0x0E,
        "KeyR" => 0x0F,
        "KeyY" => 0x10,
        "KeyT" => 0x11,
        "Digit1" => 0x12,
        "Digit2" => 0x13,
        "Digit3" => 0x14,
        "Digit4" => 0x15,
        "Digit6" => 0x16,
        "Digit5" => 0x17,
        "Equal" => 0x18,
        "Digit9" => 0x19,
        "Digit7" => 0x1A,
        "Minus" => 0x1B,
        "Digit8" => 0x1C,
        "Digit0" => 0x1D,
        "BracketRight" => 0x1E,
        "KeyO" => 0x1F,
        "KeyU" => 0x20,
        "BracketLeft" => 0x21,
        "KeyI" => 0x22,
        "KeyP" => 0x23,
        "Enter" => 0x24,
        "KeyL" => 0x25,
        "KeyJ" => 0x26,
        "Quote" => 0x27,
        "KeyK" => 0x28,
        "Semicolon" => 0x29,
        "Backslash" => 0x2A,
        "Comma" => 0x2B,
        "Slash" => 0x2C,
        "KeyN" => 0x2D,
        "KeyM" => 0x2E,
        "Period" => 0x2F,
        "Tab" => 0x30,
        "Space" => 0x31,
        "Backquote" => 0x32,
        "Backspace" => 0x33,
        "Escape" => 0x35,
        "MetaRight" => 0x36,
        "MetaLeft" => 0x37,
        "ShiftLeft" => 0x38,
        "CapsLock" => 0x39,
        "AltLeft" => 0x3A,
        "ControlLeft" => 0x3B,
        "ShiftRight" => 0x3C,
        "AltRight" => 0x3D,
        "ControlRight" => 0x3E,
        "F17" => 0x40,
        "NumpadDecimal" => 0x41,
        "NumpadMultiply" => 0x43,
        "NumpadAdd" => 0x45,
        "NumLock" => 0x47,
        "NumpadDivide" => 0x4B,
        "NumpadEnter" => 0x4C,
        "NumpadSubtract" => 0x4E,
        "F18" => 0x4F,
        "F19" => 0x50,
        "NumpadEqual" => 0x51,
        "Numpad0" => 0x52,
        "Numpad1" => 0x53,
        "Numpad2" => 0x54,
        "Numpad3" => 0x55,
        "Numpad4" => 0x56,
        "Numpad5" => 0x57,
        "Numpad6" => 0x58,
        "Numpad7" => 0x59,
        "F20" => 0x5A,
        "Numpad8" => 0x5B,
        "Numpad9" => 0x5C,
        "F5" => 0x60,
        "F6" => 0x61,
        "F7" => 0x62,
        "F3" => 0x63,
        "F8" => 0x64,
        "F9" => 0x65,
        "F11" => 0x67,
        "F13" => 0x69,
        "F16" => 0x6A,
        "F14" => 0x6B,
        "F10" => 0x6D,
        "ContextMenu" => 0x6E,
        "F12" => 0x6F,
        "F15" => 0x71,
        "Insert" => 0x72,
        "Home" => 0x73,
        "PageUp" => 0x74,
        "Delete" => 0x75,
        "F4" => 0x76,
        "End" => 0x77,
        "F2" => 0x78,
        "PageDown" => 0x79,
        "F1" => 0x7A,
        "ArrowLeft" => 0x7B,
        "ArrowRight" => 0x7C,
        "ArrowDown" => 0x7D,
        "ArrowUp" => 0x7E,
        _ => return None,
    })
}
