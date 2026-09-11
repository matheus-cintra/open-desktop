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
            SessionAction::UnlockPointer { .. } => {
                self.wayland(WaylandCommand::UnlockPointer { hint: None });
            }
            SessionAction::ShowProgress {
                side,
                fraction,
                progress,
            } => {
                if let Some(position) = self.edges.hint(side, fraction) {
                    self.wayland(WaylandCommand::ShowProgressBar {
                        side,
                        position,
                        progress,
                    });
                }
            }
            SessionAction::HideProgress => self.wayland(WaylandCommand::HideProgressBar),
            SessionAction::ShowArrival { side, fraction } => {
                if let Some(position) = self.edges.hint(side, fraction) {
                    self.wayland(WaylandCommand::ShowArrivalBar { side, position });
                }
            }
            SessionAction::SendRequestControl {
                peer,
                side,
                fraction,
            } => {
                self.active_peer = Some(peer);
                let drag = self.take_pending_drag_info();
                self.send_to(
                    peer,
                    ControlMessage::RequestControl {
                        side,
                        fraction,
                        drag,
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
                let point = self.edges.entry_point(side, fraction);
                debug!(%side, fraction, ?point, "warping cursor to the entry point");
                if let Some((x, y)) = point {
                    self.wayland(WaylandCommand::InjectAbsoluteMotion { x, y });
                }
            }
            SessionAction::ReleaseAllPressed => {
                self.cancel_active_drop();
                self.release_all_pressed();
            }
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
                self.start_transfer_on_grant(peer);
            }
            ControlMessage::ControlDenied { reason } => {
                debug!(%peer, ?reason, "control denied");
                self.pending_transfer = None;
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
            ControlMessage::ClipboardSet { mime, bytes } => self.on_clipboard_received(mime, bytes),
            ControlMessage::FileBegin(begin) => self.on_file_begin(begin),
            ControlMessage::FileChunk(chunk) => self.on_file_chunk(chunk),
            ControlMessage::FileEnd(end) => self.on_file_end(end),
            ControlMessage::DragCancel { transfer_id } => self.on_drag_cancel(transfer_id),
            other => warn!(%peer, ?other, "unexpected message on an established link"),
        }
    }
}
