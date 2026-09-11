#[path = "dnd_client/mod.rs"]
mod dnd_client;

use std::path::PathBuf;
use std::time::Duration;

use dnd_client::{DndResult, DndRole, connect, file_uri, init_tracing, pump, pump_until};
use smithay_client_toolkit::shell::wlr_layer::Anchor;

const ISLAND: u32 = 200;

fn main() -> DndResult<()> {
    init_tracing();
    let path: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: drag_source <file-path>")?
        .into();
    let uri = file_uri(&path);

    let role = DndRole {
        uri: Some(uri.clone()),
        print_tokens: true,
        ..DndRole::default()
    };
    let (mut event_loop, mut client) = connect(role)?;
    client.create_layer(Anchor::TOP | Anchor::LEFT, ISLAND, ISLAND, true);
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(2),
        |client| client.mapped >= 1,
    )?;
    tracing::info!(%uri, "drag_source ready");
    println!("drag_source ready");

    loop {
        pump(&mut event_loop, &mut client, Duration::from_millis(50))?;
        if !client.drag_started {
            if let Some(serial) = client.last_button_serial {
                client.begin_drag(serial)?;
            }
        } else if client.source_cancelled || client.dnd_finished {
            break;
        }
    }
    tracing::info!("drag_source exiting");
    Ok(())
}
