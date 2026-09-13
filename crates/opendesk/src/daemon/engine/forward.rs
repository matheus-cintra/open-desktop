use std::time::Instant;

use crate::platform::{PlatformCommand, PlatformEvent};
use opendesk_core::session::SessionState;
use opendesk_proto::control::{ControlMessage, PeerId};
use opendesk_proto::input::{InputDatagram, InputEvent, InputHeader};
use tracing::debug;

use super::Engine;
use crate::daemon::net::udp::{UdpCommand, UdpEvent};

impl Engine {
    pub(super) fn forward_wayland_input(&mut self, event: PlatformEvent) {
        if self.map.pending.is_some() {
            match event {
                PlatformEvent::Key { code, pressed } => self.forwarded.record_key(code, pressed),
                PlatformEvent::Button { code, pressed } => {
                    self.forwarded.record_button(code, pressed)
                }
                _ => {}
            }
            if !self.forwarded.can_transfer() {
                self.stop_map_control();
            }
            return;
        }
        if !self.session.is_controlling() {
            return;
        }
        let Some(peer) = self.active_peer else {
            return;
        };
        match event {
            PlatformEvent::RelativeMotion { dx, dy } => {
                self.send_udp(peer, InputEvent::Motion { dx, dy });
            }
            PlatformEvent::Axis {
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
            PlatformEvent::Button { code, pressed } => {
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
            PlatformEvent::Key { code, pressed } => {
                self.forwarded.record_key(code, pressed);
                self.send_to(peer, self.key_message(peer, code, pressed));
            }
            PlatformEvent::Modifiers {
                depressed,
                latched,
                locked,
                group,
            } => {
                if self.cross_platform(peer) {
                    return;
                }
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
        if self.map.pending.as_ref().is_some_and(|p| p.old.is_none())
            && self.outgoing_drag.is_some()
        {
            self.map.drag_released = true;
            return;
        }
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
            self.wayland(PlatformCommand::ReleaseDropDrag { id });
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
        self.wayland(PlatformCommand::StartGrab);
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
                epoch: self.map.epoch,
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
            ControlMessage::PhysicalKey { usage, pressed } if self.cross_platform(peer) => {
                self.injected_physical = true;
                let usage = opendesk_proto::keyboard::swap_control_super(usage);
                if let Some(code) = opendesk_proto::keyboard::hid_to_evdev(usage) {
                    self.injected.record_key(code, pressed);
                    self.wayland(PlatformCommand::InjectPhysicalKey { code, pressed });
                }
            }
            ControlMessage::Key { code, pressed } if !self.cross_platform(peer) => {
                self.injected.record_key(code, pressed);
                self.wayland(PlatformCommand::InjectKey { code, pressed });
            }
            ControlMessage::Button { code, pressed } => {
                debug!(code, pressed, "injecting forwarded pointer button");
                self.injected.record_button(code, pressed);
                self.wayland(PlatformCommand::InjectButton { code, pressed });
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
                            self.wayland(PlatformCommand::ReleaseDropDrag { id });
                        }
                    }
                }
            }
            ControlMessage::Modifiers {
                depressed,
                latched,
                locked,
                group,
            } if !self.cross_platform(peer) => {
                self.injected_locks = (locked, group);
                self.wayland(PlatformCommand::InjectModifiers {
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
        if datagram.header.epoch != self.map.epoch
            || self.map.epoch.is_some()
                && self.map.last_renew.elapsed() >= std::time::Duration::from_secs(1)
        {
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
                self.wayland(PlatformCommand::InjectMotion { dx, dy });
                if self.map.current.is_none() {
                    self.locked_motion(dx, dy);
                }
                if let Some((side, fraction, accumulated)) = self.map.edge_since {
                    let outward = match side {
                        opendesk_proto::control::Side::Left => -dx,
                        opendesk_proto::control::Side::Right => dx,
                        opendesk_proto::control::Side::Top => -dy,
                        opendesk_proto::control::Side::Bottom => dy,
                    };
                    let total = accumulated + outward;
                    if total < -self.config.general.edge_cancel_px {
                        self.map.edge_since = None;
                    } else if total >= self.config.general.edge_threshold_px {
                        self.map.edge_since = None;
                        if let Some(epoch) = self.map.epoch {
                            self.send_to(
                                peer,
                                ControlMessage::Map(opendesk_proto::map::MapControl::Edge {
                                    epoch,
                                    side,
                                    fraction,
                                }),
                            );
                        }
                    } else {
                        self.map.edge_since = Some((side, fraction, total));
                    }
                }
            }
            InputEvent::Axis {
                axis,
                value,
                value120,
                source,
            } => {
                self.wayland(PlatformCommand::InjectAxis {
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
