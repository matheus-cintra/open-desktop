use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use anyhow::Context;
use rust_i18n::t;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::{IpcRequest, IpcResponse};

pub type IpcHandlerSender = mpsc::Sender<(IpcRequest, oneshot::Sender<IpcResponse>)>;

pub async fn spawn_ipc_server(
    path: PathBuf,
    handler: IpcHandlerSender,
) -> anyhow::Result<JoinHandle<()>> {
    prepare_socket_path(&path)?;
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind ipc socket at {}", path.display()))?;
    tracing::info!(path = %path.display(), "ipc server listening");
    Ok(tokio::spawn(accept_loop(listener, handler)))
}

fn prepare_socket_path(path: &std::path::Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("ipc socket path {} has no parent", path.display()))?;
    if !parent.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => tracing::warn!(path = %path.display(), "removed stale ipc socket"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to remove stale socket {}", path.display()));
        }
    }
    Ok(())
}

async fn accept_loop(listener: UnixListener, handler: IpcHandlerSender) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(serve_connection(stream, handler.clone()));
            }
            Err(error) => {
                tracing::error!(%error, "ipc accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

async fn serve_connection(stream: UnixStream, handler: IpcHandlerSender) {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                tracing::debug!(%error, "ipc connection read failed");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = respond(&line, &handler).await;
        if let Err(error) = write_response(&mut write_half, &response).await {
            tracing::debug!(%error, "ipc connection write failed");
            break;
        }
    }
}

async fn respond(line: &str, handler: &IpcHandlerSender) -> IpcResponse {
    let request = match serde_json::from_str::<IpcRequest>(line) {
        Ok(request) => request,
        Err(error) => {
            tracing::warn!(%error, "ipc request could not be decoded");
            return IpcResponse::Error {
                message: t!("ipc.invalid_request", error = error.to_string()).into_owned(),
            };
        }
    };
    tracing::debug!(?request, "ipc request received");
    let (response_sender, response_receiver) = oneshot::channel();
    if handler.send((request, response_sender)).await.is_err() {
        return handler_unavailable();
    }
    response_receiver
        .await
        .unwrap_or_else(|_| handler_unavailable())
}

fn handler_unavailable() -> IpcResponse {
    IpcResponse::Error {
        message: t!("ipc.handler_unavailable").into_owned(),
    }
}

async fn write_response(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    response: &IpcResponse,
) -> anyhow::Result<()> {
    let mut encoded = serde_json::to_vec(response)?;
    encoded.push(b'\n');
    write_half.write_all(&encoded).await?;
    write_half.flush().await?;
    Ok(())
}
