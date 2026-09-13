mod bootstrap;
mod clipboard;
mod control;
mod drag;
mod edges;
mod events;
mod forward;
mod forward_drag;
mod handshake;
mod ipc;
mod links;
mod locked;
mod monitor;
mod pairing;
mod return_drag;
mod run;
mod tick;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use opendesk_core::config::Config;
use opendesk_core::peers::PeerStore;
use opendesk_core::pressed::PressedInputs;
use opendesk_core::session::{Session, SessionConfig, SessionEvent, SessionState};
use opendesk_proto::control::{ControlMessage, OutputGeometry, PeerId};
use opendesk_wayland::{StripSpec, WaylandCommand, WaylandEvent, WaylandHandle};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tracing::{error, warn};

use crate::daemon::config_watch::ConfigChanged;
use crate::daemon::ipc::{IpcRequest, IpcResponse};
use crate::daemon::net::discovery::{DiscoveredPeer, DiscoveryEvent, LocalIdentity};
use crate::daemon::net::tcp::{ConnectionId, TcpCommand, TcpEvent};
use crate::daemon::net::udp::{UdpCommand, UdpEvent};
use edges::EdgeMap;
use links::LinkTable;
use pairing::Pairing;

pub use bootstrap::run_daemon;
use bootstrap::{CompositorEvent, bar_style, hotkey_spec};

const TICK: Duration = Duration::from_millis(100);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const ARRIVAL_GRACE: Duration = Duration::from_millis(300);
const REENTRY_GRACE: Duration = Duration::from_millis(800);

pub struct EngineChannels {
    pub wayland_events: UnboundedReceiver<WaylandEvent>,
    pub tcp_events: UnboundedReceiver<TcpEvent>,
    pub udp_events: UnboundedReceiver<UdpEvent>,
    pub discovery_events: UnboundedReceiver<DiscoveryEvent>,
    pub ipc_requests: Receiver<(IpcRequest, oneshot::Sender<IpcResponse>)>,
    pub config_events: UnboundedReceiver<ConfigChanged>,
    pub compositor_events: UnboundedReceiver<CompositorEvent>,
    pub shutdown: oneshot::Receiver<()>,
}

pub struct EngineSinks {
    pub wayland: WaylandHandle,
    pub tcp: UnboundedSender<TcpCommand>,
    pub udp: UnboundedSender<UdpCommand>,
}

pub struct EnginePaths {
    pub config: PathBuf,
    pub peers: PathBuf,
}

pub struct Engine {
    identity: LocalIdentity,
    paths: EnginePaths,
    config: Config,
    peer_store: PeerStore,
    session: Session,
    active_peer: Option<PeerId>,
    forwarded: PressedInputs,
    forwarded_left_pressed: bool,
    outgoing_drag: Option<(PeerId, u64, Instant)>,
    incoming_drag: Option<(PeerId, u64, bool)>,
    injected: PressedInputs,
    injected_locks: (u32, u32),
    links: LinkTable,
    discovered: HashMap<PeerId, DiscoveredPeer>,
    outputs: Vec<OutputGeometry>,
    edges: EdgeMap,
    applied_strips: Vec<StripSpec>,
    local_keymap: Option<String>,
    enabled: bool,
    pairing: Option<Pairing>,
    sinks: EngineSinks,
    config_dirty_since: Option<Instant>,
    last_ping_at: Instant,
    last_keepalive_at: Instant,
    clipboard_hash: Option<[u8; 32]>,
    pending_drag: Option<forward_drag::PendingDrag>,
    pending_transfer: drag::TransferPlanSlot,
    next_transfer_id: u64,
    drop_accumulator: crate::daemon::transfer::DropAccumulator,
    active_drop: Option<u64>,
    active_drop_peer: Option<PeerId>,
    active_drop_id: Option<u64>,
    next_drop_id: u64,
    return_drop_active: bool,
    return_drop_since: Option<Instant>,
    receiving_transfer: Option<(PeerId, u64)>,
    authorized_return_drop: Option<control::ReturnAuthorization>,
    fatal: Option<String>,
    monitor: locked::MonitorState,
}

impl Engine {
    pub fn new(
        identity: LocalIdentity,
        paths: EnginePaths,
        config: Config,
        peer_store: PeerStore,
        sinks: EngineSinks,
    ) -> Engine {
        let session = Session::new(session_config(identity.peer_id, &config));
        let drop_accumulator = drag::new_accumulator(config.general.dnd_dir.clone());
        let now = Instant::now();
        Engine {
            identity,
            paths,
            config,
            peer_store,
            session,
            active_peer: None,
            forwarded: PressedInputs::default(),
            forwarded_left_pressed: false,
            outgoing_drag: None,
            incoming_drag: None,
            injected: PressedInputs::default(),
            injected_locks: (0, 0),
            links: LinkTable::default(),
            discovered: HashMap::new(),
            outputs: Vec::new(),
            edges: EdgeMap::default(),
            applied_strips: Vec::new(),
            local_keymap: None,
            enabled: true,
            pairing: None,
            sinks,
            config_dirty_since: None,
            last_ping_at: now,
            last_keepalive_at: now,
            clipboard_hash: None,
            pending_drag: None,
            pending_transfer: None,
            next_transfer_id: 1,
            drop_accumulator,
            active_drop: None,
            active_drop_peer: None,
            active_drop_id: None,
            next_drop_id: 1,
            return_drop_active: false,
            return_drop_since: None,
            receiving_transfer: None,
            authorized_return_drop: None,
            fatal: None,
            monitor: locked::MonitorState::new(now),
        }
    }

    pub(super) fn dispatch(&mut self, event: SessionEvent) {
        if matches!(event, SessionEvent::HotkeyPressed | SessionEvent::Disabled) {
            self.abort_pending_drag();
            self.authorized_return_drop = None;
            self.cancel_active_drop();
        }
        let previous = std::mem::discriminant(self.session.state());
        let was_idle = matches!(self.session.state(), SessionState::Idle);
        let actions = self.session.handle(event, Instant::now());
        if was_idle && !matches!(self.session.state(), SessionState::Idle) {
            self.authorized_return_drop = None;
        }
        if previous != std::mem::discriminant(self.session.state()) {
            self.monitor.invalidate(Instant::now());
        }
        for action in actions {
            self.apply(action);
        }
        self.apply_strips();
    }

    pub(super) fn wayland(&self, command: WaylandCommand) {
        if self.sinks.wayland.commands.send(command).is_err() {
            error!("wayland thread is gone");
        }
    }

    pub(super) fn send_on(&self, connection: ConnectionId, message: ControlMessage) {
        let _ = self.sinks.tcp.send(TcpCommand::Send {
            connection,
            message,
        });
    }

    pub(super) fn send_to(&self, peer_id: PeerId, message: ControlMessage) {
        match self.links.link(peer_id) {
            Some(link) => self.send_on(link.connection, message),
            None => warn!(%peer_id, "dropping message to a peer without a link"),
        }
    }

    pub(super) fn close(&self, connection: ConnectionId) {
        let _ = self.sinks.tcp.send(TcpCommand::Close { connection });
    }

    fn broadcast(&self, message: ControlMessage) {
        for (_, link) in self.links.links() {
            self.send_on(link.connection, message.clone());
        }
    }

    pub(super) fn reconfigure_strips(&mut self) {
        self.monitor.invalidate(Instant::now());
        self.edges.rebuild(&self.outputs, &self.config);
        self.apply_strips();
    }

    fn apply_strips(&mut self) {
        if !matches!(
            self.session.state(),
            SessionState::Idle | SessionState::Controlled { .. }
        ) {
            return;
        }
        let strips = self.edges.strips(&self.config, self.session.return_side());
        if strips == self.applied_strips {
            return;
        }
        self.applied_strips = strips.clone();
        self.wayland(WaylandCommand::ConfigureStrips { strips });
    }
    pub(super) fn apply_hotkey(&self) {
        let hotkey = hotkey_spec(&self.config.general.release_hotkey);
        self.wayland(WaylandCommand::SetReleaseHotkey { hotkey });
    }
    pub(super) fn apply_bar_style(&self) {
        let style = bar_style(&self.config.general.bar_color);
        self.wayland(WaylandCommand::SetBarStyle { style });
    }

    pub(super) fn connected_peer_for_side(
        &self,
        side: opendesk_proto::control::Side,
    ) -> Option<PeerId> {
        let peer_config = self.config.peer_for_side(side)?;
        let record = self.peer_store.find_by_name(&peer_config.name)?;
        self.links.link(record.id).map(|_| record.id)
    }

    pub(super) fn save_peer_store(&self) {
        if let Err(error) = self.peer_store.save(&self.paths.peers) {
            error!(%error, "saving peer store failed");
        }
    }

    pub(super) fn save_config(&self) {
        if let Err(error) = self.config.save(&self.paths.config) {
            error!(%error, "saving config failed");
        }
    }
}

fn session_config(local_id: PeerId, config: &Config) -> SessionConfig {
    SessionConfig {
        local_id,
        threshold_px: config.general.edge_threshold_px,
        cancel_px: config.general.edge_cancel_px,
        request_timeout: REQUEST_TIMEOUT,
        arrival_grace: ARRIVAL_GRACE,
        reentry_grace: REENTRY_GRACE,
        immediate_cross: false,
        first_session_id: 1,
    }
}
