mod spike_support;

#[path = "dnd_client/mod.rs"]
mod dnd_client;

use std::path::PathBuf;
use std::time::Duration;

use dnd_client::{BTN_LEFT, DndResult, spawn_example, wait_for_child_line};
use opendesk_proto::control::{OutputGeometry, Side};
use opendesk_wayland::{StripSpec, WaylandCommand, WaylandEvent};
use spike_support::{Spike, first_output, hyprctl, sleep_millis};

fn temp_file(name: &str, payload: &[u8]) -> DndResult<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;
    let path = PathBuf::from(runtime_dir).join(name);
    std::fs::write(&path, payload)?;
    Ok(path)
}

fn wait_keymap(spike: &mut Spike) -> DndResult<String> {
    match spike.wait_for(Duration::from_secs(2), |event| {
        matches!(event, WaylandEvent::Keymap { .. })
    }) {
        Some(WaylandEvent::Keymap { xkb }) => Ok(xkb),
        _ => Err("no seat keymap within 2 s".into()),
    }
}

fn wait_drag_entered(spike: &mut Spike) -> Option<Vec<PathBuf>> {
    let event = spike.wait_for(Duration::from_secs(3), |event| {
        matches!(event, WaylandEvent::DragEnteredEdge { .. })
    })?;
    match event {
        WaylandEvent::DragEnteredEdge { uris, .. } => {
            tracing::info!(?uris, "strip reported drag enter");
            Some(uris)
        }
        _ => None,
    }
}

fn run_source_path(spike: &mut Spike, output: &OutputGeometry) -> DndResult<()> {
    let file = temp_file("opendesk-dnd-prod-source.txt", b"opendesk drag payload\n")?;

    let child = spawn_example("drag_source", &[file.to_string_lossy().as_ref()])?;
    if !wait_for_child_line(&child, "drag_source ready", Duration::from_secs(3)) {
        child.stop();
        return Err("drag_source did not map its surface".into());
    }

    spike.send(WaylandCommand::ConfigureStrips {
        strips: vec![StripSpec {
            side: Side::Right,
            output: output.name.clone(),
        }],
    })?;
    sleep_millis(200);

    let island_x = f64::from(output.x + 100);
    let island_y = f64::from(output.y + 100);
    let edge_x = f64::from(output.x + output.width - 1);
    let middle_y = f64::from(output.y + output.height / 2);

    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: island_x,
        y: island_y,
    })?;
    sleep_millis(80);
    spike.send(WaylandCommand::InjectButton {
        code: BTN_LEFT,
        pressed: true,
    })?;
    if !wait_for_child_line(&child, "drag started", Duration::from_secs(2)) {
        child.stop();
        return Err("drag_source never started its drag".into());
    }

    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: edge_x,
        y: middle_y,
    })?;
    sleep_millis(40);
    spike.send(WaylandCommand::InjectMotion { dx: 6.0, dy: 0.0 })?;

    let uris = wait_drag_entered(spike);
    let s1 = uris
        .as_ref()
        .is_some_and(|uris| uris.iter().any(|path| path == &file));
    println!(
        "{} S1: strip peeked the local uri {:?}",
        if s1 { "PASS" } else { "FAIL" },
        uris
    );
    spike.check("S1: DragEnteredEdge carries the local file path", s1);

    spike.send(WaylandCommand::AbortLocalDrag)?;
    let s2 = wait_for_child_line(&child, "cancelled", Duration::from_secs(2))
        || wait_for_child_line(&child, "dnd_finished", Duration::from_secs(1));
    println!(
        "{} S2: AbortLocalDrag ended the app drag",
        if s2 { "PASS" } else { "FAIL" }
    );
    spike.check("S2: AbortLocalDrag aborts the in-progress app drag", s2);

    let s3 = spike
        .wait_for(Duration::from_secs(2), |event| {
            matches!(event, WaylandEvent::EdgeEntered { .. })
        })
        .is_some();
    println!(
        "{} S3: strip got pointer focus after the abort",
        if s3 { "PASS" } else { "FAIL" }
    );
    spike.check("S3: strip gets pointer focus after the abort", s3);

    child.stop();
    spike.send(WaylandCommand::ConfigureStrips { strips: Vec::new() })?;
    sleep_millis(100);
    Ok(())
}

fn run_target_path(spike: &mut Spike, output: &OutputGeometry) -> DndResult<()> {
    let file = temp_file("opendesk-dnd-prod-target.txt", b"opendesk drop payload\n")?;
    let uri = format!("file://{}", file.display());

    let child = spawn_example("drop_target", &[])?;
    if !wait_for_child_line(&child, "drop_target ready", Duration::from_secs(3)) {
        child.stop();
        return Err("drop_target did not map its surface".into());
    }

    let center_x = f64::from(output.x + output.width / 2);
    let center_y = f64::from(output.y + output.height / 2);
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: center_x,
        y: center_y,
    })?;
    sleep_millis(80);

    spike.send(WaylandCommand::StartDropDrag {
        uris: vec![file.clone()],
    })?;
    sleep_millis(400);

    let drop_x = f64::from(output.x + output.width - 40);
    let drop_y = f64::from(output.y + output.height - 40);
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: drop_x,
        y: drop_y,
    })?;
    sleep_millis(40);
    spike.send(WaylandCommand::InjectMotion { dx: -4.0, dy: -4.0 })?;
    let entered = wait_for_child_line(&child, "ENTER", Duration::from_secs(2));
    println!("note T: drop_target saw drag enter = {entered}");

    spike.send(WaylandCommand::InjectButton {
        code: BTN_LEFT,
        pressed: false,
    })?;
    let dropped = wait_for_child_line(&child, "DROP", Duration::from_secs(3));
    let line = child.line_with("DROP");
    let t1 = dropped && line.as_deref() == Some(format!("DROP {uri}").as_str());
    println!(
        "{} T1: drop delivered the uri, line {:?}",
        if t1 { "PASS" } else { "FAIL" },
        line
    );
    spike.check("T1: StartDropDrag drops the uri on the real app", t1);

    child.stop();
    Ok(())
}

fn main() -> DndResult<()> {
    hyprctl(&["keyword", "misc:always_follow_on_dnd", "true"])?;

    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;
    let xkb = wait_keymap(&mut spike)?;
    spike.send(WaylandCommand::SetKeymap { xkb })?;
    sleep_millis(100);

    let source = run_source_path(&mut spike, &output);
    if let Err(error) = &source {
        println!("FAIL source path: {error}");
    }
    let target = run_target_path(&mut spike, &output);
    if let Err(error) = &target {
        println!("FAIL target path: {error}");
    }

    let outcome = spike.finish();
    source?;
    target?;
    outcome
}
