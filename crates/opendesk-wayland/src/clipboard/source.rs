use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;

use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_source_v1::{
    self, ExtDataControlSourceV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_source_v1::{
    self, ZwlrDataControlSourceV1,
};

use crate::state::State;

pub enum SourceObject {
    Ext(ExtDataControlSourceV1),
    Wlr(ZwlrDataControlSourceV1),
}

impl SourceObject {
    pub fn destroy(&self) {
        match self {
            SourceObject::Ext(source) => source.destroy(),
            SourceObject::Wlr(source) => source.destroy(),
        }
    }
}

pub struct ActiveSource {
    pub object: SourceObject,
    pub bytes: Arc<Vec<u8>>,
}

pub struct SourceData;

impl Dispatch2<ExtDataControlSourceV1, State> for SourceData {
    fn event(
        &self,
        state: &mut State,
        _: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { fd, .. } => state.clipboard_serve(fd),
            ext_data_control_source_v1::Event::Cancelled => state.clipboard_source_cancelled(),
            _ => {}
        }
    }
}

impl Dispatch2<ZwlrDataControlSourceV1, State> for SourceData {
    fn event(
        &self,
        state: &mut State,
        _: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { fd, .. } => state.clipboard_serve(fd),
            zwlr_data_control_source_v1::Event::Cancelled => state.clipboard_source_cancelled(),
            _ => {}
        }
    }
}

pub fn serve_bytes(fd: OwnedFd, bytes: Arc<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut file = File::from(fd);
        if let Err(error) = file.write_all(&bytes) {
            tracing::debug!(%error, "clipboard send could not write all the bytes");
        }
    });
}
