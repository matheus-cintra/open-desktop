use std::net::SocketAddr;
use std::time::Instant;

use opendesk_core::session::SessionEvent;
use opendesk_proto::PROTOCOL_VERSION;
use opendesk_proto::control::{ControlMessage, OutputGeometry, PeerId, RejectReason, Token};
use tracing::{info, warn};

use super::Engine;
use super::links::{DialPurpose, PeerLink, PendingConnection, PendingRole};
use crate::daemon::net::tcp::{ConnectionId, TcpCommand, TcpEvent};

struct Hello {
    protocol_version: u16,
    peer_id: PeerId,
    name: String,
    token: Option<Token>,
    udp_port: u16,
    layout: Vec<OutputGeometry>,
}

impl Engine {
    pub(super) fn on_tcp(&mut self, event: TcpEvent) {
        match event {
            TcpEvent::Accepted {
                connection,
                address,
            } => {
                let pending = PendingConnection {
                    address,
                    role: PendingRole::Accepted,
                };
                self.links.add_pending(connection, pending);
            }
            TcpEvent::Connected {
                connection,
                address,
            } => self.on_connected(connection, address),
            TcpEvent::ConnectFailed { address, error } => {
                if let Some(dial) = self.links.take_dial(address) {
                    warn!(%address, error, "connection attempt failed");
                    self.links.record_dial_failure(dial.peer_id, Instant::now());
                    if dial.purpose == DialPurpose::Pairing {
                        self.fail_pairing(rust_i18n::t!("pair.connect_failed").into_owned());
                    }
                }
            }
            TcpEvent::Message {
                connection,
                message,
            } => match self.links.peer_for_connection(connection) {
                Some(peer) => self.on_control_message(peer, message),
                None => self.on_pending_message(connection, message),
            },
            TcpEvent::Closed { connection, reason } => self.on_closed(connection, reason),
        }
    }

    fn on_connected(&mut self, connection: ConnectionId, address: SocketAddr) {
        let Some(dial) = self.links.take_dial(address) else {
            self.close(connection);
            return;
        };
        match dial.purpose {
            DialPurpose::Link => self.send_hello(connection, dial.peer_id),
            DialPurpose::Pairing => {
                let message = ControlMessage::PairRequest {
                    peer_id: self.identity.peer_id,
                    name: self.identity.name.clone(),
                };
                self.send_on(connection, message);
                self.on_pairing_connected(connection);
            }
        }
        let pending = PendingConnection {
            address,
            role: PendingRole::Dialed(dial),
        };
        self.links.add_pending(connection, pending);
    }

    fn on_closed(&mut self, connection: ConnectionId, reason: String) {
        if let Some((peer, link)) = self.links.remove_link(connection) {
            info!(%peer, name = link.name, reason, "peer disconnected");
            if self
                .pending_drag
                .as_ref()
                .is_some_and(|drag| drag.peer == peer)
            {
                self.cancel_pending_drag();
            }
            self.clear_return_drag(peer);
            self.dispatch(SessionEvent::PeerDisconnected { peer });
            if self.active_peer == Some(peer) {
                self.active_peer = None;
            }
            return;
        }
        if let Some(pending) = self.links.take_pending(connection)
            && let PendingRole::Dialed(dial) = pending.role
        {
            self.links.record_dial_failure(dial.peer_id, Instant::now());
            if dial.purpose == DialPurpose::Pairing {
                self.fail_pairing(rust_i18n::t!("pair.connection_closed").into_owned());
            }
        }
    }

    fn on_pending_message(&mut self, connection: ConnectionId, message: ControlMessage) {
        match message {
            ControlMessage::Hello {
                protocol_version,
                peer_id,
                name,
                token,
                udp_port,
                layout,
            } => {
                let hello = Hello {
                    protocol_version,
                    peer_id,
                    name,
                    token,
                    udp_port,
                    layout,
                };
                self.on_hello(connection, hello);
            }
            ControlMessage::HelloAck {
                peer_id,
                name,
                udp_port,
                layout,
            } => {
                self.on_hello_ack(connection, peer_id, name, udp_port, layout);
            }
            ControlMessage::HelloRejected { reason } => {
                warn!(?reason, "peer rejected our hello");
                if let Some(pending) = self.links.take_pending(connection)
                    && let PendingRole::Dialed(dial) = pending.role
                {
                    self.links.record_dial_failure(dial.peer_id, Instant::now());
                }
                self.close(connection);
            }
            ControlMessage::PairRequest { peer_id, name } => {
                self.on_pair_request(connection, peer_id, name);
            }
            ControlMessage::PairPin { pin } => self.on_pair_pin(connection, pin),
            ControlMessage::PairAccepted { token } => self.on_pair_accepted(connection, token),
            ControlMessage::PairRejected { remaining_attempts } => {
                self.on_pair_rejected(connection, remaining_attempts);
            }
            other => {
                warn!(?other, "unexpected message before the handshake");
                self.close(connection);
            }
        }
    }

    fn send_hello(&self, connection: ConnectionId, peer_id: PeerId) {
        let token = self
            .peer_store
            .find_by_id(peer_id)
            .map(|record| record.token);
        let message = ControlMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            peer_id: self.identity.peer_id,
            name: self.identity.name.clone(),
            token,
            udp_port: self.config.general.port,
            layout: self.outputs.clone(),
        };
        self.send_on(connection, message);
    }

    fn on_hello(&mut self, connection: ConnectionId, hello: Hello) {
        let Some(pending) = self.links.take_pending(connection) else {
            self.close(connection);
            return;
        };
        let Hello {
            protocol_version,
            peer_id,
            name,
            token,
            udp_port,
            layout,
        } = hello;
        let rejection = if protocol_version != PROTOCOL_VERSION {
            Some(RejectReason::ProtocolVersion {
                expected: PROTOCOL_VERSION,
                received: protocol_version,
            })
        } else if !self.is_authorized(peer_id, token) {
            Some(RejectReason::Unauthorized)
        } else if self.links.link(peer_id).is_some() {
            Some(RejectReason::Busy)
        } else {
            None
        };
        if let Some(reason) = rejection {
            warn!(%peer_id, name, ?reason, "rejecting hello");
            self.send_on(connection, ControlMessage::HelloRejected { reason });
            self.close(connection);
            return;
        }
        let acknowledgement = ControlMessage::HelloAck {
            peer_id: self.identity.peer_id,
            name: self.identity.name.clone(),
            udp_port: self.config.general.port,
            layout: self.outputs.clone(),
        };
        self.send_on(connection, acknowledgement);
        self.establish_link(peer_id, connection, pending.address, udp_port, name, layout);
    }

    fn on_hello_ack(
        &mut self,
        connection: ConnectionId,
        peer_id: PeerId,
        name: String,
        udp_port: u16,
        layout: Vec<OutputGeometry>,
    ) {
        let Some(pending) = self.links.take_pending(connection) else {
            self.close(connection);
            return;
        };
        match pending.role {
            PendingRole::Dialed(dial) if dial.peer_id == peer_id => {
                self.establish_link(peer_id, connection, pending.address, udp_port, name, layout);
            }
            _ => {
                warn!(%peer_id, "hello acknowledgement from an unexpected connection");
                self.close(connection);
            }
        }
    }

    fn is_authorized(&self, peer_id: PeerId, token: Option<Token>) -> bool {
        let Some(token) = token else {
            return false;
        };
        self.peer_store
            .find_by_id(peer_id)
            .is_some_and(|record| record.token == token)
    }

    fn establish_link(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
        address: SocketAddr,
        udp_port: u16,
        name: String,
        layout: Vec<OutputGeometry>,
    ) {
        info!(%peer_id, name, %address, "peer connected");
        let link = PeerLink::new(connection, address, udp_port, name, layout, Instant::now());
        self.links.insert_link(peer_id, link);
        if let Some(xkb) = self.local_keymap.clone() {
            self.send_on(connection, ControlMessage::Keymap { xkb });
        }
    }

    pub(super) fn dial(&mut self, address: SocketAddr, peer_id: PeerId, purpose: DialPurpose) {
        self.links
            .start_dial(address, super::links::Dial { peer_id, purpose });
        let _ = self.sinks.tcp.send(TcpCommand::Connect { address });
    }
}
