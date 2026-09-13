use std::error::Error;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use opendesk_wayland::{WaylandCommand, WaylandEvent, spawn};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

type InjectResult<T> = Result<T, Box<dyn Error>>;

const SETTLE: Duration = Duration::from_millis(20);
const KEYMAP_WAIT: Duration = Duration::from_millis(500);

fn main() -> InjectResult<()> {
    let steps: Vec<String> = std::env::args().skip(1).collect();
    let stdin_mode = steps == ["--stdin"];
    if steps.is_empty() {
        eprintln!(
            "usage: inject [--stdin] <abs:X,Y|motion:DX,DY|key:CODE:down|up|mods:DEPRESSED|button:CODE:down|up|sleep:MS>..."
        );
        return Err("no steps given".into());
    }
    let (sender, mut events) = unbounded_channel();
    let handle = spawn(sender)?;
    let ready = wait_for(&mut events, Duration::from_secs(2), |event| {
        matches!(event, WaylandEvent::Ready { .. })
    })
    .ok_or("no Ready event within 2 s")?;
    if stdin_mode
        && let WaylandEvent::Ready { outputs } = ready
        && let Some(output) = outputs.first()
    {
        let x = f64::from(output.x) + f64::from(output.width) / 2.0;
        let y = f64::from(output.y) + f64::from(output.height) / 2.0;
        handle
            .commands
            .send(WaylandCommand::InjectAbsoluteMotion { x, y })?;
        std::thread::sleep(SETTLE);
    }
    if let Some(WaylandEvent::Keymap { xkb }) = wait_for(&mut events, KEYMAP_WAIT, |event| {
        matches!(event, WaylandEvent::Keymap { .. })
    }) {
        handle.commands.send(WaylandCommand::SetKeymap { xkb })?;
    }
    if stdin_mode {
        println!("inject ready");
        std::io::stdout().flush()?;
        for line in std::io::stdin().lock().lines() {
            let line = line?;
            if line == "quit" {
                break;
            }
            run_step(&handle, &line)?;
            println!("ok");
            std::io::stdout().flush()?;
        }
    } else {
        for step in &steps {
            run_step(&handle, step)?;
        }
    }
    std::thread::sleep(Duration::from_millis(100));
    handle.shutdown()?;
    Ok(())
}

fn run_step(handle: &opendesk_wayland::WaylandHandle, step: &str) -> InjectResult<()> {
    match parse_step(step)? {
        Step::Sleep(duration) => std::thread::sleep(duration),
        Step::Command(command) => handle.commands.send(command)?,
    }
    std::thread::sleep(SETTLE);
    Ok(())
}

enum Step {
    Command(WaylandCommand),
    Sleep(Duration),
}

fn parse_step(step: &str) -> InjectResult<Step> {
    let mut parts = step.split(':');
    let kind = parts.next().unwrap_or_default();
    let arguments: Vec<&str> = parts.collect();
    let command = match (kind, arguments.as_slice()) {
        ("abs", [pair]) => {
            let (x, y) = parse_pair(pair)?;
            WaylandCommand::InjectAbsoluteMotion { x, y }
        }
        ("motion", [pair]) => {
            let (dx, dy) = parse_pair(pair)?;
            WaylandCommand::InjectMotion { dx, dy }
        }
        ("key", [code, state]) => WaylandCommand::InjectKey {
            code: code.parse()?,
            pressed: parse_state(state)?,
        },
        ("button", [code, state]) => WaylandCommand::InjectButton {
            code: code.parse()?,
            pressed: parse_state(state)?,
        },
        ("mods", [depressed]) => WaylandCommand::InjectModifiers {
            depressed: depressed.parse()?,
            latched: 0,
            locked: 0,
            group: 0,
        },
        ("sleep", [millis]) => return Ok(Step::Sleep(Duration::from_millis(millis.parse()?))),
        _ => return Err(format!("unknown step `{step}`").into()),
    };
    Ok(Step::Command(command))
}

fn parse_pair(text: &str) -> InjectResult<(f64, f64)> {
    let (first, second) = text
        .split_once(',')
        .ok_or_else(|| format!("expected X,Y in `{text}`"))?;
    Ok((first.parse()?, second.parse()?))
}

fn parse_state(text: &str) -> InjectResult<bool> {
    match text {
        "down" => Ok(true),
        "up" => Ok(false),
        other => Err(format!("expected down or up, got `{other}`").into()),
    }
}

fn wait_for(
    events: &mut UnboundedReceiver<WaylandEvent>,
    timeout: Duration,
    predicate: impl Fn(&WaylandEvent) -> bool,
) -> Option<WaylandEvent> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match events.try_recv() {
            Ok(event) if predicate(&event) => return Some(event),
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    None
}
