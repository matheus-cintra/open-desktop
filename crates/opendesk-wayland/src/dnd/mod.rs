mod device;
mod overlay;
mod peek;
mod release;
mod source;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use smithay_client_toolkit::data_device_manager::DataDeviceManagerState;
use smithay_client_toolkit::data_device_manager::data_device::DataDevice;
use wayland_client::QueueHandle;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_seat::WlSeat;
use xkbcommon::xkb;

use crate::dnd::overlay::DropDrag;
use crate::error::WaylandError;
use crate::state::State;

const BTN_LEFT: u32 = 0x110;
const KEY_ESCAPE: u32 = 1;
const DROP_DRAG_FOCUS_TIMEOUT: Duration = Duration::from_secs(15);
const DROP_DRAG_ACTIVE_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Dnd {
    manager: Option<DataDeviceManagerState>,
    device: Option<DataDevice>,
    pub(crate) entered_strip: Option<usize>,
    drop: Option<DropDrag>,
    generation: u64,
    drag_generation: Arc<AtomicU64>,
}

impl Dnd {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<State>,
        drag_generation: Arc<AtomicU64>,
    ) -> Dnd {
        let manager = match DataDeviceManagerState::bind(globals, queue_handle) {
            Ok(manager) => {
                tracing::info!("bound wl_data_device_manager");
                Some(manager)
            }
            Err(error) => {
                tracing::warn!(%error, "no wl_data_device_manager is available");
                None
            }
        };
        Dnd {
            manager,
            device: None,
            entered_strip: None,
            drop: None,
            generation: 0,
            drag_generation,
        }
    }
}

impl State {
    pub fn ensure_data_device(&mut self, queue_handle: &QueueHandle<State>, seat: &WlSeat) {
        if self.dnd.device.is_some() {
            return;
        }
        let Some(manager) = self.dnd.manager.as_ref() else {
            return;
        };
        self.dnd.device = Some(manager.get_data_device(queue_handle, seat));
        tracing::info!("wl_data_device created for the seat");
    }

    pub fn dnd_seat_gone(&mut self) {
        self.invalidate_drag_generation();
        self.cancel_drop_drag();
        self.dnd.device = None;
        self.dnd.entered_strip = None;
    }

    pub fn dnd_shutdown(&mut self) {
        self.invalidate_drag_generation();
        self.cancel_drop_drag();
        self.dnd.device = None;
        self.dnd.entered_strip = None;
    }

    pub fn abort_local_drag(&mut self) -> Result<(), WaylandError> {
        let time = self.elapsed_millis();
        let fallback = self.seat_keymap.clone();
        let keyboard = self
            .emulator
            .keyboard
            .as_mut()
            .ok_or(WaylandError::NoKeyboard)?;
        if !keyboard.has_keymap() {
            let xkb_text = match fallback {
                Some(text) => text,
                None => default_keymap_text()?,
            };
            keyboard.set_keymap(&xkb_text)?;
        }
        keyboard.key(time, KEY_ESCAPE, true);
        keyboard.key(time, KEY_ESCAPE, false);
        tracing::info!("injected Escape to abort the local drag");
        Ok(())
    }

    pub(crate) fn invalidate_drag_generation(&mut self) -> u64 {
        let generation = self.dnd.drag_generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.emit(crate::events::WaylandEvent::DragGeneration { generation });
        generation
    }
}

fn default_keymap_text() -> Result<String, WaylandError> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",
        "",
        "us",
        "",
        None,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or(WaylandError::KeymapCompile)?;
    Ok(keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1))
}
