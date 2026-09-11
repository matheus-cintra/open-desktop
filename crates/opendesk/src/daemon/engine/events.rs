use opendesk_core::session::SessionEvent;
use opendesk_proto::control::ControlMessage;
use opendesk_wayland::WaylandEvent;
use tracing::{debug, error};

use super::Engine;
use crate::daemon::net::discovery::DiscoveryEvent;

impl Engine {
    pub(super) fn on_wayland(&mut self, event: WaylandEvent) {
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
                debug!(%side, position, fraction, ?peer, "edge entered");
                self.dispatch(SessionEvent::EdgeEntered {
                    side,
                    fraction,
                    peer,
                });
            }
            WaylandEvent::EdgeLeft { side } => {
                debug!(%side, "edge left");
                self.dispatch(SessionEvent::EdgeLeft { side });
            }
            WaylandEvent::RelativeMotion { dx, dy } if !self.session.is_controlling() => {
                self.dispatch(SessionEvent::RelativeMotion { dx, dy });
            }
            WaylandEvent::HotkeyPressed => self.dispatch(SessionEvent::HotkeyPressed),
            WaylandEvent::ClipboardChanged { content } => self.on_clipboard_changed(content),
            WaylandEvent::Fatal { message } => {
                error!(message, "wayland connection failed");
                self.fatal = Some(message);
            }
            other => self.forward_wayland_input(other),
        }
    }

    pub(super) fn on_discovery(&mut self, event: DiscoveryEvent) {
        match event {
            DiscoveryEvent::Found(peer) => {
                self.discovered.insert(peer.peer_id, peer);
            }
            DiscoveryEvent::Lost(peer_id) => {
                self.discovered.remove(&peer_id);
            }
        }
    }
}
