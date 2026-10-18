//! Wire protocol.
//!
//! Every exchange runs on its own QUIC bidirectional stream: the opener writes
//! one `Request` frame, the other side answers with one `Response` frame.
//! A `Fetch` is answered with `FileStart` followed by the raw file bytes on
//! the same stream.
//!
//! Files always move by pull: the side that wants a file fetches it from a
//! byte offset, which is what makes interrupted transfers resumable.
//!
//! Frame = u32 big-endian length + JSON body.

use anyhow::{bail, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::fs_util::SharedEntry;

pub const ALPN: &[u8] = b"ropeway/1";
pub const PROTOCOL_VERSION: u32 = 2;
pub const DEFAULT_PORT: u16 = 47801;
const MAX_FRAME: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Hello {
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub protocol: u32,
    /// Port this device accepts QUIC connections on.
    pub port: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatWire {
    pub id: String,
    /// `true` for the network-wide chat, `false` for a direct message.
    pub general: bool,
    pub text: String,
    pub ts: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OfferFile {
    pub path: String,
    pub size: u64,
}

/// "I would like to send you these files." The receiver answers right away
/// and decides later; the decision comes back as `OfferAnswer`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OfferWire {
    pub id: String,
    pub files: Vec<OfferFile>,
    /// Empty folders, so the structure arrives intact.
    pub dirs: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum FetchTarget {
    /// File `index` of an offer the server made to the requester.
    Offer { transfer_id: String, index: usize },
    /// A file in the server's shared folder.
    Shared { path: String },
    /// The new version of a file, for a change request the server sent.
    Change { request_id: String },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Replace,
    Delete,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChangeWire {
    pub id: String,
    pub kind: ChangeKind,
    /// Path in the owner's shared folder.
    pub path: String,
    /// Size of the proposed new version (replace only).
    pub size: Option<u64>,
    /// Modification time of the owner's file when the requester copied it,
    /// so the owner can see whether it changed since.
    pub base_modified: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOutcome {
    Applied,
    Rejected,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Request {
    Hello(Hello),
    Chat(ChatWire),
    Offer(OfferWire),
    OfferAnswer { id: String, accepted: bool },
    TransferDone { id: String },
    TransferCancel { id: String },
    Fetch { target: FetchTarget, offset: u64 },
    SharedList,
    Change(ChangeWire),
    ChangeResult { id: String, outcome: ChangeOutcome, error: Option<String> },
    /// "I am sharing my screen with you." `requested`: in answer to your ask;
    /// `control`: you may send input.
    ScreenInvite {
        session: String,
        title: String,
        audio: bool,
        requested: bool,
        #[serde(default)]
        control: bool,
    },
    ScreenEnded { session: String },
    /// Answered with `Ok`, then media packets follow on the same stream and
    /// input events (`InputEvent` frames) may go the other way.
    ScreenWatch { session: String },
    /// "Please show me your screen" (`control`: and let me control it).
    ScreenAsk {
        id: String,
        #[serde(default)]
        control: bool,
    },
    ScreenAskAnswer { id: String, accepted: bool },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Response {
    Hello(Hello),
    Ok,
    Error { message: String },
    /// Followed by `size - offset` raw bytes.
    FileStart { size: u64 },
    SharedList { enabled: bool, entries: Vec<SharedEntry> },
}

pub async fn write_frame<T: Serialize>(stream: &mut quinn::SendStream, value: &T) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME {
        bail!("frame too large");
    }
    stream.write_all(&(body.len() as u32).to_be_bytes()).await?;
    stream.write_all(&body).await?;
    Ok(())
}

pub async fn read_frame<T: DeserializeOwned>(stream: &mut quinn::RecvStream) -> Result<T> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        bail!("frame too large: {len} bytes");
    }
    // The buffer grows as data arrives instead of trusting the announced
    // length up front, so a peer cannot reserve memory it never sends.
    let mut body = Vec::with_capacity(len.min(64 * 1024));
    (&mut *stream).take(len as u64).read_to_end(&mut body).await?;
    if body.len() != len {
        bail!("frame was cut off");
    }
    Ok(serde_json::from_slice(&body)?)
}
