use super::Engine;
use opendesk_proto::control::{ControlMessage, PeerId};
impl Engine {
    pub(super) fn input_status(&self) -> &'static str {
        #[cfg(target_os = "macos")]
        return match opendesk_macos::health() {
            0 => "ready",
            1 => "permissions-required",
            _ => "session-unavailable",
        };
        #[cfg(target_os = "linux")]
        if !self.monitor.known(std::time::Instant::now()) {
            "unavailable"
        } else if self.monitor.lock == super::monitor::LockState::Locked {
            "locked"
        } else {
            "ready"
        }
    }

    pub(super) fn set_capabilities(&mut self, peer: PeerId, macos: bool, file_drag: bool) {
        if let Some(link) = self.links.link_mut(peer) {
            link.macos = macos;
            link.file_drag = file_drag;
        }
        if self.cross_platform(peer)
            && let Some(xkb) = self.local_keymap.clone()
        {
            self.wayland(crate::platform::PlatformCommand::SetKeymap { xkb });
        }
    }
    pub(super) fn cross_platform(&self, peer: PeerId) -> bool {
        self.links
            .link(peer)
            .is_some_and(|link| link.macos != cfg!(target_os = "macos"))
    }
    pub(super) fn supports_drag(&self, peer: PeerId) -> bool {
        cfg!(target_os = "linux") && self.links.link(peer).is_some_and(|link| link.file_drag)
    }
    pub(super) fn key_message(&self, peer: PeerId, code: u32, pressed: bool) -> ControlMessage {
        if self.cross_platform(peer) {
            ControlMessage::PhysicalKey {
                usage: opendesk_proto::keyboard::evdev_to_hid(code).unwrap_or(0),
                pressed,
            }
        } else {
            ControlMessage::Key { code, pressed }
        }
    }
}
