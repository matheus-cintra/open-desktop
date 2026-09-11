use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;

use tokio::sync::mpsc::UnboundedSender;

use crate::events::{ClipboardContent, WaylandEvent};

pub const CLIPBOARD_READ_CAP: usize = 16 * 1024 * 1024;

pub fn read_capped(reader: impl Read, cap: usize) -> Option<Vec<u8>> {
    let mut buffer = Vec::new();
    reader.take(cap as u64 + 1).read_to_end(&mut buffer).ok()?;
    if buffer.len() > cap {
        return None;
    }
    Some(buffer)
}

pub fn spawn_reader(read: OwnedFd, mime: String, events: UnboundedSender<WaylandEvent>) {
    std::thread::spawn(
        move || match read_capped(File::from(read), CLIPBOARD_READ_CAP) {
            Some(bytes) => {
                let content = ClipboardContent { mime, bytes };
                if events
                    .send(WaylandEvent::ClipboardChanged { content })
                    .is_err()
                {
                    tracing::debug!("clipboard event receiver is gone");
                }
            }
            None => {
                tracing::debug!("clipboard payload exceeded the cap or could not be read, dropped")
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{CLIPBOARD_READ_CAP, read_capped};

    #[test]
    fn reads_small_payload() {
        let data = b"opendesk".to_vec();
        assert_eq!(read_capped(data.as_slice(), CLIPBOARD_READ_CAP), Some(data));
    }

    #[test]
    fn drops_payload_over_cap() {
        let data = vec![b'a'; 32];
        assert_eq!(read_capped(data.as_slice(), 16), None);
    }

    #[test]
    fn keeps_payload_at_cap() {
        let data = vec![b'a'; 16];
        assert_eq!(read_capped(data.as_slice(), 16), Some(data));
    }
}
