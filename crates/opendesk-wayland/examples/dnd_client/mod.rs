#![allow(dead_code, unused_imports)]
mod client;
mod dnd;
mod handlers;

use std::error::Error;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use client::{DndClient, DndRole, connect};

use smithay_client_toolkit::delegate_registry;
use smithay_client_toolkit::reexports::calloop::EventLoop;

pub type DndResult<T> = Result<T, Box<dyn Error>>;

pub const URI_LIST_MIME: &str = "text/uri-list";
pub const BTN_LEFT: u32 = 0x110;

pub struct ChildClient {
    pub child: Child,
    pub lines: Arc<Mutex<Vec<String>>>,
}

impl ChildClient {
    pub fn saw(&self, needle: &str) -> bool {
        self.lines
            .lock()
            .map(|lines| lines.iter().any(|line| line.contains(needle)))
            .unwrap_or(false)
    }

    pub fn line_with(&self, needle: &str) -> Option<String> {
        self.lines
            .lock()
            .ok()?
            .iter()
            .find(|line| line.contains(needle))
            .cloned()
    }

    pub fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn sibling_example(name: &str) -> DndResult<PathBuf> {
    let current = std::env::current_exe()?;
    let directory = current
        .parent()
        .ok_or("current executable has no parent directory")?;
    Ok(directory.join(name))
}

pub fn spawn_example(name: &str, arguments: &[&str]) -> DndResult<ChildClient> {
    let path = sibling_example(name)?;
    let mut child = Command::new(&path)
        .args(arguments)
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("child stdout was not captured")?;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(mut collected) = sink.lock() {
                collected.push(line);
            }
        }
    });
    Ok(ChildClient { child, lines })
}

pub fn wait_for_child_line(child: &ChildClient, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.saw(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.saw(needle)
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .try_init();
}

pub fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

pub fn pump(
    event_loop: &mut EventLoop<'static, DndClient>,
    client: &mut DndClient,
    duration: Duration,
) -> DndResult<()> {
    event_loop.dispatch(Some(duration), client)?;
    Ok(())
}

pub fn pump_until(
    event_loop: &mut EventLoop<'static, DndClient>,
    client: &mut DndClient,
    timeout: Duration,
    ready: impl Fn(&DndClient) -> bool,
) -> DndResult<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready(client) {
            return Ok(true);
        }
        pump(event_loop, client, Duration::from_millis(20))?;
    }
    Ok(ready(client))
}

delegate_registry!(DndClient);
smithay_client_toolkit::delegate_dispatch2!(DndClient);
