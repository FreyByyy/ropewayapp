//! Adding devices: invite links shown as QR codes, temporary codes, and
//! the commands for favorites, visibility and deleting messages.

use ropeway_core::{Node, PeerView};
use serde::Serialize;
use tauri::State;

use crate::{err, CmdResult};

/// A QR code as a grid of dark (`1`) and light (`0`) modules, row by row.
/// The UI draws it in the app's style; error correction is high so the
/// logo in the middle does not stop it from being read.
#[derive(Serialize)]
pub struct QrMatrix {
    pub text: String,
    pub width: usize,
    pub cells: String,
}

#[tauri::command]
pub fn invite_qr(node: State<Node>) -> CmdResult<QrMatrix> {
    // Kept short for the camera: no name (the phone learns it on connecting)
    // and only the likeliest addresses.
    let me = node.me();
    let text = ropeway_core::pairing::invite_qr_text(&ropeway_core::pairing::Invite {
        id: me.id,
        port: me.port,
        addrs: ropeway_core::pairing::invite_addresses(&ropeway_core::discovery::local_addresses(), 3),
        name: String::new(),
    });
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::H).map_err(err)?;
    let width = code.width();
    let cells = code
        .to_colors()
        .into_iter()
        .map(|c| if c == qrcode::Color::Dark { '1' } else { '0' })
        .collect();
    Ok(QrMatrix { text, width, cells })
}

#[derive(Serialize)]
pub struct PairingCode {
    pub code: String,
    /// Unix time, milliseconds.
    pub expires: i64,
}

#[tauri::command]
pub fn start_pairing_code(node: State<Node>) -> PairingCode {
    let (code, expires) = node.start_pairing_code();
    PairingCode { code, expires }
}

#[tauri::command]
pub fn stop_pairing_code(node: State<Node>) {
    node.stop_pairing_code();
}

#[tauri::command]
pub async fn connect_invite(node: State<'_, Node>, link: String) -> CmdResult<PeerView> {
    let node = node.inner().clone();
    node.connect_invite(&link).await.map_err(err)
}

#[tauri::command]
pub async fn connect_code(node: State<'_, Node>, code: String) -> CmdResult<PeerView> {
    let node = node.inner().clone();
    node.connect_code(&code).await.map_err(err)
}

#[tauri::command]
pub fn set_favorite(node: State<Node>, id: String, favorite: bool) -> CmdResult<()> {
    node.set_favorite(&id, favorite).map_err(err)
}

#[tauri::command]
pub fn set_discoverable(node: State<Node>, enabled: bool) -> CmdResult<()> {
    node.set_discoverable(enabled).map_err(err)
}

#[tauri::command]
pub fn delete_messages(node: State<Node>, conversation: String, ids: Vec<String>, for_everyone: bool) -> CmdResult<()> {
    node.delete_messages(&conversation, ids, for_everyone).map_err(err)
}

#[tauri::command]
pub fn clear_chat(node: State<Node>, conversation: String) -> CmdResult<()> {
    node.clear_chat(&conversation).map_err(err)
}
