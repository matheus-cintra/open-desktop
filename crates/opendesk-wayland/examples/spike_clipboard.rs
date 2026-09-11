mod spike_support;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opendesk_wayland::{ClipboardContent, WaylandCommand, WaylandEvent};
use spike_support::{
    Spike, SpikeResult, cursor_position, first_output, park_cursor_at_center, sleep_millis,
};

const TINY_PNG: [u8; 69] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

const LARGE_BYTES: usize = 17_000_000;

fn token() -> SpikeResult<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos())
}

fn temp_path(name: &str) -> SpikeResult<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;
    Ok(PathBuf::from(runtime_dir).join(name))
}

fn run(program: &str, arguments: &[&str], stdin: Option<PathBuf>) -> SpikeResult<()> {
    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(path) = stdin {
        command.stdin(Stdio::from(std::fs::File::open(path)?));
    }
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{program} {arguments:?} failed with {status}").into());
    }
    Ok(())
}

fn wl_paste(arguments: &[&str]) -> SpikeResult<Vec<u8>> {
    let output = Command::new("wl-paste").args(arguments).output()?;
    if !output.status.success() {
        return Err(format!(
            "wl-paste {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}

fn changed_content(event: &WaylandEvent) -> Option<&ClipboardContent> {
    match event {
        WaylandEvent::ClipboardChanged { content } => Some(content),
        _ => None,
    }
}

fn text_out(spike: &mut Spike, value: &str) -> SpikeResult<()> {
    spike.send(WaylandCommand::SetClipboard {
        content: ClipboardContent {
            mime: "text/plain;charset=utf-8".to_owned(),
            bytes: value.as_bytes().to_vec(),
        },
    })?;
    sleep_millis(300);
    let pasted = wl_paste(&["-n"])?;
    spike.check(
        "text out: wl-paste returns the set text",
        pasted == value.as_bytes(),
    );
    Ok(())
}

fn text_in(spike: &mut Spike, value: &str) -> SpikeResult<()> {
    spike.collect_for(Duration::from_millis(300));
    run("wl-copy", &["-n", value], None)?;
    let expected = value.as_bytes().to_vec();
    let event = spike.wait_for(Duration::from_secs(1), |event| {
        changed_content(event).is_some_and(|content| {
            content.mime == "text/plain;charset=utf-8" && content.bytes == expected
        })
    });
    spike.check(
        "text in: ClipboardChanged with the copied text",
        event.is_some(),
    );
    Ok(())
}

fn image_in(spike: &mut Spike) -> SpikeResult<()> {
    let path = temp_path("opendesk-clip.png")?;
    std::fs::write(&path, TINY_PNG)?;
    spike.collect_for(Duration::from_millis(300));
    run("wl-copy", &["-t", "image/png"], Some(path.clone()))?;
    let event = spike.wait_for(Duration::from_secs(1), |event| {
        changed_content(event)
            .is_some_and(|content| content.mime == "image/png" && content.bytes == TINY_PNG)
    });
    spike.check(
        "image in: ClipboardChanged with the png bytes",
        event.is_some(),
    );

    spike.send(WaylandCommand::SetClipboard {
        content: ClipboardContent {
            mime: "image/png".to_owned(),
            bytes: TINY_PNG.to_vec(),
        },
    })?;
    sleep_millis(300);
    let pasted = wl_paste(&["-n", "-t", "image/png"])?;
    spike.check(
        "image round-trip: wl-paste returns the png bytes",
        pasted == TINY_PNG,
    );
    std::fs::remove_file(&path)?;
    Ok(())
}

fn large_drop(spike: &mut Spike) -> SpikeResult<()> {
    let path = temp_path("opendesk-clip-large.bin")?;
    let mut file = std::fs::File::create(&path)?;
    file.write_all(&vec![b'a'; LARGE_BYTES])?;
    drop(file);
    spike.collect_for(Duration::from_millis(300));
    run("wl-copy", &["-t", "text/plain"], Some(path.clone()))?;
    let events = spike.collect_for(Duration::from_secs(1));
    let leaked = events.iter().any(|event| changed_content(event).is_some());
    spike.check("large drop: no ClipboardChanged over the cap", !leaked);
    std::fs::remove_file(&path)?;
    Ok(())
}

fn main() -> SpikeResult<()> {
    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;
    park_cursor_at_center(&spike, &output)?;
    println!(
        "output: {}, cursorpos: {:?}",
        output.name,
        cursor_position()?
    );

    text_out(&mut spike, &format!("opendesk-clip-{}", token()?))?;
    text_in(&mut spike, &format!("hello-from-wl-{}", token()?))?;
    image_in(&mut spike)?;
    large_drop(&mut spike)?;

    spike.finish()
}
