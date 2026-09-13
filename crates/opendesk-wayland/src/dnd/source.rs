use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::Arc;

use calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::data_device_manager::WritePipe;
use smithay_client_toolkit::data_device_manager::data_source::DataSourceHandler;
use smithay_client_toolkit::shell::WaylandSurface;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::{Connection, QueueHandle};

use crate::dnd::overlay::DropPhase;
use crate::dnd::peek::{URI_LIST_MIME, render_uri_list};
use crate::dnd::{BTN_LEFT, DROP_DRAG_ACTIVE_TIMEOUT, DROP_DRAG_FOCUS_TIMEOUT};
use crate::error::WaylandError;
use crate::events::WaylandEvent;
use crate::state::State;

impl State {
    pub fn start_drop_drag(
        &mut self,
        queue_handle: &QueueHandle<State>,
        id: u64,
        uris: Vec<PathBuf>,
    ) -> Result<(), WaylandError> {
        if self.dnd.manager.is_none() {
            return Err(WaylandError::NoDataDeviceManager);
        }
        if self.dnd.device.is_none() {
            return Err(WaylandError::NoDataDevice);
        }
        if uris.is_empty() {
            return Err(WaylandError::NoDragUris);
        }
        let bytes = Arc::new(render_uri_list(&uris));
        self.cancel_drop_drag();
        self.dnd.generation = self.dnd.generation.wrapping_add(1);
        let generation = self.dnd.generation;
        let mut drag = self.create_drop_overlay(queue_handle, id, bytes, generation)?;
        drag.timeout = self.arm_drop_timeout(generation, DROP_DRAG_FOCUS_TIMEOUT);
        tracing::info!(
            uris = uris.len(),
            "drop drag armed, waiting for overlay focus"
        );
        self.dnd.drop = Some(drag);
        Ok(())
    }

    fn arm_drop_timeout(
        &self,
        generation: u64,
        duration: std::time::Duration,
    ) -> Option<calloop::RegistrationToken> {
        let timer = Timer::from_duration(duration);
        match self
            .loop_handle
            .insert_source(timer, move |_, _, state: &mut State| {
                state.drop_drag_timeout(generation);
                TimeoutAction::Drop
            }) {
            Ok(token) => Some(token),
            Err(error) => {
                tracing::error!(error = %error.error, "failed to arm the drop-drag timeout");
                None
            }
        }
    }

    fn drop_drag_timeout(&mut self, generation: u64) {
        let stuck = self
            .dnd
            .drop
            .as_ref()
            .is_some_and(|drag| drag.generation == generation);
        if !stuck {
            return;
        }
        tracing::warn!("drop drag timed out, giving up");
        if let Some(drag) = self.dnd.drop.as_mut() {
            drag.timeout = None;
        }
        self.cancel_drop_drag();
    }

    pub fn drop_drag_pointer_entered(
        &mut self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        _queue_handle: &QueueHandle<State>,
    ) {
        let armed = self.dnd.drop.as_ref().is_some_and(|drag| {
            drag.phase == DropPhase::AwaitingEnter && drag.overlay_surface() == Some(surface)
        });
        if !armed {
            return;
        }
        tracing::debug!("drop overlay focused, injecting the capture button press");
        let time = self.elapsed_millis();
        let Some(pointer) = self.emulator.pointer.as_ref() else {
            tracing::error!("no virtual pointer to capture a drag serial");
            self.cancel_drop_drag();
            return;
        };
        pointer.button(time, BTN_LEFT, true);
        if let Some(drag) = self.dnd.drop.as_mut() {
            drag.phase = DropPhase::AwaitingSerial;
            drag.synthetic_button_down = true;
        }
    }

    pub fn drop_drag_pointer_button(
        &mut self,
        serial: u32,
        button: u32,
        pressed: bool,
        queue_handle: &QueueHandle<State>,
    ) {
        if button != BTN_LEFT || !pressed {
            return;
        }
        let ready = self
            .dnd
            .drop
            .as_ref()
            .is_some_and(|drag| drag.phase == DropPhase::AwaitingSerial);
        if !ready {
            return;
        }
        if let Err(error) = self.begin_drop_drag(serial, queue_handle) {
            tracing::error!(%error, "failed to start the drop drag");
            self.cancel_drop_drag();
        }
    }

    fn begin_drop_drag(
        &mut self,
        serial: u32,
        queue_handle: &QueueHandle<State>,
    ) -> Result<(), WaylandError> {
        let manager = self
            .dnd
            .manager
            .as_ref()
            .ok_or(WaylandError::NoDataDeviceManager)?;
        let device = self.dnd.device.as_ref().ok_or(WaylandError::NoDataDevice)?;
        let drag = self.dnd.drop.as_ref().ok_or(WaylandError::NoDropDrag)?;
        let id = drag.id;
        let generation = drag.generation;
        let overlay = drag
            .overlay
            .as_ref()
            .ok_or(WaylandError::NoDropDrag)?
            .wl_surface()
            .clone();
        let icon = drag.icon.clone();
        let source =
            manager.create_drag_and_drop_source(queue_handle, [URI_LIST_MIME], DndAction::Copy);
        source.start_drag(device, &overlay, Some(&icon), serial);
        tracing::info!(serial, "drop drag started");
        if let Some(drag) = self.dnd.drop.as_mut() {
            drag.source = Some(source);
            drag.phase = DropPhase::Dragging;
            if let Some(token) = drag.timeout.take() {
                self.loop_handle.remove(token);
            }
            if let Some(overlay) = drag.overlay.take() {
                overlay.wl_surface().attach(None, 0, 0);
                overlay.wl_surface().commit();
            }
            drag.overlay_buffer = None;
        }
        let timeout = self.arm_drop_timeout(generation, DROP_DRAG_ACTIVE_TIMEOUT);
        if let Some(drag) = self.dnd.drop.as_mut() {
            drag.timeout = timeout;
        }
        if self
            .dnd
            .drop
            .as_ref()
            .is_some_and(|drag| drag.release_requested)
        {
            self.request_drop_release(id);
        }
        Ok(())
    }

    pub fn cancel_drop_drag(&mut self) {
        self.finish_drop_drag(false);
    }

    fn finish_drop_drag(&mut self, accepted: bool) {
        if let Some(drag) = self.dnd.drop.take() {
            if let Some(token) = drag.timeout {
                self.loop_handle.remove(token);
            }
            if let Some(token) = drag.release_timer {
                self.loop_handle.remove(token);
            }
            if drag.synthetic_button_down
                && let Some(pointer) = self.emulator.pointer.as_ref()
            {
                pointer.button(self.elapsed_millis(), BTN_LEFT, false);
            }
            drag.icon.destroy();
            self.emit(WaylandEvent::DropDragEnded {
                id: drag.id,
                accepted,
            });
            tracing::debug!(accepted, "drop drag ended");
        }
    }
}

fn serve_uri_list(write_pipe: WritePipe, bytes: Arc<Vec<u8>>) {
    let fd = OwnedFd::from(write_pipe);
    std::thread::spawn(move || {
        let mut file = File::from(fd);
        if let Err(error) = file.write_all(&bytes) {
            tracing::debug!(%error, "drop-drag send could not write all the bytes");
        }
    });
}

impl DataSourceHandler for State {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        mime: Option<String>,
    ) {
        tracing::trace!(?mime, "drop target accepted a mime type");
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        mime: String,
        write_pipe: WritePipe,
    ) {
        match self.dnd.drop.as_ref().map(|drag| drag.uris.clone()) {
            Some(bytes) => {
                tracing::info!(%mime, "serving the drop-drag uri list");
                serve_uri_list(write_pipe, bytes);
            }
            None => tracing::debug!("send_request arrived with no active drop drag"),
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        tracing::info!("drop drag source cancelled by the compositor");
        self.cancel_drop_drag();
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        tracing::info!("drop drag source dropped");
    }

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        tracing::info!("drop drag source finished");
        self.finish_drop_drag(true);
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        action: DndAction,
    ) {
        tracing::trace!(?action, "drop drag action");
    }
}
