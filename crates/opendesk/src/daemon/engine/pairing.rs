use std::net::SocketAddr;
use std::time::{Duration, Instant};

use opendesk_core::peers::PeerRecord;
use opendesk_core::pin::{PairingAttempt, Pin, PinVerdict, generate_token};
use opendesk_proto::control::{ControlMessage, PeerId, RejectReason, Token};
use tokio::sync::oneshot;
use tracing::{info, warn};

use super::Engine;
use super::links::DialPurpose;
use crate::daemon::ipc::IpcResponse;
use crate::daemon::net::tcp::ConnectionId;
use crate::daemon::notify::send_notification;

const OUTGOING_TIMEOUT: Duration = Duration::from_secs(90);
const NOTIFICATION_TIMEOUT_MS: i32 = 60_000;

pub enum Pairing {
    Outgoing {
        peer_id: PeerId,
        name: String,
        reply: Option<oneshot::Sender<IpcResponse>>,
        connection: Option<ConnectionId>,
        started_at: Instant,
    },
    Incoming {
        peer_id: PeerId,
        name: String,
        connection: ConnectionId,
        attempt: PairingAttempt,
    },
}

fn error_response(message: String) -> IpcResponse {
    IpcResponse::Error { message }
}

impl Engine {
    pub(super) fn start_pairing(&mut self, name: &str, reply: oneshot::Sender<IpcResponse>) {
        if self.pairing.is_some() {
            let _ = reply.send(error_response(
                rust_i18n::t!("pair.in_progress").into_owned(),
            ));
            return;
        }
        let Some((peer_id, address)) = self.discovered_address(name) else {
            let message = rust_i18n::t!("pair.unknown_peer", name = name).into_owned();
            let _ = reply.send(error_response(message));
            return;
        };
        self.pairing = Some(Pairing::Outgoing {
            peer_id,
            name: name.to_owned(),
            reply: Some(reply),
            connection: None,
            started_at: Instant::now(),
        });
        self.dial(address, peer_id, DialPurpose::Pairing);
    }

    fn discovered_address(&self, name: &str) -> Option<(PeerId, SocketAddr)> {
        self.discovered
            .values()
            .find(|peer| peer.name == name)
            .map(|peer| (peer.peer_id, peer.address))
    }

    pub(super) fn on_pairing_connected(&mut self, connection: ConnectionId) {
        if let Some(Pairing::Outgoing {
            connection: slot,
            reply,
            ..
        }) = &mut self.pairing
        {
            *slot = Some(connection);
            if let Some(reply) = reply.take() {
                let _ = reply.send(IpcResponse::PinRequired);
            }
        }
    }

    pub(super) fn submit_pin(&mut self, pin: String, reply: oneshot::Sender<IpcResponse>) {
        match &mut self.pairing {
            Some(Pairing::Outgoing {
                connection: Some(connection),
                reply: slot,
                ..
            }) => {
                let connection = *connection;
                *slot = Some(reply);
                self.send_on(connection, ControlMessage::PairPin { pin });
            }
            _ => {
                let message = rust_i18n::t!("pair.not_waiting_for_pin").into_owned();
                let _ = reply.send(error_response(message));
            }
        }
    }

    pub(super) fn on_pair_request(
        &mut self,
        connection: ConnectionId,
        peer_id: PeerId,
        name: String,
    ) {
        if self.pairing.is_some() {
            let reason = RejectReason::Busy;
            self.send_on(connection, ControlMessage::HelloRejected { reason });
            self.close(connection);
            return;
        }
        let pin = Pin::generate();
        info!(%peer_id, name, pin = pin.as_str(), "pairing request received");
        let title = rust_i18n::t!("pair.notification_title").into_owned();
        let body =
            rust_i18n::t!("pair.notification_body", name = name, pin = pin.as_str()).into_owned();
        tokio::spawn(async move {
            if let Err(error) = send_notification(&title, &body, NOTIFICATION_TIMEOUT_MS).await {
                warn!(%error, "pairing notification failed");
            }
        });
        let attempt = PairingAttempt::new(pin, Instant::now());
        self.pairing = Some(Pairing::Incoming {
            peer_id,
            name,
            connection,
            attempt,
        });
    }

    pub(super) fn on_pair_pin(&mut self, connection: ConnectionId, pin: String) {
        let Some(Pairing::Incoming {
            peer_id,
            name,
            connection: expected,
            attempt,
        }) = &mut self.pairing
        else {
            self.close(connection);
            return;
        };
        if *expected != connection {
            self.close(connection);
            return;
        }
        match attempt.verify(&pin, Instant::now()) {
            PinVerdict::Accepted => {
                let record = PeerRecord {
                    id: *peer_id,
                    name: name.clone(),
                    token: generate_token(),
                };
                let token = record.token;
                info!(peer_id = %record.id, name = record.name, "pairing accepted");
                self.peer_store.upsert(record);
                self.save_peer_store();
                self.send_on(connection, ControlMessage::PairAccepted { token });
                self.pairing = None;
            }
            PinVerdict::Wrong { remaining_attempts } => {
                self.send_on(
                    connection,
                    ControlMessage::PairRejected { remaining_attempts },
                );
            }
            PinVerdict::Locked | PinVerdict::Expired => {
                self.send_on(
                    connection,
                    ControlMessage::PairRejected {
                        remaining_attempts: 0,
                    },
                );
                self.close(connection);
                self.pairing = None;
            }
        }
    }

    pub(super) fn on_pair_accepted(&mut self, connection: ConnectionId, token: Token) {
        let Some(Pairing::Outgoing {
            peer_id,
            name,
            reply,
            ..
        }) = self.pairing.take()
        else {
            self.close(connection);
            return;
        };
        info!(%peer_id, name, "paired");
        self.peer_store.upsert(PeerRecord {
            id: peer_id,
            name,
            token,
        });
        self.save_peer_store();
        if let Some(reply) = reply {
            let _ = reply.send(IpcResponse::Ok);
        }
        self.links.take_pending(connection);
        self.close(connection);
    }

    pub(super) fn on_pair_rejected(&mut self, connection: ConnectionId, remaining_attempts: u8) {
        let Some(Pairing::Outgoing { reply, .. }) = &mut self.pairing else {
            return;
        };
        if let Some(reply) = reply.take() {
            let message =
                rust_i18n::t!("pair.rejected", remaining = remaining_attempts).into_owned();
            let _ = reply.send(error_response(message));
        }
        if remaining_attempts == 0 {
            self.pairing = None;
            self.links.take_pending(connection);
            self.close(connection);
        }
    }

    pub(super) fn fail_pairing(&mut self, message: String) {
        if let Some(Pairing::Outgoing { reply, .. }) = self.pairing.take()
            && let Some(reply) = reply
        {
            let _ = reply.send(error_response(message));
        }
    }

    pub(super) fn expire_pairing(&mut self, now: Instant) {
        match &self.pairing {
            Some(Pairing::Outgoing {
                started_at,
                connection,
                ..
            }) if now.duration_since(*started_at) > OUTGOING_TIMEOUT => {
                if let Some(connection) = *connection {
                    self.links.take_pending(connection);
                    self.close(connection);
                }
                self.fail_pairing(rust_i18n::t!("pair.timeout").into_owned());
            }
            Some(Pairing::Incoming {
                attempt,
                connection,
                ..
            }) if attempt.is_expired(now) => {
                let connection = *connection;
                self.links.take_pending(connection);
                self.close(connection);
                self.pairing = None;
            }
            _ => {}
        }
    }

    pub(super) fn pending_pin(&self) -> Option<String> {
        match &self.pairing {
            Some(Pairing::Incoming { attempt, .. }) => Some(attempt.pin().as_str().to_owned()),
            _ => None,
        }
    }
}
