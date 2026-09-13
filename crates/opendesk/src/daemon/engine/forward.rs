use std::time::Instant;

use opendesk_core::session::SessionState;
use opendesk_proto::control::{ControlMessage, PeerId};
use opendesk_proto::input::{InputDatagram, InputEvent, InputHeader};
use opendesk_wayland::{WaylandCommand, WaylandEvent};
use tracing::debug;

use super::Engine;
use crate::daemon::net::udp::{UdpCommand, UdpEvent};

impl Engine {
    pub(super) fn forward_wayland_input(&mut self, event: WaylandEvent) {
        if !self.session.is_controlling() {
            return;
        }
        let Some(peer) = self.active_peer else {
            return;
        };
        match event {
            WaylandEvent::RelativeMotion { dx, dy } => {
                self.send_udp(peer, InputEvent::Motion { dx, dy });
            }
            WaylandEvent::Axis {
                axis,
                value,
                value120,
                source,
            } => {
                self.send_udp(
                    peer,
                    InputEvent::Axis {
                        axis,
                        value,
                        value120,
                        source,
                    },
                );
            }
            WaylandEvent::Button { code, pressed } => {
                debug!(code, pressed, "forwarding pointer button");
                if code == 272 {
                    if pressed {
                        self.forwarded_left_pressed = true;
                    } else if self.finish_outgoing_drag(peer, Instant::now()) {
                        return;
                    } else if self.forwarded_left_pressed {
                        self.forwarded_left_pressed = false;
                    } else {
                        return;
                    }
                }
                self.forwarded.record_button(code, pressed);
                self.send_to(peer, ControlMessage::Button { code, pressed });
            }
            WaylandEvent::Key { code, pressed } => {
                self.forwarded.record_key(code, pressed);
                self.send_to(peer, ControlMessage::Key { code, pressed });
            }
            WaylandEvent::Modifiers {
                depressed,
                latched,
                locked,
                group,
            } => {
                self.send_to(
                    peer,
                    ControlMessage::Modifiers {
                        depressed,
                        latched,
                        locked,
                        group,
                    },
                );
            }
            _ => {}
        }
    }

    pub(super) fn on_left_released(&mut self, at: Instant) {
        debug!(
            controlling = self.session.is_controlling(),
            return_drop = self.return_drop_active,
            outgoing_drag = self.outgoing_drag.is_some(),
            forwarded_left = self.forwarded_left_pressed,
            "handling Hyprland left-button release"
        );
        if let Some(auth) = self.authorized_return_drop.as_mut()
            && at >= auth.created_at
        {
            auth.release_requested = true;
            return;
        }
        if self.return_drop_active
            && self.return_drop_since.is_some_and(|since| at >= since)
            && let Some(id) = self.active_drop_id
        {
            self.wayland(WaylandCommand::ReleaseDropDrag { id });
            return;
        }
        if !self.session.is_controlling()
            && !matches!(self.session.state(), SessionState::Requesting { .. })
        {
            return;
        }
        let Some(peer) = self.active_peer else {
            return;
        };
        if self.finish_outgoing_drag(peer, at) {
            return;
        }
        if self.outgoing_drag.is_some() || !self.forwarded_left_pressed {
            return;
        }
        self.forwarded_left_pressed = false;
        self.forwarded.record_button(272, false);
        self.send_to(
            peer,
            ControlMessage::Button {
                code: 272,
                pressed: false,
            },
        );
    }

    fn finish_outgoing_drag(&mut self, peer: PeerId, at: Instant) -> bool {
        let Some((owner, transfer_id, since)) = self.outgoing_drag else {
            return false;
        };
        if owner != peer || at < since {
            return false;
        }
        debug!(
            transfer_id,
            "forward file drag released; restoring keyboard capture"
        );
        self.outgoing_drag = None;
        self.forwarded_left_pressed = false;
        self.forwarded.record_button(272, false);
        self.send_to(
            peer,
            ControlMessage::Button {
                code: 272,
                pressed: false,
            },
        );
        self.wayland(WaylandCommand::StartGrab);
        true
    }

    pub(super) fn send_udp(&mut self, peer: PeerId, event: InputEvent) {
        let Some(link) = self.links.link_mut(peer) else {
            return;
        };
        let Some(session_id) = link.session_id else {
            return;
        };
        let sequence = link.next_udp_sequence();
        let datagram = InputDatagram {
            header: InputHeader {
                session_id,
                sequence,
            },
            event,
        };
        let _ = self.sinks.udp.send(UdpCommand::Send {
            to: link.udp_address,
            datagram,
        });
    }

    pub(super) fn on_peer_input(&mut self, peer: PeerId, message: ControlMessage) {
        if !self.session.is_controlled() || self.active_peer != Some(peer) {
            return;
        }
        match message {
            ControlMessage::Key { code, pressed } => {
                self.injected.record_key(code, pressed);
                self.wayland(WaylandCommand::InjectKey { code, pressed });
            }
            ControlMessage::Button { code, pressed } => {
                debug!(code, pressed, "injecting forwarded pointer button");
                self.injected.record_button(code, pressed);
                self.wayland(WaylandCommand::InjectButton { code, pressed });
                if !pressed {
                    let active_transfer = self.active_drop;
                    if code == 272
                        && let Some((owner, transfer_id, release)) = self.incoming_drag.as_mut()
                        && *owner == peer
                    {
                        *release = true;
                        if let Some(id) = self.active_drop_id
                            && self.active_drop_peer == Some(peer)
                            && active_transfer == Some(*transfer_id)
                        {
                            self.wayland(WaylandCommand::ReleaseDropDrag { id });
                        }
                    }
                }
            }
            ControlMessage::Modifiers {
                depressed,
                latched,
                locked,
                group,
            } => {
                self.injected_locks = (locked, group);
                self.wayland(WaylandCommand::InjectModifiers {
                    depressed,
                    latched,
                    locked,
                    group,
                });
            }
            _ => {}
        }
    }

    pub(super) fn on_udp(&mut self, event: UdpEvent) {
        let UdpEvent::Datagram { from, datagram } = event;
        let Some(peer) = self.links.peer_for_udp_source(from) else {
            debug!(%from, "datagram from an unknown source");
            return;
        };
        if !self.session.is_controlled() || self.active_peer != Some(peer) {
            return;
        }
        let Some(link) = self.links.link_mut(peer) else {
            return;
        };
        if link.session_id != Some(datagram.header.session_id)
            || !link.accept_udp_sequence(datagram.header.sequence)
        {
            return;
        }
        link.last_udp = Instant::now();
        match datagram.event {
            InputEvent::Motion { dx, dy } => {
                self.wayland(WaylandCommand::InjectMotion { dx, dy });
                self.locked_motion(dx, dy);
            }
            InputEvent::Axis {
                axis,
                value,
                value120,
                source,
            } => {
                self.wayland(WaylandCommand::InjectAxis {
                    axis,
                    value,
                    value120,
                    source,
                });
            }
            InputEvent::Keepalive => {}
        }
    }
}
