mod active;
mod transitions;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use opendesk_proto::control::{DenyReason, PeerId, ReleaseReason, Side};

#[derive(Clone, Debug, PartialEq)]
pub struct SessionConfig {
    pub local_id: PeerId,
    pub threshold_px: f64,
    pub cancel_px: f64,
    pub request_timeout: Duration,
    pub arrival_grace: Duration,
    pub reentry_grace: Duration,
    pub immediate_cross: bool,
    pub first_session_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Idle,
    Pushing {
        side: Side,
        fraction: f32,
        peer: PeerId,
        accumulated_px: f64,
    },
    Requesting {
        peer: PeerId,
        side: Side,
        fraction: f32,
        since: Instant,
    },
    Controlling {
        peer: PeerId,
        session_id: u32,
        side: Side,
    },
    Controlled {
        peer: PeerId,
        session_id: u32,
        return_side: Side,
        since: Instant,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    EdgeEntered {
        side: Side,
        fraction: f32,
        peer: Option<PeerId>,
    },
    EdgeLeft {
        side: Side,
    },
    RelativeMotion {
        dx: f64,
        dy: f64,
    },
    PeerRequestedControl {
        peer: PeerId,
        side: Side,
        fraction: f32,
    },
    PeerGranted {
        peer: PeerId,
        session_id: u32,
    },
    PeerDenied {
        peer: PeerId,
    },
    PeerReleased {
        peer: PeerId,
        fraction: Option<f32>,
    },
    HotkeyPressed,
    DragCrossed {
        side: Side,
        fraction: f32,
        peer: PeerId,
    },
    PeerDisconnected {
        peer: PeerId,
    },
    Disabled,
    Tick,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionAction {
    LockPointer,
    UnlockPointer {
        side: Side,
        fraction: f32,
    },
    ShowProgress {
        side: Side,
        fraction: f32,
        progress: f32,
    },
    HideProgress,
    SendRequestControl {
        peer: PeerId,
        side: Side,
        fraction: f32,
    },
    SendControlGranted {
        peer: PeerId,
        session_id: u32,
    },
    SendControlDenied {
        peer: PeerId,
        reason: DenyReason,
    },
    SendReleaseControl {
        peer: PeerId,
        fraction: Option<f32>,
        reason: ReleaseReason,
    },
    StartGrab,
    StopGrab {
        side: Side,
        fraction: Option<f32>,
    },
    WarpCursor {
        side: Side,
        fraction: f32,
    },
    ShowArrival {
        side: Side,
        fraction: f32,
    },
    ReleaseAllPressed,
}

pub(crate) type Transition = (SessionState, Vec<SessionAction>);

#[derive(Debug)]
pub struct Session {
    config: SessionConfig,
    state: SessionState,
    next_session_id: u32,
    parked: Option<(Side, Instant)>,
}

impl Session {
    pub fn new(config: SessionConfig) -> Session {
        Session {
            next_session_id: config.first_session_id,
            config,
            state: SessionState::Idle,
            parked: None,
        }
    }

    /// Called only after the map transfer handshake has committed.
    pub fn activate_origin(&mut self, peer: PeerId, session_id: u32, side: Side) {
        self.state = SessionState::Controlling {
            peer,
            session_id,
            side,
        };
    }
    pub fn activate_destination(
        &mut self,
        peer: PeerId,
        session_id: u32,
        side: Side,
        now: Instant,
    ) {
        self.state = SessionState::Controlled {
            peer,
            session_id,
            return_side: side.opposite(),
            since: now,
        };
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn is_controlling(&self) -> bool {
        matches!(self.state, SessionState::Controlling { .. })
    }

    pub fn is_controlled(&self) -> bool {
        matches!(self.state, SessionState::Controlled { .. })
    }

    pub fn return_side(&self) -> Option<Side> {
        match self.state {
            SessionState::Controlled { return_side, .. } => Some(return_side),
            _ => None,
        }
    }

    pub fn handle(&mut self, event: SessionEvent, now: Instant) -> Vec<SessionAction> {
        if let SessionEvent::EdgeLeft { side } = &event
            && self
                .parked
                .is_some_and(|(parked_side, _)| parked_side == *side)
        {
            self.parked = None;
        }
        if self.is_parked_reentry(&event, now) {
            return Vec::new();
        }
        let current = std::mem::replace(&mut self.state, SessionState::Idle);
        let was_idle = matches!(current, SessionState::Idle);
        let was_controlled = matches!(current, SessionState::Controlled { .. });
        let entered_side = match &event {
            SessionEvent::EdgeEntered { side, .. } => Some(*side),
            _ => None,
        };
        let (next, actions) = match current {
            SessionState::Idle => transitions::from_idle(self, event, now),
            SessionState::Pushing {
                side,
                fraction,
                peer,
                accumulated_px,
            } => transitions::from_pushing(self, side, fraction, peer, accumulated_px, event, now),
            SessionState::Requesting {
                peer,
                side,
                fraction,
                since,
            } => transitions::from_requesting(self, peer, side, fraction, since, event, now),
            SessionState::Controlling {
                peer,
                session_id,
                side,
            } => active::from_controlling(peer, session_id, side, event),
            SessionState::Controlled {
                peer,
                session_id,
                return_side,
                since,
            } => active::from_controlled(self, peer, session_id, return_side, since, event, now),
        };
        if matches!(next, SessionState::Idle) {
            if !was_idle {
                let parked_side = parked_side_from(&actions)
                    .or_else(|| was_controlled.then_some(entered_side).flatten());
                self.parked = parked_side.map(|side| (side, now));
            }
        } else {
            self.parked = None;
        }
        self.state = next;
        actions
    }

    fn is_parked_reentry(&self, event: &SessionEvent, now: Instant) -> bool {
        let SessionEvent::EdgeEntered { side, .. } = event else {
            return false;
        };
        matches!(self.state, SessionState::Idle)
            && self.parked.is_some_and(|(parked_side, parked_at)| {
                parked_side == *side
                    && now.saturating_duration_since(parked_at) < self.config.reentry_grace
            })
    }

    pub(crate) fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub(crate) fn allocate_session_id(&mut self) -> u32 {
        let session_id = self.next_session_id;
        self.next_session_id = self.next_session_id.wrapping_add(1);
        session_id
    }

    pub(crate) fn grant(
        &mut self,
        peer: PeerId,
        requester_side: Side,
        fraction: f32,
        now: Instant,
    ) -> Transition {
        let session_id = self.allocate_session_id();
        let return_side = requester_side.opposite();
        let state = SessionState::Controlled {
            peer,
            session_id,
            return_side,
            since: now,
        };
        let actions = vec![
            SessionAction::SendControlGranted { peer, session_id },
            SessionAction::WarpCursor {
                side: return_side,
                fraction,
            },
            SessionAction::ShowArrival {
                side: return_side,
                fraction,
            },
        ];
        (state, actions)
    }
}

fn parked_side_from(actions: &[SessionAction]) -> Option<Side> {
    actions.iter().find_map(|action| match action {
        SessionAction::StopGrab {
            side,
            fraction: Some(_),
        } => Some(*side),
        _ => None,
    })
}
