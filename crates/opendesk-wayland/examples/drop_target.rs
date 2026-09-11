#[path = "dnd_client/mod.rs"]
mod dnd_client;

use std::time::Duration;

use dnd_client::{DndResult, DndRole, connect, init_tracing, pump, pump_until};
use smithay_client_toolkit::shell::wlr_layer::Anchor;

const ISLAND: u32 = 200;

fn main() -> DndResult<()> {
    init_tracing();
    let role = DndRole {
        accept_on_enter: true,
        read_on_drop: true,
        print_tokens: true,
        ..DndRole::default()
    };
    let (mut event_loop, mut client) = connect(role)?;
    client.create_layer(Anchor::BOTTOM | Anchor::RIGHT, ISLAND, ISLAND, true);
    pump_until(
        &mut event_loop,
        &mut client,
        Duration::from_secs(2),
        |client| client.mapped >= 1,
    )?;
    tracing::info!("drop_target ready");
    println!("drop_target ready");

    loop {
        pump(&mut event_loop, &mut client, Duration::from_millis(50))?;
        if client.data_drop {
            break;
        }
    }
    tracing::info!("drop_target exiting after the first drop");
    Ok(())
}
