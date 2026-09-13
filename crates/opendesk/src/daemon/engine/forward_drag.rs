use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use opendesk_core::session::{SessionEvent, SessionState};
use opendesk_proto::control::{PeerId, Side};
use opendesk_wayland::WaylandCommand;
use tracing::{debug, warn};

use super::Engine;
use crate::daemon::transfer::plan_transfer;

const DWELL: Duration = Duration::from_millis(200);
const FOCUS_TIMEOUT: Duration = Duration::from_secs(2);
const ABORT_GRACE: Duration = Duration::from_millis(400);

pub struct PendingDrag {
    pub generation: u64,
    pub id: u64,
    pub peer: PeerId,
    pub side: Side,
    pub position: f64,
    pub uris: Vec<PathBuf>,
    pub since: Instant,
    pub preparing_at: Option<Instant>,
    pub aborted_at: Option<Instant>,
}

impl Engine {
    pub(super) fn on_drag_entered_edge(
        &mut self,
        generation: u64,
        side: Side,
        position: f64,
        uris: Vec<PathBuf>,
    ) {
        if !self.enabled || uris.is_empty() || !matches!(self.session.state(), SessionState::Idle) {
            return;
        }
        if generation != self.drag_generation() {
            return;
        }
        let Some(peer) = self.connected_peer_for_side(side) else {
            return;
        };
        self.cancel_pending_drag();
        debug!(%side, files = uris.len(), "drag reached the edge");
        let id = self.next_transfer_id;
        self.next_transfer_id = self.next_transfer_id.wrapping_add(1);
        self.pending_drag = Some(PendingDrag {
            generation,
            id,
            peer,
            side,
            position,
            uris,
            since: Instant::now(),
            preparing_at: None,
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

    pub(super) fn on_drag_left_edge(&mut self, generation: u64, side: Side) {
        if generation != self.drag_generation() {
            return;
        }
        if self.pending_drag.as_ref().is_some_and(|drag| {
            drag.side == side && drag.generation < generation && drag.aborted_at.is_none()
        }) {
            self.cancel_pending_drag();
        }
    }

    pub(super) fn on_drag_released_edge(&mut self, generation: u64) {
        if generation != self.drag_generation() {
            return;
        }
        if self
            .pending_drag
            .as_ref()
            .is_some_and(|drag| drag.generation < generation && drag.aborted_at.is_none())
        {
            self.cancel_pending_drag();
        }
    }

    pub(super) fn on_drag_generation(&mut self, generation: u64) {
        if generation != self.drag_generation() {
            return;
        }
        if self
            .pending_drag
            .as_ref()
            .is_some_and(|drag| drag.generation != generation && drag.aborted_at.is_none())
        {
            self.cancel_pending_drag();
        }
    }

    pub(super) fn on_drag_focus_ready(&mut self, id: u64) {
        if self.pending_drag.as_ref().is_some_and(|drag| {
            drag.id == id && drag.aborted_at.is_none() && drag.generation != self.drag_generation()
        }) {
            self.cancel_pending_drag();
            return;
        }
        let Some(drag) = self.pending_drag.as_mut() else {
            return;
        };
        if drag.id != id || drag.preparing_at.is_none() || drag.aborted_at.is_some() {
            return;
        }
        drag.aborted_at = Some(Instant::now());
        self.wayland(WaylandCommand::AbortLocalDrag);
    }

    pub(super) fn tick_drag(&mut self, now: Instant) {
        let Some(drag) = &self.pending_drag else {
            return;
        };
        if drag.aborted_at.is_none() && drag.generation != self.drag_generation() {
            self.cancel_pending_drag();
            return;
        }
        match (drag.preparing_at, drag.aborted_at) {
            (None, None) if now.saturating_duration_since(drag.since) >= DWELL => {
                self.wayland(WaylandCommand::PrepareDragFocus { id: drag.id });
                if let Some(drag) = self.pending_drag.as_mut() {
                    drag.preparing_at = Some(now);
                }
            }
            (Some(preparing), None)
                if now.saturating_duration_since(preparing) >= FOCUS_TIMEOUT =>
            {
                debug!("drag focus was not acquired in time");
                self.cancel_pending_drag();
            }
            (_, Some(aborted)) if now.saturating_duration_since(aborted) >= ABORT_GRACE => {
                debug!("drag abort did not produce an edge crossing, giving up");
                self.cancel_pending_drag();
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
        let id = drag.id;
        let peer = drag.peer;
        let uris = drag.uris.clone();
        if self.connected_peer_for_side(side) != Some(peer) {
            self.cancel_pending_drag();
            return false;
        }
        self.pending_drag = None;
        match plan_transfer(&uris, id) {
            Ok(plan) => {
                self.outgoing_drag = Some((peer, id, Instant::now()));
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
                self.wayland(WaylandCommand::CancelDragFocus { id });
                false
            }
        }
    }

    pub(super) fn cancel_pending_drag(&mut self) {
        if let Some(drag) = self.pending_drag.take()
            && drag.preparing_at.is_some()
        {
            self.wayland(WaylandCommand::CancelDragFocus { id: drag.id });
        }
    }

    pub(super) fn abort_pending_drag(&mut self) {
        if self.pending_drag.is_some() {
            self.wayland(WaylandCommand::AbortLocalDrag);
            self.cancel_pending_drag();
        }
    }

    fn drag_generation(&self) -> u64 {
        self.sinks.wayland.drag_generation.load(Ordering::Acquire)
    }
}
