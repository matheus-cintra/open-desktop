use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::protocol::wl_keyboard::{self, KeyState, KeymapFormat, WlKeyboard};
use wayland_client::{Connection, QueueHandle, WEnum};

use crate::events::WaylandEvent;
use crate::keymap::read_keymap;
use crate::state::State;

pub struct KeyboardData;

impl Dispatch2<WlKeyboard, State> for KeyboardData {
    fn event(
        &self,
        state: &mut State,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if format != WEnum::Value(KeymapFormat::XkbV1) {
                    tracing::warn!(?format, "unsupported keymap format");
                    return;
                }
                match read_keymap(&fd, size) {
                    Ok(xkb) => state.keymap_changed(xkb),
                    Err(error) => tracing::error!(%error, "failed to read the seat keymap"),
                }
            }
            wl_keyboard::Event::Enter { surface, .. } => {
                let focused = state.strips.index_of_surface(&surface).is_some();
                tracing::debug!(focused, "keyboard focus entered");
                state.drag_focus_entered(&surface);
            }
            wl_keyboard::Event::Leave { .. } => {
                tracing::debug!("keyboard focus left");
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = key_state == WEnum::Value(KeyState::Pressed);
                state.key_event(key, pressed);
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                state
                    .hotkey
                    .update_modifiers(mods_depressed, mods_latched, mods_locked, group);
                if state.grab.active {
                    state.emit(WaylandEvent::Modifiers {
                        depressed: mods_depressed,
                        latched: mods_latched,
                        locked: mods_locked,
                        group,
                    });
                }
            }
            _ => {}
        }
    }
}

impl State {
    fn keymap_changed(&mut self, xkb: String) {
        if let Err(error) = self.hotkey.set_keymap(&xkb) {
            tracing::error!(%error, "hotkey matcher could not compile the seat keymap");
        }
        tracing::info!(bytes = xkb.len(), "seat keymap received");
        self.seat_keymap = Some(xkb.clone());
        self.emit(WaylandEvent::Keymap { xkb });
    }

    fn key_event(&mut self, code: u32, pressed: bool) {
        if !self.grab.active {
            return;
        }
        if pressed && self.hotkey.consume_press(code) {
            tracing::info!(code, "release hotkey pressed");
            self.emit(WaylandEvent::HotkeyPressed);
            return;
        }
        if !pressed && self.hotkey.consume_release(code) {
            return;
        }
        self.emit(WaylandEvent::Key { code, pressed });
    }
}
