use std::path::PathBuf;
use std::time::Instant;

use opendesk_core::session::SessionState;
use opendesk_proto::control::PeerId;
use opendesk_proto::transfer::{DragInfo, FileBegin, FileChunk, FileEnd};
use opendesk_wayland::WaylandCommand;
use tracing::{debug, warn};

use super::Engine;
use crate::daemon::transfer::{DropAccumulator, TransferPlan, run_transfer};

impl Engine {
    pub(super) fn take_pending_drag_info(&mut self) -> Option<DragInfo> {
        self.pending_transfer.as_ref().map(|plan| plan.drag.clone())
    }

    pub(super) fn start_transfer_on_grant(&mut self, peer: PeerId) {
        let Some(plan) = self.pending_transfer.take() else {
            return;
        };
        let Some(link) = self.links.link(peer) else {
            return;
        };
        let connection = link.connection;
        let tcp = self.sinks.tcp.clone();
        debug!(
            transfer_id = plan.drag.transfer_id,
            "starting the file transfer"
        );
        tokio::spawn(async move {
            let transfer_id = plan.drag.transfer_id;
            if let Err(error) = run_transfer(plan, connection, tcp.clone()).await {
                warn!(%error, "file transfer failed");
                let _ = tcp.send(crate::daemon::net::tcp::TcpCommand::Send {
                    connection,
                    message: opendesk_proto::control::ControlMessage::DragCancel { transfer_id },
                });
            }
        });
    }

    pub(super) fn on_file_begin(&mut self, peer: PeerId, begin: FileBegin) {
        if self.monitor.lock != super::monitor::LockState::Unlocked
            || !self.monitor.known(Instant::now())
        {
            self.send_to(
                peer,
                opendesk_proto::control::ControlMessage::DragCancel {
                    transfer_id: begin.transfer_id,
                },
            );
            return;
        }
        let authorized_return = self.authorized_return_drop.as_ref().is_some_and(|auth| {
            auth.peer == peer
                && auth.transfer_id == begin.transfer_id
                && auth.entries == begin.entries
                && Instant::now() < auth.expires_at
                && auth.released
                && matches!(self.session.state(), SessionState::Idle)
        });
        if !(self.session.is_controlled() && self.active_peer == Some(peer)) && !authorized_return {
            return;
        }
        if self.receiving_transfer.is_some() {
            warn!(%peer, "rejecting a concurrent file transfer");
            return;
        }
        if self.active_drop.is_some() {
            self.cancel_active_drop();
        }
        let transfer_id = begin.transfer_id;
        let entries = begin.entries.clone();
        let release_requested = authorized_return
            && self
                .authorized_return_drop
                .as_ref()
                .is_some_and(|auth| auth.release_requested);
        let release_requested = release_requested
            || self.incoming_drag.is_some_and(|(owner, id, released)| {
                owner == peer && id == transfer_id && released
            });
        if let Err(error) = self.drop_accumulator.begin(begin) {
            warn!(%error, "rejecting an incoming transfer");
            return;
        }
        if authorized_return {
            self.return_drop_since = self
                .authorized_return_drop
                .as_ref()
                .map(|auth| auth.created_at);
            self.authorized_return_drop = None;
        }
        self.receiving_transfer = Some((peer, transfer_id));
        let root = self
            .drop_accumulator
            .root_dir()
            .join(format!("transfer_{transfer_id}"));
        let uris: Vec<PathBuf> = entries
            .iter()
            .filter(|entry| !entry.relative_path.contains('/'))
            .map(|entry| root.join(&entry.relative_path))
            .collect();
        self.active_drop = Some(transfer_id);
        self.active_drop_peer = Some(peer);
        let id = self.next_drop_id;
        self.next_drop_id = self.next_drop_id.wrapping_add(1);
        self.active_drop_id = Some(id);
        self.return_drop_active = authorized_return;
        self.wayland(WaylandCommand::StartDropDrag { id, uris });
        if release_requested {
            self.wayland(WaylandCommand::ReleaseDropDrag { id });
        }
    }

    pub(super) fn on_file_chunk(&mut self, peer: PeerId, chunk: FileChunk) {
        if self.receiving_transfer != Some((peer, chunk.transfer_id)) {
            return;
        }
        if let Err(error) = self.drop_accumulator.chunk(chunk) {
            warn!(%error, "dropping a bad file chunk");
        }
    }

    pub(super) fn on_file_end(&mut self, peer: PeerId, end: FileEnd) {
        if self.receiving_transfer != Some((peer, end.transfer_id)) {
            return;
        }
        self.receiving_transfer = None;
        match self.drop_accumulator.end(end) {
            Ok(uris) if !uris.is_empty() => debug!(files = uris.len(), "file transfer received"),
            Ok(_) => {}
            Err(error) => warn!(%error, "finishing a transfer failed"),
        }
    }

    pub(super) fn on_drag_cancel(&mut self, peer: PeerId, transfer_id: u64) {
        if self.receiving_transfer != Some((peer, transfer_id))
            && !(self.active_drop == Some(transfer_id) && self.active_drop_peer == Some(peer))
        {
            return;
        }
        self.receiving_transfer = None;
        self.drop_accumulator.cancel(transfer_id);
        self.active_drop = None;
        self.active_drop_peer = None;
        self.active_drop_id = None;
        self.return_drop_active = false;
        self.return_drop_since = None;
        if self
            .incoming_drag
            .is_some_and(|(owner, id, _)| owner == peer && id == transfer_id)
        {
            self.incoming_drag = None;
        }
        self.wayland(WaylandCommand::CancelDropDrag);
    }

    pub(super) fn cancel_active_drop(&mut self) {
        if let Some((_, transfer_id)) = self.receiving_transfer.take() {
            self.drop_accumulator.cancel(transfer_id);
        }
        if let Some(transfer_id) = self.active_drop.take() {
            self.active_drop_peer = None;
            self.active_drop_id = None;
            self.return_drop_active = false;
            self.return_drop_since = None;
            self.drop_accumulator.cancel(transfer_id);
            self.wayland(WaylandCommand::CancelDropDrag);
        }
    }

    pub(super) fn on_drop_drag_ended(&mut self, id: u64, accepted: bool) {
        if self.active_drop_id != Some(id) {
            return;
        }
        debug!(id, accepted, "Wayland drop drag ended");
        self.active_drop = None;
        self.active_drop_peer = None;
        self.active_drop_id = None;
        self.return_drop_active = false;
        self.return_drop_since = None;
        self.incoming_drag = None;
    }
}

pub fn new_accumulator(dnd_dir: PathBuf) -> DropAccumulator {
    DropAccumulator::new(dnd_dir)
}

pub type TransferPlanSlot = Option<TransferPlan>;
