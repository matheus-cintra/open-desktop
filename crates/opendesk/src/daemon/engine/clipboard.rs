use opendesk_proto::control::ControlMessage;
use opendesk_wayland::{ClipboardContent, WaylandCommand};
use sha2::{Digest, Sha256};
use tracing::debug;

use super::Engine;

fn content_hash(mime: &str, bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(mime.as_bytes());
    hasher.update([0]);
    hasher.update(bytes);
    hasher.finalize().into()
}

impl Engine {
    pub(super) fn on_clipboard_changed(&mut self, content: ClipboardContent) {
        let limit = self.config.general.clipboard_max_bytes;
        if content.bytes.len() as u64 > limit {
            debug!(
                bytes = content.bytes.len(),
                limit, "local clipboard exceeds the limit"
            );
            return;
        }
        let hash = content_hash(&content.mime, &content.bytes);
        if self.clipboard_hash == Some(hash) {
            return;
        }
        self.clipboard_hash = Some(hash);
        debug!(
            mime = content.mime,
            bytes = content.bytes.len(),
            "broadcasting clipboard"
        );
        self.broadcast_clipboard(content);
    }

    pub(super) fn on_clipboard_received(&mut self, mime: String, bytes: Vec<u8>) {
        let limit = self.config.general.clipboard_max_bytes;
        if bytes.len() as u64 > limit {
            debug!(
                bytes = bytes.len(),
                limit, "peer clipboard exceeds the limit"
            );
            return;
        }
        let hash = content_hash(&mime, &bytes);
        if self.clipboard_hash == Some(hash) {
            return;
        }
        self.clipboard_hash = Some(hash);
        debug!(mime, bytes = bytes.len(), "applying clipboard from a peer");
        self.wayland(WaylandCommand::SetClipboard {
            content: ClipboardContent { mime, bytes },
        });
    }

    fn broadcast_clipboard(&self, content: ClipboardContent) {
        let message = ControlMessage::ClipboardSet {
            mime: content.mime,
            bytes: content.bytes,
        };
        for (_, link) in self.links.links() {
            self.send_on(link.connection, message.clone());
        }
    }
}
