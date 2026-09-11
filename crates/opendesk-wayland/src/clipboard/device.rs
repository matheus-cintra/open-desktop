use std::sync::Arc;

use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::backend::ObjectData;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::{
    self, ExtDataControlDeviceV1,
};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::ExtDataControlOfferV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::{
    self, ZwlrDataControlDeviceV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::ZwlrDataControlOfferV1;

use crate::clipboard::offer::{DataControlOffer, OfferData};
use crate::state::State;

pub struct ExtDeviceData;

impl Dispatch2<ExtDataControlDeviceV1, State> for ExtDeviceData {
    fn event(
        &self,
        state: &mut State,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        connection: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            ext_data_control_device_v1::Event::Selection { id } => {
                state.clipboard_selection(connection, id);
            }
            ext_data_control_device_v1::Event::PrimarySelection { id } => discard(id),
            ext_data_control_device_v1::Event::Finished => {
                tracing::debug!("ext data-control device finished");
            }
            _ => {}
        }
    }

    fn event_created_child(_: u16, queue_handle: &QueueHandle<State>) -> Arc<dyn ObjectData> {
        queue_handle.make_data::<ExtDataControlOfferV1, OfferData>(OfferData::default())
    }
}

pub struct WlrDeviceData;

impl Dispatch2<ZwlrDataControlDeviceV1, State> for WlrDeviceData {
    fn event(
        &self,
        state: &mut State,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        connection: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::Selection { id } => {
                state.clipboard_selection(connection, id);
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id } => discard(id),
            zwlr_data_control_device_v1::Event::Finished => {
                tracing::debug!("wlr data-control device finished");
            }
            _ => {}
        }
    }

    fn event_created_child(_: u16, queue_handle: &QueueHandle<State>) -> Arc<dyn ObjectData> {
        queue_handle.make_data::<ZwlrDataControlOfferV1, OfferData>(OfferData::default())
    }
}

fn discard<O: DataControlOffer>(offer: Option<O>) {
    if let Some(offer) = offer {
        offer.destroy_offer();
    }
}
