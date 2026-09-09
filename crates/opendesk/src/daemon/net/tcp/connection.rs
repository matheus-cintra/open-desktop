use std::sync::Arc;

use opendesk_proto::codec::{CodecError, FrameDecoder, encode_frame};
use opendesk_proto::control::ControlMessage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Notify;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use super::{ConnectionId, TcpEvent};

const READ_BUFFER_BYTES: usize = 64 * 1024;

pub struct ConnectionControl {
    outbound: UnboundedSender<Vec<u8>>,
    close: Arc<Notify>,
}

pub struct Outbound {
    frames: UnboundedReceiver<Vec<u8>>,
    close: Arc<Notify>,
}

impl ConnectionControl {
    pub fn new() -> (Self, Outbound) {
        let (outbound, frames) = mpsc::unbounded_channel();
        let close = Arc::new(Notify::new());
        (
            Self {
                outbound,
                close: Arc::clone(&close),
            },
            Outbound { frames, close },
        )
    }

    pub fn send(&self, message: &ControlMessage) -> Result<(), CodecError> {
        let frame = encode_frame(message)?;
        let _ = self.outbound.send(frame);
        Ok(())
    }

    pub fn request_close(&self) {
        self.close.notify_one();
    }
}

pub async fn run_connection(
    stream: TcpStream,
    outbound: Outbound,
    events: &UnboundedSender<TcpEvent>,
    id: ConnectionId,
) -> String {
    let (read_half, write_half) = stream.into_split();
    let writer = tokio::spawn(write_loop(write_half, outbound.frames));
    let reason = tokio::select! {
        reason = read_loop(read_half, events, id) => reason,
        () = outbound.close.notified() => "closed locally".to_owned(),
    };
    writer.abort();
    reason
}

async fn write_loop(mut write_half: OwnedWriteHalf, mut frames: UnboundedReceiver<Vec<u8>>) {
    while let Some(frame) = frames.recv().await {
        if let Err(error) = write_half.write_all(&frame).await {
            tracing::debug!(%error, "tcp write failed");
            break;
        }
    }
    let _ = write_half.shutdown().await;
}

async fn read_loop(
    mut read_half: OwnedReadHalf,
    events: &UnboundedSender<TcpEvent>,
    id: ConnectionId,
) -> String {
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    let mut decoder = FrameDecoder::default();
    loop {
        let bytes_read = match read_half.read(&mut buffer).await {
            Ok(0) => return "peer closed the connection".to_owned(),
            Ok(bytes_read) => bytes_read,
            Err(error) => return format!("read failed: {error}"),
        };
        decoder.push(buffer.get(..bytes_read).unwrap_or_default());
        loop {
            match decoder.next_frame() {
                Ok(Some(message)) => {
                    if events
                        .send(TcpEvent::Message {
                            connection: id,
                            message,
                        })
                        .is_err()
                    {
                        return "event receiver dropped".to_owned();
                    }
                }
                Ok(None) => break,
                Err(error) => return format!("decode failed: {error}"),
            }
        }
    }
}
