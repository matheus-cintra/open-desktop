use std::time::Instant;

use smithay_client_toolkit::compositor::{FrameCallbackData, Region};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{KeyboardInteractivity, Layer, LayerSurface};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::bar::fade::Fade;
use crate::bar::geometry::BarPlacement;
use crate::bar::paint::paint_bar;
use crate::error::WaylandError;
use crate::events::BarStyle;
use crate::state::State;

const NAMESPACE: &str = "opendesk-bar";

pub struct Bar {
    pub layer: LayerSurface,
    pub output: WlOutput,
    pub placement: BarPlacement,
    pub fade: Option<Fade>,
    pub configured: bool,
    buffer: Option<Buffer>,
}

impl Bar {
    pub fn matches_layer(&self, layer: &LayerSurface) -> bool {
        self.layer == *layer
    }

    pub fn matches_surface(&self, surface: &WlSurface) -> bool {
        self.layer.wl_surface() == surface
    }

    pub fn apply_placement(&self) {
        let placement = self.placement;
        self.layer.set_anchor(placement.anchor);
        self.layer.set_size(placement.width, placement.height);
        self.layer
            .set_margin(placement.margin_top, 0, 0, placement.margin_left);
    }

    pub fn paint(
        &mut self,
        pool: &mut SlotPool,
        style: BarStyle,
        now: Instant,
    ) -> Result<(), WaylandError> {
        let alpha_multiplier = self.fade.map_or(1.0, |fade| fade.alpha_multiplier(now));
        let (width, height) = self.placement.size();
        let pixels = paint_bar(width, height, style, alpha_multiplier)?;
        let (buffer, canvas) = crate::shm::argb_buffer(pool, width, height)?;
        canvas
            .get_mut(..pixels.len())
            .ok_or(WaylandError::Paint("canvas smaller than the pixmap"))?
            .copy_from_slice(&pixels);
        let surface = self.layer.wl_surface();
        buffer.attach_to(surface)?;
        surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        self.buffer = Some(buffer);
        Ok(())
    }

    pub fn request_frame(&self, queue_handle: &QueueHandle<State>) {
        let surface = self.layer.wl_surface();
        surface.frame(queue_handle, FrameCallbackData(surface.clone()));
    }

    pub fn commit(&self) {
        self.layer.commit();
    }
}

impl State {
    pub fn create_bar(
        &self,
        queue_handle: &QueueHandle<State>,
        output: WlOutput,
        placement: BarPlacement,
        fade: Option<Fade>,
    ) -> Result<Bar, WaylandError> {
        let surface = self.compositor.create_surface(queue_handle);
        let empty_input = Region::new(&self.compositor)?;
        surface.set_input_region(Some(empty_input.wl_region()));
        let layer = self.layer_shell.create_layer_surface(
            queue_handle,
            surface,
            Layer::Overlay,
            Some(NAMESPACE),
            Some(&output),
        );
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        let bar = Bar {
            layer,
            output,
            placement,
            fade,
            configured: false,
            buffer: None,
        };
        bar.apply_placement();
        bar.commit();
        Ok(bar)
    }
}
