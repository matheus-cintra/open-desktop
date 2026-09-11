use opendesk_proto::control::ControlMessage;
use opendesk_proto::transfer::{CHUNK_BYTES, FileBegin, FileChunk, FileEnd};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use super::{TransferError, TransferPlan, io_error};
use crate::daemon::net::tcp::{ConnectionId, TcpCommand};

pub async fn run_transfer(
    plan: TransferPlan,
    connection: ConnectionId,
    tcp: UnboundedSender<TcpCommand>,
) -> Result<(), TransferError> {
    let transfer_id = plan.drag.transfer_id;
    let begin = FileBegin {
        transfer_id,
        entries: plan.drag.entries.clone(),
    };
    send(&tcp, connection, ControlMessage::FileBegin(begin));
    let mut total: u64 = 0;
    for (index, source) in plan.sources.iter().enumerate() {
        let Some(path) = source else {
            continue;
        };
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|error| io_error(path, error))?;
        let mut offset: u64 = 0;
        let mut buffer = vec![0u8; CHUNK_BYTES];
        loop {
            let read = file
                .read(&mut buffer)
                .await
                .map_err(|error| io_error(path, error))?;
            if read == 0 {
                break;
            }
            let chunk = FileChunk {
                transfer_id,
                entry_index: index as u32,
                offset,
                bytes: buffer[..read].to_vec(),
            };
            send(&tcp, connection, ControlMessage::FileChunk(chunk));
            offset += read as u64;
            total += read as u64;
        }
    }
    send(
        &tcp,
        connection,
        ControlMessage::FileEnd(FileEnd {
            transfer_id,
            success: true,
        }),
    );
    debug!(transfer_id, bytes = total, "file transfer sent");
    Ok(())
}

fn send(tcp: &UnboundedSender<TcpCommand>, connection: ConnectionId, message: ControlMessage) {
    if tcp
        .send(TcpCommand::Send {
            connection,
            message,
        })
        .is_err()
    {
        warn!("tcp sender gone during a file transfer");
    }
}
