use opendesk_proto::control::{OutputGeometry, Side};
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::Buffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, QueueHandle};

use crate::edge::{THICKNESS, along_edge_global, along_edge_local, edge_point};
use crate::error::WaylandError;
use crate::events::{StripSpec, WaylandEvent};
use crate::outputs::{find_output, output_geometry, output_origin};
use crate::shm::transparent_buffer;
use crate::state::State;

const NAMESPACE: &str = "opendesk-edge";

pub struct Strip {
    pub spec: StripSpec,
    pub layer: LayerSurface,
    pub output: WlOutput,
    buffer: Option<Buffer>,
    expanded: bool,
}

impl Strip {
    pub fn expand(&mut self) {
        self.expanded = true;
        self.layer.set_anchor(Anchor::all());
        self.layer.set_size(0, 0);
        self.layer.commit();
    }

    pub fn restore(&mut self) {
        self.expanded = false;
        self.layer.set_anchor(anchor_for(self.spec.side));
        let (width, height) = requested_size(self.spec.side);
        self.layer.set_size(width, height);
        self.layer.commit();
    }

    fn requested_size(&self) -> (u32, u32) {
        if self.expanded {
            (0, 0)
        } else {
            requested_size(self.spec.side)
        }
    }
}

#[derive(Default)]
pub struct Strips {
    items: Vec<Strip>,
}

impl Strips {
    pub fn get(&self, index: usize) -> Option<&Strip> {
        self.items.get(index)
    }

    pub fn index_of_surface(&self, surface: &WlSurface) -> Option<usize> {
        self.items
            .iter()
            .position(|strip| strip.layer.wl_surface() == surface)
    }

    pub fn index_of_layer(&self, layer: &LayerSurface) -> Option<usize> {
        self.items.iter().position(|strip| strip.layer == *layer)
    }

    pub fn entered(
        &self,
        index: usize,
        output_state: &OutputState,
        local: (f64, f64),
    ) -> Option<WaylandEvent> {
        let strip = self.items.get(index)?;
        let origin = output_origin(output_state, &strip.output);
        Some(WaylandEvent::EdgeEntered {
            side: strip.spec.side,
            output: strip.spec.output.clone(),
            position: along_edge_global(strip.spec.side, origin, local),
        })
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Strip> {
        self.items.get_mut(index)
    }

    pub fn hint_local(
        &self,
        index: usize,
        output_state: &OutputState,
        global: f64,
    ) -> Option<(f64, f64)> {
        let strip = self.items.get(index)?;
        let origin = output_origin(output_state, &strip.output);
        Some(along_edge_local(strip.spec.side, origin, global))
    }

    pub fn hint_global(
        &self,
        index: usize,
        output_state: &OutputState,
        global: f64,
    ) -> Option<(f64, f64)> {
        let strip = self.items.get(index)?;
        let geometry: OutputGeometry = output_geometry(output_state, &strip.output)?;
        Some(edge_point(strip.spec.side, &geometry, global))
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

fn anchor_for(side: Side) -> Anchor {
    match side {
        Side::Left => Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
        Side::Right => Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
        Side::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        Side::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
    }
}

fn requested_size(side: Side) -> (u32, u32) {
    if side.is_horizontal() {
        (THICKNESS, 0)
    } else {
        (0, THICKNESS)
    }
}

impl State {
    pub fn configure_strips(
        &mut self,
        queue_handle: &QueueHandle<State>,
        specs: Vec<StripSpec>,
    ) -> Result<(), WaylandError> {
        self.release_grab_objects();
        self.pointer.focused_strip = None;
        self.strips.clear();
        for spec in specs {
            self.create_strip(queue_handle, spec)?;
        }
        Ok(())
    }

    fn create_strip(
        &mut self,
        queue_handle: &QueueHandle<State>,
        spec: StripSpec,
    ) -> Result<(), WaylandError> {
        let output = find_output(&self.output_state, &spec.output)
            .ok_or_else(|| WaylandError::UnknownOutput(spec.output.clone()))?;
        let surface = self.compositor.create_surface(queue_handle);
        let layer = self.layer_shell.create_layer_surface(
            queue_handle,
            surface,
            Layer::Overlay,
            Some(NAMESPACE),
            Some(&output),
        );
        layer.set_anchor(anchor_for(spec.side));
        let (width, height) = requested_size(spec.side);
        layer.set_size(width, height);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        tracing::info!(side = %spec.side, output = %spec.output, "edge strip created");
        self.strips.items.push(Strip {
            spec,
            layer,
            output,
            buffer: None,
            expanded: false,
        });
        Ok(())
    }

    fn draw_strip(&mut self, index: usize, size: (u32, u32)) -> Result<(), WaylandError> {
        let buffer = transparent_buffer(&mut self.pool, size.0, size.1)?;
        let Some(strip) = self.strips.items.get_mut(index) else {
            return Ok(());
        };
        let surface = strip.layer.wl_surface();
        buffer
            .attach_to(surface)
            .map_err(|_| WaylandError::NoFocusedStrip)?;
        surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        strip.layer.commit();
        strip.buffer = Some(buffer);
        Ok(())
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<State>, layer: &LayerSurface) {
        if let Some(index) = self.strips.index_of_layer(layer) {
            tracing::warn!(index, "edge strip closed by the compositor");
            if self.pointer.focused_strip == Some(index) {
                self.release_grab_objects();
                self.pointer.focused_strip = None;
            }
            self.strips.items.remove(index);
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<State>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(index) = self.strips.index_of_layer(layer) else {
            return;
        };
        let requested = self
            .strips
            .get(index)
            .map(Strip::requested_size)
            .unwrap_or((THICKNESS, THICKNESS));
        let size = (
            if configure.new_size.0 == 0 {
                requested.0
            } else {
                configure.new_size.0
            },
            if configure.new_size.1 == 0 {
                requested.1
            } else {
                configure.new_size.1
            },
        );
        if let Err(error) = self.draw_strip(index, size) {
            tracing::error!(%error, "failed to draw the edge strip");
        }
    }
}
