use opendesk_core::session::SessionEvent;
use opendesk_proto::control::ControlMessage;
use opendesk_wayland::WaylandEvent;
use std::sync::atomic::Ordering;
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
                if self.drag_edge_crossing(side, fraction) {
                    return;
                }
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
            WaylandEvent::DragEnteredEdge {
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
            WaylandEvent::DragMotionEdge { side, position } => {
                self.on_drag_motion_edge(side, position);
            }
            WaylandEvent::DragLeftEdge { generation, side } => {
                self.on_drag_left_edge(generation, side);
            }
            WaylandEvent::DragReleasedEdge { generation } => {
                self.on_drag_released_edge(generation);
            }
            WaylandEvent::DragGeneration { generation } => self.on_drag_generation(generation),
            WaylandEvent::DragFocusReady { id } => self.on_drag_focus_ready(id),
            WaylandEvent::DropDragEnded { id, accepted } => {
                self.on_drop_drag_ended(id, accepted);
            }
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
