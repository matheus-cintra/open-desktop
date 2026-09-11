use std::os::fd::BorrowedFd;
use std::sync::Mutex;

use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{Connection, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::{
    self, ExtDataControlOfferV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::{
    self, ZwlrDataControlOfferV1,
};

use crate::state::State;

const TEXT_MIMES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
];

pub fn choose_mime(offered: &[String]) -> Option<String> {
    for preferred in TEXT_MIMES {
        if offered.iter().any(|mime| mime == preferred) {
            return Some(preferred.to_owned());
        }
    }
    if offered.iter().any(|mime| mime == "image/png") {
        return Some("image/png".to_owned());
    }
    offered
        .iter()
        .find(|mime| mime.starts_with("image/"))
        .cloned()
}

pub fn normalize_mime(mime: &str) -> String {
    if TEXT_MIMES.contains(&mime) {
        "text/plain;charset=utf-8".to_owned()
    } else {
        mime.to_owned()
    }
}

pub fn offered_mimes(mime: &str) -> Vec<String> {
    if mime.starts_with("text/") {
        TEXT_MIMES.iter().map(|mime| (*mime).to_owned()).collect()
    } else {
        vec![mime.to_owned()]
    }
}

#[derive(Default)]
pub struct OfferData {
    mimes: Mutex<Vec<String>>,
}

impl OfferData {
    pub fn mimes(&self) -> Vec<String> {
        self.mimes
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn push(&self, mime: String) {
        if let Ok(mut guard) = self.mimes.lock() {
            guard.push(mime);
        }
    }
}

pub trait DataControlOffer: Proxy {
    fn receive_mime(&self, mime_type: String, fd: BorrowedFd<'_>);
    fn destroy_offer(&self);
    fn mimes(&self) -> Vec<String> {
        self.data::<OfferData>()
            .map(OfferData::mimes)
            .unwrap_or_default()
    }
}

impl DataControlOffer for ExtDataControlOfferV1 {
    fn receive_mime(&self, mime_type: String, fd: BorrowedFd<'_>) {
        self.receive(mime_type, fd);
    }

    fn destroy_offer(&self) {
        self.destroy();
    }
}

impl DataControlOffer for ZwlrDataControlOfferV1 {
    fn receive_mime(&self, mime_type: String, fd: BorrowedFd<'_>) {
        self.receive(mime_type, fd);
    }

    fn destroy_offer(&self) {
        self.destroy();
    }
}

impl Dispatch2<ExtDataControlOfferV1, State> for OfferData {
    fn event(
        &self,
        _: &mut State,
        _: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            self.push(mime_type);
        }
    }
}

impl Dispatch2<ZwlrDataControlOfferV1, State> for OfferData {
    fn event(
        &self,
        _: &mut State,
        _: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            self.push(mime_type);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{choose_mime, normalize_mime, offered_mimes};

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn prefers_utf8_text() {
        let offered = owned(&[
            "TEXT",
            "text/plain",
            "text/plain;charset=utf-8",
            "image/png",
        ]);
        assert_eq!(
            choose_mime(&offered).as_deref(),
            Some("text/plain;charset=utf-8")
        );
    }

    #[test]
    fn falls_back_to_png() {
        let offered = owned(&["image/bmp", "image/png"]);
        assert_eq!(choose_mime(&offered).as_deref(), Some("image/png"));
    }

    #[test]
    fn falls_back_to_first_image() {
        let offered = owned(&["application/octet-stream", "image/webp", "image/gif"]);
        assert_eq!(choose_mime(&offered).as_deref(), Some("image/webp"));
    }

    #[test]
    fn ignores_unknown() {
        let offered = owned(&["application/pdf", "text/html"]);
        assert_eq!(choose_mime(&offered), None);
    }

    #[test]
    fn normalizes_text_variants() {
        for mime in [
            "text/plain",
            "UTF8_STRING",
            "TEXT",
            "text/plain;charset=utf-8",
        ] {
            assert_eq!(normalize_mime(mime), "text/plain;charset=utf-8");
        }
        assert_eq!(normalize_mime("image/png"), "image/png");
    }

    #[test]
    fn expands_text_mimes() {
        assert_eq!(
            offered_mimes("text/plain;charset=utf-8"),
            owned(&[
                "text/plain;charset=utf-8",
                "text/plain",
                "UTF8_STRING",
                "TEXT"
            ])
        );
        assert_eq!(offered_mimes("image/png"), owned(&["image/png"]));
    }
}
