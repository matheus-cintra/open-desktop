use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

#[tokio::test]
async fn socket_lock_before_connection_then_unlock_during_control_and_recover() {
    let path =
        std::env::temp_dir().join(format!("opendesk-lock-engine-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&path).unwrap();
    let (requests, mut events) = monitor::spawn(Some(path.clone()));
    let server = tokio::spawn(async move {
        for response in [
            br#"{"locked":true}"#.as_slice(),
            br#"{"locked":false}"#.as_slice(),
            b"invalid",
            br#"{"locked":true}"#.as_slice(),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut command = [0; 8];
            stream.read_exact(&mut command).await.unwrap();
            assert_eq!(&command, b"j/locked");
            stream.write_all(response).await.unwrap();
        }
    });
    let mut f = Fixture::new();
    for iteration in 0..4 {
        requests
            .send(Request {
                generation: f.engine.monitor.generation,
                started: Instant::now(),
                query: Query::Lock,
            })
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        f.engine.on_monitor(event);
        match iteration {
            0 => {
                assert_eq!(f.engine.monitor.lock, LockState::Locked);
                f.engine.on_control_message(
                    PEER,
                    ControlMessage::RequestControl {
                        side: Side::Right,
                        fraction: 0.5,
                        drag: None,
                    },
                );
                assert!(f.engine.session.is_controlled());
            }
            1 => {
                assert_eq!(f.engine.monitor.lock, LockState::Unlocked);
                assert!(f.engine.session.is_controlled());
            }
            2 => {
                f.engine.monitor.valid_lock = Some(Instant::now() - MAX_AGE);
                f.engine.monitor.pending = true;
                f.engine.poll_compositor(Instant::now(), &requests);
                assert_eq!(f.releases(), vec![ReleaseReason::Disabled]);
            }
            _ => assert_eq!(f.engine.monitor.lock, LockState::Locked),
        }
    }
    server.await.unwrap();
    std::fs::remove_file(path).unwrap();
}
