use std::path::PathBuf;
use std::time::{Duration, Instant};

use opendesk_core::session::SessionEvent;
use opendesk_proto::control::{PeerId, Side};
use opendesk_proto::transfer::{DragInfo, FileBegin, FileChunk, FileEnd};
use opendesk_wayland::WaylandCommand;
use tracing::{debug, warn};

use super::Engine;
use crate::daemon::transfer::{DropAccumulator, TransferPlan, plan_transfer, run_transfer};

const DWELL: Duration = Duration::from_millis(200);
const ABORT_GRACE: Duration = Duration::from_millis(400);

pub struct PendingDrag {
    pub side: Side,
    pub position: f64,
    pub uris: Vec<PathBuf>,
    pub since: Instant,
    pub aborted_at: Option<Instant>,
}

impl Engine {
    pub(super) fn on_drag_entered_edge(&mut self, side: Side, position: f64, uris: Vec<PathBuf>) {
        if !self.enabled || uris.is_empty() || self.connected_peer_for_side(side).is_none() {
            return;
        }
        debug!(%side, files = uris.len(), "drag reached the edge");
        self.pending_drag = Some(PendingDrag {
            side,
            position,
            uris,
            since: Instant::now(),
            aborted_at: None,
        });
    }

    pub(super) fn on_drag_motion_edge(&mut self, side: Side, position: f64) {
        if let Some(drag) = &mut self.pending_drag
            && drag.side == side
            && drag.aborted_at.is_none()
        {
            drag.position = position;
        }
    }

    pub(super) fn on_drag_left_edge(&mut self, side: Side) {
        if self
            .pending_drag
            .as_ref()
            .is_some_and(|drag| drag.side == side && drag.aborted_at.is_none())
        {
            self.pending_drag = None;
        }
    }

    pub(super) fn on_drag_released_edge(&mut self) {
        if self
            .pending_drag
            .as_ref()
            .is_some_and(|drag| drag.aborted_at.is_none())
        {
            self.pending_drag = None;
        }
    }

    pub(super) fn tick_drag(&mut self, now: Instant) {
        let Some(drag) = &self.pending_drag else {
            return;
        };
        match drag.aborted_at {
            None if now.saturating_duration_since(drag.since) >= DWELL => {
                self.wayland(WaylandCommand::AbortLocalDrag);
                if let Some(drag) = &mut self.pending_drag {
                    drag.aborted_at = Some(now);
                }
            }
            Some(aborted_at) if now.saturating_duration_since(aborted_at) >= ABORT_GRACE => {
                debug!("drag abort did not produce an edge crossing, giving up");
                self.pending_drag = None;
            }
            _ => {}
        }
    }

    pub(super) fn drag_edge_crossing(&mut self, side: Side, fraction: f32) -> bool {
        let Some(drag) = &self.pending_drag else {
            return false;
        };
        if drag.aborted_at.is_none() || drag.side != side {
            return false;
        }
        let Some(peer) = self.connected_peer_for_side(side) else {
            self.pending_drag = None;
            return false;
        };
        let uris = drag.uris.clone();
        self.pending_drag = None;
        let transfer_id = self.next_transfer_id;
        self.next_transfer_id += 1;
        match plan_transfer(&uris, transfer_id) {
            Ok(plan) => {
                self.pending_transfer = Some(plan);
                self.dispatch(SessionEvent::DragCrossed {
                    side,
                    fraction,
                    peer,
                });
                true
            }
            Err(error) => {
                warn!(%error, "could not plan the file transfer");
                false
            }
        }
    }

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
            if let Err(error) = run_transfer(plan, connection, tcp).await {
                warn!(%error, "file transfer failed");
            }
        });
    }

    pub(super) fn on_file_begin(&mut self, begin: FileBegin) {
        if !self.session.is_controlled() {
            return;
        }
        let transfer_id = begin.transfer_id;
        let entries = begin.entries.clone();
        if let Err(error) = self.drop_accumulator.begin(begin) {
            warn!(%error, "rejecting an incoming transfer");
            return;
        }
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
        self.wayland(WaylandCommand::StartDropDrag { uris });
    }

    pub(super) fn on_file_chunk(&mut self, chunk: FileChunk) {
        if let Err(error) = self.drop_accumulator.chunk(chunk) {
            warn!(%error, "dropping a bad file chunk");
        }
    }

    pub(super) fn on_file_end(&mut self, end: FileEnd) {
        match self.drop_accumulator.end(end) {
            Ok(uris) if !uris.is_empty() => debug!(files = uris.len(), "file transfer received"),
            Ok(_) => {}
            Err(error) => warn!(%error, "finishing a transfer failed"),
        }
    }

    pub(super) fn on_drag_cancel(&mut self, transfer_id: u64) {
        self.drop_accumulator.cancel(transfer_id);
        if self.active_drop == Some(transfer_id) {
            self.active_drop = None;
            self.wayland(WaylandCommand::CancelDropDrag);
        }
    }

    pub(super) fn cancel_active_drop(&mut self) {
        if let Some(transfer_id) = self.active_drop.take() {
            self.drop_accumulator.cancel(transfer_id);
            self.wayland(WaylandCommand::CancelDropDrag);
        }
    }
}

pub fn new_accumulator(dnd_dir: PathBuf) -> DropAccumulator {
    DropAccumulator::new(dnd_dir)
}

pub type TransferPlanSlot = Option<TransferPlan>;
