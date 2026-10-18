//! The local node: owns the QUIC endpoint, tracks peers, routes requests.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use rustls::crypto::CryptoProvider;
use serde::Serialize;
use tokio::net::UdpSocket;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch, Notify};

use crate::chat::{is_valid_conversation, ChatMessage, ChatStore, ConversationSummary, MessageStatus, GENERAL, MAX_TEXT};
use crate::discovery::{self, Announce, DISCOVERY_PORT};
use crate::identity::{short_code, Identity};
use crate::protocol::{read_frame, write_frame, ChatWire, Hello, Request, Response, DEFAULT_PORT, PROTOCOL_VERSION};
use crate::settings::{Settings, TrustedDevice};
use crate::screen::{RemoteShare, ScreenAsk, ScreenState, ScreenView};
use crate::shared::{ChangeRequest, EditSession, EditView, ListStore};
use crate::tls;
use crate::transfers::{Progress, TransferStore, TransferView};

const SETTINGS_FILE: &str = "settings.json";
const PRESENCE_TIMEOUT: Duration = Duration::from_secs(8);
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_NAME: usize = 64;
/// Time a peer gets to send a whole request after opening a stream.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Open connections in total, and from one address.
const MAX_CONNECTIONS: usize = 64;
const MAX_CONNECTIONS_PER_IP: usize = 4;
/// Devices known only from discovery that are kept at most; more are
/// ignored so fake announcements cannot flood the list.
const MAX_PEERS: usize = 256;
/// Requests that cost disk space or the user's attention (messages, offers,
/// change requests, screen asks and invites) from untrusted devices: a burst
/// of this many per address, refilled at `RATE_PER_MIN` a minute.
const RATE_BURST: f64 = 60.0;
const RATE_PER_MIN: f64 = 30.0;
/// Files an untrusted device may download from this one at the same time.
const MAX_FETCHES_UNTRUSTED: usize = 4;
/// Shared-folder listings built for peers at the same time; each walks the
/// whole folder.
const MAX_LISTINGS: usize = 2;

pub struct NodeOptions {
    pub data_dir: PathBuf,
    /// QUIC port. `None`: the standard port, or any free one if it is taken.
    pub port: Option<u16>,
    /// Broadcast discovery. Disabled in tests.
    pub discovery: bool,
    /// Where replaced and deleted shared files go: the system Recycle Bin, or
    /// (`false`, for tests) a `removed` folder in the data directory.
    pub use_recycle_bin: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PeerView {
    pub id: String,
    pub name: String,
    pub os: String,
    pub address: Option<String>,
    pub online: bool,
    pub trusted: bool,
    pub code: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct MeView {
    pub id: String,
    pub name: String,
    pub code: String,
    pub port: u16,
    pub addresses: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
    Peers(Vec<PeerView>),
    Chat(ChatMessage),
    Me(MeView),
    /// A transfer was created or changed (status or progress).
    Transfer(TransferView),
    /// The full list of change requests, after any change.
    Changes(Vec<ChangeRequest>),
    /// The full list of edited copies, after any change.
    Edits(Vec<EditView>),
    /// Screen sharing state, after any change.
    Screen(ScreenView),
    /// Something the user should hear about even with the window hidden.
    Notice(Notice),
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Notice {
    /// An untrusted device wants to send files; needs a decision.
    OfferReceived { transfer: TransferView },
    /// Files sent to this device have arrived.
    FilesReceived { transfer: TransferView },
    /// Someone asks to change or delete a file in this device's shared folder.
    ChangeRequested { change: ChangeRequest },
    /// The owner answered a change request from this device.
    ChangeAnswered { change: ChangeRequest },
    /// A copy taken for editing is ready to be opened.
    EditReady { path: String },
    /// A device shares its screen with this device.
    ScreenInvite { share: RemoteShare },
    /// A device asks to see this screen; needs a decision.
    ScreenAsked { ask: ScreenAsk },
    /// A device declined (or could not answer) this device's ask.
    ScreenAskAnswered { peer_name: String, accepted: bool },
}

/// Resolved folder settings, for display.
#[derive(Serialize, Clone, Debug)]
pub struct Folders {
    pub download_dir: String,
    pub shared_dir: String,
    pub sharing_enabled: bool,
    pub shared_trusted_only: bool,
}

struct Peer {
    name: String,
    os: String,
    /// Where to dial the device.
    addr: Option<SocketAddr>,
    /// `addr` was proven by a connection with the device's key. An
    /// unauthenticated announcement then cannot replace it, only suggest
    /// `hint`, which is tried when `addr` stops answering.
    verified: bool,
    hint: Option<SocketAddr>,
    /// Last discovery announcement or hello.
    last_seen: Option<Instant>,
}

impl Peer {
    fn new() -> Self {
        Self { name: String::new(), os: String::new(), addr: None, verified: false, hint: None, last_seen: None }
    }
}

pub(crate) struct Inner {
    identity: Identity,
    provider: Arc<CryptoProvider>,
    data_dir: PathBuf,
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) use_recycle_bin: bool,
    pub(crate) rt: Handle,
    endpoint: quinn::Endpoint,
    port: u16,
    settings: Mutex<Settings>,
    peers: Mutex<HashMap<String, Peer>>,
    conns: Mutex<HashMap<String, quinn::Connection>>,
    chat: Mutex<ChatStore>,
    events: broadcast::Sender<Event>,
    last_views: Mutex<Vec<PeerView>>,
    online: Mutex<HashSet<String>>,
    delivering: Mutex<HashSet<String>>,
    announce_now: Notify,
    /// Request allowance of untrusted addresses (see [`RATE_BURST`]).
    rate: Mutex<HashMap<IpAddr, (f64, Instant)>>,
    /// Downloads running for each untrusted device (see [`MAX_FETCHES_UNTRUSTED`]).
    fetching: Mutex<HashMap<String, usize>>,
    listings: tokio::sync::Semaphore,
    /// Set once by `shutdown`; every background task stops on it.
    pub(crate) stop: watch::Sender<bool>,
    // Lock order: transfers before progress.
    pub(crate) transfers: Mutex<TransferStore>,
    pub(crate) progress: Mutex<HashMap<String, Progress>>,
    /// Incoming transfers currently downloading.
    pub(crate) running: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    pub(crate) changes: Mutex<ListStore<ChangeRequest>>,
    pub(crate) edits: Mutex<ListStore<EditSession>>,
    pub(crate) screen: Mutex<ScreenState>,
}

#[derive(Clone)]
pub struct Node {
    pub(crate) inner: Arc<Inner>,
}

impl Node {
    /// Must be called inside a Tokio runtime.
    pub async fn start(opts: NodeOptions) -> Result<Self> {
        std::fs::create_dir_all(&opts.data_dir)?;
        let identity = Identity::load_or_create(&opts.data_dir)?;
        let settings = Settings::load(&opts.data_dir.join(SETTINGS_FILE));
        let chat = ChatStore::load(opts.data_dir.join("chats"))?;
        let transfers = TransferStore::load(opts.data_dir.join("transfers.json"));
        let changes = ListStore::load(opts.data_dir.join("changes.json"));
        let edits = ListStore::load(opts.data_dir.join("edits.json"));
        if settings.sharing_enabled {
            let _ = std::fs::create_dir_all(settings.shared_dir());
        }
        let provider = tls::provider();
        let endpoint = bind_endpoint(tls::server_config(&identity, provider.clone())?, opts.port)?;
        let port = endpoint.local_addr()?.port();
        let discovery_socket = if opts.discovery {
            match discovery::bind() {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    tracing::warn!("discovery disabled: {e}");
                    None
                }
            }
        } else {
            None
        };

        let (events, _) = broadcast::channel(1024);
        let node = Node {
            inner: Arc::new(Inner {
                identity,
                provider,
                data_dir: opts.data_dir,
                use_recycle_bin: opts.use_recycle_bin,
                rt: Handle::current(),
                endpoint,
                port,
                settings: Mutex::new(settings),
                peers: Mutex::new(HashMap::new()),
                conns: Mutex::new(HashMap::new()),
                chat: Mutex::new(chat),
                events,
                last_views: Mutex::new(Vec::new()),
                online: Mutex::new(HashSet::new()),
                delivering: Mutex::new(HashSet::new()),
                announce_now: Notify::new(),
                rate: Mutex::new(HashMap::new()),
                fetching: Mutex::new(HashMap::new()),
                listings: tokio::sync::Semaphore::new(MAX_LISTINGS),
                stop: watch::channel(false).0,
                transfers: Mutex::new(transfers),
                progress: Mutex::new(HashMap::new()),
                running: Mutex::new(HashMap::new()),
                changes: Mutex::new(changes),
                edits: Mutex::new(edits),
                screen: Mutex::new(ScreenState::default()),
            }),
        };

        tracing::info!("node {} listening on port {port}", node.inner.identity.id);
        node.spawn(node.clone().accept_loop());
        node.spawn(node.clone().maintenance_loop());
        if let Some(sock) = discovery_socket {
            node.spawn(node.clone().announce_loop(sock.clone()));
            node.spawn(node.clone().listen_loop(sock));
        }
        Ok(node)
    }

    pub async fn shutdown(&self) {
        self.stop_all_screen_shares();
        self.inner.stop.send_replace(true);
        self.inner.endpoint.close(0u32.into(), b"bye");
        let _ = tokio::time::timeout(Duration::from_secs(1), self.inner.endpoint.wait_idle()).await;
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn id(&self) -> &str {
        &self.inner.identity.id
    }

    pub fn port(&self) -> u16 {
        self.inner.port
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.inner.data_dir
    }

    pub fn me(&self) -> MeView {
        MeView {
            id: self.inner.identity.id.clone(),
            name: self.my_name(),
            code: short_code(&self.inner.identity.id),
            port: self.inner.port,
            addresses: discovery::local_addresses().iter().map(|a| a.to_string()).collect(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.inner.settings.lock().unwrap().clone()
    }

    pub fn set_device_name(&self, name: &str) -> Result<()> {
        let name = clean_name(name);
        if name.is_empty() {
            bail!("name must not be empty");
        }
        self.update_settings(|s| s.device_name = name)?;
        self.emit(Event::Me(self.me()));
        self.inner.announce_now.notify_one();
        // Peers without discovery (added by address) learn the name from hello.
        let conns: Vec<quinn::Connection> = self.inner.conns.lock().unwrap().values().cloned().collect();
        for conn in conns {
            let node = self.clone();
            self.spawn(async move {
                let _ = node.request_on(&conn, &Request::Hello(node.my_hello())).await;
            });
        }
        Ok(())
    }

    pub fn set_language(&self, language: &str) -> Result<()> {
        let language = language.to_owned();
        self.update_settings(|s| s.language = language)
    }

    pub fn set_autostart(&self, enabled: bool) -> Result<()> {
        self.update_settings(|s| s.autostart = enabled)
    }

    pub fn set_trusted(&self, id: &str, trusted: bool) -> Result<()> {
        let name = self
            .peers()
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| p.name)
            .ok_or_else(|| anyhow!("unknown device"))?;
        self.update_settings(|s| {
            s.trusted.retain(|t| t.id != id);
            if trusted {
                s.trusted.push(TrustedDevice { id: id.to_owned(), name });
            }
        })?;
        if !trusted {
            // Trust is checked on every request, but a screen being watched
            // is one long stream: end it now.
            self.revoke_screen_access(id);
        }
        self.refresh_presence();
        Ok(())
    }

    pub fn folders(&self) -> Folders {
        let s = self.settings();
        Folders {
            download_dir: s.download_dir().to_string_lossy().into_owned(),
            shared_dir: s.shared_dir().to_string_lossy().into_owned(),
            sharing_enabled: s.sharing_enabled,
            shared_trusted_only: s.shared_trusted_only,
        }
    }

    pub fn set_shared_trusted_only(&self, enabled: bool) -> Result<()> {
        self.update_settings(|s| s.shared_trusted_only = enabled)
    }

    /// Whether trusted devices may watch this screen without being asked.
    pub fn set_trusted_view_without_asking(&self, enabled: bool) -> Result<()> {
        self.update_settings(|s| s.trusted_view_without_asking = enabled)
    }

    pub fn set_download_dir(&self, dir: Option<PathBuf>) -> Result<()> {
        if let Some(d) = &dir {
            std::fs::create_dir_all(d)?;
        }
        self.update_settings(|s| s.download_dir = dir)
    }

    pub fn set_shared_dir(&self, dir: Option<PathBuf>) -> Result<()> {
        if let Some(d) = &dir {
            std::fs::create_dir_all(d)?;
        }
        self.update_settings(|s| s.shared_dir = dir)
    }

    pub fn set_appearance(&self, appearance: serde_json::Value) -> Result<()> {
        self.update_settings(|s| s.appearance = appearance)
    }

    pub fn set_screen_video(&self, video: crate::screen::VideoSettings) -> Result<()> {
        self.update_settings(|s| s.screen_video = video.clamped())
    }

    pub fn set_sharing(&self, enabled: bool) -> Result<()> {
        self.update_settings(|s| s.sharing_enabled = enabled)?;
        if enabled {
            let _ = std::fs::create_dir_all(self.settings().shared_dir());
        }
        Ok(())
    }

    pub fn is_trusted(&self, id: &str) -> bool {
        self.inner.settings.lock().unwrap().is_trusted(id)
    }

    /// Connects to a device by address ("192.168.1.20" or "host:port") for
    /// networks where discovery does not work. The address is remembered.
    pub async fn connect_address(&self, input: &str) -> Result<PeerView> {
        let input = input.trim();
        let addr = resolve(input).await?;
        let (id, _) = self.dial(addr, None).await?;
        let normalized = addr.to_string();
        self.update_settings(|s| {
            if !s.manual_peers.contains(&normalized) {
                s.manual_peers.push(normalized);
            }
        })?;
        self.refresh_presence();
        self.peers()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow!("device disappeared"))
    }

    pub fn peers(&self) -> Vec<PeerView> {
        let settings = self.settings();
        let live: HashSet<String> = self
            .inner
            .conns
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| c.close_reason().is_none())
            .map(|(id, _)| id.clone())
            .collect();
        let summaries = self.inner.chat.lock().unwrap().summaries();

        let mut out = Vec::new();
        let peers = self.inner.peers.lock().unwrap();
        for (id, p) in peers.iter() {
            let recent = p.last_seen.is_some_and(|t| t.elapsed() < PRESENCE_TIMEOUT);
            out.push(PeerView {
                id: id.clone(),
                name: p.name.clone(),
                os: p.os.clone(),
                address: p.addr.map(|a| a.ip().to_string()),
                online: recent || live.contains(id),
                trusted: settings.is_trusted(id),
                code: short_code(id),
            });
        }
        // Devices not seen this session: trusted ones and ones we chatted with.
        let mut seen: HashSet<String> = peers.keys().cloned().collect();
        for t in &settings.trusted {
            if seen.insert(t.id.clone()) {
                out.push(offline_view(&t.id, &t.name, true));
            }
        }
        for s in summaries {
            if s.conversation == GENERAL || seen.contains(&s.conversation) {
                continue;
            }
            let history = self.inner.chat.lock().unwrap().history(&s.conversation);
            let name = history
                .iter()
                .rev()
                .find(|m| !m.outgoing)
                .map(|m| m.from_name.clone())
                .unwrap_or_else(|| short_code(&s.conversation));
            seen.insert(s.conversation.clone());
            out.push(offline_view(&s.conversation, &name, false));
        }
        out.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    // ---- chat ----------------------------------------------------------

    pub fn conversations(&self) -> Vec<ConversationSummary> {
        self.inner.chat.lock().unwrap().summaries()
    }

    pub fn history(&self, conversation: &str) -> Vec<ChatMessage> {
        self.inner.chat.lock().unwrap().history(conversation)
    }

    /// Stores the message and delivers it in the background; status updates
    /// arrive as [`Event::Chat`].
    pub fn send_chat(&self, conversation: &str, text: &str) -> Result<ChatMessage> {
        if !is_valid_conversation(conversation) {
            bail!("invalid conversation");
        }
        let text = text.trim();
        if text.is_empty() {
            bail!("message is empty");
        }
        if text.len() > MAX_TEXT {
            bail!("message is too long");
        }
        let msg = ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            conversation: conversation.to_owned(),
            from_id: self.inner.identity.id.clone(),
            from_name: self.my_name(),
            text: text.to_owned(),
            ts: now_ms(),
            outgoing: true,
            status: MessageStatus::Pending,
        };
        self.inner.chat.lock().unwrap().add(msg.clone());
        self.emit(Event::Chat(msg.clone()));
        let node = self.clone();
        let m = msg.clone();
        self.spawn(async move { node.deliver(m).await });
        Ok(msg)
    }

    pub fn retry_chat(&self, conversation: &str, id: &str) -> Result<()> {
        let msg = self
            .inner
            .chat
            .lock()
            .unwrap()
            .get(conversation, id)
            .ok_or_else(|| anyhow!("message not found"))?;
        if !msg.outgoing || msg.status != MessageStatus::Failed {
            return Ok(());
        }
        let node = self.clone();
        self.spawn(async move { node.deliver(msg).await });
        Ok(())
    }

    async fn deliver(&self, msg: ChatMessage) {
        if !self.inner.delivering.lock().unwrap().insert(msg.id.clone()) {
            return;
        }
        self.set_chat_status(&msg, MessageStatus::Pending);
        let wire = Request::Chat(ChatWire {
            id: msg.id.clone(),
            general: msg.conversation == GENERAL,
            text: msg.text.clone(),
            ts: msg.ts,
        });

        let status = if msg.conversation == GENERAL {
            let targets: Vec<String> = self.peers().into_iter().filter(|p| p.online).map(|p| p.id).collect();
            let tasks: Vec<_> = targets
                .into_iter()
                .map(|peer| {
                    let node = self.clone();
                    let req = wire.clone();
                    self.inner.rt.spawn(async move {
                        if let Err(e) = node.request(&peer, &req).await {
                            tracing::debug!("general chat to {peer}: {e}");
                        }
                    })
                })
                .collect();
            for t in tasks {
                let _ = t.await;
            }
            MessageStatus::Sent
        } else {
            match self.request(&msg.conversation, &wire).await {
                Ok(Response::Ok) => MessageStatus::Sent,
                Ok(other) => {
                    tracing::warn!("unexpected chat response: {other:?}");
                    MessageStatus::Failed
                }
                Err(e) => {
                    tracing::debug!("chat to {}: {e}", msg.conversation);
                    MessageStatus::Failed
                }
            }
        };

        self.inner.delivering.lock().unwrap().remove(&msg.id);
        self.set_chat_status(&msg, status);
    }

    fn set_chat_status(&self, msg: &ChatMessage, status: MessageStatus) {
        let updated = self.inner.chat.lock().unwrap().set_status(&msg.conversation, &msg.id, status);
        if let Some(updated) = updated {
            self.emit(Event::Chat(updated));
        }
    }

    fn flush_undelivered(&self, peer: &str) {
        let pending = self.inner.chat.lock().unwrap().undelivered(peer);
        for msg in pending {
            let node = self.clone();
            self.spawn(async move { node.deliver(msg).await });
        }
    }

    fn on_chat(&self, peer: &str, wire: ChatWire) {
        let mut text = clean_text(&wire.text);
        if text.len() > MAX_TEXT {
            let mut cut = MAX_TEXT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        let mut id = wire.id;
        id.truncate(64);
        let msg = ChatMessage {
            id,
            conversation: if wire.general { GENERAL.into() } else { peer.into() },
            from_id: peer.into(),
            from_name: self.peer_name(peer),
            text,
            ts: wire.ts,
            outgoing: false,
            status: MessageStatus::Received,
        };
        let added = self.inner.chat.lock().unwrap().add(msg.clone());
        if added {
            self.emit(Event::Chat(msg));
        }
    }

    // ---- connections ---------------------------------------------------

    async fn accept_loop(self) {
        while let Some(incoming) = self.inner.endpoint.accept().await {
            let ip = incoming.remote_address().ip();
            let (total, from_ip) = {
                let conns = self.inner.conns.lock().unwrap();
                let live = conns.values().filter(|c| c.close_reason().is_none());
                let (mut total, mut from_ip) = (0, 0);
                for c in live {
                    total += 1;
                    if c.remote_address().ip() == ip {
                        from_ip += 1;
                    }
                }
                (total, from_ip)
            };
            // Several copies on one machine (tests) all come from loopback.
            if total >= MAX_CONNECTIONS || (from_ip >= MAX_CONNECTIONS_PER_IP && !ip.is_loopback()) {
                tracing::warn!("refusing connection from {ip}: too many connections");
                incoming.refuse();
                continue;
            }
            let node = self.clone();
            self.spawn(async move {
                let conn = match incoming.await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!("incoming connection failed: {e}");
                        return;
                    }
                };
                match tls::peer_id(&conn) {
                    Ok(id) if id != node.inner.identity.id => node.register(id, conn),
                    Ok(_) => conn.close(0u32.into(), b"self"),
                    Err(e) => tracing::warn!("rejecting connection: {e}"),
                }
            });
        }
    }

    /// Keeps the connection for outgoing requests and serves the streams the
    /// peer opens on it.
    fn register(&self, id: String, conn: quinn::Connection) {
        self.inner.conns.lock().unwrap().insert(id.clone(), conn.clone());
        let node = self.clone();
        self.spawn(async move {
            while let Ok((send, recv)) = conn.accept_bi().await {
                let n = node.clone();
                let peer = id.clone();
                let remote = conn.remote_address();
                node.spawn(async move {
                    if let Err(e) = n.handle_stream(&peer, remote.ip(), send, recv).await {
                        tracing::debug!("stream from {peer}: {e}");
                    }
                });
            }
            {
                let mut conns = node.inner.conns.lock().unwrap();
                if conns.get(&id).map(|c| c.stable_id()) == Some(conn.stable_id()) {
                    conns.remove(&id);
                }
            }
            // A clean close means the app on the other side quit: show it
            // offline now instead of after the presence timeout.
            if matches!(conn.close_reason(), Some(quinn::ConnectionError::ApplicationClosed(_))) {
                if let Some(p) = node.inner.peers.lock().unwrap().get_mut(&id) {
                    p.last_seen = None;
                }
            }
            node.refresh_presence();
        });
        self.refresh_presence();
    }

    async fn handle_stream(
        &self,
        peer: &str,
        remote_ip: IpAddr,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
    ) -> Result<()> {
        let request: Request = tokio::time::timeout(REQUEST_READ_TIMEOUT, read_frame(&mut recv))
            .await
            .map_err(|_| anyhow!("request was not sent in time"))??;
        let costly = matches!(
            request,
            Request::Chat(_)
                | Request::Offer(_)
                | Request::Change(_)
                | Request::ScreenAsk { .. }
                | Request::ScreenInvite { .. }
                | Request::SharedList
        );
        if costly && !self.is_trusted(peer) && !self.take_allowance(remote_ip) {
            write_frame(&mut send, &Response::Error { message: "too many requests, try again later".into() }).await?;
            send.finish()?;
            return Ok(());
        }
        let response = match request {
            Request::Hello(hello) => {
                self.on_hello(peer, remote_ip, hello);
                Response::Hello(self.my_hello())
            }
            Request::Chat(wire) => {
                self.on_chat(peer, wire);
                Response::Ok
            }
            Request::Offer(wire) => self.on_offer(peer, wire),
            Request::OfferAnswer { id, accepted } => {
                self.on_offer_answer(peer, &id, accepted);
                Response::Ok
            }
            Request::TransferDone { id } => {
                self.on_transfer_done(peer, &id);
                Response::Ok
            }
            Request::TransferCancel { id } => {
                self.on_transfer_cancel(peer, &id);
                Response::Ok
            }
            Request::Fetch { target, offset } => {
                let Some(_slot) = self.fetch_slot(peer) else {
                    write_frame(&mut send, &Response::Error { message: "too many downloads at once, try again later".into() }).await?;
                    send.finish()?;
                    return Ok(());
                };
                return self.serve_fetch(peer, target, offset, send).await;
            }
            Request::SharedList => {
                let _permit = self.inner.listings.acquire().await?;
                self.on_shared_list(peer).await
            }
            Request::Change(wire) => self.on_change(peer, wire),
            Request::ChangeResult { id, outcome, error } => {
                self.on_change_result(peer, &id, outcome, error);
                Response::Ok
            }
            Request::ScreenInvite { session, title, audio, requested, control } => {
                self.on_screen_invite(peer, session, title, audio, requested, control)
            }
            Request::ScreenEnded { session } => {
                self.on_screen_ended(peer, &session);
                Response::Ok
            }
            Request::ScreenWatch { session } => return self.serve_screen_watch(peer, &session, send, recv).await,
            Request::ScreenAsk { id, control } => self.on_screen_ask(peer, id, control),
            Request::ScreenAskAnswer { id, accepted } => {
                self.on_screen_ask_answer(peer, &id, accepted);
                Response::Ok
            }
        };
        write_frame(&mut send, &response).await?;
        send.finish()?;
        Ok(())
    }

    async fn dial(&self, addr: SocketAddr, expected_id: Option<&str>) -> Result<(String, quinn::Connection)> {
        let config = tls::client_config(&self.inner.identity, self.inner.provider.clone(), expected_id)?;
        let connecting = self.inner.endpoint.connect_with(config, addr, "ropeway.local")?;
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| anyhow!("connection to {addr} timed out"))??;
        let id = tls::peer_id(&conn)?;
        if id == self.inner.identity.id {
            conn.close(0u32.into(), b"self");
            bail!("this address belongs to this device");
        }
        self.register(id.clone(), conn.clone());
        if let Response::Hello(hello) = self.request_on(&conn, &Request::Hello(self.my_hello())).await? {
            self.on_hello(&id, addr.ip(), hello);
        }
        Ok((id, conn))
    }

    fn live_connection(&self, peer: &str) -> Option<quinn::Connection> {
        let conns = self.inner.conns.lock().unwrap();
        conns.get(peer).filter(|c| c.close_reason().is_none()).cloned()
    }

    pub(crate) async fn connection(&self, peer: &str) -> Result<quinn::Connection> {
        if let Some(conn) = self.live_connection(peer) {
            return Ok(conn);
        }
        let (addr, hint) = self
            .inner
            .peers
            .lock()
            .unwrap()
            .get(peer)
            .map(|p| (p.addr, p.hint))
            .unwrap_or_default();
        let addr = addr.or(hint).ok_or_else(|| anyhow!("device address is unknown"))?;
        // Both are dialed with the device's fingerprint pinned, so a wrong
        // address can only fail, never reach someone else.
        match self.dial(addr, Some(peer)).await {
            Ok((_, conn)) => Ok(conn),
            Err(e) => match hint.filter(|h| *h != addr) {
                Some(h) => Ok(self.dial(h, Some(peer)).await?.1),
                None => Err(e),
            },
        }
    }

    pub(crate) async fn request(&self, peer: &str, request: &Request) -> Result<Response> {
        let conn = self.connection(peer).await?;
        self.request_on(&conn, request).await
    }

    async fn request_on(&self, conn: &quinn::Connection, request: &Request) -> Result<Response> {
        let exchange = async {
            let (mut send, mut recv) = conn.open_bi().await?;
            write_frame(&mut send, request).await?;
            send.finish()?;
            read_frame::<Response>(&mut recv).await
        };
        tokio::time::timeout(REQUEST_TIMEOUT, exchange)
            .await
            .map_err(|_| anyhow!("request timed out"))?
    }

    /// Takes one request from the allowance of an untrusted address.
    fn take_allowance(&self, ip: IpAddr) -> bool {
        let mut rate = self.inner.rate.lock().unwrap();
        let now = Instant::now();
        if rate.len() > 1024 {
            rate.retain(|_, (_, at)| now.duration_since(*at) < Duration::from_secs(120));
        }
        let (tokens, at) = rate.entry(ip).or_insert((RATE_BURST, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * RATE_PER_MIN / 60.0).min(RATE_BURST);
        *at = now;
        if *tokens < 1.0 {
            return false;
        }
        *tokens -= 1.0;
        true
    }

    /// Counts a download for an untrusted device, or `None` when it already
    /// runs as many as it may. Trusted devices are not limited.
    fn fetch_slot(&self, peer: &str) -> Option<FetchSlot> {
        if self.is_trusted(peer) {
            return Some(FetchSlot { node: None, peer: String::new() });
        }
        let mut fetching = self.inner.fetching.lock().unwrap();
        let n = fetching.entry(peer.to_owned()).or_insert(0);
        if *n >= MAX_FETCHES_UNTRUSTED {
            return None;
        }
        *n += 1;
        Some(FetchSlot { node: Some(self.clone()), peer: peer.to_owned() })
    }

    /// Room for one more device in the peer list. Devices that are trusted
    /// or connected stay; of the others the one seen longest ago goes. With
    /// `only_stale`, a device seen recently is never pushed out and `false`
    /// means there is no room.
    fn make_room(&self, peers: &mut HashMap<String, Peer>, only_stale: bool) -> bool {
        if peers.len() < MAX_PEERS {
            return true;
        }
        let trusted: HashSet<String> = self.settings().trusted.into_iter().map(|t| t.id).collect();
        let live: HashSet<String> = {
            let conns = self.inner.conns.lock().unwrap();
            conns.iter().filter(|(_, c)| c.close_reason().is_none()).map(|(id, _)| id.clone()).collect()
        };
        let oldest = peers
            .iter()
            .filter(|(id, _)| !trusted.contains(*id) && !live.contains(*id))
            .filter(|(_, p)| !only_stale || !p.last_seen.is_some_and(|t| t.elapsed() < PRESENCE_TIMEOUT))
            .min_by_key(|(_, p)| p.last_seen)
            .map(|(id, _)| id.clone());
        match oldest {
            Some(id) => {
                peers.remove(&id);
                true
            }
            None => false,
        }
    }

    fn on_hello(&self, id: &str, ip: IpAddr, hello: Hello) {
        {
            let mut peers = self.inner.peers.lock().unwrap();
            if !peers.contains_key(id) {
                // The device is connected, so it gets a place.
                self.make_room(&mut peers, false);
            }
            let p = peers.entry(id.to_owned()).or_insert_with(Peer::new);
            p.name = clean_name(&hello.name);
            p.os = clean_name(&hello.os);
            // The connection proved this address belongs to the device.
            p.addr = Some(SocketAddr::new(ip, hello.port));
            p.verified = true;
            p.hint = None;
            p.last_seen = Some(Instant::now());
        }
        self.refresh_presence();
        // A hello means a fresh connection, possibly after a quick restart
        // that presence tracking did not notice.
        self.flush_undelivered(id);
        self.resume_transfers(id);
        self.invite_to_public_shares(id);
    }

    fn my_hello(&self) -> Hello {
        Hello {
            name: self.my_name(),
            os: std::env::consts::OS.into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            protocol: PROTOCOL_VERSION,
            port: self.inner.port,
        }
    }

    // ---- discovery and presence ---------------------------------------

    async fn announce_loop(self, sock: Arc<UdpSocket>) {
        loop {
            let announce = Announce::new(&self.inner.identity.id, &self.my_name(), self.inner.port);
            if let Ok(bytes) = serde_json::to_vec(&announce) {
                for target in discovery::broadcast_targets() {
                    let _ = sock.send_to(&bytes, target).await;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(ANNOUNCE_INTERVAL) => {}
                _ = self.inner.announce_now.notified() => {}
            }
        }
    }

    async fn listen_loop(self, sock: Arc<UdpSocket>) {
        let mut buf = vec![0u8; 2048];
        loop {
            // Windows reports ICMP "port unreachable" from earlier sends as
            // errors on recv; they are harmless.
            let Ok((n, src)) = sock.recv_from(&mut buf).await else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };
            let Some(announce) = Announce::parse(&buf[..n]) else { continue };
            if announce.id == self.inner.identity.id {
                continue;
            }
            // An announcement is unauthenticated. While a verified connection
            // to the device is open, one from another address is a fake: it
            // must not redirect where this device sends to.
            if let Some(conn) = self.live_connection(&announce.id) {
                if conn.remote_address().ip() != src.ip() {
                    continue;
                }
            }
            let appeared = {
                let mut peers = self.inner.peers.lock().unwrap();
                // Fake announcements cannot push out devices seen recently.
                if !peers.contains_key(&announce.id) && !self.make_room(&mut peers, true) {
                    continue;
                }
                let p = peers.entry(announce.id.clone()).or_insert_with(Peer::new);
                let addr = SocketAddr::new(src.ip(), announce.port);
                if p.verified && p.addr.is_some_and(|a| a.ip() != addr.ip()) {
                    // Possibly a fake: keep the proven address, remember
                    // this one for when that stops answering.
                    p.hint = Some(addr);
                    continue;
                }
                let was_recent = p.last_seen.is_some_and(|t| t.elapsed() < PRESENCE_TIMEOUT);
                p.name = clean_name(&announce.name);
                p.os = clean_name(&announce.os);
                p.addr = Some(addr);
                p.last_seen = Some(Instant::now());
                !was_recent
            };
            if appeared {
                // Answer directly so the newcomer sees us without waiting.
                let reply = Announce::new(&self.inner.identity.id, &self.my_name(), self.inner.port);
                if let Ok(bytes) = serde_json::to_vec(&reply) {
                    let _ = sock.send_to(&bytes, SocketAddr::new(src.ip(), DISCOVERY_PORT)).await;
                }
                self.refresh_presence();
            }
        }
    }

    async fn maintenance_loop(self) {
        let mut tick: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            self.refresh_presence();
            tick += 1;
            if tick % 5 == 0 {
                self.reconnect_manual_peers();
            }
        }
    }

    fn reconnect_manual_peers(&self) {
        let addrs = self.settings().manual_peers;
        let online: HashSet<String> = self
            .peers()
            .into_iter()
            .filter(|p| p.online)
            .filter_map(|p| p.address.map(|a| a.to_string()))
            .collect();
        for addr in addrs {
            let Ok(sock) = addr.parse::<SocketAddr>() else { continue };
            if online.contains(&sock.ip().to_string()) {
                continue;
            }
            let node = self.clone();
            self.spawn(async move {
                let _ = node.dial(sock, None).await;
            });
        }
    }

    /// Recomputes the peer list, notifies the UI if it changed, and retries
    /// undelivered messages to peers that just came online.
    fn refresh_presence(&self) {
        let views = self.peers();
        let online_now: HashSet<String> = views.iter().filter(|v| v.online).map(|v| v.id.clone()).collect();
        let came_online: Vec<String> = {
            let mut prev = self.inner.online.lock().unwrap();
            let fresh = online_now.difference(&prev).cloned().collect();
            *prev = online_now;
            fresh
        };
        let changed = {
            let mut last = self.inner.last_views.lock().unwrap();
            if *last != views {
                *last = views.clone();
                true
            } else {
                false
            }
        };
        if changed {
            self.emit(Event::Peers(views));
        }
        for id in came_online {
            self.flush_undelivered(&id);
        }
    }

    // ---- helpers -------------------------------------------------------

    pub(crate) fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut stop = self.inner.stop.subscribe();
        self.inner.rt.spawn(async move {
            tokio::select! {
                _ = fut => {}
                _ = stop.wait_for(|stopped| *stopped) => {}
            }
        });
    }

    pub(crate) fn emit(&self, event: Event) {
        let _ = self.inner.events.send(event);
    }

    fn my_name(&self) -> String {
        self.inner.settings.lock().unwrap().device_name.clone()
    }

    pub(crate) fn peer_name(&self, id: &str) -> String {
        if let Some(p) = self.inner.peers.lock().unwrap().get(id) {
            if !p.name.is_empty() {
                return p.name.clone();
            }
        }
        short_code(id)
    }

    fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut settings = self.inner.settings.lock().unwrap();
        f(&mut settings);
        settings.save(&self.inner.data_dir.join(SETTINGS_FILE))
    }
}

fn offline_view(id: &str, name: &str, trusted: bool) -> PeerView {
    PeerView {
        id: id.to_owned(),
        name: name.to_owned(),
        os: String::new(),
        address: None,
        online: false,
        trusted,
        code: short_code(id),
    }
}

fn bind_endpoint(config: quinn::ServerConfig, port: Option<u16>) -> Result<quinn::Endpoint> {
    let any = |p: u16| SocketAddr::from((Ipv4Addr::UNSPECIFIED, p));
    match port {
        Some(p) => Ok(quinn::Endpoint::server(config, any(p))?),
        None => match quinn::Endpoint::server(config.clone(), any(DEFAULT_PORT)) {
            Ok(e) => Ok(e),
            Err(e) => {
                tracing::warn!("port {DEFAULT_PORT} unavailable ({e}), using a random port");
                Ok(quinn::Endpoint::server(config, any(0))?)
            }
        },
    }
}

async fn resolve(input: &str) -> Result<SocketAddr> {
    if input.is_empty() {
        bail!("address is empty");
    }
    let addrs: Vec<SocketAddr> = if input.contains(':') {
        tokio::net::lookup_host(input).await?.collect()
    } else {
        tokio::net::lookup_host((input, DEFAULT_PORT)).await?.collect()
    };
    addrs
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .ok_or_else(|| anyhow!("could not resolve {input}"))
}

/// A name from the network, as shown in the UI: no control characters and
/// none of the invisible ones that reorder or hide text.
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|&c| !c.is_control() && !crate::fs_util::is_deceptive_char(c))
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect()
}

/// Message text from the network: line breaks and tabs stay, other control
/// characters and text-reordering ones go.
fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|&c| (!c.is_control() || c == '\n' || c == '\t') && !crate::fs_util::is_deceptive_char(c))
        .collect()
}

/// One running download of an untrusted device; frees its place when dropped.
struct FetchSlot {
    node: Option<Node>,
    peer: String,
}

impl Drop for FetchSlot {
    fn drop(&mut self) {
        let Some(node) = &self.node else { return };
        let mut fetching = node.inner.fetching.lock().unwrap();
        if let Some(n) = fetching.get_mut(&self.peer) {
            *n -= 1;
            if *n == 0 {
                fetching.remove(&self.peer);
            }
        }
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{clean_name, clean_text};

    #[test]
    fn names_lose_invisible_and_reordering_characters() {
        assert_eq!(clean_name("  Ноутбук\u{202E}gpj.exe "), "Ноутбукgpj.exe");
        assert_eq!(clean_name("A\u{200B}B\u{2066}C\nD"), "ABCD");
        assert_eq!(clean_name("👨\u{200D}💻 Work"), "👨\u{200D}💻 Work");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), 64);
    }

    #[test]
    fn messages_keep_line_breaks_only() {
        assert_eq!(clean_text("line 1\nline 2\tend"), "line 1\nline 2\tend");
        assert_eq!(clean_text("bad\u{202E}txt\u{0007}"), "badtxt");
    }
}
