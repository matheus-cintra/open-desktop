#![allow(clippy::unwrap_used)]
use super::*;
use crate::daemon::{
    engine::{
        EnginePaths, EngineSinks,
        links::PeerLink,
        monitor::{MonitorEvent, Query, Request, Sample},
    },
    net::{
        discovery::LocalIdentity,
        tcp::{ConnectionId, TcpCommand},
        udp::{UdpCommand, UdpEvent},
    },
};
use crate::platform::{PlatformEvent, PlatformHandle};
use opendesk_core::{
    config::Config,
    peers::{PeerRecord, PeerStore},
};
use opendesk_proto::control::{OutputGeometry, Token};

struct Node {
    engine: Engine,
    tcp: tokio::sync::mpsc::UnboundedReceiver<TcpCommand>,
    udp: tokio::sync::mpsc::UnboundedReceiver<UdpCommand>,
    commands: std::sync::mpsc::Receiver<PlatformCommand>,
}
fn id(i: usize) -> PeerId {
    PeerId([i as u8; 16])
}
fn map() -> DesktopMap {
    DesktopMap {
        group: id(0),
        revision: Revision {
            counter: 1,
            author: id(0),
        },
        screens: vec![
            Screen {
                peer: id(0),
                name: "A".into(),
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            Screen {
                peer: id(1),
                name: "B".into(),
                x: 100,
                y: 0,
                width: 100,
                height: 100,
            },
            Screen {
                peer: id(2),
                name: "C".into(),
                x: 0,
                y: 100,
                width: 200,
                height: 100,
            },
        ],
    }
}
fn nodes() -> Vec<Node> {
    (0..3)
        .map(|i| {
            let (wayland, commands) = PlatformHandle::test_harness();
            let (tx, tcp) = tokio::sync::mpsc::unbounded_channel();
            let (utx, udp) = tokio::sync::mpsc::unbounded_channel();
            let mut peers = PeerStore::default();
            for j in 0..3 {
                if i != j {
                    peers.upsert(PeerRecord {
                        id: id(j),
                        name: format!("{j}"),
                        token: Token([1; 32]),
                    });
                }
            }
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "opendesk-map-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let mut config = Config::default();
            config.general.dnd_dir = path.join("dnd");
            let mut engine = Engine::new(
                LocalIdentity {
                    peer_id: id(i),
                    name: format!("{i}"),
                    port: 0,
                    version: "test".into(),
                },
                EnginePaths {
                    config: path.join("config.toml"),
                    peers: path.join("peers.toml"),
                },
                config,
                peers,
                EngineSinks {
                    wayland,
                    tcp: tx,
                    udp: utx,
                },
            );
            engine.map.current = Some(map());
            engine.outputs = vec![OutputGeometry {
                name: "display".into(),
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            }];
            engine.reconfigure_strips();
            engine.on_monitor(MonitorEvent::Sample(
                Request {
                    generation: engine.monitor.generation,
                    started: Instant::now(),
                    query: Query::Lock,
                },
                Sample::Lock(false),
            ));
            for j in 0..3 {
                if i != j {
                    let mut link = PeerLink::new(
                        ConnectionId(j as u64),
                        format!("127.0.0.{}:47820", j + 1).parse().unwrap(),
                        47820,
                        format!("{j}"),
                        vec![],
                        Instant::now(),
                    );
                    link.map_ready = true;
                    engine.links.insert_link(id(j), link);
                    engine.map.synced.insert(id(j));
                }
            }
            Node {
                engine,
                tcp,
                udp,
                commands,
            }
        })
        .collect()
}
impl Drop for Node {
    fn drop(&mut self) {
        if let Some(path) = self.engine.paths.config.parent() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
fn commands(node: &Node) -> Vec<PlatformCommand> {
    node.engine.wayland(PlatformCommand::SetKeymap {
        xkb: "barrier".into(),
    });
    let mut result = Vec::new();
    loop {
        let command = node.commands.recv_timeout(Duration::from_secs(1)).unwrap();
        if matches!(&command,PlatformCommand::SetKeymap {xkb} if xkb=="barrier") {
            return result;
        }
        result.push(command);
    }
}
fn pump(nodes: &mut [Node]) {
    for _ in 0..50 {
        let mut pending = Vec::new();
        for (i, node) in nodes.iter_mut().enumerate() {
            while let Ok(command) = node.tcp.try_recv() {
                if let TcpCommand::Send {
                    connection,
                    message,
                } = command
                {
                    pending.push((i, connection.0 as usize, message));
                }
            }
        }
        if pending.is_empty() {
            return;
        }
        for (from, to, message) in pending {
            nodes[to].engine.on_control_message(id(from), message);
        }
    }
    unreachable!("message loop");
}
fn start(nodes: &mut [Node], origin: usize, target: usize) {
    nodes[origin]
        .engine
        .begin_map_transfer(id(target), Side::Right, 0.5, None);
    pump(nodes);
    assert!(nodes[origin].engine.session.is_controlling());
    assert!(nodes[target].engine.session.is_controlled());
}
fn transfer(nodes: &mut [Node], origin: usize, target: usize) {
    let old = nodes[origin].engine.active_peer;
    nodes[origin]
        .engine
        .begin_map_transfer(id(target), Side::Left, 0.5, old);
    pump(nodes);
}

#[test]
fn handoff_before_first_udp_uses_map_lease_instead_of_previous_link_activity() {
    let mut n = nodes();
    start(&mut n, 2, 1);
    n[0].engine.links.link_mut(id(2)).unwrap().last_udp = Instant::now() - Duration::from_secs(10);
    transfer(&mut n, 2, 0);
    let epoch = n[0].engine.map.epoch.unwrap();
    // The event-loop tick can run between TCP commit and the first UDP packet.
    n[0].engine.on_tick(Instant::now());
    pump(&mut n);
    assert!(n[0].engine.session.is_controlled());
    assert!(n[2].engine.session.is_controlling());
    assert_eq!(n[0].engine.map.epoch, Some(epoch));
    // A real loss of renewal still releases both ends within the map deadline.
    n[0].engine.map.last_renew = Instant::now() - Duration::from_secs(1);
    n[0].engine.on_tick(Instant::now());
    pump(&mut n);
    assert!(!n[0].engine.session.is_controlled());
    assert!(!n[2].engine.session.is_controlling());
    assert!(n[0].engine.map.epoch.is_none());
}

#[test]
fn every_origin_controls_three_targets_directly_and_returns_without_ungrabbing_between_targets() {
    for origin in 0..3 {
        let mut n = nodes();
        let b = (origin + 1) % 3;
        let c = (origin + 2) % 3;
        start(&mut n, origin, b);
        let old = n[origin].engine.map.epoch.unwrap();
        commands(&n[origin]);
        transfer(&mut n, origin, c);
        assert_eq!(n[origin].engine.active_peer, Some(id(c)));
        assert!(!n[b].engine.session.is_controlled());
        assert!(n[c].engine.session.is_controlled());
        assert!(
            !commands(&n[origin])
                .iter()
                .any(|c| matches!(c, PlatformCommand::StopGrab { .. }))
        );
        n[origin]
            .engine
            .forward_wayland_input(PlatformEvent::RelativeMotion { dx: 5.0, dy: 0.0 });
        let UdpCommand::Send { to, datagram } = n[origin].udp.try_recv().unwrap();
        assert_eq!(
            to.ip(),
            format!("127.0.0.{}", c + 1)
                .parse::<std::net::IpAddr>()
                .unwrap()
        );
        assert!(n[b].udp.try_recv().is_err());
        assert_eq!(
            datagram.header.epoch.unwrap().generation,
            old.generation + 1
        );
        transfer(&mut n, origin, origin);
        assert!(matches!(
            n[origin].engine.session.state(),
            opendesk_core::session::SessionState::Idle
        ));
        assert!(!n[c].engine.session.is_controlled());
    }
}
#[test]
fn stale_inputs_prepares_and_releases_cannot_reactivate_old_generation() {
    let mut n = nodes();
    start(&mut n, 0, 1);
    let old = n[0].engine.map.epoch.unwrap();
    transfer(&mut n, 0, 2);
    commands(&n[1]);
    n[1].engine.on_map_message(
        id(0),
        MapControl::Input {
            epoch: old,
            message: Box::new(ControlMessage::Key {
                code: 30,
                pressed: true,
            }),
        },
    );
    n[1].engine.on_map_message(
        id(0),
        MapControl::Prepare {
            drag: None,
            epoch: old,
            revision: map().revision,
            side: Side::Right,
            fraction: 0.5,
        },
    );
    assert!(n[1].engine.map.prepared.is_none());
    assert!(commands(&n[1]).is_empty());
    commands(&n[2]);
    n[2].engine.on_udp(UdpEvent::Datagram {
        from: "127.0.0.1:47820".parse().unwrap(),
        datagram: opendesk_proto::input::InputDatagram {
            header: opendesk_proto::input::InputHeader {
                epoch: Some(old),
                session_id: old.generation as u32,
                sequence: 99,
            },
            event: opendesk_proto::input::InputEvent::Motion { dx: 500.0, dy: 0.0 },
        },
    });
    assert!(commands(&n[2]).is_empty());
    n[2].engine
        .on_map_message(id(0), MapControl::Release { epoch: old });
    assert!(n[2].engine.session.is_controlled());
}
#[test]
fn modifiers_remap_and_common_keys_or_buttons_block_transfer() {
    let mut n = nodes();
    start(&mut n, 0, 1);
    n[0].engine.forward_wayland_input(PlatformEvent::Key {
        code: 30,
        pressed: true,
    });
    pump(&mut n);
    transfer(&mut n, 0, 2);
    assert_eq!(n[0].engine.active_peer, Some(id(1)));
    n[0].engine.forward_wayland_input(PlatformEvent::Key {
        code: 30,
        pressed: false,
    });
    pump(&mut n);
    n[0].engine.forward_wayland_input(PlatformEvent::Button {
        code: 273,
        pressed: true,
    });
    pump(&mut n);
    transfer(&mut n, 0, 2);
    assert_eq!(n[0].engine.active_peer, Some(id(1)));
    n[0].engine.forward_wayland_input(PlatformEvent::Button {
        code: 273,
        pressed: false,
    });
    pump(&mut n);
    n[0].engine.set_capabilities(id(2), true, false);
    n[2].engine.set_capabilities(id(0), true, false);
    n[0].engine.forward_wayland_input(PlatformEvent::Key {
        code: 29,
        pressed: true,
    });
    pump(&mut n);
    commands(&n[2]);
    transfer(&mut n, 0, 2);
    let commands: Vec<_> = commands(&n[2]);
    assert!(
        commands.iter().any(|c| matches!(
            c,
            PlatformCommand::InjectPhysicalKey {
                code: 125,
                pressed: true
            }
        )),
        "commands={commands:?}, source={:?}, forwarded={:?}",
        n[0].engine.session.state(),
        n[0].engine.forwarded
    );
    assert!(n[1].engine.injected.is_empty());
}
#[test]
fn expiry_refusal_disconnect_and_physical_takeover_release_all_inputs() {
    let mut n = nodes();
    start(&mut n, 0, 1);
    n[0].engine.forward_wayland_input(PlatformEvent::Key {
        code: 30,
        pressed: true,
    });
    pump(&mut n);
    let now = Instant::now() + Duration::from_secs(1);
    n[1].engine.tick_map(now);
    pump(&mut n);
    assert!(n[1].engine.injected.is_empty());
    n[0].engine.tick_map(now);
    pump(&mut n);
    assert!(!n[0].engine.session.is_controlling());
    let mut n = nodes();
    n[1].engine.enabled = false;
    n[0].engine
        .begin_map_transfer(id(1), Side::Right, 0.5, None);
    pump(&mut n);
    assert!(n[0].engine.map.pending.is_none());
    let mut n = nodes();
    start(&mut n, 0, 1);
    n[2].engine.claim_physical();
    pump(&mut n);
    assert!(!n[0].engine.session.is_controlling());
    assert!(!n[1].engine.session.is_controlled());
}
#[test]
fn simultaneous_claims_converge_and_map_edits_require_current_revision() {
    let mut n = nodes();
    for node in &mut n {
        node.engine.map.claim = Revision {
            counter: 3,
            author: id(0),
        };
    }
    n[1].engine.claim_physical();
    n[2].engine.claim_physical();
    pump(&mut n);
    assert!(n.iter().all(|node| node.engine.map.claim
        == Revision {
            counter: 4,
            author: id(2)
        }));
    let mut draft = map();
    draft.screens[0].name = "new".into();
    assert!(matches!(
        n[0].engine.apply_map(Some(map().revision), draft.clone()),
        IpcResponse::Map(_)
    ));
    pump(&mut n);
    assert!(
        n.iter()
            .all(|node| node.engine.map.current.as_ref().unwrap().screens[0].name == "new")
    );
    assert!(matches!(
        n[1].engine.apply_map(Some(map().revision), draft),
        IpcResponse::MapConflict(_)
    ));
    let path = n[1].engine.paths.config.with_file_name("map.json");
    assert_eq!(desktop_map::load(&path).unwrap(), n[1].engine.map.current);
}

#[test]
fn restart_and_reconnection_never_reuse_an_old_control_counter() {
    let mut n = nodes();
    assert!(n[0].engine.observe_control_clock(500));
    let path = n[0].engine.paths.config.with_file_name("map.json");
    n[0].engine.map = MapState::new(&path, id(0));
    assert_eq!(n[0].engine.map.clock, 500);
    assert!(n[0].engine.map.epoch.is_none());
    n[0].engine.map.current = Some(map());
    n[0].engine.map.synced.extend([id(1), id(2)]);
    n[0].engine
        .on_map_message(id(1), MapControl::Clock { counter: 800 });
    start(&mut n, 0, 1);
    assert_eq!(n[0].engine.map.claim.counter, 801);
    assert_eq!(
        desktop_map::load_control_clock(&path.with_file_name("control-clock.json")).unwrap(),
        801
    );
}

#[test]
fn destination_rechecks_permission_at_commit_and_expired_renewals_do_not_revive_control() {
    let mut n = nodes();
    let claim = Revision {
        counter: 1,
        author: id(0),
    };
    let epoch = ControlEpoch {
        claim,
        session: 1,
        generation: 1,
    };
    n[1].engine
        .on_map_message(id(0), MapControl::Claim { revision: claim });
    n[1].engine.on_map_message(
        id(0),
        MapControl::Prepare {
            epoch,
            revision: map().revision,
            drag: None,
            side: Side::Right,
            fraction: 0.5,
        },
    );
    assert!(n[1].engine.map.prepared.is_some());
    n[1].engine.enabled = false;
    n[1].engine
        .on_map_message(id(0), MapControl::Commit { epoch });
    assert!(n[1].engine.map.epoch.is_none());
    let mut n = nodes();
    start(&mut n, 0, 1);
    let epoch = n[1].engine.map.epoch.unwrap();
    n[1].engine.map.last_renew = Instant::now() - Duration::from_secs(1);
    n[1].engine
        .on_map_message(id(0), MapControl::Renew { epoch });
    assert!(n[1].engine.map.last_renew.elapsed() >= Duration::from_secs(1));
    n[1].engine.tick_map(Instant::now());
    assert!(!n[1].engine.session.is_controlled());
}

#[tokio::test]
async fn bilateral_file_drag_is_authorized_only_after_commit_and_cannot_cross_a_third_machine() {
    let mut n = nodes();
    n[0].engine.set_capabilities(id(1), false, true);
    n[1].engine.set_capabilities(id(0), false, true);
    let path = n[0].engine.paths.config.with_file_name("file.txt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"drag test").unwrap();
    n[0].engine.pending_transfer =
        Some(crate::daemon::transfer::plan_transfer(&[path], 42).unwrap());
    n[0].engine.outgoing_drag = Some((id(1), 42, Instant::now()));
    start(&mut n, 0, 1);
    assert_eq!(n[1].engine.incoming_drag, Some((id(0), 42, false)));
    transfer(&mut n, 0, 2);
    assert_eq!(n[0].engine.active_peer, Some(id(1)));
    let epoch = n[0].engine.map.epoch.unwrap();
    n[0].engine.on_map_message(
        id(1),
        MapControl::ReturnDrag {
            epoch,
            side: Side::Left,
            fraction: 0.5,
            drag: opendesk_proto::transfer::DragInfo {
                transfer_id: 43,
                entries: vec![],
            },
        },
    );
    pump(&mut n);
    assert!(matches!(
        n[0].engine.session.state(),
        opendesk_core::session::SessionState::Idle
    ));
    assert!(
        n[0].engine
            .authorized_return_drop
            .as_ref()
            .is_some_and(|a| a.released && a.transfer_id == 43)
    );
}

#[test]
fn concurrent_map_edits_converge_and_preserve_the_losing_revision() {
    let mut n = nodes();
    let mut a = map();
    a.screens[0].name = "A edit".into();
    let mut b = map();
    b.screens[0].name = "B edit".into();
    assert!(matches!(
        n[0].engine.apply_map(Some(map().revision), a),
        IpcResponse::Map(_)
    ));
    assert!(matches!(
        n[1].engine.apply_map(Some(map().revision), b),
        IpcResponse::Map(_)
    ));
    pump(&mut n);
    assert!(n.iter().all(|node| {
        node.engine
            .map
            .current
            .as_ref()
            .is_some_and(|map| map.screens[0].name == "B edit")
    }));
    let directory = n[0].engine.paths.config.parent().unwrap();
    assert!(
        directory
            .join(format!("map-conflict-2-{}.json", id(0)))
            .exists()
    );
}

#[test]
fn protocol_three_is_explicitly_rejected_before_establishing_a_link() {
    use crate::daemon::net::tcp::TcpEvent;
    use opendesk_proto::control::RejectReason;
    let mut n = nodes();
    let connection = ConnectionId(99);
    n[0].engine.on_tcp(TcpEvent::Accepted {
        connection,
        address: "127.0.0.2:51000".parse().unwrap(),
    });
    n[0].engine.on_tcp(TcpEvent::Message {
        connection,
        message: ControlMessage::Hello {
            protocol_version: 3,
            peer_id: id(1),
            name: "old peer".into(),
            token: Some(Token([1; 32])),
            udp_port: 47820,
            layout: vec![],
        },
    });
    assert!(
        std::iter::from_fn(|| n[0].tcp.try_recv().ok()).any(|command| matches!(
            command,
            TcpCommand::Send {
                connection: ConnectionId(99),
                message: ControlMessage::HelloRejected {
                    reason: RejectReason::ProtocolVersion {
                        expected: 4,
                        received: 3
                    }
                }
            }
        ))
    );
}
