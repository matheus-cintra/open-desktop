use std::os::fd::AsFd;

use wayland_client::QueueHandle;
use wayland_client::protocol::wl_keyboard::{KeyState, KeymapFormat};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

use crate::error::WaylandError;
use crate::globals::NoEvents;
use crate::keymap::keymap_memfd;
use crate::state::State;

pub struct VirtualKeyboard {
    object: ZwpVirtualKeyboardV1,
    has_keymap: bool,
    state: Option<xkbcommon::xkb::State>,
}

impl VirtualKeyboard {
    pub fn new(
        manager: &ZwpVirtualKeyboardManagerV1,
        seat: &WlSeat,
        queue_handle: &QueueHandle<State>,
    ) -> VirtualKeyboard {
        VirtualKeyboard {
            object: manager.create_virtual_keyboard(seat, queue_handle, NoEvents),
            has_keymap: false,
            state: None,
        }
    }

    pub fn set_keymap(&mut self, xkb: &str) -> Result<(), WaylandError> {
        let context = xkbcommon::xkb::Context::new(0);
        let keymap = xkbcommon::xkb::Keymap::new_from_string(&context, xkb.to_owned(), 1, 0)
            .ok_or(WaylandError::KeymapCompile)?;
        self.state = Some(xkbcommon::xkb::State::new(&keymap));
        let (fd, size) = keymap_memfd(xkb)?;
        self.object
            .keymap(KeymapFormat::XkbV1 as u32, fd.as_fd(), size);
        self.has_keymap = true;
        tracing::info!(bytes = size, "virtual keyboard keymap set");
        Ok(())
    }

    pub fn physical_key(&mut self, time: u32, code: u32, pressed: bool) {
        use xkbcommon::xkb;
        self.key(time, code, pressed);
        if let Some(state) = self.state.as_mut() {
            state.update_key(
                xkb::Keycode::new(code + 8),
                if pressed {
                    xkb::KeyDirection::Down
                } else {
                    xkb::KeyDirection::Up
                },
            );
            self.object.modifiers(
                state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
                state.serialize_mods(xkb::STATE_MODS_LATCHED),
                state.serialize_mods(xkb::STATE_MODS_LOCKED),
                state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
            );
        }
    }

    pub fn has_keymap(&self) -> bool {
        self.has_keymap
    }

    pub fn key(&self, time: u32, code: u32, pressed: bool) {
        if !self.has_keymap {
            tracing::warn!(code, "dropping key: virtual keyboard has no keymap yet");
            return;
        }
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        self.object.key(time, code, state as u32);
    }

    pub fn modifiers(&self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if !self.has_keymap {
            tracing::warn!("dropping modifiers: virtual keyboard has no keymap yet");
            return;
        }
        self.object.modifiers(depressed, latched, locked, group);
    }
}
