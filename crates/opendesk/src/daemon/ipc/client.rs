use std::path::Path;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::{IpcRequest, IpcResponse};

#[derive(Debug, thiserror::Error)]
pub enum IpcClientError {
    #[error("daemon is not running (no socket at {0})")]
    DaemonNotRunning(String),
    #[error("ipc io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("ipc message could not be encoded or decoded: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("daemon closed the ipc connection without answering")]
    ClosedWithoutResponse,
}

pub async fn request(path: &Path, request: IpcRequest) -> anyhow::Result<IpcResponse> {
    Ok(send(path, request).await?)
}

async fn send(path: &Path, request: IpcRequest) -> Result<IpcResponse, IpcClientError> {
    let stream = match UnixStream::connect(path).await {
        Ok(stream) => stream,
        Err(error) if is_not_running(&error) => {
            return Err(IpcClientError::DaemonNotRunning(path.display().to_string()));
        }
        Err(error) => return Err(IpcClientError::Io(error)),
    };
    let (read_half, mut write_half) = stream.into_split();
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    write_half.write_all(&encoded).await?;
    write_half.flush().await?;

    let mut line = String::new();
    let bytes_read = BufReader::new(read_half).read_line(&mut line).await?;
    if bytes_read == 0 {
        return Err(IpcClientError::ClosedWithoutResponse);
    }
    Ok(serde_json::from_str(&line)?)
}

fn is_not_running(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    )
}
