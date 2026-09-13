use std::time::{Duration, Instant};

use crate::platform::PlatformCommand;
use opendesk_core::pressed::Release;
use opendesk_core::session::{SessionAction, SessionEvent};
use opendesk_proto::control::{ControlMessage, PeerId};
use opendesk_proto::transfer::DragInfo;
use tracing::{debug, warn};

use super::Engine;

const RETURN_AUTH_TIMEOUT: Duration = Duration::from_secs(15);

pub struct ReturnAuthorization {
    pub peer: PeerId,
    pub transfer_id: u64,
    pub entries: Vec<opendesk_proto::transfer::TransferEntry>,
    pub expires_at: Instant,
    pub released: bool,
    pub created_at: Instant,
    pub release_requested: bool,
}

impl Engine {
    pub(super) fn authorize_return_drag(&mut self, peer: PeerId, drag: DragInfo) {
        self.authorized_return_drop = Some(ReturnAuthorization {
            peer,
            transfer_id: drag.transfer_id,
            entries: drag.entries,
            expires_at: Instant::now() + RETURN_AUTH_TIMEOUT,
            released: false,
            created_at: Instant::now(),
            release_requested: false,
        });
    }

    pub(super) fn mark_return_drag_released(&mut self, peer: PeerId) {
        if let Some(auth) = self.authorized_return_drop.as_mut()
            && auth.peer == peer
        {
            auth.released = true;
        }
    }

    pub(super) fn expire_return_drag(&mut self, now: Instant) {
        if self
            .authorized_return_drop
            .as_ref()
            .is_some_and(|auth| now >= auth.expires_at)
        {
            self.authorized_return_drop = None;
        }
    }

    pub(super) fn clear_return_drag(&mut self, peer: PeerId) {
        if self
            .authorized_return_drop
            .as_ref()
            .is_some_and(|auth| auth.peer == peer)
        {
            self.authorized_return_drop = None;
        }
        if let Some((owner, transfer_id)) = self.receiving_transfer
            && owner == peer
        {
            self.receiving_transfer = None;
            self.drop_accumulator.cancel(transfer_id);
        }
        if self.active_drop_peer == Some(peer) {
            self.cancel_active_drop();
        }
        if self
            .incoming_drag
            .is_some_and(|(owner, _, _)| owner == peer)
        {
            self.incoming_drag = None;
        }
    }

    pub(super) fn apply(&mut self, action: SessionAction) {
        match action {
            SessionAction::LockPointer => self.wayland(PlatformCommand::LockPointer),
            SessionAction::UnlockPointer { .. } => {
                self.wayland(PlatformCommand::UnlockPointer { hint: None });
            }
            SessionAction::ShowProgress {
                side,
                fraction,
                progress,
            } => {
                if let Some(position) = self.edges.hint(side, fraction) {
                    self.wayland(PlatformCommand::ShowProgressBar {
                        side,
                        position,
                        progress,
                    });
                }
            }
            SessionAction::HideProgress => self.wayland(PlatformCommand::HideProgressBar),
            SessionAction::ShowArrival { side, fraction } => {
                if let Some(position) = self.edges.hint(side, fraction) {
                    self.wayland(PlatformCommand::ShowArrivalBar { side, position });
                }
            }
            SessionAction::SendRequestControl {
                peer,
                side,
                fraction,
            } => {
                if self.map.current.is_some() {
                    if let Some(crossing) =
                        self.map_destination(self.identity.peer_id, side, fraction)
                    {
                        self.begin_map_transfer(crossing.peer, side, crossing.fraction, None);
                    } else {
                        self.stop_map_control();
                    }
                    return;
                }
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
            SessionAction::StartGrab => self.wayland(PlatformCommand::StartGrab),
            SessionAction::StopGrab { side, fraction } => {
                self.pending_transfer = None;
                self.outgoing_drag = None;
                let hint = fraction.and_then(|fraction| self.edges.hint(side, fraction));
                self.wayland(PlatformCommand::StopGrab { hint });
            }
            SessionAction::WarpCursor { side, fraction } => {
                let point = self.edges.entry_point(side, fraction);
                debug!(%side, fraction, ?point, "warping cursor to the entry point");
                if let Some((x, y)) = point {
                    self.wayland(PlatformCommand::InjectAbsoluteMotion { x, y });
                }
            }
            SessionAction::ReleaseAllPressed => {
                self.cancel_active_drop();
                self.release_all_pressed();
            }
        }
    }

    fn release_all_pressed(&mut self) {
        self.forwarded_left_pressed = false;
        self.outgoing_drag = None;
        self.incoming_drag = None;
        for release in self.forwarded.drain_releases() {
            if let Some(peer) = self.active_peer {
                let message = match release {
                    Release::Key(code) => self.key_message(peer, code, false),
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
                Release::Key(code) => {
                    if self.injected_physical {
                        PlatformCommand::InjectPhysicalKey {
                            code,
                            pressed: false,
                        }
                    } else {
                        PlatformCommand::InjectKey {
                            code,
                            pressed: false,
                        }
                    }
                }
                Release::Button(code) => PlatformCommand::InjectButton {
                    code,
                    pressed: false,
                },
            };
            self.wayland(command);
        }
        self.wayland(PlatformCommand::InjectModifiers {
            depressed: 0,
            latched: 0,
            locked: self.injected_locks.0,
            group: self.injected_locks.1,
        });
        self.injected_physical = false;
        self.active_peer = None;
    }

    pub(super) fn on_control_message(&mut self, peer: PeerId, message: ControlMessage) {
        if self.map.current.is_some()
            && matches!(
                message,
                ControlMessage::ControlGranted { .. }
                    | ControlMessage::ControlDenied { .. }
                    | ControlMessage::ReleaseControl { .. }
                    | ControlMessage::ReturnDrag { .. }
            )
        {
            return;
        }
        match message {
            ControlMessage::Map(message) => self.on_map_message(peer, message),
            ControlMessage::Capabilities { macos, file_drag } => {
                self.set_capabilities(peer, macos, file_drag);
            }
            ControlMessage::RequestControl {
                side,
                fraction,
                drag,
            } => {
                if self.map.current.is_none()
                    && self.enabled
                    && self.monitor.known(Instant::now())
                    && (!cfg!(target_os = "macos")
                        || self.monitor.lock == super::monitor::LockState::Unlocked)
                    && (drag.is_none() || self.supports_drag(peer))
                    && (drag.is_none() || self.monitor.lock == super::monitor::LockState::Unlocked)
                {
                    self.incoming_drag = drag.map(|info| (peer, info.transfer_id, false));
                    self.dispatch(SessionEvent::PeerRequestedControl {
                        peer,
                        side,
                        fraction,
                    });
                    if !self.session.is_controlled() || self.active_peer != Some(peer) {
                        self.incoming_drag = None;
                    }
                } else {
                    debug!(lock = ?self.monitor.lock, enabled = self.enabled, "control refused: paused, unavailable monitor or locked file destination");
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
                if reason == opendesk_proto::control::ReleaseReason::EdgeCrossed
                    && self.session.is_controlling()
                    && self.active_peer == Some(peer)
                {
                    self.mark_return_drag_released(peer);
                } else {
                    self.clear_return_drag(peer);
                }
                self.dispatch(SessionEvent::PeerReleased { peer, fraction });
            }
            ControlMessage::LayoutChanged { layout } => {
                if let Some(link) = self.links.link_mut(peer) {
                    link.layout = layout;
                }
            }
            ControlMessage::Keymap { xkb } => {
                if !self.cross_platform(peer) {
                    self.wayland(PlatformCommand::SetKeymap { xkb });
                }
            }
            ControlMessage::PhysicalKey { .. }
            | ControlMessage::Key { .. }
            | ControlMessage::Button { .. }
            | ControlMessage::Modifiers { .. } => {
                if self.map.current.is_none() {
                    self.on_peer_input(peer, message);
                }
            }
            ControlMessage::Ping { nonce } => self.send_to(peer, ControlMessage::Pong { nonce }),
            ControlMessage::Pong { .. } => {
                if let Some(link) = self.links.link_mut(peer) {
                    link.last_pong = Instant::now();
                }
            }
            ControlMessage::ClipboardSet { mime, bytes } => self.on_clipboard_received(mime, bytes),
            ControlMessage::FileBegin(begin) => self.on_file_begin(peer, begin),
            ControlMessage::FileChunk(chunk) => self.on_file_chunk(peer, chunk),
            ControlMessage::FileEnd(end) => self.on_file_end(peer, end),
            ControlMessage::DragCancel { transfer_id } => self.on_drag_cancel(peer, transfer_id),
            ControlMessage::ReturnDrag { drag } => {
                if self.supports_drag(peer)
                    && self.session.is_controlling()
                    && self.active_peer == Some(peer)
                {
                    self.authorize_return_drag(peer, drag);
                }
            }
            other => warn!(%peer, ?other, "unexpected message on an established link"),
        }
    }
}
