mod spike_support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use opendesk_proto::control::Side;
use opendesk_wayland::{StripSpec, WaylandCommand, WaylandEvent};
use spike_support::{
    Spike, SpikeResult, cursor_position, first_output, hyprctl, park_cursor_at_center, sleep_millis,
};
use xkbcommon::xkb;

const KEY_SUPER_L: u32 = 125;
const KEY_F12: u32 = 88;

fn marker_path() -> SpikeResult<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")?;
    Ok(PathBuf::from(runtime_dir).join("opendesk-nested/spike-inhibit-marker"))
}

fn logo_mask(xkb_text: &str) -> SpikeResult<u32> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_string(
        &context,
        xkb_text.to_owned(),
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or("seat keymap did not compile")?;
    let index = keymap.mod_get_index(xkb::MOD_NAME_LOGO);
    if index == xkb::MOD_INVALID {
        return Err("keymap has no Mod4".into());
    }
    Ok(1 << index)
}

fn inject_super_f12(spike: &Spike, logo: u32) -> SpikeResult<()> {
    let steps = [
        (
            WaylandCommand::InjectKey {
                code: KEY_SUPER_L,
                pressed: true,
            },
            Some(logo),
        ),
        (
            WaylandCommand::InjectKey {
                code: KEY_F12,
                pressed: true,
            },
            None,
        ),
        (
            WaylandCommand::InjectKey {
                code: KEY_F12,
                pressed: false,
            },
            None,
        ),
        (
            WaylandCommand::InjectKey {
                code: KEY_SUPER_L,
                pressed: false,
            },
            Some(0),
        ),
    ];
    for (command, depressed) in steps {
        spike.send(command)?;
        if let Some(depressed) = depressed {
            spike.send(WaylandCommand::InjectModifiers {
                depressed,
                latched: 0,
                locked: 0,
                group: 0,
            })?;
        }
        sleep_millis(20);
    }
    Ok(())
}

fn wait_for_marker(marker: &Path, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if marker.exists() {
            return true;
        }
        sleep_millis(10);
    }
    marker.exists()
}

fn main() -> SpikeResult<()> {
    let marker = marker_path()?;
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }
    let bind = format!("SUPER, F12, exec, touch {}", marker.display());
    println!("bind: {}", hyprctl(&["keyword", "bind", &bind])?);

    let (mut spike, outputs) = Spike::start()?;
    let output = first_output(&outputs)?;
    let keymap = spike.wait_for(Duration::from_secs(2), |event| {
        matches!(event, WaylandEvent::Keymap { .. })
    });
    let Some(WaylandEvent::Keymap { xkb }) = keymap else {
        return Err("no Keymap event within 2 s".into());
    };
    let logo = logo_mask(&xkb)?;
    println!("logo modifier mask from the seat keymap: {logo}");
    spike.send(WaylandCommand::SetKeymap { xkb })?;
    sleep_millis(100);

    inject_super_f12(&spike, logo)?;
    let triggered = wait_for_marker(&marker, Duration::from_secs(1));
    spike.check(
        "case 1: virtual keyboard triggers the SUPER+F12 bind",
        triggered,
    );
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }

    park_cursor_at_center(&spike, &output)?;
    spike.send(WaylandCommand::ConfigureStrips {
        strips: vec![StripSpec {
            side: Side::Right,
            output: output.name.clone(),
        }],
    })?;
    sleep_millis(300);
    spike.send(WaylandCommand::InjectAbsoluteMotion {
        x: f64::from(output.x + output.width - 1),
        y: f64::from(output.y + output.height / 2),
    })?;
    let entered = spike.wait_for(Duration::from_secs(1), |event| {
        matches!(event, WaylandEvent::EdgeEntered { .. })
    });
    println!(
        "edge event: {entered:?}, cursorpos: {:?}",
        cursor_position()?
    );
    spike.check("case 2: EdgeEntered within 1 s", entered.is_some());
    spike.send(WaylandCommand::StartGrab)?;
    sleep_millis(100);
    inject_super_f12(&spike, logo)?;
    let triggered_while_grabbed = wait_for_marker(&marker, Duration::from_secs(1));
    let events = spike.collect_for(Duration::from_millis(200));
    println!("events while grabbed: {events:?}");
    let f12_seen = events.iter().any(
        |event| matches!(event, WaylandEvent::Key { code, pressed: true } if *code == KEY_F12),
    );
    spike.check("case 2: bind held while grabbed", !triggered_while_grabbed);
    spike.check("case 2: F12 press delivered through the API", f12_seen);
    spike.send(WaylandCommand::StopGrab { hint: None })?;
    sleep_millis(100);

    println!("unbind: {}", hyprctl(&["keyword", "unbind", "SUPER, F12"])?);
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }
    spike.finish()
}
