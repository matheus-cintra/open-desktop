use std::sync::Arc;

use calloop::RegistrationToken;
use smithay_client_toolkit::data_device_manager::data_source::DragSource;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::Buffer;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::error::WaylandError;
use crate::shm::{argb_buffer, transparent_buffer};
use crate::state::State;

const NAMESPACE: &str = "opendesk-drop";
const ICON_ALPHA: u8 = 0xFF;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropPhase {
    AwaitingEnter,
    AwaitingSerial,
    Dragging,
}

pub struct DropDrag {
    pub uris: Arc<Vec<u8>>,
    pub overlay: Option<LayerSurface>,
    pub overlay_buffer: Option<Buffer>,
    pub icon: WlSurface,
    pub source: Option<DragSource>,
    pub phase: DropPhase,
    pub generation: u64,
    pub timeout: Option<RegistrationToken>,
}

impl DropDrag {
    fn owns_overlay(&self, layer: &LayerSurface) -> bool {
        self.overlay
            .as_ref()
            .is_some_and(|overlay| overlay == layer)
    }

    pub fn overlay_surface(&self) -> Option<&WlSurface> {
        self.overlay.as_ref().map(WaylandSurface::wl_surface)
    }
}

impl State {
    pub fn create_drop_overlay(
        &mut self,
        queue_handle: &QueueHandle<State>,
        uris: Arc<Vec<u8>>,
        generation: u64,
    ) -> Result<DropDrag, WaylandError> {
        let (icon_buffer, canvas) = argb_buffer(&mut self.pool, 1, 1)?;
        canvas.fill(ICON_ALPHA);
        let icon = self.compositor.create_surface(queue_handle);
        icon_buffer.attach_to(&icon)?;
        icon.damage_buffer(0, 0, 1, 1);
        icon.commit();

        let surface = self.compositor.create_surface(queue_handle);
        surface.set_input_region(None);
        let overlay = self.layer_shell.create_layer_surface(
            queue_handle,
            surface,
            Layer::Overlay,
            Some(NAMESPACE),
            None,
        );
        overlay.set_anchor(Anchor::all());
        overlay.set_size(0, 0);
        overlay.set_exclusive_zone(-1);
        overlay.set_keyboard_interactivity(KeyboardInteractivity::None);
        overlay.commit();

        Ok(DropDrag {
            uris,
            overlay: Some(overlay),
            overlay_buffer: None,
            icon,
            source: None,
            phase: DropPhase::AwaitingEnter,
            generation,
            timeout: None,
        })
    }

    pub fn configure_drop_overlay(
        &mut self,
        layer: &LayerSurface,
        configure: &LayerSurfaceConfigure,
    ) -> bool {
        let owns = self
            .dnd
            .drop
            .as_ref()
            .is_some_and(|drag| drag.owns_overlay(layer));
        if !owns {
            return false;
        }
        let width = configure.new_size.0.max(1);
        let height = configure.new_size.1.max(1);
        match transparent_buffer(&mut self.pool, width, height) {
            Ok(buffer) => self.attach_overlay_buffer(buffer),
            Err(error) => tracing::error!(%error, "failed to paint the drop overlay"),
        }
        true
    }

    fn attach_overlay_buffer(&mut self, buffer: Buffer) {
        let Some(drag) = self.dnd.drop.as_mut() else {
            return;
        };
        let Some(overlay) = drag.overlay.as_ref() else {
            return;
        };
        let surface = overlay.wl_surface();
        match buffer.attach_to(surface) {
            Ok(()) => {
                surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
                overlay.commit();
                drag.overlay_buffer = Some(buffer);
            }
            Err(error) => tracing::error!(%error, "failed to attach the drop overlay buffer"),
        }
    }

    pub fn drop_overlay_closed(&mut self, layer: &LayerSurface) -> bool {
        let owns = self
            .dnd
            .drop
            .as_ref()
            .is_some_and(|drag| drag.owns_overlay(layer));
        if owns {
            tracing::warn!("drop overlay closed by the compositor");
            self.cancel_drop_drag();
        }
        owns
    }
}
