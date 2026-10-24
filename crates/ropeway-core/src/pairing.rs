//! Connecting without discovery.
//!
//! An **invite** (shown as a QR code) carries a device's addresses, port and
//! key fingerprint. The device that reads it dials with the fingerprint
//! pinned, so it reaches exactly that device or nothing.
//!
//! A **temporary code** lets a device find another on the same network: the
//! seeker broadcasts a hash of the code, and the device showing that code
//! answers with its id and port. Six digits are easy to type but also easy
//! to guess, so a code only finds a device; trusting it stays a separate
//! step where the user compares the device code.

use std::net::Ipv4Addr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::identity::is_valid_id;

/// How long a temporary code works.
pub const CODE_TTL: Duration = Duration::from_secs(10 * 60);
const SCHEME: &str = "ropeway://connect?";
const APP_TAG: &str = "ropeway";

#[derive(Clone, Debug, PartialEq)]
pub struct Invite {
    pub id: String,
    pub port: u16,
    pub addrs: Vec<Ipv4Addr>,
    pub name: String,
}

/// `ropeway://connect?id=<fingerprint>&p=<port>&a=<ip>,<ip>[&n=<name>]`
pub fn invite_link(invite: &Invite) -> String {
    let addrs: Vec<String> = invite.addrs.iter().map(|a| a.to_string()).collect();
    let mut link = format!("{SCHEME}id={}&p={}&a={}", invite.id, invite.port, addrs.join(","));
    if !invite.name.is_empty() {
        link += &format!("&n={}", encode(&invite.name));
    }
    link
}

/// The addresses worth putting in a QR code, most likely first: home and
/// office networks before VPNs and virtual adapters (VirtualBox, WSL).
/// Fewer addresses make a smaller code that cameras read more easily.
pub fn invite_addresses(all: &[Ipv4Addr], max: usize) -> Vec<Ipv4Addr> {
    let rank = |a: &Ipv4Addr| {
        let o = a.octets();
        match o {
            [192, 168, 56, _] => 3,
            [192, 168, ..] => 0,
            [10, ..] => 1,
            [172, b, ..] if (16..32).contains(&b) => 2,
            _ => 4,
        }
    };
    let mut sorted = all.to_vec();
    sorted.sort_by_key(rank);
    sorted.truncate(max);
    sorted
}

/// The same invite in QR codes' alphanumeric alphabet (digits, capitals and
/// `:/`), which a QR code stores in fewer modules than a link:
/// `RW:<FINGERPRINT>:<port>:<ip>/<ip>`.
pub fn invite_qr_text(invite: &Invite) -> String {
    let addrs: Vec<String> = invite.addrs.iter().map(|a| a.to_string()).collect();
    format!("RW:{}:{}:{}", invite.id.to_ascii_uppercase(), invite.port, addrs.join("/"))
}

/// Reads an invite link or its QR form.
pub fn parse_invite(link: &str) -> Option<Invite> {
    let link = link.trim();
    if let Some(rest) = link.strip_prefix("RW:") {
        let mut parts = rest.splitn(3, ':');
        let id = parts.next()?.to_ascii_lowercase();
        let port = parts.next()?.parse::<u16>().ok().filter(|p| *p != 0)?;
        let addrs: Vec<Ipv4Addr> = parts.next()?.split('/').filter_map(|a| a.parse().ok()).take(16).collect();
        return (is_valid_id(&id) && !addrs.is_empty()).then_some(Invite { id, port, addrs, name: String::new() });
    }
    let query = link.strip_prefix(SCHEME)?;
    let (mut id, mut port, mut addrs, mut name) = (None, None, Vec::new(), String::new());
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        match key {
            "id" => id = Some(value.to_ascii_lowercase()),
            "p" => port = value.parse::<u16>().ok(),
            "a" => addrs = value.split(',').filter_map(|a| a.parse::<Ipv4Addr>().ok()).take(16).collect(),
            "n" => name = decode(value)?.chars().filter(|c| !c.is_control()).take(64).collect(),
            _ => {}
        }
    }
    let id = id.filter(|i| is_valid_id(i))?;
    let port = port.filter(|p| *p != 0)?;
    (!addrs.is_empty()).then_some(Invite { id, port, addrs, name })
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A new six-digit code.
pub fn new_code() -> String {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 4];
    ring::rand::SystemRandom::new().fill(&mut b).expect("system random generator");
    format!("{:06}", u32::from_le_bytes(b) % 1_000_000)
}

/// Digits only, so "123 456" and "123-456" work too.
pub fn normalize_code(code: &str) -> Option<String> {
    let digits: String = code.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 6 && digits.len() == code.chars().filter(|c| !c.is_whitespace() && *c != '-').count()).then_some(digits)
}

fn hash(parts: &[&str]) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    ctx.update(b"ropeway-code");
    for p in parts {
        ctx.update(&[0]);
        ctx.update(p.as_bytes());
    }
    hex::encode(&ctx.finish().as_ref()[..16])
}

/// "Who shows this code?" Broadcast by the device looking for one.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CodeQuery {
    pub app: String,
    pub salt: String,
    pub find: String,
}

/// "I do." Sent back to the asking address.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CodeReply {
    pub app: String,
    pub salt: String,
    /// Proves the code was known when the reply was made.
    pub proof: String,
    pub id: String,
    pub name: String,
    pub port: u16,
}

impl CodeQuery {
    pub fn new(code: &str) -> Self {
        let salt = new_code() + &new_code();
        Self { app: APP_TAG.into(), find: hash(&[&salt, code]), salt }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let q: Self = serde_json::from_slice(bytes).ok()?;
        (q.app == APP_TAG && q.salt.len() <= 64 && q.find.len() == 32).then_some(q)
    }

    pub fn matches(&self, code: &str) -> bool {
        hash(&[&self.salt, code]) == self.find
    }

    pub fn reply(&self, code: &str, id: &str, name: &str, port: u16) -> CodeReply {
        CodeReply {
            app: APP_TAG.into(),
            salt: self.salt.clone(),
            proof: hash(&[&self.salt, code, id]),
            id: id.into(),
            name: name.into(),
            port,
        }
    }
}

impl CodeReply {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let r: Self = serde_json::from_slice(bytes).ok()?;
        (r.app == APP_TAG && is_valid_id(&r.id) && r.port != 0).then_some(r)
    }

    /// Answers `query` and was made by someone who knows `code`.
    pub fn answers(&self, query: &CodeQuery, code: &str) -> bool {
        self.salt == query.salt && self.proof == hash(&[&query.salt, code, &self.id])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_round_trip() {
        let invite = Invite {
            id: "0123456789abcdef0123456789abcdef".into(),
            port: 47801,
            addrs: vec![Ipv4Addr::new(192, 168, 1, 20), Ipv4Addr::new(10, 0, 0, 5)],
            name: "Ноутбук & co".into(),
        };
        let link = invite_link(&invite);
        assert!(link.starts_with("ropeway://connect?id=0123"));
        assert_eq!(parse_invite(&link), Some(invite));
        assert_eq!(parse_invite("ropeway://connect?id=xyz&p=1&a=1.2.3.4"), None);
        assert_eq!(parse_invite("https://example.com"), None);
        assert_eq!(parse_invite("ropeway://connect?id=0123456789abcdef0123456789abcdef&p=1&a="), None);
    }

    #[test]
    fn qr_addresses_put_home_networks_first() {
        let all: Vec<Ipv4Addr> = ["26.197.122.39", "172.19.0.1", "192.168.0.100", "192.168.56.1", "10.0.0.5"]
            .iter()
            .map(|a| a.parse().unwrap())
            .collect();
        let picked: Vec<String> = invite_addresses(&all, 3).iter().map(|a| a.to_string()).collect();
        let qr = Invite { id: "0123456789abcdef0123456789abcdef".into(), port: 47801, addrs: invite_addresses(&all, 3), name: String::new() };
        let text = invite_qr_text(&qr);
        assert_eq!(text, "RW:0123456789ABCDEF0123456789ABCDEF:47801:192.168.0.100/10.0.0.5/172.19.0.1");
        assert_eq!(parse_invite(&text), Some(qr));
        assert_eq!(parse_invite("RW:XYZ:1:1.2.3.4"), None);
        assert_eq!(picked, vec!["192.168.0.100", "10.0.0.5", "172.19.0.1"]);
        let link = invite_link(&Invite { id: "0123456789abcdef0123456789abcdef".into(), port: 1, addrs: all, name: String::new() });
        assert!(!link.contains("&n="));
    }

    #[test]
    fn codes() {
        let code = new_code();
        assert_eq!(code.len(), 6);
        assert_eq!(normalize_code("123 456").as_deref(), Some("123456"));
        assert_eq!(normalize_code("123-456").as_deref(), Some("123456"));
        assert_eq!(normalize_code("12345"), None);
        assert_eq!(normalize_code("12a456"), None);

        let q = CodeQuery::new("123456");
        let q = CodeQuery::parse(&serde_json::to_vec(&q).unwrap()).unwrap();
        assert!(q.matches("123456"));
        assert!(!q.matches("123457"));
        let r = q.reply("123456", "0123456789abcdef0123456789abcdef", "PC", 47801);
        let r = CodeReply::parse(&serde_json::to_vec(&r).unwrap()).unwrap();
        assert!(r.answers(&q, "123456"));
        assert!(!r.answers(&q, "000000"));
        assert!(!r.answers(&CodeQuery::new("123456"), "123456"));
    }
}
