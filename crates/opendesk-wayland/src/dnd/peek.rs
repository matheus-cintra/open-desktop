use std::ffi::OsString;
use std::fs::File;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use opendesk_proto::control::Side;
use tokio::sync::mpsc::UnboundedSender;

use crate::clipboard::read::read_capped;
use crate::events::WaylandEvent;

pub const URI_LIST_MIME: &str = "text/uri-list";

const FILE_SCHEME: &str = "file://";
const PEEK_READ_CAP: usize = 256 * 1024;

pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(path_from_file_uri)
        .collect()
}

fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix(FILE_SCHEME)?;
    let path_part = match rest.strip_prefix("localhost") {
        Some(tail) => tail,
        None => rest,
    };
    if !path_part.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(percent_decode(path_part))))
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let decoded_byte = bytes
            .get(index + 1..index + 3)
            .filter(|_| bytes[index] == b'%')
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match decoded_byte {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    decoded
}

pub fn render_uri_list(paths: &[PathBuf]) -> Vec<u8> {
    let mut output = String::new();
    for path in paths {
        output.push_str(FILE_SCHEME);
        percent_encode_into(&mut output, path.as_os_str().as_bytes());
        output.push_str("\r\n");
    }
    output.into_bytes()
}

fn percent_encode_into(output: &mut String, bytes: &[u8]) {
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
}

pub fn spawn_peek_reader(
    fd: OwnedFd,
    events: UnboundedSender<WaylandEvent>,
    current_generation: Arc<AtomicU64>,
    generation: u64,
    side: Side,
    output: String,
    position: f64,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let uris = read_capped(File::from(fd), PEEK_READ_CAP)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|text| parse_uri_list(&text))
            .unwrap_or_default();
        if current_generation.load(Ordering::Acquire) != generation {
            return;
        }
        let event = WaylandEvent::DragEnteredEdge {
            generation,
            side,
            output,
            position,
            uris,
        };
        if events.send(event).is_err() {
            tracing::debug!("drag-enter event receiver is gone");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_uri_list, render_uri_list, spawn_peek_reader};
    use opendesk_proto::control::Side;
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn keeps_only_local_file_uris() {
        let text = "file:///tmp/a.txt\r\nfile://localhost/tmp/b.txt\r\nfile://other/tmp/c.txt\r\nhttps://x/y\r\n";
        assert_eq!(
            parse_uri_list(text),
            vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]
        );
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let text = "# comment\n\nfile:///tmp/a b.txt\n";
        assert_eq!(parse_uri_list(text), vec![PathBuf::from("/tmp/a b.txt")]);
    }

    #[test]
    fn renders_percent_encoded_file_uris() {
        let paths = [PathBuf::from("/tmp/a b.txt")];
        assert_eq!(
            render_uri_list(&paths),
            b"file:///tmp/a%20b.txt\r\n".to_vec()
        );
    }

    #[test]
    fn round_trips_a_path() {
        let paths = [PathBuf::from("/run/user/1000/opendesk.txt")];
        let rendered = render_uri_list(&paths);
        let text = String::from_utf8(rendered).unwrap();
        assert_eq!(parse_uri_list(&text), paths.to_vec());
    }

    #[test]
    fn late_peek_does_not_emit_after_drag_generation_changes() {
        let (read, mut write) = UnixStream::pair().unwrap();
        let generation = Arc::new(AtomicU64::new(7));
        let (sender, mut receiver) = unbounded_channel();
        let reader = spawn_peek_reader(
            OwnedFd::from(read),
            sender,
            generation.clone(),
            7,
            Side::Left,
            "output".to_owned(),
            25.0,
        );
        write.write_all(b"file:///tmp/late.txt\r\n").unwrap();
        generation.store(8, Ordering::Release);
        drop(write);
        reader.join().unwrap();
        assert!(receiver.try_recv().is_err());
    }
}
