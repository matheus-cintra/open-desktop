use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;

use crate::emulate::keyboard::VirtualKeyboard;
use crate::emulate::pointer::VirtualPointer;
use crate::globals::NoEvents;
use crate::keyboard::KeyboardData;
use crate::pointer::PointerData;
use crate::state::State;

#[derive(Default)]
pub struct Devices {
    pub seat: Option<WlSeat>,
    pub pointer: Option<WlPointer>,
    pub keyboard: Option<WlKeyboard>,
    pub cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pub enter_serial: u32,
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<State>, _: WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        queue_handle: &QueueHandle<State>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if self
            .devices
            .seat
            .as_ref()
            .is_some_and(|known| *known != seat)
        {
            tracing::debug!("ignoring capability on a secondary seat");
            return;
        }
        self.devices.seat = Some(seat.clone());
        self.ensure_clipboard_device(queue_handle, &seat);
        self.ensure_data_device(queue_handle, &seat);
        match capability {
            Capability::Pointer if self.devices.pointer.is_none() => {
                let pointer = seat.get_pointer(queue_handle, PointerData);
                self.devices.cursor_shape_device = Some(self.managers.cursor_shape.get_pointer(
                    &pointer,
                    queue_handle,
                    NoEvents,
                ));
                self.devices.pointer = Some(pointer);
                if self.emulator.pointer.is_none() {
                    self.emulator.pointer = Some(VirtualPointer::new(
                        &self.managers.virtual_pointer,
                        &seat,
                        queue_handle,
                    ));
                }
                tracing::info!("seat pointer ready");
            }
            Capability::Keyboard if self.devices.keyboard.is_none() => {
                self.devices.keyboard = Some(seat.get_keyboard(queue_handle, KeyboardData));
                if self.emulator.keyboard.is_none() {
                    self.emulator.keyboard = Some(VirtualKeyboard::new(
                        &self.managers.virtual_keyboard,
                        &seat,
                        queue_handle,
                    ));
                }
                tracing::info!("seat keyboard ready");
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<State>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if self.devices.seat.as_ref() != Some(&seat) {
            return;
        }
        match capability {
            Capability::Pointer => {
                if let Some(device) = self.devices.cursor_shape_device.take() {
                    device.destroy();
                }
                if let Some(pointer) = self.devices.pointer.take() {
                    pointer.release();
                }
                self.pointer.focused_strip = None;
            }
            Capability::Keyboard => {
                if let Some(keyboard) = self.devices.keyboard.take() {
                    keyboard.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<State>, seat: WlSeat) {
        if self.devices.seat.as_ref() == Some(&seat) {
            self.devices = Devices::default();
            self.dnd_seat_gone();
        }
    }
}
