mod device;
mod offer;
pub(crate) mod read;
mod source;

use std::os::fd::AsFd;
use std::sync::Arc;

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::ExtDataControlDeviceV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::ZwlrDataControlDeviceV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;

use crate::clipboard::device::{ExtDeviceData, WlrDeviceData};
use crate::clipboard::offer::{DataControlOffer, choose_mime, normalize_mime, offered_mimes};
use crate::clipboard::read::spawn_reader;
use crate::clipboard::source::{ActiveSource, SourceData, SourceObject, serve_bytes};
use crate::error::WaylandError;
use crate::events::ClipboardContent;
use crate::globals::NoEvents;
use crate::state::State;

enum Manager {
    Ext(ExtDataControlManagerV1),
    Wlr(ZwlrDataControlManagerV1),
}

enum Device {
    Ext(ExtDataControlDeviceV1),
    Wlr(ZwlrDataControlDeviceV1),
}

impl Device {
    fn destroy(&self) {
        match self {
            Device::Ext(device) => device.destroy(),
            Device::Wlr(device) => device.destroy(),
        }
    }
}

#[derive(Default)]
pub struct Clipboard {
    manager: Option<Manager>,
    device: Option<Device>,
    source: Option<ActiveSource>,
}

impl Clipboard {
    pub fn new(globals: &GlobalList, queue_handle: &QueueHandle<State>) -> Clipboard {
        Clipboard {
            manager: bind_manager(globals, queue_handle),
            device: None,
            source: None,
        }
    }
}

fn bind_manager(globals: &GlobalList, queue_handle: &QueueHandle<State>) -> Option<Manager> {
    match globals.bind::<ExtDataControlManagerV1, _, _>(queue_handle, 1..=1, NoEvents) {
        Ok(manager) => {
            tracing::info!("bound ext_data_control_manager_v1");
            return Some(Manager::Ext(manager));
        }
        Err(error) => tracing::debug!(%error, "ext_data_control_manager_v1 is unavailable"),
    }
    match globals.bind::<ZwlrDataControlManagerV1, _, _>(queue_handle, 1..=2, NoEvents) {
        Ok(manager) => {
            tracing::info!("bound zwlr_data_control_manager_v1");
            Some(Manager::Wlr(manager))
        }
        Err(error) => {
            tracing::warn!(%error, "no data-control clipboard manager is available");
            None
        }
    }
}

impl State {
    pub fn ensure_clipboard_device(&mut self, queue_handle: &QueueHandle<State>, seat: &WlSeat) {
        if self.clipboard.device.is_some() {
            return;
        }
        match &self.clipboard.manager {
            Some(Manager::Ext(manager)) => {
                let device = manager.get_data_device(seat, queue_handle, ExtDeviceData);
                self.clipboard.device = Some(Device::Ext(device));
                tracing::info!("ext data-control device watching the selection");
            }
            Some(Manager::Wlr(manager)) => {
                let device = manager.get_data_device(seat, queue_handle, WlrDeviceData);
                self.clipboard.device = Some(Device::Wlr(device));
                tracing::info!("wlr data-control device watching the selection");
            }
            None => {}
        }
    }

    pub fn set_clipboard(
        &mut self,
        queue_handle: &QueueHandle<State>,
        content: ClipboardContent,
    ) -> Result<(), WaylandError> {
        self.clipboard_drop_source();
        let mimes = offered_mimes(&content.mime);
        let bytes = Arc::new(content.bytes);
        let object = match (&self.clipboard.manager, &self.clipboard.device) {
            (Some(Manager::Ext(manager)), Some(Device::Ext(device))) => {
                let source = manager.create_data_source(queue_handle, SourceData);
                for mime in &mimes {
                    source.offer(mime.clone());
                }
                device.set_selection(Some(&source));
                SourceObject::Ext(source)
            }
            (Some(Manager::Wlr(manager)), Some(Device::Wlr(device))) => {
                let source = manager.create_data_source(queue_handle, SourceData);
                for mime in &mimes {
                    source.offer(mime.clone());
                }
                device.set_selection(Some(&source));
                SourceObject::Wlr(source)
            }
            _ => return Err(WaylandError::NoClipboard),
        };
        tracing::info!(mime = %content.mime, bytes = bytes.len(), "clipboard selection set");
        self.clipboard.source = Some(ActiveSource { object, bytes });
        Ok(())
    }

    pub fn clipboard_selection<O: DataControlOffer>(
        &mut self,
        connection: &Connection,
        offer: Option<O>,
    ) {
        let Some(offer) = offer else {
            return;
        };
        let mimes = offer.mimes();
        let Some(mime) = choose_mime(&mimes) else {
            tracing::debug!(?mimes, "clipboard offer has no supported mime");
            offer.destroy_offer();
            return;
        };
        match rustix::pipe::pipe() {
            Ok((read, write)) => {
                offer.receive_mime(mime.clone(), write.as_fd());
                if let Err(error) = connection.flush() {
                    tracing::debug!(%error, "flushing the clipboard receive request failed");
                }
                drop(write);
                offer.destroy_offer();
                spawn_reader(read, normalize_mime(&mime), self.events.clone());
            }
            Err(error) => {
                tracing::error!(%error, "could not create a pipe for the clipboard read");
                offer.destroy_offer();
            }
        }
    }

    pub fn clipboard_serve(&self, fd: std::os::fd::OwnedFd) {
        match self.clipboard.source.as_ref() {
            Some(active) => serve_bytes(fd, active.bytes.clone()),
            None => tracing::debug!("clipboard send arrived with no active source"),
        }
    }

    pub fn clipboard_source_cancelled(&mut self) {
        tracing::debug!("clipboard source cancelled by the compositor");
        self.clipboard_drop_source();
    }

    pub fn clipboard_shutdown(&mut self) {
        self.clipboard_drop_source();
        if let Some(device) = self.clipboard.device.take() {
            device.destroy();
        }
    }

    fn clipboard_drop_source(&mut self) {
        if let Some(active) = self.clipboard.source.take() {
            active.object.destroy();
        }
    }
}
