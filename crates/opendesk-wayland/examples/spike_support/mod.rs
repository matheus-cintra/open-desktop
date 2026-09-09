use std::error::Error;
use std::process::Command;
use std::time::{Duration, Instant};

use opendesk_proto::control::OutputGeometry;
use opendesk_wayland::{WaylandCommand, WaylandEvent, WaylandHandle, spawn};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

pub type SpikeResult<T> = Result<T, Box<dyn Error>>;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

pub struct Spike {
    handle: WaylandHandle,
    events: UnboundedReceiver<WaylandEvent>,
    checks: Vec<(String, bool)>,
}

impl Spike {
    pub fn start() -> SpikeResult<(Spike, Vec<OutputGeometry>)> {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_writer(std::io::stderr)
            .init();
        let (sender, events) = unbounded_channel();
        let handle = spawn(sender)?;
        let mut spike = Spike {
            handle,
            events,
            checks: Vec::new(),
        };
        let ready = spike.wait_for(Duration::from_secs(2), |event| {
            matches!(event, WaylandEvent::Ready { .. })
        });
        match ready {
            Some(WaylandEvent::Ready { outputs }) => Ok((spike, outputs)),
            _ => Err("no Ready event within 2 s".into()),
        }
    }

    pub fn send(&self, command: WaylandCommand) -> SpikeResult<()> {
        self.handle
            .commands
            .send(command)
            .map_err(|_| "wayland thread is gone")?;
        Ok(())
    }

    pub fn wait_for(
        &mut self,
        timeout: Duration,
        predicate: impl Fn(&WaylandEvent) -> bool,
    ) -> Option<WaylandEvent> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.events.try_recv() {
                Ok(event) if predicate(&event) => return Some(event),
                Ok(event) => tracing::debug!(?event, "spike ignored event"),
                Err(_) => std::thread::sleep(POLL_INTERVAL),
            }
        }
        None
    }

    pub fn collect_for(&mut self, duration: Duration) -> Vec<WaylandEvent> {
        let deadline = Instant::now() + duration;
        let mut collected = Vec::new();
        while Instant::now() < deadline {
            match self.events.try_recv() {
                Ok(event) => collected.push(event),
                Err(_) => std::thread::sleep(POLL_INTERVAL),
            }
        }
        collected
    }

    pub fn check(&mut self, name: &str, passed: bool) {
        println!("{}: {name}", if passed { "PASS" } else { "FAIL" });
        self.checks.push((name.to_owned(), passed));
    }

    pub fn finish(self) -> SpikeResult<()> {
        self.handle.shutdown()?;
        let failed: Vec<&str> = self
            .checks
            .iter()
            .filter(|(_, passed)| !passed)
            .map(|(name, _)| name.as_str())
            .collect();
        if failed.is_empty() {
            println!("ALL CHECKS PASSED");
            Ok(())
        } else {
            Err(format!("failed checks: {}", failed.join(", ")).into())
        }
    }
}

pub fn hyprctl(arguments: &[&str]) -> SpikeResult<String> {
    let output = Command::new("hyprctl").args(arguments).output()?;
    if !output.status.success() {
        return Err(format!(
            "hyprctl {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub fn cursor_position() -> SpikeResult<(f64, f64)> {
    let text = hyprctl(&["cursorpos"])?;
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("unexpected cursorpos output `{text}`"))?;
    Ok((x.trim().parse()?, y.trim().parse()?))
}

pub fn sleep_millis(millis: u64) {
    std::thread::sleep(Duration::from_millis(millis));
}

pub fn first_output(outputs: &[OutputGeometry]) -> SpikeResult<OutputGeometry> {
    outputs
        .first()
        .cloned()
        .ok_or_else(|| "no outputs reported".into())
}

pub fn park_cursor_at_center(spike: &Spike, output: &OutputGeometry) -> SpikeResult<()> {
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: f64::from(output.x + output.width / 2),
        y: f64::from(output.y + output.height / 2),
    })?;
    sleep_millis(100);
    Ok(())
}
