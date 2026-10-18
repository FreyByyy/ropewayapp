//! File transfers: files sent from one device to another, and copies taken
//! from someone's shared folder.
//!
//! The receiver always pulls. An offer only lists the files; once accepted,
//! the receiver fetches each file from the byte offset it already has on disk
//! (`*.ropeway-part`), so a dropped connection or a restart on either side
//! resumes instead of starting over.

use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::fs_util::{self, join_rel, mtime_ms, part_path, sanitize_rel, unique_path};
use crate::identity::is_valid_id;
use crate::node::{now_ms, Event, Node, Notice};
use crate::protocol::{read_frame, write_frame, FetchTarget, OfferFile, OfferWire, Request, Response};
use crate::settings::write_atomic;

const CHUNK: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const SAVE_INTERVAL: Duration = Duration::from_secs(2);
const HEADER_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_KEPT: usize = 300;
const MAX_OFFER_FILES: usize = 200_000;
/// Undecided offers kept from one untrusted device, and in total.
pub(crate) const MAX_WAITING_PER_PEER: usize = 5;
pub(crate) const MAX_WAITING: usize = 50;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Incoming,
    Outgoing,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    /// Files sent from one device to another.
    Offer,
    /// Copy from someone's shared folder.
    Download,
    /// Copy of a shared file taken for editing.
    Edit,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    /// Outgoing offer waiting for the receiver's decision.
    Waiting,
    /// Incoming offer waiting for this user's decision.
    Pending,
    Active,
    /// Connection lost; resumes when the device is back.
    Interrupted,
    Completed,
    Rejected,
    Cancelled,
    /// Outgoing offer that could not be delivered.
    Failed,
}

impl TransferStatus {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Completed | Self::Rejected | Self::Cancelled | Self::Failed)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    /// Relative path, `/`-separated, starting with the top-level item's name.
    pub path: String,
    pub size: u64,
    #[serde(default)]
    pub done: u64,
    /// Outgoing: the source file. Incoming: where the file is written.
    #[serde(default)]
    pub local: Option<PathBuf>,
    /// Outgoing: modification time when offered, to detect later edits.
    #[serde(default)]
    pub mtime: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transfer {
    pub id: String,
    pub peer: String,
    pub peer_name: String,
    pub direction: Direction,
    pub kind: TransferKind,
    pub status: TransferStatus,
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub dirs: Vec<String>,
    #[serde(default)]
    pub local_dirs: Vec<PathBuf>,
    /// Incoming: final location of each top-level item.
    #[serde(default)]
    pub roots_local: Vec<PathBuf>,
    pub created: i64,
    #[serde(default)]
    pub finished: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub dest: Option<PathBuf>,
    /// Download/Edit: folder in the owner's shared folder that `files[].path`
    /// is relative to.
    #[serde(default)]
    pub remote_base: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct TransferView {
    pub id: String,
    pub peer: String,
    pub peer_name: String,
    pub direction: Direction,
    pub kind: TransferKind,
    pub status: TransferStatus,
    pub file_count: usize,
    /// Top-level items (files or folders), at most a few.
    pub names: Vec<String>,
    pub root_count: usize,
    pub total: u64,
    pub transferred: u64,
    /// Bytes per second while active.
    pub speed: u64,
    pub current: Option<String>,
    pub created: i64,
    pub finished: Option<i64>,
    pub error: Option<String>,
    /// What "Open" should show: the single received item, or the folder.
    pub open_path: Option<String>,
}

impl Transfer {
    pub fn total(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn transferred(&self) -> u64 {
        self.files.iter().map(|f| f.done.min(f.size)).sum()
    }

    fn roots(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for p in self.files.iter().map(|f| f.path.as_str()).chain(self.dirs.iter().map(String::as_str)) {
            let root = fs_util::first_component(p);
            if seen.insert(root.to_owned()) {
                out.push(root.to_owned());
            }
        }
        out
    }

    fn view(&self, speed: u64, current: Option<String>) -> TransferView {
        let roots = self.roots();
        let names: Vec<String> = if self.roots_local.is_empty() {
            roots.iter().take(4).cloned().collect()
        } else {
            self.roots_local
                .iter()
                .take(4)
                .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                .collect()
        };
        let open_path = match self.direction {
            Direction::Incoming if self.roots_local.len() == 1 => Some(&self.roots_local[0]),
            Direction::Incoming => self.dest.as_ref(),
            Direction::Outgoing => None,
        };
        TransferView {
            id: self.id.clone(),
            peer: self.peer.clone(),
            peer_name: self.peer_name.clone(),
            direction: self.direction,
            kind: self.kind,
            status: self.status,
            file_count: self.files.len(),
            names,
            root_count: roots.len(),
            total: self.total(),
            transferred: self.transferred(),
            speed: if self.status == TransferStatus::Active { speed } else { 0 },
            current: if self.status == TransferStatus::Active { current } else { None },
            created: self.created,
            finished: self.finished,
            error: self.error.clone(),
            open_path: open_path.map(|p| p.to_string_lossy().into_owned()),
        }
    }

    fn remove_parts(&self) {
        if self.direction != Direction::Incoming {
            return;
        }
        for f in &self.files {
            if let Some(local) = &f.local {
                let _ = std::fs::remove_file(part_path(local));
            }
        }
    }
}

/// Assigns the on-disk location of every incoming file. With `unique`, a
/// top-level item whose name is taken gets " (1)" and so on.
pub(crate) fn prepare_targets(t: &mut Transfer, dest: &Path, unique: bool) {
    let mut map: HashMap<String, PathBuf> = HashMap::new();
    t.roots_local.clear();
    for root in t.roots() {
        let path = dest.join(&root);
        let path = if unique { unique_path(path) } else { path };
        t.roots_local.push(path.clone());
        map.insert(root, path);
    }
    let locate = |rel: &str| -> PathBuf {
        let root = fs_util::first_component(rel);
        let base = map[root].clone();
        match rel.split_once('/') {
            Some((_, rest)) => join_rel(&base, rest),
            None => base,
        }
    };
    for f in &mut t.files {
        f.local = Some(locate(&f.path));
    }
    t.local_dirs = t.dirs.iter().map(|d| locate(d)).collect();
    t.dest = Some(dest.to_path_buf());
}

pub(crate) struct TransferStore {
    path: PathBuf,
    pub(crate) list: Vec<Transfer>,
    last_save: Option<Instant>,
}

impl TransferStore {
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut list: Vec<Transfer> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for t in &mut list {
            if t.direction == Direction::Incoming && t.status == TransferStatus::Active {
                t.status = TransferStatus::Interrupted;
            }
        }
        Self { path, list, last_save: None }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Transfer> {
        self.list.iter().find(|t| t.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut Transfer> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    pub(crate) fn save(&mut self) {
        if self.list.len() > MAX_KEPT {
            let mut excess = self.list.len() - MAX_KEPT;
            self.list.retain(|t| {
                if excess > 0 && t.status.is_final() {
                    excess -= 1;
                    false
                } else {
                    true
                }
            });
        }
        if let Ok(bytes) = serde_json::to_vec(&self.list) {
            if let Err(e) = write_atomic(&self.path, &bytes) {
                tracing::warn!("saving transfers: {e}");
            }
        }
        self.last_save = Some(Instant::now());
    }

    fn save_throttled(&mut self) {
        if self.last_save.is_none_or(|t| t.elapsed() >= SAVE_INTERVAL) {
            self.save();
        }
    }
}

#[derive(Default)]
pub(crate) struct Progress {
    last: Option<Instant>,
    last_bytes: u64,
    speed: u64,
    current: Option<String>,
}

pub(crate) fn valid_wire_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

struct FetchSource {
    path: PathBuf,
    /// Size and mtime the file must still have.
    expect: Option<(u64, i64)>,
    /// Outgoing transfer and file index, for progress.
    offer: Option<(String, usize)>,
}

impl Node {
    pub fn transfers(&self) -> Vec<TransferView> {
        let store = self.inner.transfers.lock().unwrap();
        store.list.iter().map(|t| self.view_of(t)).collect()
    }

    /// Offers files and folders to a device.
    pub async fn send_paths(&self, peer: &str, paths: Vec<PathBuf>) -> Result<TransferView> {
        if !is_valid_id(peer) {
            bail!("unknown device");
        }
        if paths.is_empty() {
            bail!("nothing to send");
        }
        let (files, dirs) = tokio::task::spawn_blocking(move || fs_util::walk(&paths)).await??;
        if files.is_empty() && dirs.is_empty() {
            bail!("nothing to send");
        }
        let t = Transfer {
            id: uuid::Uuid::new_v4().to_string(),
            peer: peer.to_owned(),
            peer_name: self.peer_name(peer),
            direction: Direction::Outgoing,
            kind: TransferKind::Offer,
            status: TransferStatus::Waiting,
            files: files
                .into_iter()
                .map(|f| FileEntry { path: f.rel, size: f.size, done: 0, local: Some(f.abs), mtime: f.mtime })
                .collect(),
            dirs,
            local_dirs: Vec::new(),
            roots_local: Vec::new(),
            created: now_ms(),
            finished: None,
            error: None,
            dest: None,
            remote_base: String::new(),
        };
        let id = t.id.clone();
        let view = self.insert_transfer(t);
        self.spawn_offer(id);
        Ok(view)
    }

    pub fn accept_transfer(&self, id: &str) -> Result<()> {
        let dest = self.settings().download_dir();
        std::fs::create_dir_all(&dest)?;
        let peer = {
            let mut store = self.inner.transfers.lock().unwrap();
            let t = store.get_mut(id).ok_or_else(|| anyhow!("transfer not found"))?;
            if t.direction != Direction::Incoming || t.status != TransferStatus::Pending {
                return Ok(());
            }
            prepare_targets(t, &dest, true);
            t.status = TransferStatus::Active;
            let peer = t.peer.clone();
            store.save();
            peer
        };
        self.emit_transfer(id);
        self.notify_peer(&peer, Request::OfferAnswer { id: id.to_owned(), accepted: true });
        self.start_run(id);
        Ok(())
    }

    pub fn reject_transfer(&self, id: &str) -> Result<()> {
        let peer = {
            let mut store = self.inner.transfers.lock().unwrap();
            let t = store.get_mut(id).ok_or_else(|| anyhow!("transfer not found"))?;
            if t.status != TransferStatus::Pending {
                return Ok(());
            }
            t.status = TransferStatus::Rejected;
            t.finished = Some(now_ms());
            let peer = t.peer.clone();
            store.save();
            peer
        };
        self.emit_transfer(id);
        self.notify_peer(&peer, Request::OfferAnswer { id: id.to_owned(), accepted: false });
        Ok(())
    }

    pub fn cancel_transfer(&self, id: &str) -> Result<()> {
        let pending = self
            .inner
            .transfers
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|t| t.status == TransferStatus::Pending);
        if pending {
            return self.reject_transfer(id);
        }
        let (peer, kind) = {
            let mut store = self.inner.transfers.lock().unwrap();
            let t = store.get_mut(id).ok_or_else(|| anyhow!("transfer not found"))?;
            if t.status.is_final() {
                return Ok(());
            }
            t.status = TransferStatus::Cancelled;
            t.finished = Some(now_ms());
            t.remove_parts();
            let r = (t.peer.clone(), t.kind);
            store.save();
            r
        };
        if let Some(task) = self.inner.running.lock().unwrap().remove(id) {
            task.abort();
        }
        self.emit_transfer(id);
        if kind == TransferKind::Offer {
            self.notify_peer(&peer, Request::TransferCancel { id: id.to_owned() });
        }
        Ok(())
    }

    pub fn retry_transfer(&self, id: &str) -> Result<()> {
        let (direction, status) = {
            let store = self.inner.transfers.lock().unwrap();
            let t = store.get(id).ok_or_else(|| anyhow!("transfer not found"))?;
            (t.direction, t.status)
        };
        match (direction, status) {
            (Direction::Outgoing, TransferStatus::Failed) => {
                self.set_status(id, TransferStatus::Waiting, None);
                self.spawn_offer(id.to_owned());
            }
            (Direction::Incoming, TransferStatus::Interrupted) => {
                self.set_status(id, TransferStatus::Active, None);
                self.start_run(id);
            }
            _ => {}
        }
        Ok(())
    }

    /// Clears finished transfers of one device from the list (files stay).
    pub fn clear_transfers(&self, peer: &str) {
        let mut store = self.inner.transfers.lock().unwrap();
        store.list.retain(|t| !(t.peer == peer && t.status.is_final()));
        store.save();
    }

    // ---- bookkeeping ---------------------------------------------------

    pub(crate) fn insert_transfer(&self, t: Transfer) -> TransferView {
        let view = self.view_of(&t);
        {
            let mut store = self.inner.transfers.lock().unwrap();
            store.list.push(t);
            store.save();
        }
        self.emit(Event::Transfer(view.clone()));
        view
    }

    fn view_of(&self, t: &Transfer) -> TransferView {
        let progress = self.inner.progress.lock().unwrap();
        let p = progress.get(&t.id);
        t.view(p.map_or(0, |p| p.speed), p.and_then(|p| p.current.clone()))
    }

    pub(crate) fn transfer_view(&self, id: &str) -> Option<TransferView> {
        let store = self.inner.transfers.lock().unwrap();
        store.get(id).map(|t| self.view_of(t))
    }

    pub(crate) fn emit_transfer(&self, id: &str) {
        if let Some(view) = self.transfer_view(id) {
            self.emit(Event::Transfer(view));
        }
    }

    fn set_status(&self, id: &str, status: TransferStatus, error: Option<String>) {
        {
            let mut store = self.inner.transfers.lock().unwrap();
            let Some(t) = store.get_mut(id) else { return };
            t.status = status;
            t.error = error;
            if status.is_final() {
                t.finished = Some(now_ms());
            }
            store.save();
        }
        self.emit_transfer(id);
    }

    pub(crate) fn notify_peer(&self, peer: &str, request: Request) {
        let node = self.clone();
        let peer = peer.to_owned();
        self.spawn(async move {
            if let Err(e) = node.request(&peer, &request).await {
                tracing::debug!("notifying {peer}: {e}");
            }
        });
    }

    fn spawn_offer(&self, id: String) {
        let node = self.clone();
        self.spawn(async move {
            let Some((peer, wire)) = node.inner.transfers.lock().unwrap().get(&id).map(|t| {
                (
                    t.peer.clone(),
                    OfferWire {
                        id: t.id.clone(),
                        files: t.files.iter().map(|f| OfferFile { path: f.path.clone(), size: f.size }).collect(),
                        dirs: t.dirs.clone(),
                    },
                )
            }) else {
                return;
            };
            let error = match node.request(&peer, &Request::Offer(wire)).await {
                Ok(Response::Ok) => return,
                Ok(Response::Error { message }) => message,
                Ok(other) => format!("unexpected answer: {other:?}"),
                Err(e) => e.to_string(),
            };
            let still_waiting = node
                .inner
                .transfers
                .lock()
                .unwrap()
                .get(&id)
                .is_some_and(|t| t.status == TransferStatus::Waiting);
            if still_waiting {
                node.set_status(&id, TransferStatus::Failed, Some(error));
            }
        });
    }

    /// Called when a device (re)connects: resume what was cut off and
    /// re-send offers that could not be delivered.
    pub(crate) fn resume_transfers(&self, peer: &str) {
        let (resume, resend): (Vec<String>, Vec<String>) = {
            let store = self.inner.transfers.lock().unwrap();
            let resume = store
                .list
                .iter()
                .filter(|t| t.peer == peer && t.direction == Direction::Incoming && t.status == TransferStatus::Interrupted)
                .map(|t| t.id.clone())
                .collect();
            let resend = store
                .list
                .iter()
                .filter(|t| t.peer == peer && t.direction == Direction::Outgoing && t.status == TransferStatus::Failed)
                .map(|t| t.id.clone())
                .collect();
            (resume, resend)
        };
        for id in resume.iter().chain(resend.iter()) {
            let _ = self.retry_transfer(id);
        }
    }

    // ---- incoming side ---------------------------------------------------

    pub(crate) fn on_offer(&self, peer: &str, wire: OfferWire) -> Response {
        if !valid_wire_id(&wire.id) || wire.files.len() > MAX_OFFER_FILES {
            return Response::Error { message: "invalid offer".into() };
        }
        let mut files = Vec::with_capacity(wire.files.len());
        for f in wire.files {
            let Some(path) = sanitize_rel(&f.path) else {
                return Response::Error { message: format!("invalid file name: {}", f.path) };
            };
            files.push(FileEntry { path, size: f.size, done: 0, local: None, mtime: 0 });
        }
        let dirs: Vec<String> = wire.dirs.iter().filter_map(|d| sanitize_rel(d)).collect();
        if files.is_empty() && dirs.is_empty() {
            return Response::Error { message: "empty offer".into() };
        }
        {
            let store = self.inner.transfers.lock().unwrap();
            if let Some(existing) = store.get(&wire.id) {
                return if existing.peer == peer {
                    Response::Ok
                } else {
                    Response::Error { message: "duplicate transfer id".into() }
                };
            }
            // Offers waiting for the user's decision are kept until answered,
            // so their number is limited.
            let waiting = |p: Option<&str>| {
                store
                    .list
                    .iter()
                    .filter(|t| t.direction == Direction::Incoming && t.status == TransferStatus::Pending)
                    .filter(|t| p.is_none_or(|p| t.peer == p))
                    .count()
            };
            if !self.is_trusted(peer) && (waiting(Some(peer)) >= MAX_WAITING_PER_PEER || waiting(None) >= MAX_WAITING) {
                return Response::Error { message: "too many offers are waiting for an answer".into() };
            }
        }

        let t = Transfer {
            id: wire.id.clone(),
            peer: peer.to_owned(),
            peer_name: self.peer_name(peer),
            direction: Direction::Incoming,
            kind: TransferKind::Offer,
            status: TransferStatus::Pending,
            files,
            dirs,
            local_dirs: Vec::new(),
            roots_local: Vec::new(),
            created: now_ms(),
            finished: None,
            error: None,
            dest: None,
            remote_base: String::new(),
        };
        let view = self.insert_transfer(t);
        if self.is_trusted(peer) {
            if let Err(e) = self.accept_transfer(&wire.id) {
                tracing::warn!("auto-accept failed: {e}");
            }
        } else {
            self.emit(Event::Notice(Notice::OfferReceived { transfer: view }));
        }
        Response::Ok
    }

    pub(crate) fn on_offer_answer(&self, peer: &str, id: &str, accepted: bool) {
        let changed = {
            let mut store = self.inner.transfers.lock().unwrap();
            match store.get_mut(id) {
                Some(t) if t.peer == peer && t.direction == Direction::Outgoing && !t.status.is_final() => {
                    if accepted {
                        if t.status == TransferStatus::Waiting {
                            t.status = TransferStatus::Active;
                        }
                    } else {
                        t.status = TransferStatus::Rejected;
                        t.finished = Some(now_ms());
                    }
                    store.save();
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.emit_transfer(id);
        }
    }

    pub(crate) fn on_transfer_done(&self, peer: &str, id: &str) {
        let changed = {
            let mut store = self.inner.transfers.lock().unwrap();
            match store.get_mut(id) {
                Some(t) if t.peer == peer && t.direction == Direction::Outgoing => {
                    t.status = TransferStatus::Completed;
                    t.finished = Some(now_ms());
                    t.error = None;
                    for f in &mut t.files {
                        f.done = f.size;
                    }
                    store.save();
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.inner.progress.lock().unwrap().remove(id);
            self.emit_transfer(id);
        }
    }

    pub(crate) fn on_transfer_cancel(&self, peer: &str, id: &str) {
        let changed = {
            let mut store = self.inner.transfers.lock().unwrap();
            match store.get_mut(id) {
                Some(t) if t.peer == peer && !t.status.is_final() => {
                    t.status = TransferStatus::Cancelled;
                    t.finished = Some(now_ms());
                    t.remove_parts();
                    store.save();
                    true
                }
                _ => false,
            }
        };
        if changed {
            if let Some(task) = self.inner.running.lock().unwrap().remove(id) {
                task.abort();
            }
            self.emit_transfer(id);
        }
    }

    pub(crate) fn start_run(&self, id: &str) {
        let mut running = self.inner.running.lock().unwrap();
        if running.contains_key(id) {
            return;
        }
        let node = self.clone();
        let task_id = id.to_owned();
        let mut stop = self.inner.stop.subscribe();
        let handle = self.inner.rt.spawn(async move {
            tokio::select! {
                _ = node.clone().run_incoming(task_id) => {}
                _ = stop.wait_for(|s| *s) => {}
            }
        });
        running.insert(id.to_owned(), handle.abort_handle());
    }

    async fn run_incoming(self, id: String) {
        let result = self.run_incoming_files(&id).await;
        self.inner.running.lock().unwrap().remove(&id);
        let kind = match result {
            Ok(()) => {
                let kind = {
                    let mut store = self.inner.transfers.lock().unwrap();
                    let Some(t) = store.get_mut(&id) else { return };
                    t.status = TransferStatus::Completed;
                    t.finished = Some(now_ms());
                    t.error = None;
                    let r = (t.kind, t.peer.clone());
                    store.save();
                    r
                };
                self.inner.progress.lock().unwrap().remove(&id);
                Some(kind)
            }
            Err(e) => {
                tracing::info!("transfer {id} stopped: {e}");
                let mut store = self.inner.transfers.lock().unwrap();
                if let Some(t) = store.get_mut(&id) {
                    if t.status == TransferStatus::Active {
                        t.status = TransferStatus::Interrupted;
                        t.error = Some(e.to_string());
                        store.save();
                    }
                }
                None
            }
        };
        self.emit_transfer(&id);
        match kind {
            Some((TransferKind::Offer, peer)) => {
                self.notify_peer(&peer, Request::TransferDone { id: id.clone() });
                if let Some(view) = self.transfer_view(&id) {
                    self.emit(Event::Notice(Notice::FilesReceived { transfer: view }));
                }
            }
            Some((TransferKind::Edit, _)) => self.on_edit_downloaded(&id),
            _ => {}
        }
    }

    async fn run_incoming_files(&self, id: &str) -> Result<()> {
        let t = self
            .inner
            .transfers
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("transfer not found"))?;
        for dir in &t.local_dirs {
            tokio::fs::create_dir_all(dir).await?;
        }
        for (index, file) in t.files.iter().enumerate() {
            let target = file.local.clone().ok_or_else(|| anyhow!("no destination for {}", file.path))?;
            if file.done >= file.size && tokio::fs::try_exists(&target).await.unwrap_or(false) {
                continue;
            }
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let part = part_path(&target);
            if let Some(p) = self.inner.progress.lock().unwrap().get_mut(id) {
                p.current = Some(file.path.clone());
            }
            let fetch = match t.kind {
                TransferKind::Offer => FetchTarget::Offer { transfer_id: id.to_owned(), index },
                TransferKind::Download | TransferKind::Edit => FetchTarget::Shared {
                    path: fs_util::join_remote(&t.remote_base, &file.path),
                },
            };
            self.pull_file(&t.peer, fetch, &part, Some(file.size), |done| self.incoming_progress(id, index, &file.path, done))
                .await?;

            let final_target = if tokio::fs::try_exists(&target).await.unwrap_or(false) {
                if t.kind == TransferKind::Edit {
                    tokio::fs::remove_file(&target).await?;
                    target.clone()
                } else {
                    unique_path(target.clone())
                }
            } else {
                target.clone()
            };
            tokio::fs::rename(&part, &final_target).await?;
            if !self.is_trusted(&t.peer) {
                fs_util::mark_from_network(&final_target);
            }

            let mut store = self.inner.transfers.lock().unwrap();
            if let Some(f) = store.get_mut(id).and_then(|t| t.files.get_mut(index)) {
                f.done = f.size;
                f.local = Some(final_target);
            }
            store.save_throttled();
        }
        Ok(())
    }

    /// Fetches one file into `part`, continuing from its current length.
    /// `expected_size: None` accepts whatever size the other side reports.
    pub(crate) async fn pull_file(
        &self,
        peer: &str,
        target: FetchTarget,
        part: &Path,
        expected_size: Option<u64>,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64> {
        let mut offset = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
        if expected_size.is_some_and(|s| offset > s) || expected_size.is_none() {
            offset = 0;
        }
        let conn = self.connection(peer).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        write_frame(&mut send, &Request::Fetch { target, offset }).await?;
        send.finish()?;
        let header: Response = tokio::time::timeout(HEADER_TIMEOUT, read_frame(&mut recv))
            .await
            .map_err(|_| anyhow!("no answer from device"))??;
        let size = match header {
            Response::FileStart { size } => size,
            Response::Error { message } => bail!(message),
            other => bail!("unexpected answer: {other:?}"),
        };
        if let Some(expected) = expected_size {
            if size != expected {
                bail!("the file was changed on the other device");
            }
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(offset == 0)
            .open(part)
            .await?;
        file.seek(SeekFrom::Start(offset)).await?;
        let mut done = offset;
        progress(done);
        let mut buf = vec![0u8; CHUNK];
        loop {
            let read = tokio::time::timeout(READ_TIMEOUT, recv.read(&mut buf))
                .await
                .map_err(|_| anyhow!("connection stalled"))??;
            let Some(n) = read else { break };
            if done + n as u64 > size {
                // Never write more than was announced (and agreed to).
                bail!("the device sent more data than announced");
            }
            file.write_all(&buf[..n]).await?;
            done += n as u64;
            progress(done);
        }
        file.flush().await?;
        if done != size {
            bail!("transfer was cut off");
        }
        Ok(size)
    }

    fn incoming_progress(&self, id: &str, index: usize, path: &str, done: u64) {
        {
            let mut store = self.inner.transfers.lock().unwrap();
            if let Some(f) = store.get_mut(id).and_then(|t| t.files.get_mut(index)) {
                f.done = done;
            }
        }
        self.tick_progress(id, Some(path));
    }

    /// Updates speed and emits at most every [`PROGRESS_INTERVAL`].
    fn tick_progress(&self, id: &str, current: Option<&str>) {
        let due = {
            let mut progress = self.inner.progress.lock().unwrap();
            let p = progress.entry(id.to_owned()).or_default();
            if let Some(c) = current {
                if p.current.as_deref() != Some(c) {
                    p.current = Some(c.to_owned());
                }
            }
            p.last.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL)
        };
        if !due {
            return;
        }
        let total = self.inner.transfers.lock().unwrap().get(id).map(Transfer::transferred);
        let Some(total) = total else { return };
        {
            let mut progress = self.inner.progress.lock().unwrap();
            let p = progress.entry(id.to_owned()).or_default();
            if let Some(last) = p.last {
                let secs = last.elapsed().as_secs_f64().max(0.001);
                let instant = (total.saturating_sub(p.last_bytes) as f64 / secs) as u64;
                p.speed = if p.speed == 0 { instant } else { (p.speed * 2 + instant) / 3 };
            }
            p.last = Some(Instant::now());
            p.last_bytes = total;
        }
        self.emit_transfer(id);
    }

    // ---- outgoing side ---------------------------------------------------

    pub(crate) async fn serve_fetch(
        &self,
        peer: &str,
        target: FetchTarget,
        offset: u64,
        mut send: quinn::SendStream,
    ) -> Result<()> {
        let source = match self.resolve_fetch(peer, &target) {
            Ok(s) => s,
            Err(e) => return refuse(&mut send, e.to_string()).await,
        };
        let mut file = match tokio::fs::File::open(&source.path).await {
            Ok(f) => f,
            Err(e) => return refuse(&mut send, format!("cannot open file: {e}")).await,
        };
        let meta = file.metadata().await?;
        if let Some((size, mtime)) = source.expect {
            if meta.len() != size || (mtime != 0 && mtime_ms(&meta) != mtime) {
                return refuse(&mut send, "the file was changed after it was sent".into()).await;
            }
        }
        let size = meta.len();
        let offset = offset.min(size);
        file.seek(SeekFrom::Start(offset)).await?;
        write_frame(&mut send, &Response::FileStart { size }).await?;

        let mut sent = offset;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            send.write_all(&buf[..n]).await?;
            sent += n as u64;
            if let Some((id, index)) = &source.offer {
                if !self.outgoing_progress(id, *index, sent) {
                    let _ = send.reset(0u32.into());
                    bail!("transfer cancelled");
                }
            }
        }
        send.finish()?;
        Ok(())
    }

    fn resolve_fetch(&self, peer: &str, target: &FetchTarget) -> Result<FetchSource> {
        match target {
            FetchTarget::Offer { transfer_id, index } => {
                let source = {
                    let mut store = self.inner.transfers.lock().unwrap();
                    let t = store
                        .get_mut(transfer_id)
                        .filter(|t| t.peer == peer && t.direction == Direction::Outgoing)
                        .ok_or_else(|| anyhow!("transfer not found"))?;
                    if matches!(t.status, TransferStatus::Cancelled | TransferStatus::Rejected) {
                        bail!("transfer was cancelled");
                    }
                    let file = t.files.get(*index).ok_or_else(|| anyhow!("file not found"))?;
                    let source = FetchSource {
                        path: file.local.clone().ok_or_else(|| anyhow!("file not found"))?,
                        expect: Some((file.size, file.mtime)),
                        offer: Some((transfer_id.clone(), *index)),
                    };
                    if t.status != TransferStatus::Active {
                        t.status = TransferStatus::Active;
                        t.error = None;
                        store.save();
                    }
                    source
                };
                self.emit_transfer(transfer_id);
                Ok(source)
            }
            FetchTarget::Shared { path } => Ok(FetchSource { path: self.shared_file(peer, path)?, expect: None, offer: None }),
            FetchTarget::Change { request_id } => Ok(FetchSource {
                path: self.change_source(peer, request_id)?,
                expect: None,
                offer: None,
            }),
        }
    }

    /// Records bytes served for an outgoing transfer. Returns `false` if the
    /// transfer was cancelled meanwhile.
    fn outgoing_progress(&self, id: &str, index: usize, sent: u64) -> bool {
        let path = {
            let mut store = self.inner.transfers.lock().unwrap();
            let Some(t) = store.get_mut(id) else { return false };
            if t.status == TransferStatus::Cancelled {
                return false;
            }
            let Some(f) = t.files.get_mut(index) else { return false };
            f.done = sent;
            f.path.clone()
        };
        self.tick_progress(id, Some(&path));
        true
    }
}

async fn refuse(send: &mut quinn::SendStream, message: String) -> Result<()> {
    write_frame(send, &Response::Error { message }).await?;
    send.finish()?;
    Ok(())
}
