//! Shared folder: every device exposes one folder; others can list it, copy
//! from it, and ask the owner to replace or delete files. Nothing changes on
//! the owner's disk without the owner's approval, and replaced or deleted
//! files go to the Recycle Bin.
//!
//! Editing works on a copy: "edit" downloads the file into the app's data
//! folder, the user edits it with any program, then proposes the new version.
//! On approval the owner pulls the new version from the requester.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::fs_util::{self, file_name, join_safe, list_tree, mtime_ms, parent_rel, part_path, sanitize_rel, SharedEntry};
use crate::identity::is_valid_id;
use crate::node::{now_ms, Event, Node, Notice};
use crate::protocol::{ChangeKind, ChangeOutcome, ChangeWire, FetchTarget, Request, Response};
use crate::settings::write_atomic;
use crate::transfers::{prepare_targets, valid_wire_id, MAX_WAITING, MAX_WAITING_PER_PEER, Direction, FileEntry, Transfer, TransferKind, TransferStatus, TransferView};

const MAX_CHANGES_KEPT: usize = 200;

#[derive(Serialize, Clone, Debug)]
pub struct SharedListing {
    pub enabled: bool,
    /// Local path, only for this device's own listing.
    pub root: Option<String>,
    pub entries: Vec<SharedEntry>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Pending,
    Applying,
    Applied,
    Rejected,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChangeRequest {
    pub id: String,
    /// The other side: the owner for outgoing requests, the requester for incoming.
    pub peer: String,
    pub peer_name: String,
    pub direction: Direction,
    pub kind: ChangeKind,
    pub path: String,
    pub size: Option<u64>,
    pub base_modified: Option<i64>,
    /// Owner side: the file was modified after the requester copied it.
    #[serde(default)]
    pub conflict: bool,
    pub status: ChangeStatus,
    pub created: i64,
    #[serde(default)]
    pub finished: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
    /// Requester side: the file holding the proposed version.
    #[serde(default)]
    pub local: Option<PathBuf>,
}

/// A local copy of someone else's shared file, taken for editing.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditSession {
    pub peer: String,
    pub peer_name: String,
    pub path: String,
    pub local: PathBuf,
    pub transfer_id: String,
    pub base_size: u64,
    pub base_modified: i64,
    #[serde(default)]
    pub ready: bool,
    /// Modification time of the copy right after download.
    #[serde(default)]
    pub local_modified: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct EditView {
    pub peer: String,
    pub peer_name: String,
    pub path: String,
    pub local: String,
    pub ready: bool,
    /// The copy was saved since it was downloaded.
    pub modified: bool,
}

pub(crate) struct ListStore<T> {
    path: PathBuf,
    pub(crate) items: Vec<T>,
}

impl<T: Serialize + DeserializeOwned> ListStore<T> {
    pub(crate) fn load(path: PathBuf) -> Self {
        let items = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { path, items }
    }

    pub(crate) fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec(&self.items) {
            if let Err(e) = write_atomic(&self.path, &bytes) {
                tracing::warn!("saving {}: {e}", self.path.display());
            }
        }
    }
}


/// Whether any part of `path` below `root` is a symbolic link or junction.
/// The listing hides them, so peers must not reach through them either.
fn has_link(root: &std::path::Path, path: &std::path::Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return true };
    let mut current = root.to_path_buf();
    for part in rel.components() {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

impl Node {
    // ---- listing -------------------------------------------------------

    /// This device's shared folder, or `None` when sharing is off.
    pub fn shared_root(&self) -> Option<PathBuf> {
        let s = self.settings();
        s.sharing_enabled.then(|| s.shared_dir())
    }

    pub async fn my_shared(&self) -> SharedListing {
        match self.shared_root() {
            Some(root) => {
                let _ = std::fs::create_dir_all(&root);
                let r = root.clone();
                let entries = tokio::task::spawn_blocking(move || list_tree(&r)).await.unwrap_or_default();
                SharedListing { enabled: true, root: Some(root.to_string_lossy().into_owned()), entries }
            }
            None => SharedListing { enabled: false, root: None, entries: Vec::new() },
        }
    }

    pub async fn peer_shared(&self, peer: &str) -> Result<SharedListing> {
        match self.request(peer, &Request::SharedList).await? {
            Response::SharedList { enabled, entries } => Ok(SharedListing {
                enabled,
                root: None,
                entries: entries.into_iter().filter(|e| sanitize_rel(&e.path).as_deref() == Some(e.path.as_str())).collect(),
            }),
            Response::Error { message } => bail!(message),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    pub(crate) async fn on_shared_list(&self, peer: &str) -> Response {
        if let Err(e) = self.peer_may_use_shared(peer) {
            return Response::Error { message: e.to_string() };
        }
        let listing = self.my_shared().await;
        Response::SharedList { enabled: listing.enabled, entries: listing.entries }
    }

    /// Whether `peer` may see this device's shared folder at all.
    fn peer_may_use_shared(&self, peer: &str) -> Result<()> {
        if self.settings().shared_trusted_only && !self.is_trusted(peer) {
            bail!("the shared folder is open to trusted devices only");
        }
        Ok(())
    }

    /// A path in this device's shared folder that a peer named. Links that
    /// lead out of the folder are refused.
    fn shared_path(&self, peer: &str, rel: &str) -> Result<(PathBuf, PathBuf)> {
        self.peer_may_use_shared(peer)?;
        let root = self.shared_root().ok_or_else(|| anyhow!("shared folder is turned off"))?;
        let path = join_safe(&root, rel).ok_or_else(|| anyhow!("invalid path"))?;
        let real_root = std::fs::canonicalize(&root).map_err(|_| anyhow!("shared folder is missing"))?;
        let real = std::fs::canonicalize(&path).map_err(|_| anyhow!("file not found"))?;
        if !real.starts_with(&real_root) || has_link(&root, &path) {
            bail!("file not found");
        }
        Ok((root, path))
    }

    /// A file in this device's shared folder that a peer asked for.
    pub(crate) fn shared_file(&self, peer: &str, rel: &str) -> Result<PathBuf> {
        let (_, path) = self.shared_path(peer, rel)?;
        if !path.is_file() {
            bail!("file not found");
        }
        Ok(path)
    }

    // ---- copying ---------------------------------------------------------

    /// Copies a file or folder from a peer's shared folder into Downloads.
    pub async fn download_shared(&self, peer: &str, path: &str) -> Result<TransferView> {
        let path = sanitize_rel(path).ok_or_else(|| anyhow!("invalid path"))?;
        let listing = self.peer_shared(peer).await?;
        let base = parent_rel(&path).to_owned();
        let prefix = format!("{path}/");
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        for e in listing.entries {
            if e.path != path && !e.path.starts_with(&prefix) {
                continue;
            }
            let rel = if base.is_empty() { e.path.clone() } else { e.path[base.len() + 1..].to_owned() };
            if e.is_dir {
                dirs.push(rel);
            } else {
                files.push(FileEntry { path: rel, size: e.size, done: 0, local: None, mtime: e.modified });
            }
        }
        if files.is_empty() && dirs.is_empty() {
            bail!("not found in the shared folder");
        }
        let dest = self.settings().download_dir();
        std::fs::create_dir_all(&dest)?;
        let mut t = self.new_incoming(peer, TransferKind::Download, files, dirs, base);
        prepare_targets(&mut t, &dest, true);
        let id = t.id.clone();
        let view = self.insert_transfer(t);
        self.start_run(&id);
        Ok(view)
    }

    /// Downloads a copy of a peer's shared file for editing. When it is
    /// ready, a `Notice::EditReady` is emitted so the shell can open it.
    pub async fn edit_shared(&self, peer: &str, path: &str) -> Result<TransferView> {
        if !is_valid_id(peer) {
            bail!("unknown device");
        }
        let path = sanitize_rel(path).ok_or_else(|| anyhow!("invalid path"))?;
        let listing = self.peer_shared(peer).await?;
        let entry = listing
            .entries
            .iter()
            .find(|e| e.path == path && !e.is_dir)
            .cloned()
            .ok_or_else(|| anyhow!("only files can be edited"))?;

        let dest = fs_util::join_rel(&self.data_dir().join("edits").join(peer), parent_rel(&path));
        std::fs::create_dir_all(&dest)?;
        let files = vec![FileEntry { path: file_name(&path).to_owned(), size: entry.size, done: 0, local: None, mtime: entry.modified }];
        let mut t = self.new_incoming(peer, TransferKind::Edit, files, Vec::new(), parent_rel(&path).to_owned());
        prepare_targets(&mut t, &dest, false);
        let session = EditSession {
            peer: peer.to_owned(),
            peer_name: t.peer_name.clone(),
            path: path.clone(),
            local: t.files[0].local.clone().unwrap_or_default(),
            transfer_id: t.id.clone(),
            base_size: entry.size,
            base_modified: entry.modified,
            ready: false,
            local_modified: 0,
        };
        {
            let mut edits = self.inner.edits.lock().unwrap();
            edits.items.retain(|e| !(e.peer == peer && e.path == path));
            edits.items.push(session);
            edits.save();
        }
        self.emit_edits();
        let id = t.id.clone();
        let view = self.insert_transfer(t);
        self.start_run(&id);
        Ok(view)
    }

    fn new_incoming(&self, peer: &str, kind: TransferKind, files: Vec<FileEntry>, dirs: Vec<String>, remote_base: String) -> Transfer {
        Transfer {
            id: uuid::Uuid::new_v4().to_string(),
            peer: peer.to_owned(),
            peer_name: self.peer_name(peer),
            direction: Direction::Incoming,
            kind,
            status: TransferStatus::Active,
            files,
            dirs,
            local_dirs: Vec::new(),
            roots_local: Vec::new(),
            created: now_ms(),
            finished: None,
            error: None,
            dest: None,
            remote_base,
        }
    }

    pub(crate) fn on_edit_downloaded(&self, transfer_id: &str) {
        let local = {
            let mut edits = self.inner.edits.lock().unwrap();
            let Some(e) = edits.items.iter_mut().find(|e| e.transfer_id == transfer_id) else { return };
            e.ready = true;
            e.local_modified = std::fs::metadata(&e.local).map(|m| mtime_ms(&m)).unwrap_or(0);
            let local = e.local.clone();
            edits.save();
            local
        };
        self.emit_edits();
        self.emit(Event::Notice(Notice::EditReady { path: local.to_string_lossy().into_owned() }));
    }

    pub fn edits(&self) -> Vec<EditView> {
        self.inner
            .edits
            .lock()
            .unwrap()
            .items
            .iter()
            .map(|e| EditView {
                peer: e.peer.clone(),
                peer_name: e.peer_name.clone(),
                path: e.path.clone(),
                local: e.local.to_string_lossy().into_owned(),
                ready: e.ready,
                modified: e.ready
                    && std::fs::metadata(&e.local).map(|m| mtime_ms(&m)).unwrap_or(0) != e.local_modified,
            })
            .collect()
    }

    /// Forgets an edit and deletes the local copy.
    pub fn discard_edit(&self, peer: &str, path: &str) {
        let removed: Vec<EditSession> = {
            let mut edits = self.inner.edits.lock().unwrap();
            let (gone, keep) = std::mem::take(&mut edits.items).into_iter().partition(|e| e.peer == peer && e.path == path);
            edits.items = keep;
            edits.save();
            gone
        };
        for e in removed {
            let _ = std::fs::remove_file(&e.local);
            let _ = std::fs::remove_file(part_path(&e.local));
        }
        self.emit_edits();
    }

    fn emit_edits(&self) {
        self.emit(Event::Edits(self.edits()));
    }

    // ---- change requests: requester side ---------------------------------

    pub fn changes(&self) -> Vec<ChangeRequest> {
        let mut list = self.inner.changes.lock().unwrap().items.clone();
        list.sort_by(|a, b| b.created.cmp(&a.created));
        list
    }

    /// Asks the owner to replace their file with the edited copy.
    pub fn propose_replace(&self, peer: &str, path: &str) -> Result<ChangeRequest> {
        let session = self
            .inner
            .edits
            .lock()
            .unwrap()
            .items
            .iter()
            .find(|e| e.peer == peer && e.path == path && e.ready)
            .cloned()
            .ok_or_else(|| anyhow!("there is no edited copy of this file"))?;
        let size = std::fs::metadata(&session.local)?.len();
        self.propose(peer, ChangeKind::Replace, path, Some(size), Some(session.base_modified), Some(session.local))
    }

    /// Asks the owner to delete a file or folder from their shared folder.
    pub fn propose_delete(&self, peer: &str, path: &str) -> Result<ChangeRequest> {
        self.propose(peer, ChangeKind::Delete, path, None, None, None)
    }

    fn propose(
        &self,
        peer: &str,
        kind: ChangeKind,
        path: &str,
        size: Option<u64>,
        base_modified: Option<i64>,
        local: Option<PathBuf>,
    ) -> Result<ChangeRequest> {
        if !is_valid_id(peer) {
            bail!("unknown device");
        }
        let path = sanitize_rel(path).ok_or_else(|| anyhow!("invalid path"))?;
        let request = ChangeRequest {
            id: uuid::Uuid::new_v4().to_string(),
            peer: peer.to_owned(),
            peer_name: self.peer_name(peer),
            direction: Direction::Outgoing,
            kind,
            path: path.clone(),
            size,
            base_modified,
            conflict: false,
            status: ChangeStatus::Pending,
            created: now_ms(),
            finished: None,
            error: None,
            local,
        };
        self.store_change(request.clone());
        let wire = Request::Change(ChangeWire { id: request.id.clone(), kind, path, size, base_modified });
        let node = self.clone();
        let id = request.id.clone();
        let peer = peer.to_owned();
        self.spawn(async move {
            let error = match node.request(&peer, &wire).await {
                Ok(Response::Ok) => return,
                Ok(Response::Error { message }) => message,
                Ok(other) => format!("unexpected answer: {other:?}"),
                Err(e) => e.to_string(),
            };
            node.update_change(&id, |c| {
                c.status = ChangeStatus::Failed;
                c.error = Some(error);
                c.finished = Some(now_ms());
            });
        });
        Ok(request)
    }

    /// The edited copy, served to the owner after they approved.
    pub(crate) fn change_source(&self, peer: &str, id: &str) -> Result<PathBuf> {
        self.inner
            .changes
            .lock()
            .unwrap()
            .items
            .iter()
            .find(|c| c.id == id && c.peer == peer && c.direction == Direction::Outgoing && c.kind == ChangeKind::Replace)
            .and_then(|c| c.local.clone())
            .ok_or_else(|| anyhow!("change request not found"))
    }

    pub(crate) fn on_change_result(&self, peer: &str, id: &str, outcome: ChangeOutcome, error: Option<String>) {
        let updated = {
            let mut changes = self.inner.changes.lock().unwrap();
            let Some(c) = changes.items.iter_mut().find(|c| c.id == id && c.peer == peer && c.direction == Direction::Outgoing)
            else {
                return;
            };
            c.status = match outcome {
                ChangeOutcome::Applied => ChangeStatus::Applied,
                ChangeOutcome::Rejected => ChangeStatus::Rejected,
                ChangeOutcome::Failed => ChangeStatus::Failed,
            };
            c.error = error;
            c.finished = Some(now_ms());
            let c = c.clone();
            changes.save();
            c
        };
        if updated.status == ChangeStatus::Applied {
            // The owner now has this version; the edit is finished.
            let mut edits = self.inner.edits.lock().unwrap();
            edits.items.retain(|e| !(e.peer == peer && e.path == updated.path));
            edits.save();
        }
        self.emit_changes();
        self.emit_edits();
        self.emit(Event::Notice(Notice::ChangeAnswered { change: updated }));
    }

    // ---- change requests: owner side -------------------------------------

    pub(crate) fn on_change(&self, peer: &str, wire: ChangeWire) -> Response {
        if !valid_wire_id(&wire.id) {
            return Response::Error { message: "invalid request".into() };
        }
        let Some(path) = sanitize_rel(&wire.path) else {
            return Response::Error { message: "invalid path".into() };
        };
        let target = match self.shared_path(peer, &path) {
            Ok((_, target)) => target,
            Err(e) => return Response::Error { message: e.to_string() },
        };
        let Ok(meta) = std::fs::metadata(&target) else {
            return Response::Error { message: "file not found".into() };
        };
        if wire.kind == ChangeKind::Replace && meta.is_dir() {
            return Response::Error { message: "a folder cannot be replaced".into() };
        }
        if wire.kind == ChangeKind::Replace && wire.size.is_none() {
            return Response::Error { message: "the new version has no size".into() };
        }
        {
            let changes = self.inner.changes.lock().unwrap();
            if changes.items.iter().any(|c| c.id == wire.id) {
                return Response::Ok;
            }
            let waiting: Vec<&ChangeRequest> = changes
                .items
                .iter()
                .filter(|c| c.direction == Direction::Incoming && c.status == ChangeStatus::Pending)
                .collect();
            if !self.is_trusted(peer)
                && (waiting.iter().filter(|c| c.peer == peer).count() >= MAX_WAITING_PER_PEER || waiting.len() >= MAX_WAITING)
            {
                return Response::Error { message: "too many requests are waiting for an answer".into() };
            }
        }
        let request = ChangeRequest {
            id: wire.id,
            peer: peer.to_owned(),
            peer_name: self.peer_name(peer),
            direction: Direction::Incoming,
            kind: wire.kind,
            path,
            size: wire.size,
            base_modified: wire.base_modified,
            conflict: wire.kind == ChangeKind::Replace && wire.base_modified.is_some_and(|b| b != mtime_ms(&meta)),
            status: ChangeStatus::Pending,
            created: now_ms(),
            finished: None,
            error: None,
            local: None,
        };
        self.store_change(request.clone());
        self.emit(Event::Notice(Notice::ChangeRequested { change: request }));
        Response::Ok
    }

    pub fn approve_change(&self, id: &str) -> Result<()> {
        let request = self
            .inner
            .changes
            .lock()
            .unwrap()
            .items
            .iter()
            .find(|c| c.id == id && c.direction == Direction::Incoming && c.status == ChangeStatus::Pending)
            .cloned()
            .ok_or_else(|| anyhow!("request not found"))?;
        self.update_change(id, |c| c.status = ChangeStatus::Applying);
        let node = self.clone();
        self.spawn(async move {
            let result = node.apply_change(&request).await;
            let (outcome, error) = match &result {
                Ok(()) => (ChangeOutcome::Applied, None),
                Err(e) => (ChangeOutcome::Failed, Some(e.to_string())),
            };
            node.update_change(&request.id, |c| {
                c.status = if result.is_ok() { ChangeStatus::Applied } else { ChangeStatus::Failed };
                c.error = error.clone();
                c.finished = Some(now_ms());
            });
            node.notify_peer(&request.peer, Request::ChangeResult { id: request.id.clone(), outcome, error });
        });
        Ok(())
    }

    pub fn reject_change(&self, id: &str) -> Result<()> {
        let peer = {
            let changes = self.inner.changes.lock().unwrap();
            changes
                .items
                .iter()
                .find(|c| c.id == id && c.direction == Direction::Incoming && c.status == ChangeStatus::Pending)
                .map(|c| c.peer.clone())
                .ok_or_else(|| anyhow!("request not found"))?
        };
        self.update_change(id, |c| {
            c.status = ChangeStatus::Rejected;
            c.finished = Some(now_ms());
        });
        self.notify_peer(&peer, Request::ChangeResult { id: id.to_owned(), outcome: ChangeOutcome::Rejected, error: None });
        Ok(())
    }

    async fn apply_change(&self, request: &ChangeRequest) -> Result<()> {
        // Checked again: the folder may have changed since the request came.
        let (_, target) = self.shared_path(&request.peer, &request.path)?;
        match request.kind {
            ChangeKind::Replace => {
                // The owner approved a file of this size; nothing else is accepted.
                let size = request.size.ok_or_else(|| anyhow!("the request has no file size"))?;
                let part = part_path(&target);
                let _ = tokio::fs::remove_file(&part).await;
                let pulled = self
                    .pull_file(&request.peer, FetchTarget::Change { request_id: request.id.clone() }, &part, Some(size), |_| {})
                    .await;
                if let Err(e) = pulled {
                    let _ = tokio::fs::remove_file(&part).await;
                    return Err(e);
                }
                if tokio::fs::try_exists(&target).await.unwrap_or(false) {
                    if let Err(e) = self.remove_to_bin(target.clone()).await {
                        let _ = tokio::fs::remove_file(&part).await;
                        return Err(e);
                    }
                }
                tokio::fs::rename(&part, &target).await?;
                if !self.is_trusted(&request.peer) {
                    fs_util::mark_from_network(&target);
                }
            }
            ChangeKind::Delete => {
                if !tokio::fs::try_exists(&target).await.unwrap_or(false) {
                    bail!("file not found");
                }
                self.remove_to_bin(target).await?;
            }
        }
        Ok(())
    }

    /// Removes a file or folder without destroying it.
    async fn remove_to_bin(&self, path: PathBuf) -> Result<()> {
        // Android has no recycle bin; it keeps removed files like tests do.
        #[cfg(not(target_os = "android"))]
        if self.inner.use_recycle_bin {
            return tokio::task::spawn_blocking(move || trash::delete(&path))
                .await?
                .map_err(|e| anyhow!("could not move to Recycle Bin: {e}"));
        }
        let bin = self.data_dir().join("removed").join(now_ms().to_string());
        tokio::fs::create_dir_all(&bin).await?;
        let name = path.file_name().ok_or_else(|| anyhow!("invalid path"))?;
        tokio::fs::rename(&path, bin.join(name)).await?;
        Ok(())
    }

    fn store_change(&self, request: ChangeRequest) {
        {
            let mut changes = self.inner.changes.lock().unwrap();
            changes.items.push(request);
            if changes.items.len() > MAX_CHANGES_KEPT {
                let excess = changes.items.len() - MAX_CHANGES_KEPT;
                let mut dropped = 0;
                changes.items.retain(|c| {
                    let old = dropped < excess && !matches!(c.status, ChangeStatus::Pending | ChangeStatus::Applying);
                    if old {
                        dropped += 1;
                    }
                    !old
                });
            }
            changes.save();
        }
        self.emit_changes();
    }

    fn update_change(&self, id: &str, f: impl FnOnce(&mut ChangeRequest)) {
        {
            let mut changes = self.inner.changes.lock().unwrap();
            let Some(c) = changes.items.iter_mut().find(|c| c.id == id) else { return };
            f(c);
            changes.save();
        }
        self.emit_changes();
    }

    fn emit_changes(&self) {
        self.emit(Event::Changes(self.changes()));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use crate::{Node, NodeOptions};

    #[tokio::test(flavor = "multi_thread")]
    async fn links_out_of_the_shared_folder_are_refused() {
        let dir = std::env::temp_dir().join(format!("ropeway-link-{}", uuid::Uuid::new_v4()));
        let node = Node::start(NodeOptions { data_dir: dir.join("data"), port: Some(0), discovery: false, use_recycle_bin: false })
            .await
            .unwrap();
        node.set_shared_dir(Some(dir.join("shared"))).unwrap();
        std::fs::create_dir_all(dir.join("secret")).unwrap();
        std::fs::write(dir.join("secret/passwords.txt"), b"secret").unwrap();
        std::fs::write(dir.join("shared/ok.txt"), b"ok").unwrap();
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.join("shared").join("outside"))
            .arg(dir.join("secret"))
            .output()
            .unwrap();
        assert!(out.status.success(), "mklink failed");

        let peer = "0123456789abcdef0123456789abcdef";
        assert!(node.shared_file(peer, "ok.txt").is_ok());
        assert!(node.shared_file(peer, "outside/passwords.txt").is_err());
        assert!(node.shared_file(peer, "../secret/passwords.txt").is_err());
        node.shutdown().await;
    }
}
