//! Bounded, serial Hyprland queries. No compositor I/O runs on the engine loop.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

pub const QUERY_TIMEOUT: Duration = Duration::from_millis(200);
pub const MAX_AGE: Duration = Duration::from_secs(1);
const RESPONSE_LIMIT: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
    Unknown,
    Locked,
    Unlocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    Lock,
    Cursor,
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub generation: u64,
    pub started: Instant,
    pub query: Query,
}

#[derive(Debug)]
pub enum Sample {
    Lock(bool),
    Cursor { x: f64, y: f64 },
}

#[derive(Debug)]
pub enum MonitorEvent {
    Sample(Request, Sample),
    Unavailable(Request),
}

pub fn socket_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?)
            .join("hypr")
            .join(std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?)
            .join(".socket.sock"),
    )
}

pub fn spawn(path: Option<PathBuf>) -> (mpsc::Sender<Request>, mpsc::Receiver<MonitorEvent>) {
    let (requests, mut incoming) = mpsc::channel::<Request>(1);
    let (events, receiver) = mpsc::channel(1);
    tokio::spawn(async move {
        while let Some(request) = incoming.recv().await {
            let result = match &path {
                Some(path) => query(path, request.query).await.ok(),
                None => None,
            };
            let event = match result {
                Some(sample) => MonitorEvent::Sample(request, sample),
                None => MonitorEvent::Unavailable(request),
            };
            if events.send(event).await.is_err() {
                break;
            }
        }
    });
    (requests, receiver)
}

pub async fn query(path: &Path, kind: Query) -> anyhow::Result<Sample> {
    tokio::time::timeout(QUERY_TIMEOUT, async {
        let mut stream = UnixStream::connect(path).await?;
        let command: &[u8] = match kind {
            Query::Lock => b"j/locked",
            Query::Cursor => b"j/cursorpos",
        };
        stream.write_all(command).await?;
        let mut bytes = Vec::new();
        stream
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 > RESPONSE_LIMIT {
            bail!("compositor response exceeds limit");
        }
        parse(kind, &bytes)
    })
    .await
    .context("compositor query timed out")?
}

fn parse(kind: Query, bytes: &[u8]) -> anyhow::Result<Sample> {
    #[derive(Deserialize)]
    struct Locked {
        locked: bool,
    }
    #[derive(Deserialize)]
    struct Cursor {
        x: f64,
        y: f64,
    }
    Ok(match kind {
        Query::Lock => Sample::Lock(serde_json::from_slice::<Locked>(bytes)?.locked),
        Query::Cursor => {
            let Cursor { x, y } = serde_json::from_slice(bytes)?;
            if !x.is_finite() || !y.is_finite() {
                bail!("non-finite compositor position");
            }
            Sample::Cursor { x, y }
        }
    })
}

#[cfg(test)]
mod tests;
