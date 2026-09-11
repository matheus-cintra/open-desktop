use std::fs;
use std::path::PathBuf;

use opendesk_proto::control::ControlMessage;
use opendesk_proto::transfer::FileBegin;
use tokio::sync::mpsc::unbounded_channel;

use super::{DropAccumulator, TransferError, plan_transfer, run_transfer};
use crate::daemon::net::tcp::{ConnectionId, TcpCommand};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("opendesk-transfer-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_directory_tree_round_trips_byte_for_byte() {
    let base = temp_dir("send");
    let source = base.join("payload");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("top.txt"), b"top level bytes").unwrap();
    fs::write(source.join("nested/inner.bin"), vec![7u8; 300 * 1024]).unwrap();

    let plan = plan_transfer(std::slice::from_ref(&source), 42).unwrap();
    let (tcp, mut commands) = unbounded_channel();
    run_transfer(plan, ConnectionId(1), tcp).await.unwrap();

    let dnd_dir = base.join("dnd");
    let mut accumulator = DropAccumulator::new(dnd_dir.clone());
    let mut uris = Vec::new();
    while let Ok(TcpCommand::Send { message, .. }) = commands.try_recv() {
        match message {
            ControlMessage::FileBegin(begin) => accumulator.begin(begin).unwrap(),
            ControlMessage::FileChunk(chunk) => accumulator.chunk(chunk).unwrap(),
            ControlMessage::FileEnd(end) => uris = accumulator.end(end).unwrap(),
            _ => {}
        }
    }

    assert_eq!(uris, vec![dnd_dir.join("transfer_42").join("payload")]);
    let received = dnd_dir.join("transfer_42").join("payload");
    assert_eq!(
        fs::read(received.join("top.txt")).unwrap(),
        b"top level bytes"
    );
    assert_eq!(
        fs::read(received.join("nested/inner.bin")).unwrap(),
        vec![7u8; 300 * 1024]
    );
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_traversal_path_is_rejected() {
    let dnd_dir = temp_dir("evil");
    let mut accumulator = DropAccumulator::new(dnd_dir.clone());
    let begin = FileBegin {
        transfer_id: 1,
        entries: vec![opendesk_proto::transfer::TransferEntry {
            relative_path: "../escape".to_owned(),
            size: 1,
            is_directory: false,
        }],
    };
    assert!(matches!(
        accumulator.begin(begin),
        Err(TransferError::UnsafePath(_))
    ));
    assert!(!dnd_dir.parent().unwrap().join("escape").exists());
    fs::remove_dir_all(&dnd_dir).unwrap();
}
