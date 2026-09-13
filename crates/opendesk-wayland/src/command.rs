use opendesk_proto::input::{Axis, AxisSource};
use wayland_client::QueueHandle;
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::Lifetime;

use crate::error::WaylandError;
use crate::events::WaylandCommand;
use crate::outputs::{layout_bounds, output_geometries};
use crate::state::State;

impl State {
    pub fn handle_command(&mut self, queue_handle: &QueueHandle<State>, command: WaylandCommand) {
        tracing::trace!(?command, "wayland command");
        let result = match command {
            WaylandCommand::ConfigureStrips { strips } => {
                self.configure_strips(queue_handle, strips)
            }
            WaylandCommand::SetReleaseHotkey { hotkey } => {
                self.hotkey.set_spec(hotkey);
                Ok(())
            }
            WaylandCommand::LockPointer => self.lock_pointer(queue_handle, Lifetime::Oneshot),
            WaylandCommand::UnlockPointer { hint } => {
                self.unlock_pointer(hint);
                Ok(())
            }
            WaylandCommand::StartGrab => self.start_grab(queue_handle),
            WaylandCommand::PrepareDragFocus { id } => self.prepare_drag_focus(queue_handle, id),
            WaylandCommand::CancelDragFocus { id } => {
                self.cancel_drag_focus(id);
                Ok(())
            }
            WaylandCommand::StopGrab { hint } => {
                self.stop_grab(hint);
                Ok(())
            }
            WaylandCommand::SetKeymap { xkb } => self.set_virtual_keymap(&xkb),
            WaylandCommand::SetBarStyle { style } => self.set_bar_style(style),
            WaylandCommand::SetClipboard { content } => self.set_clipboard(queue_handle, content),
            WaylandCommand::ShowProgressBar {
                side,
                position,
                progress,
            } => self.show_progress_bar(queue_handle, side, position, progress),
            WaylandCommand::HideProgressBar => {
                self.hide_progress_bar();
                Ok(())
            }
            WaylandCommand::ShowArrivalBar { side, position } => {
                self.show_arrival_bar(queue_handle, side, position)
            }
            WaylandCommand::InjectAbsoluteMotion { x, y } => self.inject_absolute_motion(x, y),
            WaylandCommand::InjectMotion { dx, dy } => {
                let time = self.elapsed_millis();
                self.virtual_pointer()
                    .map(|pointer| pointer.motion(time, dx, dy))
            }
            WaylandCommand::InjectButton { code, pressed } => {
                let time = self.elapsed_millis();
                self.virtual_pointer()
                    .map(|pointer| pointer.button(time, code, pressed))
            }
            WaylandCommand::InjectAxis {
                axis,
                value,
                value120,
                source,
            } => self.inject_axis(axis, value, value120, source),
            WaylandCommand::InjectPhysicalKey { code, pressed } => {
                let time = self.elapsed_millis();
                self.emulator
                    .keyboard
                    .as_mut()
                    .ok_or(WaylandError::NoKeyboard)
                    .map(|keyboard| keyboard.physical_key(time, code, pressed))
            }
            WaylandCommand::InjectKey { code, pressed } => {
                let time = self.elapsed_millis();
                self.virtual_keyboard()
                    .map(|keyboard| keyboard.key(time, code, pressed))
            }
            WaylandCommand::InjectModifiers {
                depressed,
                latched,
                locked,
                group,
            } => self
                .virtual_keyboard()
                .map(|keyboard| keyboard.modifiers(depressed, latched, locked, group)),
            WaylandCommand::AbortLocalDrag => self.abort_local_drag(),
            WaylandCommand::StartDropDrag { id, uris } => {
                self.start_drop_drag(queue_handle, id, uris)
            }
            WaylandCommand::ReleaseDropDrag { id } => {
                self.request_drop_release(id);
                Ok(())
            }
            WaylandCommand::CancelDropDrag => {
                self.cancel_drop_drag();
                Ok(())
            }
            WaylandCommand::Shutdown => {
                tracing::info!("shutdown requested");
                self.clipboard_shutdown();
                self.dnd_shutdown();
                self.release_grab_objects();
                self.strips.clear();
                self.bars.clear();
                self.loop_signal.stop();
                Ok(())
            }
        };
        if let Err(error) = result {
            tracing::error!(%error, "wayland command failed");
        }
    }

    fn virtual_pointer(&self) -> Result<&crate::emulate::pointer::VirtualPointer, WaylandError> {
        self.emulator
            .pointer
            .as_ref()
            .ok_or(WaylandError::NoPointer)
    }

    fn virtual_keyboard(&self) -> Result<&crate::emulate::keyboard::VirtualKeyboard, WaylandError> {
        self.emulator
            .keyboard
            .as_ref()
            .ok_or(WaylandError::NoKeyboard)
    }

    fn set_virtual_keymap(&mut self, xkb: &str) -> Result<(), WaylandError> {
        self.emulator
            .keyboard
            .as_mut()
            .ok_or(WaylandError::NoKeyboard)?
            .set_keymap(xkb)
    }

    pub fn inject_absolute_motion(&self, x: f64, y: f64) -> Result<(), WaylandError> {
        let outputs = output_geometries(&self.output_state);
        let bounds = layout_bounds(&outputs).ok_or(WaylandError::NoPointer)?;
        let time = self.elapsed_millis();
        self.virtual_pointer()?.absolute_motion(time, x, y, bounds);
        Ok(())
    }

    fn inject_axis(
        &self,
        axis: Axis,
        value: f64,
        value120: i32,
        source: AxisSource,
    ) -> Result<(), WaylandError> {
        let time = self.elapsed_millis();
        self.virtual_pointer()?
            .axis(time, axis, value, value120, source);
        Ok(())
    }
}
