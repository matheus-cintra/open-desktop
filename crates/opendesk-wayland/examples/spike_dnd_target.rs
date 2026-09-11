mod spike_support;

#[path = "dnd_client/mod.rs"]
mod dnd_client;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use dnd_client::{
    BTN_LEFT, ChildClient, DndClient, DndResult, DndRole, connect, pump, pump_until, spawn_example,
    wait_for_child_line,
};
use opendesk_wayland::WaylandCommand;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use spike_support::{Spike, first_output, hyprctl, sleep_millis};

fn temp_uri_file() -> DndResult<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;
    let path = PathBuf::from(runtime_dir).join("opendesk-dnd-target.txt");
    std::fs::write(&path, b"opendesk drop payload\n")?;
    Ok(path)
}

fn pump_while_waiting(
    event_loop: &mut EventLoop<'static, DndClient>,
    client: &mut DndClient,
    child: &ChildClient,
    needle: &str,
    timeout: Duration,
) -> DndResult<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.saw(needle) {
            return Ok(true);
        }
        pump(event_loop, client, Duration::from_millis(20))?;
    }
    Ok(child.saw(needle))
}

fn main() -> DndResult<()> {
    hyprctl(&["keyword", "misc:always_follow_on_dnd", "true"])?;
    let file = temp_uri_file()?;
    let uri = format!("file://{}", file.display());

    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;

    let child = spawn_example("drop_target", &[])?;
    if !wait_for_child_line(&child, "drop_target ready", Duration::from_secs(3)) {
        child.stop();
        return Err("drop_target did not map its surface".into());
    }

    let role = DndRole {
        uri: Some(uri.clone()),
        ..DndRole::default()
    };
    let (mut event_loop, mut client) = connect(role)?;
    client.create_layer(Anchor::all(), 0, 0, false);
    client.create_icon()?;
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(2),
        |client| client.mapped >= 1,
    )?;

    let center_x = f64::from(output.x + output.width / 2);
    let center_y = f64::from(output.y + output.height / 2);
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: center_x,
        y: center_y,
    })?;
    sleep_millis(60);
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(1),
        |client| client.pointer_entered,
    )?;

    spike.send(WaylandCommand::InjectButton {
        code: BTN_LEFT,
        pressed: true,
    })?;
    sleep_millis(60);
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(1),
        |client| client.last_button_serial.is_some(),
    )?;
    let serial = client.last_button_serial;
    let d1 = serial.is_some();
    println!(
        "{} D1: got serial {:?}",
        if d1 { "PASS" } else { "FAIL" },
        serial
    );
    spike.check("D1: overlay captured the button serial", d1);

    let Some(serial) = serial else {
        child.stop();
        return spike.finish();
    };
    client.begin_drag(serial)?;
    client.destroy_origin();
    pump(&mut event_loop, &mut client, Duration::from_millis(120))?;

    let drop_x = f64::from(output.x + output.width - 40);
    let drop_y = f64::from(output.y + output.height - 40);
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: drop_x,
        y: drop_y,
    })?;
    sleep_millis(40);
    spike.send(WaylandCommand::InjectMotion { dx: -4.0, dy: -4.0 })?;
    let d2 = pump_while_waiting(
        &mut event_loop,
        &mut client,
        &child,
        "ENTER",
        Duration::from_secs(2),
    )?;
    println!(
        "{} D2: target saw drag enter",
        if d2 { "PASS" } else { "FAIL" }
    );
    spike.check(
        "D2: drag survives origin destroy and reaches the target",
        d2,
    );

    spike.send(WaylandCommand::InjectButton {
        code: BTN_LEFT,
        pressed: false,
    })?;
    let dropped = pump_while_waiting(
        &mut event_loop,
        &mut client,
        &child,
        "DROP",
        Duration::from_secs(3),
    )?;
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(1),
        |client| client.dnd_finished,
    )?;
    let dropped_line = child.line_with("DROP");
    let d3 = dropped
        && client.send_count > 0
        && dropped_line.as_deref() == Some(format!("DROP {uri}").as_str());
    println!(
        "{} D3: drop delivered the uri (served {} time(s)) line {:?}",
        if d3 { "PASS" } else { "FAIL" },
        client.send_count,
        dropped_line
    );
    spike.check("D3: the drop delivers the uri to the target", d3);

    child.stop();
    spike.finish()?;
    Ok(())
}
