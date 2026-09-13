#![allow(clippy::unwrap_used)]
mod recovery;
mod socket;
use super::*;
use crate::daemon::engine::links::PeerLink;
use crate::daemon::engine::monitor::*;
use crate::daemon::engine::*;
use crate::daemon::net::tcp::TcpCommand;
use opendesk_proto::control::{ControlMessage, OutputGeometry, PeerId, ReleaseReason};
use opendesk_wayland::{WaylandCommand, WaylandEvent, WaylandHandle};

const PEER: PeerId = PeerId([2; 16]);

struct Fixture {
    engine: Engine,
    tcp: tokio::sync::mpsc::UnboundedReceiver<TcpCommand>,
    commands: std::sync::mpsc::Receiver<WaylandCommand>,
}

impl Fixture {
    fn new() -> Self {
        let (wayland, commands) = WaylandHandle::test_harness();
        let (tcp, received) = tokio::sync::mpsc::unbounded_channel();
        let (udp, _) = tokio::sync::mpsc::unbounded_channel();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let mut config = Config::default();
        config.general.dnd_dir = std::env::temp_dir().join(format!(
            "opendesk-lock-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut engine = Engine::new(
            LocalIdentity {
                peer_id: PeerId([1; 16]),
                name: "test".into(),
                port: 0,
                version: "test".into(),
            },
            EnginePaths {
                config: PathBuf::new(),
                peers: PathBuf::new(),
            },
            config,
            PeerStore::default(),
            EngineSinks { wayland, tcp, udp },
        );
        engine.links.insert_link(
            PEER,
            PeerLink::new(
                ConnectionId(1),
                "127.0.0.1:47820".parse().unwrap(),
                47820,
                "peer".into(),
                vec![],
                Instant::now(),
            ),
        );
        engine.outputs = vec![OutputGeometry {
            name: "scaled".into(),
            x: -1000,
            y: -500,
            width: 1000,
            height: 500,
        }];
        engine.reconfigure_strips();
        Self {
            engine,
            tcp: received,
            commands,
        }
    }

    fn sample(&mut self, sample: Sample) {
        let request = Request {
            generation: self.engine.monitor.generation,
            started: Instant::now(),
            query: match sample {
                Sample::Lock(_) => Query::Lock,
                _ => Query::Cursor,
            },
        };
        self.engine
            .on_monitor(MonitorEvent::Sample(request, sample));
    }

    fn receive(&mut self, side: Side, locked: bool) {
        self.sample(Sample::Lock(locked));
        self.engine.on_control_message(
            PEER,
            ControlMessage::RequestControl {
                side: side.opposite(),
                fraction: 0.5,
                drag: None,
            },
        );
        assert!(self.engine.session.is_controlled());
    }

    fn age_arrival(&mut self, side: Side) {
        let mut config = session_config(PeerId([1; 16]), &self.engine.config);
        config.arrival_grace = Duration::ZERO;
        self.engine.session = Session::new(config);
        self.engine.dispatch(SessionEvent::PeerRequestedControl {
            peer: PEER,
            side: side.opposite(),
            fraction: 0.5,
        });
    }

    fn releases(&mut self) -> Vec<ReleaseReason> {
        let mut reasons = vec![];
        while let Ok(command) = self.tcp.try_recv() {
            if let TcpCommand::Send {
                message: ControlMessage::ReleaseControl { reason, .. },
                ..
            } = command
            {
                reasons.push(reason);
            }
        }
        reasons
    }
}

fn point(side: Side) -> (f64, f64, f64, f64) {
    match side {
        Side::Left => (-1000.0, -250.0, -10.0, 0.0),
        Side::Right => (-1.0, -250.0, 10.0, 0.0),
        Side::Top => (-500.0, -500.0, 0.0, -10.0),
        Side::Bottom => (-500.0, -1.0, 0.0, 10.0),
    }
}

#[test]
fn all_edges_require_outward_motion_confirmed_position_and_arrival_grace() {
    for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
        let mut f = Fixture::new();
        f.receive(side, true);
        let (x, y, dx, dy) = point(side);
        assert_eq!(f.engine.edges.cursor_fraction(side, x, y), Some(0.5));
        f.sample(Sample::Cursor { x, y }); // initial position cannot return
        assert!(f.engine.session.is_controlled());
        f.engine.locked_motion(dx, dy);
        f.sample(Sample::Cursor { x, y }); // grace
        assert!(f.engine.session.is_controlled());
        f.age_arrival(side);
        f.engine.locked_motion(-dx, -dy);
        f.sample(Sample::Cursor { x, y }); // inward
        assert!(f.engine.session.is_controlled());
        f.engine.locked_motion(dx, dy);
        let (wrong_x, wrong_y, _, _) = point(side.opposite());
        f.sample(Sample::Cursor {
            x: wrong_x,
            y: wrong_y,
        });
        assert!(f.engine.session.is_controlled());
        f.sample(Sample::Cursor { x, y });
        assert!(!f.engine.session.is_controlled());
        f.sample(Sample::Cursor { x, y });
        f.engine.on_wayland(WaylandEvent::EdgeEntered {
            side,
            position: if side.is_horizontal() { y } else { x },
            output: "scaled".into(),
        });
        assert_eq!(f.releases(), vec![ReleaseReason::EdgeCrossed]);
    }
}

#[test]
fn stale_session_layout_and_pre_motion_samples_cannot_return() {
    let mut f = Fixture::new();
    f.receive(Side::Left, true);
    f.age_arrival(Side::Left);
    let old = Request {
        generation: f.engine.monitor.generation,
        started: Instant::now(),
        query: Query::Cursor,
    };
    f.engine.locked_motion(-10.0, 0.0);
    f.engine.on_monitor(MonitorEvent::Sample(
        old,
        Sample::Cursor {
            x: -1000.0,
            y: -250.0,
        },
    ));
    assert!(f.engine.session.is_controlled());
    f.engine.reconfigure_strips();
    f.engine
        .on_monitor(MonitorEvent::Sample(old, Sample::Lock(false)));
    assert_eq!(f.engine.monitor.lock, LockState::Locked);
    f.engine.dispatch(SessionEvent::Disabled);
    f.receive(Side::Left, true);
    f.age_arrival(Side::Left);
    f.engine.locked_motion(-10.0, 0.0);
    f.engine.on_monitor(MonitorEvent::Sample(
        old,
        Sample::Cursor {
            x: -1000.0,
            y: -250.0,
        },
    ));
    assert!(f.engine.session.is_controlled());
}

#[test]
fn continuous_motion_during_query_and_inward_cancellation() {
    let mut f = Fixture::new();
    f.receive(Side::Left, true);
    f.age_arrival(Side::Left);
    f.engine.locked_motion(-1.0, 0.0);
    let request = Request {
        generation: f.engine.monitor.generation,
        started: Instant::now(),
        query: Query::Cursor,
    };
    f.engine.locked_motion(-1.0, 0.0);
    f.engine.on_monitor(MonitorEvent::Sample(
        request,
        Sample::Cursor {
            x: -1000.0,
            y: -250.0,
        },
    ));
    assert_eq!(f.releases(), vec![ReleaseReason::EdgeCrossed]);
}

#[test]
fn logical_scaled_geometry_last_pixels_fractions_and_invalid_positions() {
    let f = Fixture::new(); // 2000x1000 physical at scale 2 => 1000x500 logical.
    for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
        let (mut x, mut y, _, _) = point(side);
        if side.is_horizontal() {
            y = -375.0;
        } else {
            x = -750.0;
        }
        assert_eq!(f.engine.edges.cursor_fraction(side, x, y), Some(0.25));
        assert_eq!(f.engine.edges.cursor_fraction(side, f64::NAN, y), None);
        assert_eq!(f.engine.edges.cursor_fraction(side, x, f64::INFINITY), None);
    }
    assert_eq!(
        f.engine.edges.cursor_fraction(Side::Right, 0.0, -250.0),
        None
    );
    assert_eq!(
        f.engine.edges.cursor_fraction(Side::Bottom, -500.0, 0.0),
        None
    );
    assert_eq!(
        f.engine.edges.cursor_fraction(Side::Right, 999.0, -250.0),
        None
    );
    assert_eq!(
        f.engine.edges.cursor_fraction(Side::Left, -1000.0, -501.0),
        None
    );
}

#[test]
fn inward_motion_after_query_started_and_expired_outward_intent_cancel_return() {
    let mut f = Fixture::new();
    f.receive(Side::Left, true);
    f.age_arrival(Side::Left);
    f.engine.locked_motion(-1.0, 0.0);
    let request = Request {
        generation: f.engine.monitor.generation,
        started: Instant::now(),
        query: Query::Cursor,
    };
    f.engine.locked_motion(1.0, 0.0);
    f.engine.on_monitor(MonitorEvent::Sample(
        request,
        Sample::Cursor {
            x: -1000.0,
            y: -250.0,
        },
    ));
    assert!(f.engine.session.is_controlled());
    let expired = Instant::now() - MOTION_AGE - Duration::from_millis(1);
    f.engine.monitor.outward = Some((expired, expired));
    f.sample(Sample::Cursor {
        x: -1000.0,
        y: -250.0,
    });
    assert!(f.engine.session.is_controlled());
}
