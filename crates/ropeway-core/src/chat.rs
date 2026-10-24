//! Chat history, kept in memory and persisted as one JSON-lines file per
//! conversation. A conversation is either a peer id (direct messages) or
//! [`GENERAL`] (the network-wide chat).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::identity::is_valid_id;
use crate::settings::write_atomic;

pub const GENERAL: &str = "general";
pub const MAX_TEXT: usize = 32 * 1024;
const MAX_HISTORY: usize = 5000;
/// Messages kept beyond [`MAX_HISTORY`] before old ones are dropped.
const TRIM_SLACK: usize = 250;
/// Messages one deletion may name.
pub const MAX_DELETE: usize = 1000;
const DELETES_FILE: &str = "deletes.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Pending,
    Sent,
    Failed,
    Received,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub conversation: String,
    pub from_id: String,
    pub from_name: String,
    pub text: String,
    /// Unix time, milliseconds.
    pub ts: i64,
    pub outgoing: bool,
    pub status: MessageStatus,
}

#[derive(Serialize, Clone, Debug)]
pub struct ConversationSummary {
    pub conversation: String,
    pub last: Option<ChatMessage>,
}

pub fn is_valid_conversation(conv: &str) -> bool {
    conv == GENERAL || is_valid_id(conv)
}

pub struct ChatStore {
    dir: PathBuf,
    conversations: HashMap<String, Vec<ChatMessage>>,
    /// "Delete for everyone" for devices that were offline: peer → ids.
    deletes: HashMap<String, Vec<String>>,
}

impl ChatStore {
    pub fn load(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)?;
        let mut conversations = HashMap::new();
        for entry in fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(conv) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
                continue;
            };
            if !is_valid_conversation(&conv) {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            let mut messages: Vec<ChatMessage> = Vec::new();
            let mut index: HashMap<String, usize> = HashMap::new();
            for m in text.lines().filter_map(|line| serde_json::from_str::<ChatMessage>(line).ok()) {
                match index.get(&m.id) {
                    Some(&i) => messages[i] = m,
                    None => {
                        index.insert(m.id.clone(), messages.len());
                        messages.push(m);
                    }
                }
            }
            // A message still "pending" when the app closed was never confirmed.
            if messages.len() > MAX_HISTORY {
                messages.drain(..messages.len() - MAX_HISTORY);
            }
            for m in &mut messages {
                if m.status == MessageStatus::Pending {
                    m.status = MessageStatus::Failed;
                }
            }
            conversations.insert(conv, messages);
        }
        let deletes = fs::read(dir.join(DELETES_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, Vec<String>>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|(peer, _)| is_valid_id(peer))
            .collect();
        Ok(Self { dir, conversations, deletes })
    }

    /// Removes the messages with these ids; with `from`, only ones that
    /// device sent. Returns the ids removed.
    pub fn remove(&mut self, conv: &str, ids: &HashSet<String>, from: Option<&str>) -> Vec<String> {
        let Some(list) = self.conversations.get_mut(conv) else { return Vec::new() };
        let mut removed = Vec::new();
        list.retain(|m| {
            let gone = ids.contains(&m.id) && from.is_none_or(|f| !m.outgoing && m.from_id == f);
            if gone {
                removed.push(m.id.clone());
            }
            !gone
        });
        if !removed.is_empty() {
            self.persist(conv);
        }
        removed
    }

    /// Removes the whole conversation. Returns the ids removed.
    pub fn clear(&mut self, conv: &str) -> Vec<String> {
        let removed: Vec<String> = self.conversations.remove(conv).unwrap_or_default().into_iter().map(|m| m.id).collect();
        let _ = fs::remove_file(self.dir.join(format!("{conv}.jsonl")));
        removed
    }

    /// Remembers a deletion to send when `peer` is back.
    pub fn queue_delete(&mut self, peer: &str, ids: &[String]) {
        let list = self.deletes.entry(peer.to_owned()).or_default();
        for id in ids {
            if !list.contains(id) {
                list.push(id.clone());
            }
        }
        let excess = list.len().saturating_sub(MAX_DELETE);
        list.drain(..excess);
        self.save_deletes();
    }

    /// Deletions waiting for `peer`, removed from the queue.
    pub fn take_deletes(&mut self, peer: &str) -> Vec<String> {
        let ids = self.deletes.remove(peer).unwrap_or_default();
        if !ids.is_empty() {
            self.save_deletes();
        }
        ids
    }

    fn save_deletes(&self) {
        if let Ok(bytes) = serde_json::to_vec(&self.deletes) {
            if let Err(e) = write_atomic(&self.dir.join(DELETES_FILE), &bytes) {
                tracing::warn!("saving chat deletions: {e}");
            }
        }
    }

    /// Adds a message; returns `false` if a message with this id is already
    /// in the conversation (retries are delivered more than once).
    pub fn add(&mut self, msg: ChatMessage) -> bool {
        let conv = msg.conversation.clone();
        let list = self.conversations.entry(conv.clone()).or_default();
        if list.iter().any(|m| m.id == msg.id) {
            return false;
        }
        list.push(msg);
        // New messages are appended; the whole file is rewritten only when
        // old messages are dropped, now and then rather than on every message.
        if list.len() > MAX_HISTORY + TRIM_SLACK {
            let excess = list.len() - MAX_HISTORY;
            list.drain(..excess);
            self.persist(&conv);
        } else {
            self.append(&conv);
        }
        true
    }

    fn append(&self, conv: &str) {
        if let Some(m) = self.conversations.get(conv).and_then(|l| l.last()) {
            self.append_message(conv, m);
        }
    }

    fn append_message(&self, conv: &str, m: &ChatMessage) {
        let Ok(mut line) = serde_json::to_vec(m) else { return };
        line.push(b'\n');
        let path = self.dir.join(format!("{conv}.jsonl"));
        let result = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(&line));
        if let Err(e) = result {
            tracing::warn!("saving chat {conv}: {e}");
        }
    }

    pub fn set_status(&mut self, conv: &str, id: &str, status: MessageStatus) -> Option<ChatMessage> {
        let msg = self
            .conversations
            .get_mut(conv)?
            .iter_mut()
            .find(|m| m.id == id)?;
        if msg.status == status {
            return None;
        }
        msg.status = status;
        let updated = msg.clone();
        // Appended again; on load the later copy of a message wins.
        self.append_message(conv, &updated);
        Some(updated)
    }

    pub fn get(&self, conv: &str, id: &str) -> Option<ChatMessage> {
        self.conversations.get(conv)?.iter().find(|m| m.id == id).cloned()
    }

    pub fn history(&self, conv: &str) -> Vec<ChatMessage> {
        self.conversations.get(conv).cloned().unwrap_or_default()
    }

    /// Outgoing direct messages to `peer` that have not been delivered yet.
    pub fn undelivered(&self, peer: &str) -> Vec<ChatMessage> {
        self.conversations
            .get(peer)
            .map(|list| {
                list.iter()
                    .filter(|m| m.outgoing && matches!(m.status, MessageStatus::Pending | MessageStatus::Failed))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn summaries(&self) -> Vec<ConversationSummary> {
        self.conversations
            .iter()
            .map(|(conv, list)| ConversationSummary {
                conversation: conv.clone(),
                last: list.last().cloned(),
            })
            .collect()
    }

    fn persist(&self, conv: &str) {
        let Some(list) = self.conversations.get(conv) else { return };
        if list.is_empty() {
            let _ = fs::remove_file(self.dir.join(format!("{conv}.jsonl")));
            return;
        }
        let mut out = Vec::new();
        for m in list {
            if let Ok(line) = serde_json::to_vec(m) {
                out.extend_from_slice(&line);
                out.push(b'\n');
            }
        }
        if let Err(e) = write_atomic(&self.dir.join(format!("{conv}.jsonl")), &out) {
            tracing::warn!("saving chat {conv}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, status: MessageStatus) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            conversation: GENERAL.into(),
            from_id: "0123456789abcdef0123456789abcdef".into(),
            from_name: "A".into(),
            text: format!("text {id}"),
            ts: 1,
            outgoing: true,
            status,
        }
    }

    #[test]
    fn removed_messages_stay_removed() {
        let dir = std::env::temp_dir().join(format!("ropeway-chat-{}", uuid::Uuid::new_v4()));
        let mut store = ChatStore::load(dir.clone()).unwrap();
        for i in 0..5 {
            store.add(msg(&i.to_string(), MessageStatus::Sent));
        }
        let ids: HashSet<String> = ["1", "3", "nope"].iter().map(|s| s.to_string()).collect();
        assert_eq!(store.remove(GENERAL, &ids, None), vec!["1", "3"]);
        // A device can only remove what it sent itself.
        let mut incoming = msg("x", MessageStatus::Received);
        incoming.outgoing = false;
        incoming.from_id = "ffffffffffffffffffffffffffffffff".into();
        store.add(incoming);
        let x: HashSet<String> = ["x".to_string(), "0".to_string()].into();
        assert_eq!(store.remove(GENERAL, &x, Some("0123456789abcdef0123456789abcdef")), Vec::<String>::new());
        assert_eq!(store.remove(GENERAL, &x, Some("ffffffffffffffffffffffffffffffff")), vec!["x"]);
        let left: Vec<String> = ChatStore::load(dir.clone()).unwrap().history(GENERAL).into_iter().map(|m| m.id).collect();
        assert_eq!(left, vec!["0", "2", "4"]);

        store.queue_delete("ffffffffffffffffffffffffffffffff", &["a".into(), "b".into()]);
        let mut reloaded = ChatStore::load(dir.clone()).unwrap();
        assert_eq!(reloaded.take_deletes("ffffffffffffffffffffffffffffffff"), vec!["a", "b"]);
        assert!(reloaded.take_deletes("ffffffffffffffffffffffffffffffff").is_empty());

        assert_eq!(reloaded.clear(GENERAL).len(), 3);
        assert!(ChatStore::load(dir).unwrap().history(GENERAL).is_empty());
    }

    #[test]
    fn appended_history_reloads_with_latest_status() {
        let dir = std::env::temp_dir().join(format!("ropeway-chat-{}", uuid::Uuid::new_v4()));
        let mut store = ChatStore::load(dir.clone()).unwrap();
        for i in 0..(MAX_HISTORY + TRIM_SLACK + 3) {
            store.add(msg(&i.to_string(), MessageStatus::Pending));
        }
        let last = (MAX_HISTORY + TRIM_SLACK + 2).to_string();
        store.set_status(GENERAL, &last, MessageStatus::Sent);

        let reloaded = ChatStore::load(dir).unwrap().history(GENERAL);
        assert!(reloaded.len() <= MAX_HISTORY + TRIM_SLACK);
        assert_eq!(reloaded.iter().filter(|m| m.id == last).count(), 1);
        assert_eq!(reloaded.last().unwrap().status, MessageStatus::Sent);
    }
}
