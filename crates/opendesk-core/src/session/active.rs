use std::time::Instant;

use opendesk_proto::control::{DenyReason, PeerId, ReleaseReason, Side};

use super::{Session, SessionAction, SessionEvent, SessionState, Transition};

impl Session {
    pub fn can_return_at(&self, side: Side, now: Instant) -> bool {
        match self.state {
            SessionState::Controlled {
                return_side, since, ..
            } => {
                side == return_side
                    && now.saturating_duration_since(since) >= self.config.arrival_grace
            }
            _ => false,
        }
    }
}

fn deny(peer: PeerId, reason: DenyReason) -> Vec<SessionAction> {
    vec![SessionAction::SendControlDenied { peer, reason }]
}

pub(super) fn from_controlling(
    peer: PeerId,
    session_id: u32,
    side: Side,
    event: SessionEvent,
) -> Transition {
    let keep = |actions| {
        (
            SessionState::Controlling {
                peer,
                session_id,
                side,
            },
            actions,
        )
    };
    let stop = |fraction| SessionAction::StopGrab { side, fraction };
    let release = |reason| SessionAction::SendReleaseControl {
        peer,
        fraction: None,
        reason,
    };
    match event {
        SessionEvent::PeerReleased {
            peer: releaser,
            fraction,
        } if releaser == peer => (
            SessionState::Idle,
            vec![stop(fraction), SessionAction::ReleaseAllPressed],
        ),
        SessionEvent::HotkeyPressed => (
            SessionState::Idle,
            vec![
                release(ReleaseReason::Hotkey),
                stop(None),
                SessionAction::ReleaseAllPressed,
            ],
        ),
        SessionEvent::PeerDisconnected { peer: gone } if gone == peer => (
            SessionState::Idle,
            vec![stop(None), SessionAction::ReleaseAllPressed],
        ),
        SessionEvent::Disabled => (
            SessionState::Idle,
            vec![
                release(ReleaseReason::Disabled),
                stop(None),
                SessionAction::ReleaseAllPressed,
            ],
        ),
        SessionEvent::PeerRequestedControl {
            peer: requester, ..
        } => keep(deny(requester, DenyReason::AlreadyControlling)),
        _ => keep(Vec::new()),
    }
}

pub(super) fn from_controlled(
    session: &Session,
    peer: PeerId,
    session_id: u32,
    return_side: Side,
    since: Instant,
    event: SessionEvent,
    now: Instant,
) -> Transition {
    let keep = |actions| {
        (
            SessionState::Controlled {
                peer,
                session_id,
                return_side,
                since,
            },
            actions,
        )
    };
    match event {
        SessionEvent::EdgeEntered { side, fraction, .. } if side == return_side => {
            if now.saturating_duration_since(since) < session.config().arrival_grace {
                return keep(Vec::new());
            }
            (
                SessionState::Idle,
                vec![
                    SessionAction::SendReleaseControl {
                        peer,
                        fraction: Some(fraction),
                        reason: ReleaseReason::EdgeCrossed,
                    },
                    SessionAction::ReleaseAllPressed,
                ],
            )
        }
        SessionEvent::PeerReleased { peer: releaser, .. } if releaser == peer => {
            (SessionState::Idle, vec![SessionAction::ReleaseAllPressed])
        }
        SessionEvent::PeerDisconnected { peer: gone } if gone == peer => {
            (SessionState::Idle, vec![SessionAction::ReleaseAllPressed])
        }
        SessionEvent::HotkeyPressed => (
            SessionState::Idle,
            vec![
                SessionAction::SendReleaseControl {
                    peer,
                    fraction: None,
                    reason: ReleaseReason::Hotkey,
                },
                SessionAction::ReleaseAllPressed,
            ],
        ),
        SessionEvent::Disabled => (
            SessionState::Idle,
            vec![
                SessionAction::SendReleaseControl {
                    peer,
                    fraction: None,
                    reason: ReleaseReason::Disabled,
                },
                SessionAction::ReleaseAllPressed,
            ],
        ),
        SessionEvent::PeerRequestedControl {
            peer: requester, ..
        } => keep(deny(requester, DenyReason::AlreadyControlled)),
        _ => keep(Vec::new()),
    }
}
