use std::os::fd::OwnedFd;

use opendesk_proto::control::Side;
use smithay_client_toolkit::data_device_manager::data_device::DataDeviceHandler;
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, QueueHandle};

use crate::dnd::peek::{URI_LIST_MIME, spawn_peek_reader};
use crate::edge::along_edge_global;
use crate::events::WaylandEvent;
use crate::outputs::output_origin;
use crate::state::State;

impl State {
    fn strip_edge_location(
        &self,
        index: usize,
        local_x: f64,
        local_y: f64,
    ) -> Option<(Side, String, f64)> {
        let strip = self.strips.get(index)?;
        let origin = output_origin(&self.output_state, &strip.output);
        let position = along_edge_global(strip.spec.side, origin, (local_x, local_y));
        Some((strip.spec.side, strip.spec.output.clone(), position))
    }

    fn current_drag_offer(&self) -> Option<DragOffer> {
        self.dnd.device.as_ref()?.data().drag_offer()
    }

    fn peek_and_emit_enter(
        &mut self,
        connection: &Connection,
        side: Side,
        output: String,
        position: f64,
    ) {
        let Some(offer) = self.current_drag_offer() else {
            self.emit_enter(side, output, position, Vec::new());
            return;
        };
        let mime = offer
            .with_mime_types(|mimes| mimes.iter().find(|mime| *mime == URI_LIST_MIME).cloned());
        let Some(mime) = mime else {
            self.emit_enter(side, output, position, Vec::new());
            return;
        };
        offer.accept_mime_type(offer.serial, Some(mime.clone()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        match offer.receive(mime) {
            Ok(pipe) => {
                if let Err(error) = connection.flush() {
                    tracing::debug!(%error, "flushing the drag receive request failed");
                }
                spawn_peek_reader(
                    OwnedFd::from(pipe),
                    self.events.clone(),
                    side,
                    output,
                    position,
                );
            }
            Err(error) => {
                tracing::debug!(%error, "could not receive the uri list for the peek");
                self.emit_enter(side, output, position, Vec::new());
            }
        }
    }

    fn emit_enter(
        &mut self,
        side: Side,
        output: String,
        position: f64,
        uris: Vec<std::path::PathBuf>,
    ) {
        self.emit(WaylandEvent::DragEnteredEdge {
            side,
            output,
            position,
            uris,
        });
    }
}

impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        connection: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &WlSurface,
    ) {
        let Some(index) = self.strips.index_of_surface(surface) else {
            return;
        };
        self.dnd.entered_strip = Some(index);
        let Some((side, output, position)) = self.strip_edge_location(index, x, y) else {
            return;
        };
        self.peek_and_emit_enter(connection, side, output, position);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        let Some(index) = self.dnd.entered_strip.take() else {
            return;
        };
        if let Some(side) = self.strips.get(index).map(|strip| strip.spec.side) {
            self.emit(WaylandEvent::DragLeftEdge { side });
        }
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64) {
        let Some(index) = self.dnd.entered_strip else {
            return;
        };
        if let Some((side, _output, position)) = self.strip_edge_location(index, x, y) {
            self.emit(WaylandEvent::DragMotionEdge { side, position });
        }
    }

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        if self.dnd.entered_strip.take().is_none() {
            return;
        }
        if let Some(offer) = self.current_drag_offer() {
            offer.finish();
            offer.destroy();
        }
        self.emit(WaylandEvent::DragReleasedEdge);
    }
}

impl DataOfferHandler for State {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        offer: &mut DragOffer,
        _: DndAction,
    ) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}
