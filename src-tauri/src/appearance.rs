//! Appearance: theme files, the Windows accent color and the window backdrop.
//!
//! A theme is a JSON document (`.ropeway-theme`). The app only stores and
//! validates it; everything it means is interpreted by the UI.

use std::path::{Path, PathBuf};

use ropeway_core::Node;
use serde_json::Value;
#[cfg(desktop)]
use tauri::window::{Effect, EffectsBuilder};
use tauri::{State, WebviewWindow};

use crate::{err, CmdResult};

const EXT: &str = "ropeway-theme";
const MAX_THEME_BYTES: u64 = 512 * 1024;

fn themes_dir(node: &Node) -> PathBuf {
    node.data_dir().join("themes")
}

fn is_plain_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Checks the parts the app relies on; the UI validates the rest.
fn validate(theme: Value) -> CmdResult<Value> {
    if theme.get("format").and_then(Value::as_str) != Some("ropeway-theme") {
        return Err("this is not a Ropeway theme".into());
    }
    let id = theme.get("id").and_then(Value::as_str).unwrap_or_default();
    if !is_plain_id(id) {
        return Err("the theme has an invalid id".into());
    }
    if theme.get("name").and_then(Value::as_str).is_none_or(|n| n.trim().is_empty()) {
        return Err("the theme has no name".into());
    }
    Ok(theme)
}

fn read_theme(path: &Path) -> CmdResult<Value> {
    let meta = std::fs::metadata(path).map_err(err)?;
    if meta.len() > MAX_THEME_BYTES {
        return Err("the theme file is too large".into());
    }
    let bytes = std::fs::read(path).map_err(err)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "the theme file is damaged".to_string())?;
    validate(value)
}

fn file_name_for(theme: &Value) -> String {
    let name = theme.get("name").and_then(Value::as_str).unwrap_or("theme");
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .take(60)
        .collect();
    format!("{}.{EXT}", clean.trim())
}

#[tauri::command]
pub fn list_themes(node: State<Node>) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(themes_dir(&node)) else { return Vec::new() };
    let mut out: Vec<Value> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(EXT))
        .filter_map(|p| read_theme(&p).ok())
        .collect();
    out.sort_by_key(|t| t.get("name").and_then(Value::as_str).unwrap_or_default().to_lowercase());
    out
}

#[tauri::command]
pub fn save_theme(node: State<Node>, theme: Value) -> CmdResult<()> {
    let theme = validate(theme)?;
    let id = theme["id"].as_str().unwrap_or_default().to_owned();
    let dir = themes_dir(&node);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let bytes = serde_json::to_vec_pretty(&theme).map_err(err)?;
    std::fs::write(dir.join(format!("{id}.{EXT}")), bytes).map_err(err)
}

#[tauri::command]
pub fn delete_theme(node: State<Node>, id: String) -> CmdResult<()> {
    if !is_plain_id(&id) {
        return Err("invalid theme".into());
    }
    let path = themes_dir(&node).join(format!("{id}.{EXT}"));
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Reads a theme file the user picked or received. Only `.ropeway-theme`
/// files are accepted.
#[tauri::command]
pub fn read_theme_file(path: String) -> CmdResult<Value> {
    let path = PathBuf::from(path);
    if path.extension().and_then(|e| e.to_str()) != Some(EXT) {
        return Err("choose a .ropeway-theme file".into());
    }
    read_theme(&path)
}

#[tauri::command]
pub fn export_theme(path: String, theme: Value) -> CmdResult<()> {
    let theme = validate(theme)?;
    let mut path = PathBuf::from(path);
    if path.extension().and_then(|e| e.to_str()) != Some(EXT) {
        path.set_extension(EXT);
    }
    std::fs::write(path, serde_json::to_vec_pretty(&theme).map_err(err)?).map_err(err)
}

/// Writes a theme to a temporary file so it can be sent to another device.
#[tauri::command]
pub fn theme_file_for_sending(node: State<Node>, theme: Value) -> CmdResult<String> {
    let theme = validate(theme)?;
    let dir = themes_dir(&node).join("outgoing");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(file_name_for(&theme));
    std::fs::write(&path, serde_json::to_vec_pretty(&theme).map_err(err)?).map_err(err)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn get_appearance(node: State<Node>) -> Value {
    node.settings().appearance
}

#[tauri::command]
pub fn set_appearance(node: State<Node>, appearance: Value) -> CmdResult<()> {
    node.set_appearance(appearance).map_err(err)
}

/// The accent color chosen in Windows settings, as `#rrggbb`.
#[tauri::command]
pub fn windows_accent() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\DWM")
            .ok()?;
        // Stored as 0xAABBGGRR.
        let v: u32 = key.get_value("AccentColor").ok()?;
        let (r, g, b) = (v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff);
        Some(format!("#{r:02x}{g:02x}{b:02x}"))
    }
    #[cfg(not(windows))]
    None
}

/// Turns the Windows 11 Mica backdrop on or off for the calling window.
/// On older systems the call has no visible effect.
#[tauri::command]
pub fn set_backdrop(window: WebviewWindow, mica: bool, dark: bool) -> CmdResult<()> {
    #[cfg(mobile)]
    {
        let _ = (window, mica, dark);
        return Ok(());
    }
    #[cfg(desktop)]
    if mica {
        let effect = if dark { Effect::MicaDark } else { Effect::MicaLight };
        window.set_effects(EffectsBuilder::new().effect(effect).build()).map_err(err)
    } else {
        window.set_effects(None).map_err(err)
    }
}

/// Phones: the status and navigation bars take the page's background.
#[tauri::command]
pub fn set_system_bars(color: String, dark: bool) -> CmdResult<()> {
    let valid = color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if !valid {
        return Err("invalid color".into());
    }
    #[cfg(target_os = "android")]
    return crate::android::set_system_bars(&color, dark).map_err(err);
    #[cfg(not(target_os = "android"))]
    {
        let _ = dark;
        Ok(())
    }
}
