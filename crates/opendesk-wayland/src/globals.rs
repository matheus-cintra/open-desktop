use std::ops::RangeInclusive;

use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_manager_v1::WpCursorShapeManagerV1;
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::ZwpPointerConstraintsV1;
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;

use crate::error::WaylandError;
use crate::state::State;

pub struct Managers {
    pub pointer_constraints: ZwpPointerConstraintsV1,
    pub relative_pointer: ZwpRelativePointerManagerV1,
    pub shortcuts_inhibit: ZwpKeyboardShortcutsInhibitManagerV1,
    pub cursor_shape: WpCursorShapeManagerV1,
    pub virtual_pointer: ZwlrVirtualPointerManagerV1,
    pub virtual_keyboard: ZwpVirtualKeyboardManagerV1,
}

impl Managers {
    pub fn bind(
        globals: &GlobalList,
        queue_handle: &QueueHandle<State>,
    ) -> Result<Managers, WaylandError> {
        let managers = Managers {
            pointer_constraints: bind(globals, queue_handle, 1..=1)?,
            relative_pointer: bind(globals, queue_handle, 1..=1)?,
            shortcuts_inhibit: bind(globals, queue_handle, 1..=1)?,
            cursor_shape: bind(globals, queue_handle, 1..=1)?,
            virtual_pointer: bind(globals, queue_handle, 1..=2)?,
            virtual_keyboard: bind(globals, queue_handle, 1..=1)?,
        };
        tracing::debug!(
            virtual_pointer_version = managers.virtual_pointer.version(),
            "bound raw protocol managers"
        );
        Ok(managers)
    }
}

fn bind<I>(
    globals: &GlobalList,
    queue_handle: &QueueHandle<State>,
    versions: RangeInclusive<u32>,
) -> Result<I, WaylandError>
where
    I: Proxy + 'static,
    State: Dispatch<I, NoEvents>,
{
    globals
        .bind(queue_handle, versions, NoEvents)
        .map_err(|source| WaylandError::MissingGlobal {
            interface: I::interface().name,
            source,
        })
}

pub struct NoEvents;

impl<I: Proxy> Dispatch2<I, State> for NoEvents {
    fn event(
        &self,
        _: &mut State,
        _: &I,
        _: <I as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        tracing::trace!(interface = I::interface().name, "ignored event");
    }
}

delegate_registry!(State);
delegate_dispatch2!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<State>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<State>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<State>, _: &wl_surface::WlSurface, _: u32) {
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<State>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<State>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
