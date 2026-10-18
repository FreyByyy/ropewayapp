//! Path handling for anything that crosses the network.
//!
//! Relative paths travel as `/`-separated strings. Every path received from a
//! peer goes through [`sanitize_rel`] before it touches the disk, so it can
//! never escape the folder it is joined onto.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const PART_SUFFIX: &str = ".ropeway-part";
const MAX_COMPONENTS: usize = 64;
const MAX_ENTRIES: usize = 200_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SharedEntry {
    pub path: String,
    pub size: u64,
    /// Unix time, milliseconds.
    pub modified: i64,
    pub is_dir: bool,
}

/// A file found while preparing an outgoing transfer.
pub struct WalkedFile {
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

/// Normalizes a relative path from the network. Returns `None` for anything
/// that is absolute, climbs out (`..`), or is otherwise unusable. Characters
/// Windows does not allow in names are replaced.
pub fn sanitize_rel(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for raw in path.split(['/', '\\']) {
        if raw.is_empty() || raw == "." {
            continue;
        }
        if raw == ".." {
            return None;
        }
        let mut name: String = raw
            .chars()
            .map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c })
            .collect();
        // Windows silently drops trailing dots and spaces.
        while name.ends_with('.') || name.ends_with(' ') {
            name.pop();
        }
        if name.is_empty() {
            return None;
        }
        if is_reserved_name(&name) {
            name.insert(0, '_');
        }
        if name.len() > 255 {
            return None;
        }
        parts.push(name);
    }
    if parts.is_empty() || parts.len() > MAX_COMPONENTS {
        return None;
    }
    Some(parts.join("/"))
}

fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Joins a sanitized relative path onto `root`.
pub fn join_rel(root: &Path, rel: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in rel.split('/') {
        out.push(part);
    }
    out
}

/// Joins an untrusted relative path onto `root`, refusing paths that escape it.
pub fn join_safe(root: &Path, rel: &str) -> Option<PathBuf> {
    sanitize_rel(rel).map(|r| join_rel(root, &r))
}

pub fn first_component(rel: &str) -> &str {
    rel.split('/').next().unwrap_or(rel)
}

pub fn parent_rel(rel: &str) -> &str {
    rel.rfind('/').map(|i| &rel[..i]).unwrap_or("")
}

pub fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

pub fn join_remote(base: &str, rel: &str) -> String {
    if base.is_empty() {
        rel.to_owned()
    } else {
        format!("{base}/{rel}")
    }
}

/// `report.docx` → `report (1).docx`, `report (2).docx`, … until unused.
pub fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() && !part_path(&path).exists() {
        return path;
    }
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_owned();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !path.is_dir() => (name[..i].to_owned(), name[i..].to_owned()),
        _ => (name.clone(), String::new()),
    };
    for n in 1.. {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() && !part_path(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(PART_SUFFIX);
    target.with_file_name(name)
}

/// Marks a file as downloaded from the internet ("Mark of the Web"), so
/// SmartScreen and Office Protected View warn before it runs or opens
/// editable. Best effort: FAT drives and other systems have no such mark.
pub fn mark_from_network(path: &Path) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        if let Err(e) = fs::write(PathBuf::from(stream), b"[ZoneTransfer]\r\nZoneId=3\r\n") {
            tracing::debug!("marking {}: {e}", path.display());
        }
    }
    #[cfg(not(windows))]
    let _ = path;
}

pub fn mtime_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Collects files and empty folders under the given paths. Relative paths
/// start with the selected item's own name. Symlinks are skipped.
pub fn walk(paths: &[PathBuf]) -> Result<(Vec<WalkedFile>, Vec<String>)> {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for path in paths {
        let meta = fs::symlink_metadata(path)?;
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            bail!("unsupported path: {}", path.display());
        };
        let name = sanitize_rel(name).ok_or_else(|| anyhow::anyhow!("unsupported name: {name}"))?;
        if meta.is_file() {
            files.push(WalkedFile { rel: name, abs: path.clone(), size: meta.len(), mtime: mtime_ms(&meta) });
        } else if meta.is_dir() {
            walk_dir(path, &name, &mut files, &mut dirs)?;
        }
        if files.len() + dirs.len() > MAX_ENTRIES {
            bail!("too many files");
        }
    }
    Ok((files, dirs))
}

fn walk_dir(dir: &Path, rel: &str, files: &mut Vec<WalkedFile>, dirs: &mut Vec<String>) -> Result<()> {
    let mut empty = true;
    let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let Ok(meta) = entry.metadata() else { continue };
        let Some(name) = entry.file_name().to_str().and_then(sanitize_rel) else { continue };
        let child_rel = format!("{rel}/{name}");
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            empty = false;
            files.push(WalkedFile { rel: child_rel, abs: entry.path(), size: meta.len(), mtime: mtime_ms(&meta) });
        } else if meta.is_dir() {
            empty = false;
            walk_dir(&entry.path(), &child_rel, files, dirs)?;
        }
        if files.len() + dirs.len() > MAX_ENTRIES {
            bail!("too many files");
        }
    }
    if empty {
        dirs.push(rel.to_owned());
    }
    Ok(())
}

/// Everything inside a shared folder, as paths relative to it. Partial
/// downloads and Office lock files are hidden.
pub fn list_tree(root: &Path) -> Vec<SharedEntry> {
    let mut out = Vec::new();
    list_into(root, "", &mut out);
    out
}

fn list_into(dir: &Path, rel: &str, out: &mut Vec<SharedEntry>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        if out.len() >= MAX_ENTRIES {
            return;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if name.ends_with(PART_SUFFIX) || name.starts_with("~$") || name == "desktop.ini" || name == "Thumbs.db" {
            continue;
        }
        let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        out.push(SharedEntry { path: path.clone(), size: if meta.is_dir() { 0 } else { meta.len() }, modified: mtime_ms(&meta), is_dir: meta.is_dir() });
        if meta.is_dir() {
            list_into(&entry.path(), &path, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escapes() {
        assert_eq!(sanitize_rel("../etc/passwd"), None);
        assert_eq!(sanitize_rel("a/../../b"), None);
        assert_eq!(sanitize_rel(""), None);
        assert_eq!(sanitize_rel("/"), None);
    }

    #[test]
    fn normalizes() {
        assert_eq!(sanitize_rel("/abs/path.txt").as_deref(), Some("abs/path.txt"));
        assert_eq!(sanitize_rel("C:\\Windows\\x").as_deref(), Some("C_/Windows/x"));
        assert_eq!(sanitize_rel("a\\b/./c").as_deref(), Some("a/b/c"));
        assert_eq!(sanitize_rel("con.txt").as_deref(), Some("_con.txt"));
        assert_eq!(sanitize_rel("name. ").as_deref(), Some("name"));
        assert_eq!(sanitize_rel("Отчёт <финал>.docx").as_deref(), Some("Отчёт _финал_.docx"));
    }
}
