use calloop::timer::{TimeoutAction, Timer};

use crate::dnd::BTN_LEFT;
use crate::dnd::overlay::DropPhase;
use crate::state::State;

impl State {
    pub fn request_drop_release(&mut self, id: u64) {
        let should_arm = match self.dnd.drop.as_mut() {
            Some(drag) if drag.id == id => {
                drag.release_requested = true;
                drag.phase == DropPhase::Dragging
                    && drag.synthetic_button_down
                    && drag.release_timer.is_none()
            }
            _ => false,
        };
        if !should_arm {
            return;
        }
        let timer = Timer::from_duration(std::time::Duration::from_millis(150));
        match self
            .loop_handle
            .insert_source(timer, move |_, _, state: &mut State| {
                state.release_drop_button(id);
                TimeoutAction::Drop
            }) {
            Ok(token) => {
                if let Some(drag) = self.dnd.drop.as_mut() {
                    drag.release_timer = Some(token);
                }
            }
            Err(error) => {
                tracing::error!(error = %error.error, "failed to defer the drop button release");
                self.release_drop_button(id);
            }
        }
    }

    fn release_drop_button(&mut self, id: u64) {
        let Some(drag) = self.dnd.drop.as_mut() else {
            return;
        };
        if drag.id != id || drag.phase != DropPhase::Dragging || !drag.synthetic_button_down {
            return;
        }
        drag.synthetic_button_down = false;
        drag.release_timer = None;
        if let Some(pointer) = self.emulator.pointer.as_ref() {
            pointer.button(self.elapsed_millis(), BTN_LEFT, false);
            tracing::info!(id, "released the drop drag button after user release");
        }
    }
}
