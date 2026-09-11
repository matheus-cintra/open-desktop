mod spike_support;

#[path = "dnd_client/mod.rs"]
mod dnd_client;

use std::path::PathBuf;
use std::time::Duration;

use dnd_client::{
    BTN_LEFT, DndResult, DndRole, connect, pump, pump_until, spawn_example, wait_for_child_line,
};
use opendesk_wayland::{WaylandCommand, WaylandEvent};
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use spike_support::{Spike, first_output, hyprctl, sleep_millis};

const KEY_ESCAPE: u32 = 1;
const STRIP_WIDTH: u32 = 12;

fn temp_uri_file() -> DndResult<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;
    let path = PathBuf::from(runtime_dir).join("opendesk-dnd-source.txt");
    std::fs::write(&path, b"opendesk drag payload\n")?;
    Ok(path)
}

fn main() -> DndResult<()> {
    hyprctl(&["keyword", "misc:always_follow_on_dnd", "true"])?;
    let file = temp_uri_file()?;
    let uri = format!("file://{}", file.display());

    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;
    let Some(WaylandEvent::Keymap { xkb }) = spike.wait_for(Duration::from_secs(2), |event| {
        matches!(event, WaylandEvent::Keymap { .. })
    }) else {
        return Err("no seat keymap within 2 s".into());
    };
    spike.send(WaylandCommand::SetKeymap { xkb })?;
    sleep_millis(100);

    let child = spawn_example("drag_source", &[file.to_string_lossy().as_ref()])?;
    if !wait_for_child_line(&child, "drag_source ready", Duration::from_secs(3)) {
        child.stop();
        return Err("drag_source did not map its surface".into());
    }

    let role = DndRole {
        accept_on_enter: true,
        read_on_enter: true,
        ..DndRole::default()
    };
    let (mut event_loop, mut client) = connect(role)?;
    client.create_layer(
        Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
        STRIP_WIDTH,
        0,
        true,
    );
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(2),
        |client| client.mapped >= 1,
    )?;

    let left = f64::from(output.x + 100);
    let top = f64::from(output.y + 100);
    let edge_x = f64::from(output.x + output.width - 1);
    let middle_y = f64::from(output.y + output.height / 2);

    spike.send(WaylandCommand::InjectAbsoluteMotion { x: left, y: top })?;
    sleep_millis(80);
    pump(&mut event_loop, &mut client, Duration::from_millis(150))?;
    spike.send(WaylandCommand::InjectButton {
        code: BTN_LEFT,
        pressed: true,
    })?;
    sleep_millis(80);
    pump(&mut event_loop, &mut client, Duration::from_millis(150))?;
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
    let peeked = pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(2),
        |client| {
            client.data_enter
                && client
                    .peeked_uri
                    .lock()
                    .map(|slot| slot.is_some())
                    .unwrap_or(false)
        },
    )?;
    let peeked_uri = client.peeked_uri.lock().ok().and_then(|slot| slot.clone());
    let c1 = peeked && peeked_uri.as_deref() == Some(uri.as_str());
    println!(
        "{} C1: strip peeked uri {:?}",
        if c1 { "PASS" } else { "FAIL" },
        peeked_uri
    );
    spike.check("C1: strip peeks the uri before drop", c1);

    spike.send(WaylandCommand::InjectKey {
        code: KEY_ESCAPE,
        pressed: true,
    })?;
    spike.send(WaylandCommand::InjectKey {
        code: KEY_ESCAPE,
        pressed: false,
    })?;
    let c2 = wait_for_child_line(&child, "cancelled", Duration::from_secs(2))
        || wait_for_child_line(&child, "dnd_finished", Duration::from_secs(1));
    println!(
        "{} C2: Escape aborted the app drag",
        if c2 { "PASS" } else { "FAIL" }
    );
    spike.check("C2: Escape aborts the drag", c2);

    client.reset_pointer();
    spike.send(WaylandCommand::InjectMotion { dx: -4.0, dy: 0.0 })?;
    spike.send(WaylandCommand::InjectMotion { dx: 4.0, dy: 0.0 })?;
    let c3 = pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(1),
        |client| client.pointer_entered,
    )?;
    println!(
        "{} C3: strip got pointer focus with the button still held",
        if c3 { "PASS" } else { "FAIL" }
    );
    spike.check("C3: strip gets pointer focus while button held", c3);

    spike.send(WaylandCommand::InjectButton {
        code: BTN_LEFT,
        pressed: false,
    })?;
    sleep_millis(60);
    client.reset_pointer();
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: edge_x,
        y: middle_y,
    })?;
    spike.send(WaylandCommand::InjectMotion { dx: -4.0, dy: 0.0 })?;
    let after_release = pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(1),
        |client| client.pointer_entered,
    )?;
    println!("note C3: strip pointer focus after button release = {after_release}");

    child.stop();
    spike.finish()?;
    Ok(())
}
