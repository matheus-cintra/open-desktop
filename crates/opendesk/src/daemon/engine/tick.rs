use std::net::SocketAddr;
use std::time::{Duration, Instant};

use opendesk_core::config::Config;
use opendesk_core::session::SessionEvent;
use opendesk_proto::control::{ControlMessage, PeerId, ReleaseReason};
use opendesk_proto::input::InputEvent;
use tracing::{error, info, warn};

use super::Engine;
use super::links::DialPurpose;

const PING_EVERY: Duration = Duration::from_secs(2);
const PONG_TIMEOUT: Duration = Duration::from_secs(8);
const UDP_KEEPALIVE_EVERY: Duration = Duration::from_millis(500);
const UDP_TIMEOUT: Duration = Duration::from_secs(3);
const CONFIG_SETTLE: Duration = Duration::from_millis(300);

impl Engine {
    pub(super) fn on_tick(&mut self, now: Instant) {
        self.dispatch(SessionEvent::Tick);
        self.tick_drag(now);
        self.expire_pairing(now);
        self.reload_config_if_dirty(now);
        self.dial_missing_peers(now);
        self.ping_links(now);
        self.keep_udp_alive(now);
        self.detect_udp_silence(now);
    }

    fn reload_config_if_dirty(&mut self, now: Instant) {
        let Some(dirty_since) = self.config_dirty_since else {
            return;
        };
        if now.duration_since(dirty_since) < CONFIG_SETTLE {
            return;
        }
        self.config_dirty_since = None;
        match Config::load(&self.paths.config) {
            Ok(config) => {
                if config.general.port != self.config.general.port {
                    warn!("port changed in config; restart the daemon to apply it");
                }
                self.config = config;
                info!("config reloaded");
                self.apply_hotkey();
                self.apply_bar_style();
                self.reconfigure_strips();
            }
            Err(error) => error!(%error, "config reload failed, keeping the previous config"),
        }
    }

    fn dial_missing_peers(&mut self, now: Instant) {
        let candidates: Vec<(PeerId, SocketAddr)> = self
            .peer_store
            .records()
            .iter()
            .filter(|record| self.identity.peer_id < record.id)
            .filter(|record| self.links.may_dial(record.id, now))
            .filter_map(|record| {
                self.peer_address(record.id, &record.name)
                    .map(|a| (record.id, a))
            })
            .collect();
        for (peer_id, address) in candidates {
            self.dial(address, peer_id, DialPurpose::Link);
        }
    }

    fn peer_address(&self, peer_id: PeerId, name: &str) -> Option<SocketAddr> {
        self.discovered
            .get(&peer_id)
            .map(|peer| peer.address)
            .or_else(|| {
                self.config
                    .peers
                    .iter()
                    .find(|peer| peer.name == name)
                    .and_then(|peer| peer.addr)
            })
    }

    fn ping_links(&mut self, now: Instant) {
        if now.duration_since(self.last_ping_at) < PING_EVERY {
            return;
        }
        self.last_ping_at = now;
        let mut stale = Vec::new();
        for (peer_id, link) in self.links.links() {
            if now.duration_since(link.last_pong) > PONG_TIMEOUT {
                stale.push((*peer_id, link.connection));
                continue;
            }
            self.send_on(link.connection, ControlMessage::Ping { nonce: 0 });
        }
        for (peer_id, connection) in stale {
            warn!(%peer_id, "peer stopped answering pings, closing");
            self.close(connection);
        }
    }

    fn keep_udp_alive(&mut self, now: Instant) {
        if !self.session.is_controlling()
            || now.duration_since(self.last_keepalive_at) < UDP_KEEPALIVE_EVERY
        {
            return;
        }
        self.last_keepalive_at = now;
        if let Some(peer) = self.active_peer {
            self.send_udp(peer, InputEvent::Keepalive);
        }
    }

    fn detect_udp_silence(&mut self, now: Instant) {
        if !self.session.is_controlled() {
            return;
        }
        let Some(peer) = self.active_peer else {
            return;
        };
        let silent = self
            .links
            .link(peer)
            .is_some_and(|link| now.duration_since(link.last_udp) > UDP_TIMEOUT);
        if !silent {
            return;
        }
        warn!(%peer, "no input from the controlling peer, releasing");
        let message = ControlMessage::ReleaseControl {
            fraction: None,
            reason: ReleaseReason::Disconnect,
        };
        self.send_to(peer, message);
        self.dispatch(SessionEvent::PeerDisconnected { peer });
    }
}
