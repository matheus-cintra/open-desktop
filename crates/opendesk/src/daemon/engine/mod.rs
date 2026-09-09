mod bootstrap;
mod control;
mod edges;
mod forward;
mod handshake;
mod ipc;
mod links;
mod pairing;
mod tick;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use opendesk_core::config::Config;
use opendesk_core::hotkey::Hotkey;
use opendesk_core::peers::PeerStore;
use opendesk_core::pressed::PressedInputs;
use opendesk_core::session::{Session, SessionConfig, SessionEvent};
use opendesk_proto::control::{ControlMessage, OutputGeometry, PeerId};
use opendesk_wayland::{HotkeySpec, WaylandCommand, WaylandEvent, WaylandHandle};
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

const TICK: Duration = Duration::from_millis(100);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const ARRIVAL_GRACE: Duration = Duration::from_millis(300);

pub struct EngineChannels {
    pub wayland_events: UnboundedReceiver<WaylandEvent>,
    pub tcp_events: UnboundedReceiver<TcpEvent>,
    pub udp_events: UnboundedReceiver<UdpEvent>,
    pub discovery_events: UnboundedReceiver<DiscoveryEvent>,
    pub ipc_requests: Receiver<(IpcRequest, oneshot::Sender<IpcResponse>)>,
    pub config_events: UnboundedReceiver<ConfigChanged>,
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
    injected: PressedInputs,
    links: LinkTable,
    discovered: HashMap<PeerId, DiscoveredPeer>,
    outputs: Vec<OutputGeometry>,
    edges: EdgeMap,
    local_keymap: Option<String>,
    enabled: bool,
    pairing: Option<Pairing>,
    sinks: EngineSinks,
    config_dirty_since: Option<Instant>,
    last_ping_at: Instant,
    last_keepalive_at: Instant,
    fatal: Option<String>,
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
        let now = Instant::now();
        Engine {
            identity,
            paths,
            config,
            peer_store,
            session,
            active_peer: None,
            forwarded: PressedInputs::default(),
            injected: PressedInputs::default(),
            links: LinkTable::default(),
            discovered: HashMap::new(),
            outputs: Vec::new(),
            edges: EdgeMap::default(),
            local_keymap: None,
            enabled: true,
            pairing: None,
            sinks,
            config_dirty_since: None,
            last_ping_at: now,
            last_keepalive_at: now,
            fatal: None,
        }
    }

    pub async fn run(mut self, mut channels: EngineChannels) -> anyhow::Result<()> {
        self.apply_hotkey();
        let mut ticker = tokio::time::interval(TICK);
        let outcome = loop {
            tokio::select! {
                Some(event) = channels.wayland_events.recv() => self.on_wayland(event),
                Some(event) = channels.tcp_events.recv() => self.on_tcp(event),
                Some(event) = channels.udp_events.recv() => self.on_udp(event),
                Some(event) = channels.discovery_events.recv() => self.on_discovery(event),
                Some((request, reply)) = channels.ipc_requests.recv() => self.on_ipc(request, reply),
                Some(ConfigChanged) = channels.config_events.recv() => {
                    self.config_dirty_since = Some(Instant::now());
                }
                _ = ticker.tick() => self.on_tick(Instant::now()),
                _ = &mut channels.shutdown => break Ok(()),
            }
            if let Some(message) = self.fatal.take() {
                break Err(anyhow::anyhow!(message));
            }
        };
        if let Err(error) = self.sinks.wayland.shutdown() {
            warn!(%error, "wayland thread did not shut down cleanly");
        }
        outcome
    }

    fn on_wayland(&mut self, event: WaylandEvent) {
        match event {
            WaylandEvent::Ready { outputs } | WaylandEvent::OutputsChanged { outputs } => {
                self.outputs = outputs;
                self.reconfigure_strips();
                self.broadcast(ControlMessage::LayoutChanged {
                    layout: self.outputs.clone(),
                });
            }
            WaylandEvent::Keymap { xkb } => {
                self.local_keymap = Some(xkb.clone());
                self.broadcast(ControlMessage::Keymap { xkb });
            }
            WaylandEvent::EdgeEntered { side, position, .. } => {
                if !self.enabled {
                    return;
                }
                let fraction = self.edges.fraction(side, position).unwrap_or(0.5);
                let peer = self.connected_peer_for_side(side);
                self.dispatch(SessionEvent::EdgeEntered {
                    side,
                    fraction,
                    peer,
                });
            }
            WaylandEvent::EdgeLeft { side } => self.dispatch(SessionEvent::EdgeLeft { side }),
            WaylandEvent::RelativeMotion { dx, dy } if !self.session.is_controlling() => {
                self.dispatch(SessionEvent::RelativeMotion { dx, dy });
            }
            WaylandEvent::HotkeyPressed => self.dispatch(SessionEvent::HotkeyPressed),
            WaylandEvent::Fatal { message } => {
                error!(message, "wayland connection failed");
                self.fatal = Some(message);
            }
            other => self.forward_wayland_input(other),
        }
    }

    fn on_discovery(&mut self, event: DiscoveryEvent) {
        match event {
            DiscoveryEvent::Found(peer) => {
                self.discovered.insert(peer.peer_id, peer);
            }
            DiscoveryEvent::Lost(peer_id) => {
                self.discovered.remove(&peer_id);
            }
        }
    }

    pub(super) fn dispatch(&mut self, event: SessionEvent) {
        let actions = self.session.handle(event, Instant::now());
        for action in actions {
            self.apply(action);
        }
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
        let strips = self.edges.rebuild(&self.outputs, &self.config);
        self.wayland(WaylandCommand::ConfigureStrips { strips });
    }

    pub(super) fn apply_hotkey(&self) {
        let hotkey = hotkey_spec(&self.config.general.release_hotkey);
        self.wayland(WaylandCommand::SetReleaseHotkey { hotkey });
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
        immediate_cross: true,
        first_session_id: 1,
    }
}

fn hotkey_spec(hotkey: &Hotkey) -> HotkeySpec {
    HotkeySpec {
        ctrl: hotkey.ctrl,
        alt: hotkey.alt,
        shift: hotkey.shift,
        logo: hotkey.logo,
        key: hotkey.key.clone(),
    }
}
