//! LAN discovery over UDP broadcast.
//!
//! Each device periodically broadcasts a small JSON announcement on
//! [`DISCOVERY_PORT`]. The announcement is only a hint: the id it claims is
//! verified by the TLS handshake when a connection is made.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

pub const DISCOVERY_PORT: u16 = 47800;
const APP_TAG: &str = "ropeway";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Announce {
    pub app: String,
    pub v: u32,
    pub id: String,
    pub name: String,
    pub os: String,
    pub port: u16,
}

impl Announce {
    pub fn new(id: &str, name: &str, port: u16) -> Self {
        Self {
            app: APP_TAG.into(),
            v: crate::protocol::PROTOCOL_VERSION,
            id: id.into(),
            name: name.into(),
            os: std::env::consts::OS.into(),
            port,
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let a: Announce = serde_json::from_slice(bytes).ok()?;
        (a.app == APP_TAG && crate::identity::is_valid_id(&a.id)).then_some(a)
    }
}

/// Binds the shared discovery port. `SO_REUSEADDR` lets several instances on
/// one machine (e.g. while testing) all receive the broadcasts.
pub fn bind(port: u16) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    socket.bind(&addr.into())?;
    Ok(UdpSocket::from_std(socket.into())?)
}

/// Limited broadcast plus the directed broadcast address of every IPv4
/// interface (some adapters drop 255.255.255.255).
pub fn broadcast_targets() -> Vec<SocketAddr> {
    broadcast_targets_on(DISCOVERY_PORT)
}

pub fn broadcast_targets_on(port: u16) -> Vec<SocketAddr> {
    let mut targets = vec![SocketAddr::from((Ipv4Addr::BROADCAST, port))];
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if iface.is_loopback() {
            continue;
        }
        if let if_addrs::IfAddr::V4(v4) = iface.addr {
            if let Some(b) = v4.broadcast {
                let t = SocketAddr::from((b, port));
                if !targets.contains(&t) {
                    targets.push(t);
                }
            }
        }
    }
    targets
}

/// Non-loopback IPv4 addresses of this machine, for display.
pub fn local_addresses() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) if !v4.ip.is_link_local() => Some(v4.ip),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}
