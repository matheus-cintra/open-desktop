#![allow(clippy::unwrap_used)]
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::UnixListener;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Socket {
    pub path: PathBuf,
    pub listener: UnixListener,
}

impl Socket {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "opendesk-monitor-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        Self { path, listener }
    }

    async fn answer(&self, command: &[u8], response: &[u8]) {
        let (mut stream, _) = self.listener.accept().await.unwrap();
        let mut bytes = vec![0; command.len()];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(bytes, command);
        stream.write_all(response).await.unwrap();
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tokio::test]
async fn parses_locked_unlocked_and_cursor_over_direct_socket() {
    let socket = Socket::new();
    for (command, response, kind) in [
        (
            b"j/locked".as_slice(),
            br#"{"locked":true}"#.as_slice(),
            Query::Lock,
        ),
        (
            b"j/locked".as_slice(),
            br#"{"locked":false}"#.as_slice(),
            Query::Lock,
        ),
        (
            b"j/cursorpos".as_slice(),
            br#"{"x":-1920,"y":1079}"#.as_slice(),
            Query::Cursor,
        ),
    ] {
        let (result, ()) =
            tokio::join!(query(&socket.path, kind), socket.answer(command, response));
        match result.unwrap() {
            Sample::Lock(locked) => assert_eq!(locked, response == br#"{"locked":true}"#),
            Sample::Cursor { x, y } => assert_eq!((x, y), (-1920.0, 1079.0)),
        }
    }
}

#[tokio::test]
async fn malformed_oversized_disconnected_and_timeout_then_recover() {
    let socket = Socket::new();
    for response in [
        b"not json".to_vec(),
        br#"{"locked":"false"}"#.to_vec(),
        vec![b' '; 4097],
        vec![],
    ] {
        let (result, ()) = tokio::join!(
            query(&socket.path, Query::Lock),
            socket.answer(b"j/locked", &response)
        );
        assert!(result.is_err());
    }
    let stalled = async {
        let (_stream, _) = socket.listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    let start = Instant::now();
    let (result, ()) = tokio::join!(query(&socket.path, Query::Lock), stalled);
    assert!(result.unwrap_err().to_string().contains("timed out"));
    assert!(start.elapsed() < Duration::from_millis(500));
    let (result, ()) = tokio::join!(
        query(&socket.path, Query::Lock),
        socket.answer(b"j/locked", br#"{"locked":false}"#)
    );
    assert!(matches!(result.unwrap(), Sample::Lock(false)));
    assert!(
        query(&socket.path.with_extension("missing"), Query::Lock)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn worker_tags_unavailable_and_recovered_samples() {
    let socket = Socket::new();
    let (sender, mut receiver) = spawn(Some(socket.path.clone()));
    let request = Request {
        generation: 42,
        started: Instant::now(),
        query: Query::Lock,
    };
    sender.send(request).await.unwrap();
    socket.answer(b"j/locked", b"broken").await;
    assert!(matches!(
        receiver.recv().await,
        Some(MonitorEvent::Unavailable(Request { generation: 42, .. }))
    ));
    sender.send(request).await.unwrap();
    socket.answer(b"j/locked", br#"{"locked":true}"#).await;
    assert!(matches!(
        receiver.recv().await,
        Some(MonitorEvent::Sample(
            Request { generation: 42, .. },
            Sample::Lock(true)
        ))
    ));
}
