use std::time::{Duration, Instant};

use opendesk_proto::control::{DenyReason, PeerId, ReleaseReason, Side};

use super::{Session, SessionAction, SessionConfig, SessionEvent, SessionState};

const LOCAL: PeerId = PeerId([0x10; 16]);
const REMOTE: PeerId = PeerId([0x20; 16]);
const OTHER: PeerId = PeerId([0x30; 16]);
const THRESHOLD: f64 = 60.0;
const CANCEL: f64 = 8.0;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const ARRIVAL_GRACE: Duration = Duration::from_millis(300);
const REENTRY_GRACE: Duration = Duration::from_millis(500);
const FIRST_SESSION_ID: u32 = 7;

fn config(local_id: PeerId, immediate_cross: bool) -> SessionConfig {
    SessionConfig {
        local_id,
        threshold_px: THRESHOLD,
        cancel_px: CANCEL,
        request_timeout: REQUEST_TIMEOUT,
        arrival_grace: ARRIVAL_GRACE,
        reentry_grace: REENTRY_GRACE,
        immediate_cross,
        first_session_id: FIRST_SESSION_ID,
    }
}

fn session() -> Session {
    Session::new(config(LOCAL, false))
}

fn entered(side: Side, fraction: f32) -> SessionEvent {
    SessionEvent::EdgeEntered {
        side,
        fraction,
        peer: Some(REMOTE),
    }
}

fn motion(dx: f64, dy: f64) -> SessionEvent {
    SessionEvent::RelativeMotion { dx, dy }
}

fn remote_request(side: Side, fraction: f32) -> SessionEvent {
    SessionEvent::PeerRequestedControl {
        peer: REMOTE,
        side,
        fraction,
    }
}

fn pushing_session(now: Instant) -> Session {
    let mut session = session();
    session.handle(entered(Side::Right, 0.5), now);
    session
}

fn requesting_session(now: Instant) -> Session {
    let mut session = pushing_session(now);
    let actions = session.handle(motion(THRESHOLD, 0.0), now);
    assert!(matches!(session.state(), SessionState::Requesting { .. }));
    assert_eq!(actions.len(), 3);
    session
}

fn controlling_session(now: Instant) -> Session {
    let mut session = requesting_session(now);
    session.handle(
        SessionEvent::PeerGranted {
            peer: REMOTE,
            session_id: 42,
        },
        now,
    );
    assert!(session.is_controlling());
    session
}

fn controlled_session(now: Instant) -> Session {
    let mut session = session();
    session.handle(remote_request(Side::Left, 0.25), now);
    assert!(session.is_controlled());
    session
}

fn granted_actions(fraction: f32, session_id: u32, return_side: Side) -> Vec<SessionAction> {
    vec![
        SessionAction::SendControlGranted {
            peer: REMOTE,
            session_id,
        },
        SessionAction::WarpCursor {
            side: return_side,
            fraction,
        },
        SessionAction::ShowArrival {
            side: return_side,
            fraction,
        },
    ]
}

#[test]
fn rule_1_idle_edge_entered_starts_pushing() {
    let now = Instant::now();
    let mut session = session();
    let actions = session.handle(entered(Side::Right, 0.5), now);
    assert_eq!(
        actions,
        vec![
            SessionAction::LockPointer,
            SessionAction::ShowProgress {
                side: Side::Right,
                fraction: 0.5,
                progress: 0.0
            }
        ]
    );
    assert_eq!(
        session.state(),
        &SessionState::Pushing {
            side: Side::Right,
            fraction: 0.5,
            peer: REMOTE,
            accumulated_px: 0.0
        }
    );
}

#[test]
fn rule_1_idle_edge_entered_with_immediate_cross_requests_at_once() {
    let now = Instant::now();
    let mut session = Session::new(config(LOCAL, true));
    let actions = session.handle(entered(Side::Top, 0.1), now);
    assert_eq!(
        actions,
        vec![
            SessionAction::LockPointer,
            SessionAction::StartGrab,
            SessionAction::SendRequestControl {
                peer: REMOTE,
                side: Side::Top,
                fraction: 0.1
            }
        ]
    );
    assert_eq!(
        session.state(),
        &SessionState::Requesting {
            peer: REMOTE,
            side: Side::Top,
            fraction: 0.1,
            since: now
        }
    );
}

#[test]
fn rule_1_idle_edge_entered_without_peer_is_ignored() {
    let mut session = session();
    let actions = session.handle(
        SessionEvent::EdgeEntered {
            side: Side::Left,
            fraction: 0.5,
            peer: None,
        },
        Instant::now(),
    );
    assert!(actions.is_empty());
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_2_pushing_accumulates_outward_motion_per_side() {
    let now = Instant::now();
    let cases = [
        (Side::Right, (10.0, 0.0)),
        (Side::Left, (-10.0, 0.0)),
        (Side::Bottom, (0.0, 10.0)),
        (Side::Top, (0.0, -10.0)),
    ];
    for (side, (dx, dy)) in cases {
        let mut session = session();
        session.handle(entered(side, 0.5), now);
        let actions = session.handle(motion(dx, dy), now);
        assert_eq!(
            actions,
            vec![SessionAction::ShowProgress {
                side,
                fraction: 0.5,
                progress: (10.0 / THRESHOLD) as f32
            }],
            "{side:?}"
        );
        assert!(
            matches!(session.state(), SessionState::Pushing { accumulated_px, .. } if *accumulated_px == 10.0),
            "{side:?}"
        );
    }
}

#[test]
fn rule_2_pushing_past_threshold_requests_control() {
    let now = Instant::now();
    let mut session = pushing_session(now);
    session.handle(motion(30.0, 0.0), now);
    let actions = session.handle(motion(30.0, 0.0), now);
    assert_eq!(
        actions,
        vec![
            SessionAction::HideProgress,
            SessionAction::StartGrab,
            SessionAction::SendRequestControl {
                peer: REMOTE,
                side: Side::Right,
                fraction: 0.5
            }
        ]
    );
    assert_eq!(
        session.state(),
        &SessionState::Requesting {
            peer: REMOTE,
            side: Side::Right,
            fraction: 0.5,
            since: now
        }
    );
}

#[test]
fn rule_2_pushing_backwards_past_cancel_returns_to_idle() {
    let now = Instant::now();
    let mut session = pushing_session(now);
    let actions = session.handle(motion(-(CANCEL + 1.0), 0.0), now);
    assert_eq!(
        actions,
        vec![
            SessionAction::HideProgress,
            SessionAction::UnlockPointer {
                side: Side::Right,
                fraction: 0.5
            }
        ]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_2_pushing_progress_is_clamped() {
    let now = Instant::now();
    let mut session = pushing_session(now);
    let actions = session.handle(motion(-CANCEL, 0.0), now);
    assert_eq!(
        actions,
        vec![SessionAction::ShowProgress {
            side: Side::Right,
            fraction: 0.5,
            progress: 0.0
        }]
    );
}

#[test]
fn rule_3_pushing_edge_left_returns_to_idle() {
    let now = Instant::now();
    let mut session = pushing_session(now);
    let actions = session.handle(SessionEvent::EdgeLeft { side: Side::Right }, now);
    assert_eq!(
        actions,
        vec![
            SessionAction::HideProgress,
            SessionAction::UnlockPointer {
                side: Side::Right,
                fraction: 0.5
            }
        ]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_4_requesting_granted_by_same_peer_becomes_controlling() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let actions = session.handle(
        SessionEvent::PeerGranted {
            peer: REMOTE,
            session_id: 42,
        },
        now,
    );
    assert!(actions.is_empty());
    assert_eq!(
        session.state(),
        &SessionState::Controlling {
            peer: REMOTE,
            session_id: 42,
            side: Side::Right
        }
    );
    assert!(session.is_controlling());
    assert!(!session.is_controlled());
}

#[test]
fn rule_4_requesting_granted_by_other_peer_is_ignored() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let actions = session.handle(
        SessionEvent::PeerGranted {
            peer: OTHER,
            session_id: 42,
        },
        now,
    );
    assert!(actions.is_empty());
    assert!(matches!(session.state(), SessionState::Requesting { .. }));
}

#[test]
fn rule_5_requesting_denied_by_same_peer_stops_grab() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let actions = session.handle(SessionEvent::PeerDenied { peer: REMOTE }, now);
    assert_eq!(
        actions,
        vec![SessionAction::StopGrab {
            side: Side::Right,
            fraction: Some(0.5)
        }]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_5_requesting_denied_by_other_peer_is_ignored() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let actions = session.handle(SessionEvent::PeerDenied { peer: OTHER }, now);
    assert!(actions.is_empty());
    assert!(matches!(session.state(), SessionState::Requesting { .. }));
}

#[test]
fn rule_6_requesting_times_out_on_tick() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let early = session.handle(
        SessionEvent::Tick,
        now + REQUEST_TIMEOUT - Duration::from_millis(1),
    );
    assert!(early.is_empty());
    assert!(matches!(session.state(), SessionState::Requesting { .. }));
    let actions = session.handle(SessionEvent::Tick, now + REQUEST_TIMEOUT);
    assert_eq!(
        actions,
        vec![SessionAction::StopGrab {
            side: Side::Right,
            fraction: Some(0.5)
        }]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_7_simultaneous_request_lower_id_keeps_requesting() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let actions = session.handle(remote_request(Side::Left, 0.5), now);
    assert_eq!(
        actions,
        vec![SessionAction::SendControlDenied {
            peer: REMOTE,
            reason: DenyReason::AlreadyControlling
        }]
    );
    assert!(matches!(session.state(), SessionState::Requesting { .. }));
}

#[test]
fn rule_7_simultaneous_request_higher_id_yields() {
    let now = Instant::now();
    let mut session = Session::new(config(OTHER, false));
    session.handle(entered(Side::Right, 0.5), now);
    session.handle(motion(THRESHOLD, 0.0), now);
    let actions = session.handle(remote_request(Side::Left, 0.75), now);
    let mut expected = vec![SessionAction::StopGrab {
        side: Side::Right,
        fraction: Some(0.5),
    }];
    expected.extend(granted_actions(0.75, FIRST_SESSION_ID, Side::Right));
    assert_eq!(actions, expected);
    assert_eq!(
        session.state(),
        &SessionState::Controlled {
            peer: REMOTE,
            session_id: FIRST_SESSION_ID,
            return_side: Side::Right,
            since: now
        }
    );
}

#[test]
fn rule_8_controlling_ignores_motion() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    let actions = session.handle(motion(100.0, 100.0), now);
    assert!(actions.is_empty());
    assert!(session.is_controlling());
}

#[test]
fn rule_9_controlling_peer_released_returns_to_idle() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    let actions = session.handle(
        SessionEvent::PeerReleased {
            peer: REMOTE,
            fraction: Some(0.3),
        },
        now,
    );
    assert_eq!(
        actions,
        vec![
            SessionAction::StopGrab {
                side: Side::Right,
                fraction: Some(0.3)
            },
            SessionAction::ReleaseAllPressed
        ]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_9_controlling_release_from_other_peer_is_ignored() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    let actions = session.handle(
        SessionEvent::PeerReleased {
            peer: OTHER,
            fraction: None,
        },
        now,
    );
    assert!(actions.is_empty());
    assert!(session.is_controlling());
}

#[test]
fn rule_10_controlling_hotkey_releases_control() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    let actions = session.handle(SessionEvent::HotkeyPressed, now);
    assert_eq!(
        actions,
        vec![
            SessionAction::SendReleaseControl {
                peer: REMOTE,
                fraction: None,
                reason: ReleaseReason::Hotkey
            },
            SessionAction::StopGrab {
                side: Side::Right,
                fraction: None
            },
            SessionAction::ReleaseAllPressed
        ]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_11_controlling_peer_disconnected_returns_to_idle() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    let actions = session.handle(SessionEvent::PeerDisconnected { peer: REMOTE }, now);
    assert_eq!(
        actions,
        vec![
            SessionAction::StopGrab {
                side: Side::Right,
                fraction: None
            },
            SessionAction::ReleaseAllPressed
        ]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_11_requesting_peer_disconnected_returns_to_idle() {
    let now = Instant::now();
    let mut session = requesting_session(now);
    let actions = session.handle(SessionEvent::PeerDisconnected { peer: REMOTE }, now);
    assert_eq!(
        actions,
        vec![SessionAction::StopGrab {
            side: Side::Right,
            fraction: Some(0.5)
        }]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_12_idle_peer_request_is_granted_with_incrementing_ids() {
    let now = Instant::now();
    let mut session = session();
    let actions = session.handle(remote_request(Side::Left, 0.25), now);
    assert_eq!(
        actions,
        granted_actions(0.25, FIRST_SESSION_ID, Side::Right)
    );
    assert_eq!(
        session.state(),
        &SessionState::Controlled {
            peer: REMOTE,
            session_id: FIRST_SESSION_ID,
            return_side: Side::Right,
            since: now
        }
    );
    session.handle(SessionEvent::PeerDisconnected { peer: REMOTE }, now);
    let actions = session.handle(remote_request(Side::Top, 0.9), now);
    assert_eq!(
        actions,
        granted_actions(0.9, FIRST_SESSION_ID + 1, Side::Bottom)
    );
}

#[test]
fn rule_13_controlling_denies_new_requests() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    let actions = session.handle(
        SessionEvent::PeerRequestedControl {
            peer: OTHER,
            side: Side::Left,
            fraction: 0.5,
        },
        now,
    );
    assert_eq!(
        actions,
        vec![SessionAction::SendControlDenied {
            peer: OTHER,
            reason: DenyReason::AlreadyControlling
        }]
    );
    assert!(session.is_controlling());
}

#[test]
fn rule_13_controlled_denies_new_requests() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let actions = session.handle(
        SessionEvent::PeerRequestedControl {
            peer: OTHER,
            side: Side::Left,
            fraction: 0.5,
        },
        now,
    );
    assert_eq!(
        actions,
        vec![SessionAction::SendControlDenied {
            peer: OTHER,
            reason: DenyReason::AlreadyControlled
        }]
    );
    assert!(session.is_controlled());
}

#[test]
fn rule_13_pushing_peer_request_cancels_push_then_grants() {
    let now = Instant::now();
    let mut session = pushing_session(now);
    let actions = session.handle(remote_request(Side::Left, 0.25), now);
    let mut expected = vec![
        SessionAction::HideProgress,
        SessionAction::UnlockPointer {
            side: Side::Right,
            fraction: 0.5,
        },
    ];
    expected.extend(granted_actions(0.25, FIRST_SESSION_ID, Side::Right));
    assert_eq!(actions, expected);
    assert!(session.is_controlled());
}

#[test]
fn rule_14_controlled_return_edge_is_ignored_during_grace() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let actions = session.handle(
        entered(Side::Right, 0.6),
        now + ARRIVAL_GRACE - Duration::from_millis(1),
    );
    assert!(actions.is_empty());
    assert!(session.is_controlled());
}

#[test]
fn rule_14_controlled_return_edge_after_grace_releases_control() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let actions = session.handle(entered(Side::Right, 0.6), now + ARRIVAL_GRACE);
    assert_eq!(
        actions,
        vec![
            SessionAction::SendReleaseControl {
                peer: REMOTE,
                fraction: Some(0.6),
                reason: ReleaseReason::EdgeCrossed
            },
            SessionAction::ReleaseAllPressed
        ]
    );
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_14_controlled_other_edges_are_ignored() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let actions = session.handle(entered(Side::Top, 0.6), now + ARRIVAL_GRACE);
    assert!(actions.is_empty());
    assert!(session.is_controlled());
}

#[test]
fn rule_15_controlled_peer_released_returns_to_idle() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let actions = session.handle(
        SessionEvent::PeerReleased {
            peer: REMOTE,
            fraction: None,
        },
        now,
    );
    assert_eq!(actions, vec![SessionAction::ReleaseAllPressed]);
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_15_controlled_peer_disconnected_returns_to_idle() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let actions = session.handle(SessionEvent::PeerDisconnected { peer: REMOTE }, now);
    assert_eq!(actions, vec![SessionAction::ReleaseAllPressed]);
    assert_eq!(session.state(), &SessionState::Idle);
}

#[test]
fn rule_16_disabled_from_every_state() {
    let now = Instant::now();

    let mut idle = session();
    assert!(idle.handle(SessionEvent::Disabled, now).is_empty());
    assert_eq!(idle.state(), &SessionState::Idle);

    let mut pushing = pushing_session(now);
    assert_eq!(
        pushing.handle(SessionEvent::Disabled, now),
        vec![
            SessionAction::HideProgress,
            SessionAction::UnlockPointer {
                side: Side::Right,
                fraction: 0.5
            }
        ]
    );
    assert_eq!(pushing.state(), &SessionState::Idle);

    let mut requesting = requesting_session(now);
    assert_eq!(
        requesting.handle(SessionEvent::Disabled, now),
        vec![
            SessionAction::SendReleaseControl {
                peer: REMOTE,
                fraction: None,
                reason: ReleaseReason::Disabled
            },
            SessionAction::StopGrab {
                side: Side::Right,
                fraction: Some(0.5)
            },
            SessionAction::ReleaseAllPressed,
        ]
    );
    assert_eq!(requesting.state(), &SessionState::Idle);

    let mut controlling = controlling_session(now);
    assert_eq!(
        controlling.handle(SessionEvent::Disabled, now),
        vec![
            SessionAction::SendReleaseControl {
                peer: REMOTE,
                fraction: None,
                reason: ReleaseReason::Disabled
            },
            SessionAction::StopGrab {
                side: Side::Right,
                fraction: None
            },
            SessionAction::ReleaseAllPressed
        ]
    );
    assert_eq!(controlling.state(), &SessionState::Idle);

    let mut controlled = controlled_session(now);
    assert_eq!(
        controlled.handle(SessionEvent::Disabled, now),
        vec![
            SessionAction::SendReleaseControl {
                peer: REMOTE,
                fraction: None,
                reason: ReleaseReason::Disabled
            },
            SessionAction::ReleaseAllPressed
        ]
    );
    assert_eq!(controlled.state(), &SessionState::Idle);
}

#[test]
fn rule_17_unlisted_events_are_ignored() {
    let now = Instant::now();

    let mut idle = session();
    assert!(idle.handle(SessionEvent::HotkeyPressed, now).is_empty());
    assert!(idle.handle(motion(50.0, 0.0), now).is_empty());
    assert!(idle.handle(SessionEvent::Tick, now).is_empty());
    assert_eq!(idle.state(), &SessionState::Idle);

    let mut pushing = pushing_session(now);
    assert!(pushing.handle(SessionEvent::HotkeyPressed, now).is_empty());
    assert!(pushing.handle(SessionEvent::Tick, now).is_empty());
    assert!(matches!(pushing.state(), SessionState::Pushing { .. }));

    let mut controlled = controlled_session(now);
    assert!(
        controlled
            .handle(SessionEvent::HotkeyPressed, now)
            .is_empty()
    );
    assert!(
        controlled
            .handle(SessionEvent::EdgeLeft { side: Side::Right }, now)
            .is_empty()
    );
    assert!(controlled.is_controlled());
}

#[test]
fn reentry_on_the_parked_side_is_ignored_within_the_grace() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    session.handle(
        SessionEvent::PeerReleased {
            peer: REMOTE,
            fraction: Some(0.4),
        },
        now,
    );
    assert_eq!(session.state(), &SessionState::Idle);
    let ignored = session.handle(entered(Side::Right, 0.4), now + Duration::from_millis(100));
    assert!(ignored.is_empty());
    assert_eq!(session.state(), &SessionState::Idle);
    let accepted = session.handle(entered(Side::Right, 0.4), now + REENTRY_GRACE);
    assert!(!accepted.is_empty());
    assert!(matches!(session.state(), SessionState::Pushing { .. }));
}

#[test]
fn reentry_on_another_side_is_not_blocked() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    session.handle(
        SessionEvent::PeerReleased {
            peer: REMOTE,
            fraction: Some(0.4),
        },
        now,
    );
    let accepted = session.handle(entered(Side::Left, 0.4), now + Duration::from_millis(10));
    assert!(!accepted.is_empty());
}

#[test]
fn controlled_return_parks_the_return_side() {
    let now = Instant::now();
    let mut session = controlled_session(now);
    let after_grace = now + ARRIVAL_GRACE;
    let released = session.handle(entered(Side::Right, 0.7), after_grace);
    assert_eq!(session.state(), &SessionState::Idle);
    assert!(!released.is_empty());
    let ignored = session.handle(
        entered(Side::Right, 0.7),
        after_grace + Duration::from_millis(50),
    );
    assert!(ignored.is_empty());
}

#[test]
fn hotkey_release_does_not_park() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    session.handle(SessionEvent::HotkeyPressed, now);
    assert_eq!(session.state(), &SessionState::Idle);
    let accepted = session.handle(entered(Side::Right, 0.4), now + Duration::from_millis(10));
    assert!(!accepted.is_empty());
}

#[test]
fn leaving_the_parked_side_clears_the_block_before_the_grace() {
    let now = Instant::now();
    let mut session = controlling_session(now);
    session.handle(
        SessionEvent::PeerReleased {
            peer: REMOTE,
            fraction: Some(0.4),
        },
        now,
    );
    session.handle(
        SessionEvent::EdgeLeft { side: Side::Right },
        now + Duration::from_millis(50),
    );
    let accepted = session.handle(entered(Side::Right, 0.4), now + Duration::from_millis(60));
    assert!(!accepted.is_empty());
    assert!(matches!(session.state(), SessionState::Pushing { .. }));
}

#[test]
fn drag_crossed_requests_control_immediately_from_idle() {
    let now = Instant::now();
    let mut session = session();
    let actions = session.handle(
        SessionEvent::DragCrossed {
            side: Side::Right,
            fraction: 0.5,
            peer: REMOTE,
        },
        now,
    );
    assert!(matches!(session.state(), SessionState::Requesting { .. }));
    assert!(actions.contains(&SessionAction::StartGrab));
    assert!(actions.contains(&SessionAction::SendRequestControl {
        peer: REMOTE,
        side: Side::Right,
        fraction: 0.5,
    }));
}
