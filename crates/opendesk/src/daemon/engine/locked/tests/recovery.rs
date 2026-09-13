use super::*;
use opendesk_proto::control::DenyReason;
use opendesk_proto::transfer::DragInfo;

pub(super) fn commands(f: &Fixture) -> Vec<PlatformCommand> {
    f.engine.wayland(PlatformCommand::SetKeymap {
        xkb: "test-barrier".into(),
    });
    let mut result = vec![];
    loop {
        let command = f.commands.recv_timeout(Duration::from_secs(1)).unwrap();
        if matches!(&command, PlatformCommand::SetKeymap { xkb } if xkb == "test-barrier") {
            break;
        }
        result.push(command);
    }
    result
}

#[test]
fn unknown_and_locked_drag_denied_but_keyboard_mouse_work_while_locked() {
    let mut f = Fixture::new();
    let request = ControlMessage::RequestControl {
        side: Side::Right,
        fraction: 0.5,
        drag: None,
    };
    f.engine.on_control_message(PEER, request.clone());
    assert!(!f.engine.session.is_controlled());
    assert!(matches!(
        f.tcp.try_recv().unwrap(),
        TcpCommand::Send {
            message: ControlMessage::ControlDenied {
                reason: DenyReason::Disabled
            },
            ..
        }
    ));
    f.sample(Sample::Lock(true));
    f.engine.on_control_message(
        PEER,
        ControlMessage::RequestControl {
            side: Side::Right,
            fraction: 0.5,
            drag: Some(DragInfo {
                transfer_id: 1,
                entries: vec![],
            }),
        },
    );
    assert!(!f.engine.session.is_controlled());
    f.engine.on_control_message(PEER, request);
    f.engine.on_peer_input(
        PEER,
        ControlMessage::Key {
            code: 30,
            pressed: true,
        },
    );
    f.engine.on_peer_input(
        PEER,
        ControlMessage::Button {
            code: 272,
            pressed: true,
        },
    );
    f.engine.on_peer_input(
        PEER,
        ControlMessage::Modifiers {
            depressed: 4,
            latched: 8,
            locked: 2,
            group: 1,
        },
    );
    assert!(!f.engine.injected.is_empty());
    let observed = commands(&f);
    assert!(observed.iter().any(|c| matches!(
        c,
        PlatformCommand::InjectKey {
            code: 30,
            pressed: true
        }
    )));
    assert!(observed.iter().any(|c| matches!(
        c,
        PlatformCommand::InjectButton {
            code: 272,
            pressed: true
        }
    )));
    f.engine.dispatch(SessionEvent::Disabled);
    assert!(f.engine.injected.is_empty());
    let observed = commands(&f);
    assert!(observed.iter().any(|c| matches!(
        c,
        PlatformCommand::InjectModifiers {
            depressed: 0,
            latched: 0,
            locked: 2,
            group: 1
        }
    )));
    assert!(observed.iter().any(|c| matches!(
        c,
        PlatformCommand::InjectKey {
            code: 30,
            pressed: false
        }
    )));
    assert!(observed.iter().any(|c| matches!(
        c,
        PlatformCommand::InjectButton {
            code: 272,
            pressed: false
        }
    )));
}

#[test]
fn locking_source_stops_grab_and_releases_forwarded_inputs() {
    for grant in [false, true] {
        let mut f = Fixture::new();
        f.sample(Sample::Lock(false));
        f.engine.dispatch(SessionEvent::DragCrossed {
            peer: PEER,
            side: Side::Left,
            fraction: 0.5,
        });
        if grant {
            f.engine.dispatch(SessionEvent::PeerGranted {
                peer: PEER,
                session_id: 2,
            });
        }
        f.engine.forwarded.record_key(30, true);
        f.engine.forwarded.record_button(272, true);
        f.sample(Sample::Lock(true));
        assert!(matches!(f.engine.session.state(), SessionState::Idle));
        assert!(f.engine.forwarded.is_empty());
        assert!(
            commands(&f)
                .iter()
                .any(|c| matches!(c, PlatformCommand::StopGrab { .. }))
        );
        let mut releases = vec![];
        let mut key_up = false;
        let mut button_up = false;
        while let Ok(TcpCommand::Send { message, .. }) = f.tcp.try_recv() {
            match message {
                ControlMessage::ReleaseControl { reason, .. } => releases.push(reason),
                ControlMessage::Key {
                    code: 30,
                    pressed: false,
                } => key_up = true,
                ControlMessage::Button {
                    code: 272,
                    pressed: false,
                } => button_up = true,
                _ => {}
            }
        }
        assert_eq!(releases, vec![ReleaseReason::Disabled]);
        assert!(key_up && button_up);
    }
}

#[test]
fn unlock_keeps_session_and_manual_pause_is_preserved() {
    let mut f = Fixture::new();
    f.receive(Side::Left, true);
    f.age_arrival(Side::Left);
    f.sample(Sample::Lock(false));
    assert!(f.engine.session.is_controlled());
    f.engine.on_wayland(PlatformEvent::EdgeEntered {
        side: Side::Left,
        position: -250.0,
        output: "scaled".into(),
    });
    assert_eq!(f.releases(), vec![ReleaseReason::EdgeCrossed]);
    let (reply, _) = tokio::sync::oneshot::channel();
    f.engine.on_ipc(IpcRequest::Disable, reply);
    f.sample(Sample::Lock(true));
    f.sample(Sample::Lock(false));
    assert!(!f.engine.enabled);
    f.engine.on_control_message(
        PEER,
        ControlMessage::RequestControl {
            side: Side::Left,
            fraction: 0.5,
            drag: None,
        },
    );
    assert!(matches!(f.engine.session.state(), SessionState::Idle));
}

#[test]
fn one_second_without_lock_or_cursor_recovers_and_cancels_drag() {
    for cursor in [false, true] {
        let mut f = Fixture::new();
        f.receive(Side::Left, true);
        f.engine.injected.record_key(30, true);
        f.engine.injected.record_button(272, true);
        let now = Instant::now();
        if cursor {
            f.engine.monitor.valid_cursor = now - MAX_AGE;
        } else {
            f.engine.monitor.valid_lock = Some(now - MAX_AGE);
        }
        let (requests, _receiver) = mpsc::channel(1);
        f.engine.poll_compositor(now, &requests);
        assert_eq!(f.releases(), vec![ReleaseReason::Disabled]);
        assert!(f.engine.injected.is_empty());
        assert_eq!(f.engine.monitor.lock, LockState::Unknown);
        f.sample(Sample::Lock(false));
        assert!(f.engine.monitor.known(Instant::now()));
    }
    let mut f = Fixture::new();
    f.receive(Side::Left, false);
    f.engine.incoming_drag = Some((PEER, 17, false));
    f.engine.active_drop = Some(17);
    f.engine.active_drop_peer = Some(PEER);
    f.engine.active_drop_id = Some(2);
    f.sample(Sample::Lock(true));
    assert!(f.engine.incoming_drag.is_none());
    assert!(f.engine.active_drop.is_none());
    assert!(
        commands(&f)
            .iter()
            .any(|c| matches!(c, PlatformCommand::CancelDropDrag))
    );
    assert_eq!(f.releases(), vec![ReleaseReason::Disabled]);
}

#[test]
fn polling_cadence_and_single_pending_query() {
    let mut f = Fixture::new();
    f.sample(Sample::Lock(false));
    let (requests, mut received) = mpsc::channel(1);
    let now = Instant::now();
    f.engine.poll_compositor(now, &requests);
    assert_eq!(received.try_recv().unwrap().query, Query::Lock);
    f.engine
        .poll_compositor(now + Duration::from_millis(500), &requests);
    assert!(received.try_recv().is_err()); // Only one query in flight.
    f.sample(Sample::Lock(false));
    f.engine
        .poll_compositor(now + Duration::from_millis(499), &requests);
    assert!(received.try_recv().is_err());
    f.engine
        .poll_compositor(now + Duration::from_millis(500), &requests);
    assert_eq!(received.try_recv().unwrap().query, Query::Lock);
}

#[test]
fn active_lock_and_cursor_poll_at_100_and_50_ms() {
    let mut f = Fixture::new();
    f.receive(Side::Left, true);
    let (requests, mut received) = mpsc::channel(1);
    let now = Instant::now();
    f.engine.poll_compositor(now, &requests);
    assert_eq!(received.try_recv().unwrap().query, Query::Lock);
    f.sample(Sample::Lock(true));
    f.engine
        .poll_compositor(now + Duration::from_millis(25), &requests);
    assert_eq!(received.try_recv().unwrap().query, Query::Cursor);
    f.sample(Sample::Cursor {
        x: -500.0,
        y: -250.0,
    });
    f.engine
        .poll_compositor(now + Duration::from_millis(74), &requests);
    assert!(received.try_recv().is_err());
    f.engine
        .poll_compositor(now + Duration::from_millis(75), &requests);
    assert_eq!(received.try_recv().unwrap().query, Query::Cursor);
    f.sample(Sample::Cursor {
        x: -500.0,
        y: -250.0,
    });
    f.engine
        .poll_compositor(now + Duration::from_millis(100), &requests);
    assert_eq!(received.try_recv().unwrap().query, Query::Lock);
}
