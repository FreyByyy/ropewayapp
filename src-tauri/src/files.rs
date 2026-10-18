//! Commands for file transfers, the shared folder and change requests, plus
//! the notifications they raise.

use std::path::{Path, PathBuf};

use ropeway_core::protocol::ChangeKind;
use ropeway_core::{ChangeRequest, ChangeStatus, EditView, Folders, Node, Notice, SharedListing, TransferView};
use tauri::{AppHandle, Manager, State};
use crate::notify::{self, Target};
#[cfg(not(target_os = "android"))]
use tauri_plugin_opener::OpenerExt;

use crate::{err, CmdResult};

// ---- transfers ------------------------------------------------------------

#[tauri::command]
pub fn get_transfers(node: State<Node>) -> Vec<TransferView> {
    node.transfers()
}

#[tauri::command]
pub async fn send_paths(node: State<'_, Node>, peer: String, paths: Vec<String>) -> CmdResult<TransferView> {
    let node = node.inner().clone();
    node.send_paths(&peer, paths.into_iter().map(PathBuf::from).collect()).await.map_err(err)
}

#[tauri::command]
pub fn accept_transfer(node: State<Node>, id: String) -> CmdResult<()> {
    node.accept_transfer(&id).map_err(err)
}

#[tauri::command]
pub fn reject_transfer(node: State<Node>, id: String) -> CmdResult<()> {
    node.reject_transfer(&id).map_err(err)
}

#[tauri::command]
pub fn cancel_transfer(node: State<Node>, id: String) -> CmdResult<()> {
    node.cancel_transfer(&id).map_err(err)
}

#[tauri::command]
pub fn retry_transfer(node: State<Node>, id: String) -> CmdResult<()> {
    node.retry_transfer(&id).map_err(err)
}

#[tauri::command]
pub fn clear_transfers(node: State<Node>, peer: String) {
    node.clear_transfers(&peer)
}

// ---- folders ----------------------------------------------------------------

#[tauri::command]
pub fn get_folders(node: State<Node>) -> Folders {
    node.folders()
}

#[tauri::command]
pub fn set_download_dir(node: State<Node>, path: Option<String>) -> CmdResult<()> {
    node.set_download_dir(path.map(PathBuf::from)).map_err(err)
}

#[tauri::command]
pub fn set_shared_dir(node: State<Node>, path: Option<String>) -> CmdResult<()> {
    node.set_shared_dir(path.map(PathBuf::from)).map_err(err)
}

#[tauri::command]
pub fn set_sharing(node: State<Node>, enabled: bool) -> CmdResult<()> {
    node.set_sharing(enabled).map_err(err)
}

#[tauri::command]
pub fn set_shared_trusted_only(node: State<Node>, enabled: bool) -> CmdResult<()> {
    node.set_shared_trusted_only(enabled).map_err(err)
}

// ---- shared folder ----------------------------------------------------------

#[tauri::command]
pub async fn get_my_shared(node: State<'_, Node>) -> CmdResult<SharedListing> {
    Ok(node.inner().clone().my_shared().await)
}

#[tauri::command]
pub async fn get_peer_shared(node: State<'_, Node>, peer: String) -> CmdResult<SharedListing> {
    let node = node.inner().clone();
    node.peer_shared(&peer).await.map_err(err)
}

#[tauri::command]
pub async fn download_shared(node: State<'_, Node>, peer: String, path: String) -> CmdResult<TransferView> {
    let node = node.inner().clone();
    node.download_shared(&peer, &path).await.map_err(err)
}

#[tauri::command]
pub async fn edit_shared(node: State<'_, Node>, peer: String, path: String) -> CmdResult<TransferView> {
    let node = node.inner().clone();
    node.edit_shared(&peer, &path).await.map_err(err)
}

#[tauri::command]
pub fn get_edits(node: State<Node>) -> Vec<EditView> {
    node.edits()
}

#[tauri::command]
pub fn discard_edit(node: State<Node>, peer: String, path: String) {
    node.discard_edit(&peer, &path)
}

#[tauri::command]
pub fn get_changes(node: State<Node>) -> Vec<ChangeRequest> {
    node.changes()
}

#[tauri::command]
pub fn propose_replace(node: State<Node>, peer: String, path: String) -> CmdResult<ChangeRequest> {
    node.propose_replace(&peer, &path).map_err(err)
}

#[tauri::command]
pub fn propose_delete(node: State<Node>, peer: String, path: String) -> CmdResult<ChangeRequest> {
    node.propose_delete(&peer, &path).map_err(err)
}

#[tauri::command]
pub fn approve_change(node: State<Node>, id: String) -> CmdResult<()> {
    node.approve_change(&id).map_err(err)
}

#[tauri::command]
pub fn reject_change(node: State<Node>, id: String) -> CmdResult<()> {
    node.reject_change(&id).map_err(err)
}

// ---- opening files ----------------------------------------------------------

/// Opens a file or folder with its default program. Only places the app
/// itself manages are allowed, so the UI cannot be used to launch arbitrary
/// programs.
#[tauri::command]
pub fn open_path(app: AppHandle, node: State<Node>, path: String) -> CmdResult<()> {
    let path = allowed_path(&node, &path)?;
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return crate::android::open_file(&path.to_string_lossy()).map_err(err);
    }
    #[cfg(not(target_os = "android"))]
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err)
}

/// Shows a file or folder selected in Explorer. Android has no such view;
/// the file is opened instead.
#[tauri::command]
pub fn reveal_path(app: AppHandle, node: State<Node>, path: String) -> CmdResult<()> {
    #[cfg(target_os = "android")]
    return open_path(app, node, path);
    #[cfg(not(target_os = "android"))]
    {
        let path = allowed_path(&node, &path)?;
        app.opener().reveal_item_in_dir(path).map_err(err)
    }
}

/// Files picked to send. On Android the system picker gives content URIs;
/// the Kotlin side copies the files and returns plain paths. Desktop pages
/// use the dialog plugin instead.
#[tauri::command]
pub async fn pick_files() -> CmdResult<Vec<String>> {
    #[cfg(target_os = "android")]
    return crate::android::pick_files().await.map_err(err);
    #[cfg(not(target_os = "android"))]
    Err("not available on this platform".into())
}

fn allowed_path(node: &Node, path: &str) -> CmdResult<PathBuf> {
    let path = PathBuf::from(path);
    let real = dunce_canonical(&path).ok_or_else(|| "file not found".to_string())?;
    let settings = node.settings();
    let roots = [settings.download_dir(), settings.shared_dir(), node.data_dir().join("edits")];
    if roots.iter().filter_map(|r| dunce_canonical(r)).any(|r| real.starts_with(&r)) {
        Ok(real)
    } else {
        Err("this location cannot be opened from Ropeway".into())
    }
}

/// Canonical path without the `\\?\` prefix Windows adds, which Explorer and
/// many programs do not understand.
fn dunce_canonical(path: &Path) -> Option<PathBuf> {
    let p = std::fs::canonicalize(path).ok()?;
    let s = p.to_string_lossy();
    Some(match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    })
}

// ---- notifications ----------------------------------------------------------

pub fn handle_notice(app: &AppHandle, node: &Node, notice: &Notice) {
    let en = node.settings().language == "en";
    let (title, body, target) = match notice {
        Notice::OfferReceived { transfer } => (
            transfer.peer_name.clone(),
            if en {
                format!("wants to send you {} ({})", describe(transfer), fmt_size(transfer.total, en))
            } else {
                format!("хочет отправить вам {} ({})", describe(transfer), fmt_size(transfer.total, en))
            },
            Target::Chat(&transfer.peer),
        ),
        Notice::FilesReceived { transfer } => {
            if window_focused(app) {
                return;
            }
            (
                transfer.peer_name.clone(),
                if en {
                    format!("Received {} ({})", describe(transfer), fmt_size(transfer.total, en))
                } else {
                    format!("Получено: {} ({})", describe(transfer), fmt_size(transfer.total, en))
                },
                Target::Chat(&transfer.peer),
            )
        }
        Notice::ChangeRequested { change } => (
            change.peer_name.clone(),
            match (change.kind, en) {
                (ChangeKind::Replace, false) => format!("просит заменить «{}» в общей папке", change.path),
                (ChangeKind::Delete, false) => format!("просит удалить «{}» из общей папки", change.path),
                (ChangeKind::Replace, true) => format!("asks to replace “{}” in your shared folder", change.path),
                (ChangeKind::Delete, true) => format!("asks to delete “{}” from your shared folder", change.path),
            },
            Target::Shared,
        ),
        Notice::ChangeAnswered { change } => (
            change.peer_name.clone(),
            match (change.status, en) {
                (ChangeStatus::Applied, false) => format!("одобрил изменение «{}»", change.path),
                (ChangeStatus::Rejected, false) => format!("отклонил изменение «{}»", change.path),
                (_, false) => format!("не удалось применить изменение «{}»", change.path),
                (ChangeStatus::Applied, true) => format!("approved the change to “{}”", change.path),
                (ChangeStatus::Rejected, true) => format!("declined the change to “{}”", change.path),
                (_, true) => format!("could not apply the change to “{}”", change.path),
            },
            Target::Shared,
        ),
        // The copy is not opened automatically: a shared ".bat" or ".exe"
        // would run. The user opens it from the shared folder view.
        Notice::EditReady { path } => {
            if window_focused(app) {
                return;
            }
            let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (
                name,
                if en { "The copy for editing is ready".to_string() } else { "Копия для правки готова".to_string() },
                Target::Shared,
            )
        }
        _ => return,
    };
    notify::show(app, title, body, target);
}

fn window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

fn describe(t: &TransferView) -> String {
    let first = t.names.first().cloned().unwrap_or_default();
    if t.root_count <= 1 {
        format!("«{first}»")
    } else {
        format!("«{first}» +{}", t.root_count - 1)
    }
}

pub fn fmt_size(bytes: u64, en: bool) -> String {
    let units: [&str; 5] = if en { ["B", "KB", "MB", "GB", "TB"] } else { ["Б", "КБ", "МБ", "ГБ", "ТБ"] };
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", units[0])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}
