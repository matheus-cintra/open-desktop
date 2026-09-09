use std::time::Instant;

use opendesk_core::pressed::Release;
use opendesk_core::session::{SessionAction, SessionEvent};
use opendesk_proto::control::{ControlMessage, PeerId};
use opendesk_wayland::WaylandCommand;
use tracing::{debug, warn};

use super::Engine;

impl Engine {
    pub(super) fn apply(&mut self, action: SessionAction) {
        match action {
            SessionAction::LockPointer => self.wayland(WaylandCommand::LockPointer),
            SessionAction::UnlockPointer { side, fraction } => {
                let hint = self.edges.hint(side, fraction);
                self.wayland(WaylandCommand::UnlockPointer { hint });
            }
            SessionAction::ShowProgress { .. }
            | SessionAction::HideProgress
            | SessionAction::ShowArrival { .. } => {}
            SessionAction::SendRequestControl {
                peer,
                side,
                fraction,
            } => {
                self.active_peer = Some(peer);
                self.send_to(
                    peer,
                    ControlMessage::RequestControl {
                        side,
                        fraction,
                        drag: None,
                    },
                );
            }
            SessionAction::SendControlGranted { peer, session_id } => {
                self.active_peer = Some(peer);
                if let Some(link) = self.links.link_mut(peer) {
                    link.start_session(session_id);
                    link.last_udp = Instant::now();
                }
                self.send_to(peer, ControlMessage::ControlGranted { session_id });
            }
            SessionAction::SendControlDenied { peer, reason } => {
                self.send_to(peer, ControlMessage::ControlDenied { reason });
            }
            SessionAction::SendReleaseControl {
                peer,
                fraction,
                reason,
            } => {
                self.send_to(peer, ControlMessage::ReleaseControl { fraction, reason });
            }
            SessionAction::StartGrab => self.wayland(WaylandCommand::StartGrab),
            SessionAction::StopGrab { side, fraction } => {
                let hint = fraction.and_then(|fraction| self.edges.hint(side, fraction));
                self.wayland(WaylandCommand::StopGrab { hint });
            }
            SessionAction::WarpCursor { side, fraction } => {
                if let Some((x, y)) = self.edges.entry_point(side, fraction) {
                    self.wayland(WaylandCommand::InjectAbsoluteMotion { x, y });
                }
            }
            SessionAction::ReleaseAllPressed => self.release_all_pressed(),
        }
    }

    fn release_all_pressed(&mut self) {
        if let Some(peer) = self.active_peer {
            for release in self.forwarded.drain_releases() {
                let message = match release {
                    Release::Key(code) => ControlMessage::Key {
                        code,
                        pressed: false,
                    },
                    Release::Button(code) => ControlMessage::Button {
                        code,
                        pressed: false,
                    },
                };
                self.send_to(peer, message);
            }
        }
        for release in self.injected.drain_releases() {
            let command = match release {
                Release::Key(code) => WaylandCommand::InjectKey {
                    code,
                    pressed: false,
                },
                Release::Button(code) => WaylandCommand::InjectButton {
                    code,
                    pressed: false,
                },
            };
            self.wayland(command);
        }
        self.active_peer = None;
    }

    pub(super) fn on_control_message(&mut self, peer: PeerId, message: ControlMessage) {
        match message {
            ControlMessage::RequestControl { side, fraction, .. } => {
                if self.enabled {
                    self.dispatch(SessionEvent::PeerRequestedControl {
                        peer,
                        side,
                        fraction,
                    });
                } else {
                    let reason = opendesk_proto::control::DenyReason::Disabled;
                    self.send_to(peer, ControlMessage::ControlDenied { reason });
                }
            }
            ControlMessage::ControlGranted { session_id } => {
                if let Some(link) = self.links.link_mut(peer) {
                    link.start_session(session_id);
                }
                self.dispatch(SessionEvent::PeerGranted { peer, session_id });
            }
            ControlMessage::ControlDenied { reason } => {
                debug!(%peer, ?reason, "control denied");
                self.dispatch(SessionEvent::PeerDenied { peer });
            }
            ControlMessage::ReleaseControl { fraction, reason } => {
                debug!(%peer, ?reason, "control released by peer");
                self.dispatch(SessionEvent::PeerReleased { peer, fraction });
            }
            ControlMessage::LayoutChanged { layout } => {
                if let Some(link) = self.links.link_mut(peer) {
                    link.layout = layout;
                }
            }
            ControlMessage::Keymap { xkb } => self.wayland(WaylandCommand::SetKeymap { xkb }),
            ControlMessage::Key { .. }
            | ControlMessage::Button { .. }
            | ControlMessage::Modifiers { .. } => self.on_peer_input(peer, message),
            ControlMessage::Ping { nonce } => self.send_to(peer, ControlMessage::Pong { nonce }),
            ControlMessage::Pong { .. } => {
                if let Some(link) = self.links.link_mut(peer) {
                    link.last_pong = Instant::now();
                }
            }
            ControlMessage::ClipboardSet { .. }
            | ControlMessage::FileBegin(_)
            | ControlMessage::FileChunk(_)
            | ControlMessage::FileEnd(_)
            | ControlMessage::DragCancel { .. } => {
                debug!(%peer, "message not supported in this milestone");
            }
            other => warn!(%peer, ?other, "unexpected message on an established link"),
        }
    }
}
