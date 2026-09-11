mod fade;
mod geometry;
mod paint;
mod surface;

use std::time::Instant;

use opendesk_proto::control::Side;
use smithay_client_toolkit::shell::wlr_layer::{LayerSurface, LayerSurfaceConfigure};
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::bar::fade::{ARRIVAL_FADE, Fade};
use crate::bar::geometry::{ARRIVAL_LENGTH, place_bar, progress_length};
use crate::bar::surface::Bar;
use crate::error::WaylandError;
use crate::events::BarStyle;
use crate::outputs::find_output_on_edge;
use crate::state::State;

#[derive(Default)]
pub struct Bars {
    pub style: BarStyle,
    progress: Option<Bar>,
    arrival: Option<Bar>,
}

impl Bars {
    pub fn clear(&mut self) {
        self.progress = None;
        self.arrival = None;
    }

    fn slots(&mut self) -> [&mut Option<Bar>; 2] {
        [&mut self.progress, &mut self.arrival]
    }

    fn slot_of_layer(&mut self, layer: &LayerSurface) -> Option<&mut Option<Bar>> {
        self.slots()
            .into_iter()
            .find(|slot| slot.as_ref().is_some_and(|bar| bar.matches_layer(layer)))
    }
}

impl State {
    pub fn set_bar_style(&mut self, style: BarStyle) -> Result<(), WaylandError> {
        self.bars.style = style;
        if let Some(bar) = self.bars.progress.as_mut()
            && bar.configured
        {
            bar.paint(&mut self.pool, style, Instant::now())?;
            bar.commit();
        }
        Ok(())
    }

    pub fn show_progress_bar(
        &mut self,
        queue_handle: &QueueHandle<State>,
        side: Side,
        position: f64,
        progress: f32,
    ) -> Result<(), WaylandError> {
        let (output, geometry) = find_output_on_edge(&self.output_state, side, position)
            .ok_or(WaylandError::NoOutputOnEdge(side, position))?;
        let placement = place_bar(side, position, &geometry, progress_length(progress));
        match self.bars.progress.as_mut() {
            Some(bar) if bar.output == output => {
                bar.placement = placement;
                bar.apply_placement();
                if bar.configured {
                    bar.paint(&mut self.pool, self.bars.style, Instant::now())?;
                }
                bar.commit();
            }
            _ => {
                tracing::debug!(%side, position, "progress bar created");
                self.bars.progress =
                    Some(self.create_bar(queue_handle, output, placement, None)?);
            }
        }
        Ok(())
    }

    pub fn hide_progress_bar(&mut self) {
        if self.bars.progress.take().is_some() {
            tracing::debug!("progress bar hidden");
        }
    }

    pub fn show_arrival_bar(
        &mut self,
        queue_handle: &QueueHandle<State>,
        side: Side,
        position: f64,
    ) -> Result<(), WaylandError> {
        let (output, geometry) = find_output_on_edge(&self.output_state, side, position)
            .ok_or(WaylandError::NoOutputOnEdge(side, position))?;
        let placement = place_bar(side, position, &geometry, ARRIVAL_LENGTH);
        let fade = Fade::start(Instant::now(), ARRIVAL_FADE);
        tracing::debug!(%side, position, "arrival bar created");
        self.bars.arrival = Some(self.create_bar(queue_handle, output, placement, Some(fade))?);
        Ok(())
    }

    pub fn configure_bar(
        &mut self,
        queue_handle: &QueueHandle<State>,
        layer: &LayerSurface,
        configure: &LayerSurfaceConfigure,
    ) -> bool {
        let style = self.bars.style;
        let Some(slot) = self.bars.slot_of_layer(layer) else {
            return false;
        };
        let Some(bar) = slot.as_mut() else {
            return false;
        };
        let requested = bar.placement.size();
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
        if bar.configured && size == requested {
            return true;
        }
        bar.placement.width = size.0;
        bar.placement.height = size.1;
        bar.configured = true;
        if let Err(error) = bar.paint(&mut self.pool, style, Instant::now()) {
            tracing::error!(%error, "failed to paint the bar");
            *slot = None;
            return true;
        }
        if bar.fade.is_some() {
            bar.request_frame(queue_handle);
        }
        bar.commit();
        true
    }

    pub fn bar_closed(&mut self, layer: &LayerSurface) -> bool {
        match self.bars.slot_of_layer(layer) {
            Some(slot) => {
                tracing::warn!("bar closed by the compositor");
                *slot = None;
                true
            }
            None => false,
        }
    }

    pub fn bar_frame(&mut self, queue_handle: &QueueHandle<State>, surface: &WlSurface) {
        let style = self.bars.style;
        let Some(bar) = self
            .bars
            .arrival
            .as_mut()
            .filter(|bar| bar.matches_surface(surface))
        else {
            return;
        };
        let now = Instant::now();
        if bar.fade.is_some_and(|fade| fade.is_finished(now)) {
            tracing::debug!("arrival bar faded out");
            self.bars.arrival = None;
            return;
        }
        if let Err(error) = bar.paint(&mut self.pool, style, now) {
            tracing::error!(%error, "failed to repaint the arrival bar");
            self.bars.arrival = None;
            return;
        }
        bar.request_frame(queue_handle);
        bar.commit();
    }
}
