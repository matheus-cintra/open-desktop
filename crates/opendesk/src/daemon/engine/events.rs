use crate::platform::PlatformEvent;
use opendesk_core::session::SessionEvent;
use opendesk_proto::control::ControlMessage;
use std::sync::atomic::Ordering;
use tracing::{debug, error};

use super::Engine;
use crate::daemon::net::discovery::DiscoveryEvent;

impl Engine {
    pub(super) fn on_wayland(&mut self, event: PlatformEvent) {
        match event {
            PlatformEvent::PhysicalActivity => self.claim_physical(),
            PlatformEvent::Ready { outputs } | PlatformEvent::OutputsChanged { outputs } => {
                self.outputs = outputs;
                self.reconfigure_strips();
                self.broadcast(ControlMessage::LayoutChanged {
                    layout: self.outputs.clone(),
                });
            }
            PlatformEvent::Keymap { xkb } => {
                self.local_keymap = Some(xkb.clone());
                if self
                    .active_peer
                    .is_some_and(|peer| self.cross_platform(peer))
                {
                    self.wayland(crate::platform::PlatformCommand::SetKeymap { xkb: xkb.clone() });
                }
                self.broadcast(ControlMessage::Keymap { xkb });
            }
            PlatformEvent::EdgeEntered { side, position, .. } => {
                if !self.enabled
                    || self.monitor.lock != super::monitor::LockState::Unlocked
                    || !self.monitor.known(std::time::Instant::now())
                {
                    return;
                }
                if self.return_drop_active
                    || self
                        .authorized_return_drop
                        .as_ref()
                        .is_some_and(|auth| auth.released)
                {
                    return;
                }
                let fraction = self.edges.fraction(side, position).unwrap_or(0.5);
                if self.map.current.is_some() && self.session.is_controlled() {
                    if self
                        .map
                        .edge_since
                        .is_none_or(|(previous, _, _)| previous != side)
                        && matches!(self.session.state(), opendesk_core::session::SessionState::Controlled {since,..} if since.elapsed() >= std::time::Duration::from_millis(300))
                    {
                        self.map.edge_since = Some((side, fraction, 0.0));
                    }
                    return;
                }
                if self.drag_edge_crossing(side, fraction) {
                    return;
                }
                let peer = if self.map.current.is_some() {
                    self.map_destination(self.identity.peer_id, side, fraction)
                        .map(|c| c.peer)
                } else {
                    self.connected_peer_for_side(side)
                };
                debug!(%side, position, fraction, ?peer, "edge entered");
                self.dispatch(SessionEvent::EdgeEntered {
                    side,
                    fraction,
                    peer,
                });
            }
            PlatformEvent::EdgeLeft { side } => {
                self.map.edge_since = None;
                debug!(%side, "edge left");
                self.dispatch(SessionEvent::EdgeLeft { side });
            }
            PlatformEvent::RelativeMotion { dx, dy } if !self.session.is_controlling() => {
                self.dispatch(SessionEvent::RelativeMotion { dx, dy });
            }
            PlatformEvent::HotkeyPressed => self.stop_map_control(),
            PlatformEvent::ClipboardChanged { content } => self.on_clipboard_changed(content),
            PlatformEvent::DragEnteredEdge {
                generation,
                side,
                position,
                uris,
                ..
            } => {
                if self.monitor.lock != super::monitor::LockState::Unlocked
                    || !self.monitor.known(std::time::Instant::now())
                {
                    return;
                }
                if generation != self.sinks.wayland.drag_generation.load(Ordering::Acquire) {
                    return;
                }
                if self.session.is_controlled() {
                    let fraction = self.edges.fraction(side, position).unwrap_or(0.5);
                    if uris.is_empty() {
                        self.dispatch(SessionEvent::EdgeEntered {
                            side,
                            fraction,
                            peer: None,
                        });
                    } else {
                        self.return_drag_crossing(generation, side, fraction, uris);
                    }
                    return;
                }
                self.on_drag_entered_edge(generation, side, position, uris);
            }
            PlatformEvent::DragMotionEdge { side, position } => {
                self.on_drag_motion_edge(side, position);
            }
            PlatformEvent::DragLeftEdge { generation, side } => {
                self.on_drag_left_edge(generation, side);
            }
            PlatformEvent::DragReleasedEdge { generation } => {
                self.on_drag_released_edge(generation);
            }
            PlatformEvent::DragGeneration { generation } => self.on_drag_generation(generation),
            PlatformEvent::DragFocusReady { id } => self.on_drag_focus_ready(id),
            PlatformEvent::DropDragEnded { id, accepted } => {
                self.on_drop_drag_ended(id, accepted);
            }
            PlatformEvent::Fatal { message } => {
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
