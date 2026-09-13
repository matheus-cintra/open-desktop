use opendesk_core::config::PeerSide;
use opendesk_core::session::{SessionEvent, SessionState};
use tokio::sync::oneshot;

use super::Engine;
use crate::daemon::ipc::{DiscoveredPeerReport, IpcRequest, IpcResponse, PeerStatus, StatusReport};

impl Engine {
    pub(super) fn on_ipc(&mut self, request: IpcRequest, reply: oneshot::Sender<IpcResponse>) {
        let response = match request {
            IpcRequest::Status => IpcResponse::Status(self.status_report()),
            IpcRequest::Discover => IpcResponse::Discovered(self.discovered_report()),
            IpcRequest::Pair { name } => return self.start_pairing(&name, reply),
            IpcRequest::SubmitPin { pin } => return self.submit_pin(pin, reply),
            IpcRequest::PeerSet { name, side } => self.set_peer_side(&name, &side),
            IpcRequest::PeerRemove { name } => self.remove_peer(&name),
            IpcRequest::Release => {
                self.dispatch(SessionEvent::HotkeyPressed);
                IpcResponse::Ok
            }
            IpcRequest::Enable => {
                self.enabled = true;
                IpcResponse::Ok
            }
            IpcRequest::Disable => {
                self.enabled = false;
                self.dispatch(SessionEvent::Disabled);
                IpcResponse::Ok
            }
        };
        let _ = reply.send(response);
    }

    fn status_report(&self) -> StatusReport {
        let peers = self
            .peer_store
            .records()
            .iter()
            .map(|record| {
                let link = self.links.link(record.id);
                PeerStatus {
                    name: record.name.clone(),
                    side: self
                        .config
                        .side_for_peer(&record.name)
                        .map(|side| side.to_string()),
                    connected: link.is_some(),
                    address: link.map(|link| link.address.to_string()),
                }
            })
            .collect();
        StatusReport {
            name: self.identity.name.clone(),
            peer_id: self.identity.peer_id.to_hex(),
            state: state_name(self.session.state()).to_owned(),
            enabled: self.enabled,
            peers,
            pending_pin: self.pending_pin(),
        }
    }

    fn discovered_report(&self) -> Vec<DiscoveredPeerReport> {
        self.discovered
            .values()
            .map(|peer| DiscoveredPeerReport {
                name: peer.name.clone(),
                peer_id: peer.peer_id.to_hex(),
                address: peer.address.to_string(),
                version: peer.version.clone(),
                paired: self.peer_store.find_by_id(peer.peer_id).is_some(),
            })
            .collect()
    }

    fn set_peer_side(&mut self, name: &str, side: &str) -> IpcResponse {
        let Ok(side) = side.parse::<PeerSide>() else {
            let message = rust_i18n::t!("peer.invalid_side", side = side).into_owned();
            return IpcResponse::Error { message };
        };
        if self.peer_store.find_by_name(name).is_none() {
            let message = rust_i18n::t!("peer.not_paired", name = name).into_owned();
            return IpcResponse::Error { message };
        }
        if let Err(error) = self.config.set_peer_side(name, side) {
            return IpcResponse::Error {
                message: error.to_string(),
            };
        }
        self.save_config();
        self.reconfigure_strips();
        IpcResponse::Ok
    }

    fn remove_peer(&mut self, name: &str) -> IpcResponse {
        let removed_from_config = self.config.remove_peer(name);
        let record_id = self.peer_store.find_by_name(name).map(|record| record.id);
        if let Some(peer_id) = record_id {
            if let Some(link) = self.links.link(peer_id) {
                self.close(link.connection);
            }
            self.peer_store.remove(peer_id);
            self.save_peer_store();
        }
        if !removed_from_config && record_id.is_none() {
            let message = rust_i18n::t!("peer.unknown", name = name).into_owned();
            return IpcResponse::Error { message };
        }
        self.save_config();
        self.reconfigure_strips();
        IpcResponse::Ok
    }
}

fn state_name(state: &SessionState) -> &'static str {
    match state {
        SessionState::Idle => "idle",
        SessionState::Pushing { .. } => "pushing",
        SessionState::Requesting { .. } => "requesting",
        SessionState::Controlling { .. } => "controlling",
        SessionState::Controlled { .. } => "controlled",
    }
}
