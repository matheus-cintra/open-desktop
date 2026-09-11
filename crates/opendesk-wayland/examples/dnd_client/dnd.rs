use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;

use smithay_client_toolkit::data_device_manager::WritePipe;
use smithay_client_toolkit::data_device_manager::data_device::DataDeviceHandler;
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use smithay_client_toolkit::data_device_manager::data_source::DataSourceHandler;
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_source::WlDataSource;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, QueueHandle};

use super::URI_LIST_MIME;
use super::client::DndClient;

impl DndClient {
    fn current_drag_offer(&self) -> Option<DragOffer> {
        self.data_device.as_ref()?.data().drag_offer()
    }

    fn read_offer_uri(&self, offer: &DragOffer, connection: &Connection) -> Option<String> {
        let mime = offer
            .with_mime_types(|mimes| mimes.iter().find(|mime| *mime == URI_LIST_MIME).cloned())?;
        let pipe = offer.receive(mime).ok()?;
        let _ = connection.flush();
        let mut file = File::from(OwnedFd::from(pipe));
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;
        Some(text.trim().to_owned())
    }
}

impl DataDeviceHandler for DndClient {
    fn enter(
        &mut self,
        connection: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        _: f64,
        _: f64,
        _: &WlSurface,
    ) {
        self.data_enter = true;
        if self.role.print_tokens {
            println!("ENTER");
        }
        let Some(offer) = self.current_drag_offer() else {
            tracing::warn!("data device enter without a drag offer");
            return;
        };
        if self.role.accept_on_enter
            && let Some(mime) = offer
                .with_mime_types(|mimes| mimes.iter().find(|mime| *mime == URI_LIST_MIME).cloned())
        {
            offer.accept_mime_type(offer.serial, Some(mime));
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
        if self.role.read_on_enter
            && let Some(uri) = self.read_offer_uri(&offer, connection)
        {
            tracing::info!(%uri, "peeked the uri before drop");
            if let Ok(mut slot) = self.peeked_uri.lock() {
                *slot = Some(uri);
            }
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        self.data_leave = true;
        if self.role.print_tokens {
            println!("LEAVE");
        }
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, connection: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        self.data_drop = true;
        let Some(offer) = self.current_drag_offer() else {
            tracing::warn!("drop without a drag offer");
            return;
        };
        if self.role.read_on_drop {
            let uri = self.read_offer_uri(&offer, connection);
            offer.finish();
            offer.destroy();
            match uri {
                Some(uri) => {
                    if self.role.print_tokens {
                        println!("DROP {uri}");
                    }
                    if let Ok(mut slot) = self.dropped_uri.lock() {
                        *slot = Some(uri);
                    }
                }
                None => tracing::error!("drop delivered no readable uri"),
            }
        }
    }
}

impl DataOfferHandler for DndClient {
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

impl DataSourceHandler for DndClient {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        mime: Option<String>,
    ) {
        tracing::info!(?mime, "target accepted a mime type");
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        mime: String,
        write_pipe: WritePipe,
    ) {
        let Some(uri) = self.role.uri.clone() else {
            return;
        };
        let mut file = File::from(OwnedFd::from(write_pipe));
        let payload = format!("{uri}\r\n");
        if let Err(error) = file.write_all(payload.as_bytes()) {
            tracing::error!(%error, "failed to serve the uri");
            return;
        }
        self.send_count += 1;
        if self.role.print_tokens {
            println!("send");
        }
        tracing::info!(%mime, "served the uri to the drop target");
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        self.source_cancelled = true;
        if self.role.print_tokens {
            println!("cancelled");
        }
        tracing::info!("drag source cancelled by the compositor");
        source.destroy();
        self.drag_source = None;
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {
        tracing::info!("drag source dropped");
    }

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        self.dnd_finished = true;
        if self.role.print_tokens {
            println!("dnd_finished");
        }
        tracing::info!("drag source finished");
        source.destroy();
        self.drag_source = None;
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        action: DndAction,
    ) {
        tracing::info!(?action, "drag source action");
    }
}
