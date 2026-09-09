mod spike_support;

use std::time::Duration;

use opendesk_proto::control::Side;
use opendesk_wayland::{StripSpec, WaylandCommand, WaylandEvent};
use spike_support::{
    Spike, SpikeResult, cursor_position, first_output, park_cursor_at_center, sleep_millis,
};

const RELATIVE_DX: f64 = 25.0;
const INWARD_DX: f64 = 40.0;
const TOLERANCE: f64 = 2.0;

fn main() -> SpikeResult<()> {
    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;
    println!(
        "output {} {}x{} at {},{}",
        output.name, output.width, output.height, output.x, output.y
    );
    let edge_x = f64::from(output.x + output.width - 1);
    let middle_y = f64::from(output.y + output.height / 2);
    let hint_y = f64::from(output.y + output.height / 4);

    park_cursor_at_center(&spike, &output)?;
    spike.send(WaylandCommand::ConfigureStrips {
        strips: vec![StripSpec {
            side: Side::Right,
            output: output.name.clone(),
        }],
    })?;
    sleep_millis(300);
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: edge_x,
        y: middle_y,
    })?;
    let entered = spike.wait_for(Duration::from_secs(1), |event| {
        matches!(event, WaylandEvent::EdgeEntered { .. })
    });
    println!(
        "edge event: {entered:?}, cursorpos after enter: {:?}",
        cursor_position()?
    );
    spike.check("EdgeEntered within 1 s", entered.is_some());

    spike.send(WaylandCommand::LockPointer)?;
    sleep_millis(100);
    spike.send(WaylandCommand::InjectMotion {
        dx: RELATIVE_DX,
        dy: 0.0,
    })?;
    let relative = spike.wait_for(Duration::from_secs(1), |event| {
        matches!(event, WaylandEvent::RelativeMotion { .. })
    });
    let locked_position = cursor_position()?;
    println!("relative event: {relative:?}, cursorpos while locked: {locked_position:?}");
    let dx_matches = matches!(relative, Some(WaylandEvent::RelativeMotion { dx, .. }) if (dx - RELATIVE_DX).abs() <= TOLERANCE);
    spike.check("RelativeMotion dx ~ 25 while locked", dx_matches);
    spike.check(
        "cursor stays at the edge while locked",
        (locked_position.0 - edge_x).abs() <= TOLERANCE,
    );

    spike.send(WaylandCommand::UnlockPointer { hint: Some(hint_y) })?;
    sleep_millis(500);
    let unlocked_position = cursor_position()?;
    println!("cursorpos after unlock hint {hint_y}: {unlocked_position:?}");
    println!(
        "events after unlock: {:?}",
        spike.collect_for(Duration::from_millis(200))
    );

    spike.send(WaylandCommand::StartGrab)?;
    sleep_millis(200);
    spike.send(WaylandCommand::InjectMotion {
        dx: -INWARD_DX,
        dy: 0.0,
    })?;
    let inward = spike.wait_for(Duration::from_secs(1), |event| {
        matches!(event, WaylandEvent::RelativeMotion { .. })
    });
    println!(
        "relative event while grabbed: {inward:?}, cursorpos: {:?}",
        cursor_position()?
    );
    spike.check(
        "RelativeMotion delivered while grabbed after inward motion",
        matches!(inward, Some(WaylandEvent::RelativeMotion { dx, .. }) if (dx + INWARD_DX).abs() <= TOLERANCE),
    );
    spike.send(WaylandCommand::StopGrab { hint: Some(hint_y) })?;
    sleep_millis(500);
    let released_position = cursor_position()?;
    println!("cursorpos after StopGrab hint {hint_y}: {released_position:?}");
    spike.check(
        "cursor back at the edge hint after StopGrab",
        (released_position.0 - edge_x).abs() <= TOLERANCE
            && (released_position.1 - hint_y).abs() <= TOLERANCE,
    );
    spike.check(
        "cursor reappears at the hint after unlock",
        (unlocked_position.1 - hint_y).abs() <= TOLERANCE,
    );

    spike.finish()
}
