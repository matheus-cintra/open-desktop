use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use opendesk_proto::control::{OutputGeometry, PeerId};
use opendesk_proto::input::is_newer;

use crate::daemon::net::tcp::ConnectionId;

const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(10);

pub struct PeerLink {
    pub map_ready: bool,
    pub input_status: String,
    pub last_ready: Instant,
    pub macos: bool,
    pub file_drag: bool,
    pub connection: ConnectionId,
    pub address: SocketAddr,
    pub udp_address: SocketAddr,
    pub name: String,
    pub layout: Vec<OutputGeometry>,
    pub last_pong: Instant,
    pub last_udp: Instant,
    udp_last_sequence: Option<u32>,
    udp_next_sequence: u32,
    pub session_id: Option<u32>,
}

impl PeerLink {
    pub fn new(
        connection: ConnectionId,
        address: SocketAddr,
        udp_port: u16,
        name: String,
        layout: Vec<OutputGeometry>,
        now: Instant,
    ) -> PeerLink {
        PeerLink {
            map_ready: false,
            input_status: "unavailable".into(),
            last_ready: now,
            macos: false,
            file_drag: false,
            connection,
            address,
            udp_address: SocketAddr::new(address.ip(), udp_port),
            name,
            layout,
            last_pong: now,
            last_udp: now,
            udp_last_sequence: None,
            udp_next_sequence: 1,
            session_id: None,
        }
    }

    pub fn next_udp_sequence(&mut self) -> u32 {
        let sequence = self.udp_next_sequence;
        self.udp_next_sequence = self.udp_next_sequence.wrapping_add(1);
        sequence
    }

    pub fn accept_udp_sequence(&mut self, sequence: u32) -> bool {
        let accepted = self
            .udp_last_sequence
            .is_none_or(|last_seen| is_newer(sequence, last_seen));
        if accepted {
            self.udp_last_sequence = Some(sequence);
        }
        accepted
    }

    pub fn start_session(&mut self, session_id: u32) {
        self.session_id = Some(session_id);
        self.udp_last_sequence = None;
        self.udp_next_sequence = 1;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialPurpose {
    Link,
    Pairing,
}

pub struct Dial {
    pub peer_id: PeerId,
    pub purpose: DialPurpose,
}

pub enum PendingRole {
    Dialed(Dial),
    Accepted,
}

pub struct PendingConnection {
    pub address: SocketAddr,
    pub role: PendingRole,
}

#[derive(Default)]
pub struct LinkTable {
    links: HashMap<PeerId, PeerLink>,
    pending: HashMap<ConnectionId, PendingConnection>,
    dials: HashMap<SocketAddr, Dial>,
    backoff: HashMap<PeerId, (Instant, Duration)>,
}

impl LinkTable {
    pub fn link(&self, peer_id: PeerId) -> Option<&PeerLink> {
        self.links.get(&peer_id)
    }

    pub fn link_mut(&mut self, peer_id: PeerId) -> Option<&mut PeerLink> {
        self.links.get_mut(&peer_id)
    }

    pub fn links(&self) -> impl Iterator<Item = (&PeerId, &PeerLink)> {
        self.links.iter()
    }

    pub fn peer_for_connection(&self, connection: ConnectionId) -> Option<PeerId> {
        self.links
            .iter()
            .find(|(_, link)| link.connection == connection)
            .map(|(peer_id, _)| *peer_id)
    }

    pub fn peer_for_udp_source(&self, source: SocketAddr) -> Option<PeerId> {
        self.links
            .iter()
            .find(|(_, link)| link.udp_address == source)
            .map(|(peer_id, _)| *peer_id)
    }

    pub fn insert_link(&mut self, peer_id: PeerId, link: PeerLink) {
        self.backoff.remove(&peer_id);
        self.links.insert(peer_id, link);
    }

    pub fn remove_link(&mut self, connection: ConnectionId) -> Option<(PeerId, PeerLink)> {
        let peer_id = self.peer_for_connection(connection)?;
        self.links.remove(&peer_id).map(|link| (peer_id, link))
    }

    pub fn add_pending(&mut self, connection: ConnectionId, pending: PendingConnection) {
        self.pending.insert(connection, pending);
    }

    pub fn take_pending(&mut self, connection: ConnectionId) -> Option<PendingConnection> {
        self.pending.remove(&connection)
    }

    pub fn start_dial(&mut self, address: SocketAddr, dial: Dial) {
        self.dials.insert(address, dial);
    }

    pub fn take_dial(&mut self, address: SocketAddr) -> Option<Dial> {
        self.dials.remove(&address)
    }

    pub fn is_dialing(&self, peer_id: PeerId) -> bool {
        self.dials.values().any(|dial| dial.peer_id == peer_id)
            || self.pending.values().any(|pending| match &pending.role {
                PendingRole::Dialed(dial) => dial.peer_id == peer_id,
                PendingRole::Accepted => false,
            })
    }

    pub fn may_dial(&self, peer_id: PeerId, now: Instant) -> bool {
        !self.links.contains_key(&peer_id)
            && !self.is_dialing(peer_id)
            && self
                .backoff
                .get(&peer_id)
                .is_none_or(|(after, _)| *after <= now)
    }

    pub fn record_dial_failure(&mut self, peer_id: PeerId, now: Instant) {
        let next_delay = self
            .backoff
            .get(&peer_id)
            .map_or(RECONNECT_MIN, |(_, delay)| (*delay * 2).min(RECONNECT_MAX));
        self.backoff.insert(peer_id, (now + next_delay, next_delay));
    }
}
