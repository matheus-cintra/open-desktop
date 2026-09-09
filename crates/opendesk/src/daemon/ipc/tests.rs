use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;
use std::time::Duration;

use super::client::{IpcClientError, request};
use super::server::spawn_ipc_server;
use super::{IpcRequest, IpcResponse, PeerStatus, StatusReport};
use tokio::sync::mpsc;

fn temp_socket_path(label: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("opendesk-ipc-test-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    directory.join("opendesk").join("ipc.sock")
}

fn sample_report() -> StatusReport {
    StatusReport {
        name: "desktop".to_owned(),
        peer_id: "ab".repeat(16),
        state: "idle".to_owned(),
        enabled: true,
        peers: Vec::new(),
        pending_pin: None,
    }
}

#[tokio::test]
async fn client_request_reaches_handler_and_answer_comes_back() {
    let path = temp_socket_path("roundtrip");
    let (handler, mut requests) = mpsc::channel(4);
    let server = spawn_ipc_server(path.clone(), handler).await.unwrap();
    assert!(path.exists());

    let handler_task = tokio::spawn(async move {
        let mut served = Vec::new();
        while let Some((incoming, reply)) = requests.recv().await {
            let response = match &incoming {
                IpcRequest::Status => IpcResponse::Status(sample_report()),
                IpcRequest::Release => IpcResponse::Ok,
                _ => IpcResponse::Error {
                    message: "nope".to_owned(),
                },
            };
            served.push(incoming);
            reply.send(response).unwrap();
        }
        served
    });

    assert_eq!(
        request(&path, IpcRequest::Status).await.unwrap(),
        IpcResponse::Status(sample_report())
    );
    assert_eq!(
        request(&path, IpcRequest::Release).await.unwrap(),
        IpcResponse::Ok
    );
    assert_eq!(
        request(
            &path,
            IpcRequest::Pair {
                name: "x".to_owned()
            }
        )
        .await
        .unwrap(),
        IpcResponse::Error {
            message: "nope".to_owned()
        }
    );

    server.abort();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    drop(handler_task);
}

#[tokio::test]
async fn stale_socket_file_is_replaced() {
    let path = temp_socket_path("stale");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"stale").unwrap();
    let (handler, _requests) = mpsc::channel(1);
    let server = spawn_ipc_server(path.clone(), handler).await.unwrap();
    assert!(std::fs::metadata(&path).unwrap().file_type().is_socket());
    server.abort();
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[tokio::test]
async fn missing_socket_maps_to_daemon_not_running() {
    let path = temp_socket_path("missing");
    let error = request(&path, IpcRequest::Status).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<IpcClientError>(),
        Some(IpcClientError::DaemonNotRunning(_))
    ));
}

#[test]
fn requests_and_responses_round_trip_as_json_lines() {
    let request = IpcRequest::PeerSet {
        name: "notebook".to_owned(),
        side: "left".to_owned(),
    };
    let encoded = serde_json::to_string(&request).unwrap();
    assert!(!encoded.contains('\n'));
    assert_eq!(
        serde_json::from_str::<IpcRequest>(&encoded).unwrap(),
        request
    );

    let response = IpcResponse::Status(StatusReport {
        name: "desktop".to_owned(),
        peer_id: "00".repeat(16),
        state: "idle".to_owned(),
        enabled: true,
        peers: vec![PeerStatus {
            name: "notebook".to_owned(),
            side: Some("left".to_owned()),
            connected: false,
            address: None,
        }],
        pending_pin: Some("123456".to_owned()),
    });
    let encoded = serde_json::to_string(&response).unwrap();
    assert_eq!(
        serde_json::from_str::<IpcResponse>(&encoded).unwrap(),
        response
    );
}
