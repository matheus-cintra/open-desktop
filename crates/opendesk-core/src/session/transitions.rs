use std::time::Instant;

use opendesk_proto::control::{DenyReason, PeerId, Side};

use super::{Session, SessionAction, SessionEvent, SessionState, Transition};

fn stay_idle() -> Transition {
    (SessionState::Idle, Vec::new())
}

pub(super) fn from_idle(session: &mut Session, event: SessionEvent, now: Instant) -> Transition {
    match event {
        SessionEvent::EdgeEntered {
            side,
            fraction,
            peer: Some(peer),
        } => enter_edge(session, side, fraction, peer, now),
        SessionEvent::PeerRequestedControl {
            peer,
            side,
            fraction,
        } => session.grant(peer, side, fraction, now),
        _ => stay_idle(),
    }
}

fn enter_edge(
    session: &Session,
    side: Side,
    fraction: f32,
    peer: PeerId,
    now: Instant,
) -> Transition {
    if session.config().immediate_cross {
        let state = SessionState::Requesting {
            peer,
            side,
            fraction,
            since: now,
        };
        let actions = vec![
            SessionAction::LockPointer,
            SessionAction::StartGrab,
            SessionAction::SendRequestControl {
                peer,
                side,
                fraction,
            },
        ];
        return (state, actions);
    }
    let state = SessionState::Pushing {
        side,
        fraction,
        peer,
        accumulated_px: 0.0,
    };
    let actions = vec![
        SessionAction::LockPointer,
        SessionAction::ShowProgress {
            side,
            fraction,
            progress: 0.0,
        },
    ];
    (state, actions)
}

fn outward_motion(side: Side, dx: f64, dy: f64) -> f64 {
    match side {
        Side::Right => dx,
        Side::Left => -dx,
        Side::Bottom => dy,
        Side::Top => -dy,
    }
}

fn cancel_push(side: Side, fraction: f32) -> Vec<SessionAction> {
    vec![
        SessionAction::HideProgress,
        SessionAction::UnlockPointer { side, fraction },
    ]
}

pub(super) fn from_pushing(
    session: &mut Session,
    side: Side,
    fraction: f32,
    peer: PeerId,
    accumulated_px: f64,
    event: SessionEvent,
    now: Instant,
) -> Transition {
    match event {
        SessionEvent::RelativeMotion { dx, dy } => {
            let accumulated_px = accumulated_px + outward_motion(side, dx, dy);
            push_progress(session, side, fraction, peer, accumulated_px, now)
        }
        SessionEvent::EdgeLeft { .. } | SessionEvent::Disabled => {
            (SessionState::Idle, cancel_push(side, fraction))
        }
        SessionEvent::PeerRequestedControl {
            peer: requester,
            side: requester_side,
            fraction: requester_fraction,
        } => {
            let mut actions = cancel_push(side, fraction);
            let (state, grant_actions) =
                session.grant(requester, requester_side, requester_fraction, now);
            actions.extend(grant_actions);
            (state, actions)
        }
        _ => (
            SessionState::Pushing {
                side,
                fraction,
                peer,
                accumulated_px,
            },
            Vec::new(),
        ),
    }
}

fn push_progress(
    session: &Session,
    side: Side,
    fraction: f32,
    peer: PeerId,
    accumulated_px: f64,
    now: Instant,
) -> Transition {
    let config = session.config();
    if accumulated_px >= config.threshold_px {
        let state = SessionState::Requesting {
            peer,
            side,
            fraction,
            since: now,
        };
        let actions = vec![
            SessionAction::HideProgress,
            SessionAction::StartGrab,
            SessionAction::SendRequestControl {
                peer,
                side,
                fraction,
            },
        ];
        return (state, actions);
    }
    if accumulated_px < -config.cancel_px {
        return (SessionState::Idle, cancel_push(side, fraction));
    }
    let progress = (accumulated_px / config.threshold_px).clamp(0.0, 1.0) as f32;
    let state = SessionState::Pushing {
        side,
        fraction,
        peer,
        accumulated_px,
    };
    let actions = vec![SessionAction::ShowProgress {
        side,
        fraction,
        progress,
    }];
    (state, actions)
}

pub(super) fn from_requesting(
    session: &mut Session,
    peer: PeerId,
    side: Side,
    fraction: f32,
    since: Instant,
    event: SessionEvent,
    now: Instant,
) -> Transition {
    let keep = || {
        (
            SessionState::Requesting {
                peer,
                side,
                fraction,
                since,
            },
            Vec::new(),
        )
    };
    let abort = || {
        (
            SessionState::Idle,
            vec![SessionAction::StopGrab {
                side,
                fraction: Some(fraction),
            }],
        )
    };
    match event {
        SessionEvent::PeerGranted {
            peer: granter,
            session_id,
        } if granter == peer => (
            SessionState::Controlling {
                peer,
                session_id,
                side,
            },
            Vec::new(),
        ),
        SessionEvent::PeerDenied { peer: denier } if denier == peer => abort(),
        SessionEvent::PeerDisconnected { peer: gone } if gone == peer => abort(),
        SessionEvent::Disabled => abort(),
        SessionEvent::Tick
            if now.saturating_duration_since(since) >= session.config().request_timeout =>
        {
            abort()
        }
        SessionEvent::PeerRequestedControl {
            peer: requester,
            side: requester_side,
            fraction: requester_fraction,
        } if requester == peer => {
            if session.config().local_id < peer {
                let (state, _) = keep();
                let actions = vec![SessionAction::SendControlDenied {
                    peer,
                    reason: DenyReason::AlreadyControlling,
                }];
                return (state, actions);
            }
            let (_, mut actions) = abort();
            let (state, grant_actions) =
                session.grant(requester, requester_side, requester_fraction, now);
            actions.extend(grant_actions);
            (state, actions)
        }
        _ => keep(),
    }
}
