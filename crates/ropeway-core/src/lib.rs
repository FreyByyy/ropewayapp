//! Ropeway core: everything that does not depend on the UI toolkit or the
//! operating system's screen/input APIs, so it can be reused on every
//! platform the app is ported to.

pub mod chat;
pub mod discovery;
pub mod fs_util;
pub mod identity;
pub mod node;
pub mod pairing;
pub mod protocol;
pub mod screen;
pub mod settings;
pub mod shared;
mod tls;
pub mod transfers;

pub use chat::{ChatMessage, ConversationSummary, MessageStatus, GENERAL};
pub use fs_util::SharedEntry;
pub use node::{Event, Folders, MeView, Node, NodeOptions, Notice, PeerView};
pub use screen::{
    BroadcastSink, CaptureOptions, CaptureSource, InputEvent, MonitorChoice, ScreenProvider, ScreenView, ScreenWatch, StreamInfo,
};
pub use shared::{ChangeRequest, ChangeStatus, EditView, SharedListing};
pub use transfers::{Direction, TransferKind, TransferStatus, TransferView};
pub use settings::Settings;
