use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::{self, ZwpLockedPointerV1};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Lifetime;
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::{self, ZwpRelativePointerV1};

use crate::error::WaylandError;
use crate::events::WaylandEvent;
use crate::state::State;

#[derive(Default)]
pub struct Grab {
    lock: Option<Lock>,
    relative_pointer: Option<ZwpRelativePointerV1>,
    inhibitor: Option<ZwpKeyboardShortcutsInhibitorV1>,
    exclusive_strip: Option<usize>,
    pub active: bool,
}

struct Lock {
    object: ZwpLockedPointerV1,
    lifetime: Lifetime,
    strip: usize,
}

impl Grab {
    pub fn is_locked(&self) -> bool {
        self.lock.is_some()
    }
}

impl State {
    pub fn lock_pointer(
        &mut self,
        queue_handle: &QueueHandle<State>,
        lifetime: Lifetime,
    ) -> Result<(), WaylandError> {
        let strip_index = self
            .pointer
            .focused_strip
            .ok_or(WaylandError::NoFocusedStrip)?;
        let pointer = self
            .devices
            .pointer
            .clone()
            .ok_or(WaylandError::NoPointer)?;
        if let Some(lock) = self.grab.lock.as_ref() {
            if lock.strip == strip_index && lock.lifetime == lifetime {
                return Ok(());
            }
            self.destroy_lock();
        }
        let strip = self
            .strips
            .get(strip_index)
            .ok_or(WaylandError::NoFocusedStrip)?;
        let object = self.managers.pointer_constraints.lock_pointer(
            strip.layer.wl_surface(),
            &pointer,
            None,
            lifetime,
            queue_handle,
            LockData,
        );
        if self.grab.relative_pointer.is_none() {
            self.grab.relative_pointer = Some(self.managers.relative_pointer.get_relative_pointer(
                &pointer,
                queue_handle,
                RelativePointerData,
            ));
        }
        tracing::info!(strip = strip_index, ?lifetime, "pointer lock requested");
        self.grab.lock = Some(Lock {
            object,
            lifetime,
            strip: strip_index,
        });
        Ok(())
    }

    pub fn unlock_pointer(&mut self, hint: Option<f64>) {
        let Some(lock) = self.grab.lock.as_ref() else {
            return;
        };
        let strip_index = lock.strip;
        if let Some(global) = hint
            && let Some((x, y)) = self
                .strips
                .hint_local(strip_index, &self.output_state, global)
            && let Some(strip) = self.strips.get(strip_index)
        {
            lock.object.set_cursor_position_hint(x, y);
            strip.layer.commit();
            tracing::info!(x, y, "cursor position hint set");
        }
        self.destroy_lock();
        if let Some(global) = hint {
            self.warp_to_edge(strip_index, global);
        }
    }

    fn warp_to_edge(&self, strip_index: usize, global: f64) {
        let Some((x, y)) = self
            .strips
            .hint_global(strip_index, &self.output_state, global)
        else {
            return;
        };
        match self.inject_absolute_motion(x, y) {
            Ok(()) => tracing::info!(x, y, "cursor warped to the edge with the virtual pointer"),
            Err(error) => tracing::error!(%error, "failed to warp the cursor to the edge"),
        }
    }

    fn destroy_lock(&mut self) {
        if let Some(lock) = self.grab.lock.take() {
            lock.object.destroy();
        }
        if let Some(relative_pointer) = self.grab.relative_pointer.take() {
            relative_pointer.destroy();
        }
    }

    pub fn start_grab(&mut self, queue_handle: &QueueHandle<State>) -> Result<(), WaylandError> {
        self.lock_pointer(queue_handle, Lifetime::Persistent)?;
        let strip_index = self
            .pointer
            .focused_strip
            .ok_or(WaylandError::NoFocusedStrip)?;
        let seat = self.devices.seat.clone().ok_or(WaylandError::NoKeyboard)?;
        let strip = self
            .strips
            .get_mut(strip_index)
            .ok_or(WaylandError::NoFocusedStrip)?;
        strip
            .layer
            .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        strip.expand();
        self.grab.exclusive_strip = Some(strip_index);
        if self.grab.inhibitor.is_none() {
            self.grab.inhibitor = Some(self.managers.shortcuts_inhibit.inhibit_shortcuts(
                strip.layer.wl_surface(),
                &seat,
                queue_handle,
                InhibitorData,
            ));
        }
        self.hide_cursor();
        self.grab.active = true;
        tracing::info!(strip = strip_index, "grab started");
        Ok(())
    }

    pub fn stop_grab(&mut self, hint: Option<f64>) {
        self.grab.active = false;
        self.release_keyboard();
        self.unlock_pointer(hint);
        self.show_cursor();
        tracing::info!("grab stopped");
    }

    pub fn release_grab_objects(&mut self) {
        self.grab.active = false;
        self.release_keyboard();
        self.destroy_lock();
    }

    fn release_keyboard(&mut self) {
        if let Some(inhibitor) = self.grab.inhibitor.take() {
            inhibitor.destroy();
        }
        if let Some(index) = self.grab.exclusive_strip.take()
            && let Some(strip) = self.strips.get_mut(index)
        {
            strip
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            strip.restore();
        }
    }
}

pub struct LockData;

impl Dispatch2<ZwpLockedPointerV1, State> for LockData {
    fn event(
        &self,
        _: &mut State,
        _: &ZwpLockedPointerV1,
        event: zwp_locked_pointer_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            zwp_locked_pointer_v1::Event::Locked => tracing::info!("pointer locked"),
            zwp_locked_pointer_v1::Event::Unlocked => tracing::info!("pointer unlocked"),
            _ => {}
        }
    }
}

pub struct RelativePointerData;

impl Dispatch2<ZwpRelativePointerV1, State> for RelativePointerData {
    fn event(
        &self,
        state: &mut State,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event
            && state.grab.is_locked()
        {
            state.emit(WaylandEvent::RelativeMotion { dx, dy });
        }
    }
}

pub struct InhibitorData;

impl Dispatch2<ZwpKeyboardShortcutsInhibitorV1, State> for InhibitorData {
    fn event(
        &self,
        _: &mut State,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => {
                tracing::info!("shortcuts inhibitor active");
            }
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => {
                tracing::info!("shortcuts inhibitor inactive");
            }
            _ => {}
        }
    }
}
