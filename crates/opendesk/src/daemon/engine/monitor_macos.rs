use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
pub const MAX_AGE: Duration = Duration::from_secs(1);

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
    #[allow(dead_code)] // Not queried on macOS: locked sessions are released.
    Cursor {
        x: f64,
        y: f64,
    },
}

#[derive(Debug)]
pub enum MonitorEvent {
    Sample(Request, Sample),
    Unavailable(Request),
}

pub fn socket_path() -> Option<PathBuf> {
    None
}
pub fn spawn(_: Option<PathBuf>) -> (mpsc::Sender<Request>, mpsc::Receiver<MonitorEvent>) {
    let (requests, mut incoming) = mpsc::channel::<Request>(1);
    let (events, receiver) = mpsc::channel(1);
    tokio::spawn(async move {
        while let Some(request) = incoming.recv().await {
            let health = opendesk_macos::health();
            let event = if health == 1 {
                MonitorEvent::Unavailable(request)
            } else {
                MonitorEvent::Sample(request, Sample::Lock(health != 0))
            };
            if events.send(event).await.is_err() {
                break;
            }
        }
    });
    (requests, receiver)
}
