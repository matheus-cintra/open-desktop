use opendesk_proto::control::OutputGeometry;
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::{Connection, QueueHandle};

use crate::events::WaylandEvent;
use crate::state::State;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn output_geometries(output_state: &OutputState) -> Vec<OutputGeometry> {
    output_state
        .outputs()
        .filter_map(|output| output_geometry(output_state, &output))
        .collect()
}

pub fn output_geometry(output_state: &OutputState, output: &WlOutput) -> Option<OutputGeometry> {
    let info = output_state.info(output)?;
    let (x, y) = info.logical_position.unwrap_or(info.location);
    let (width, height) = info
        .logical_size
        .or_else(|| {
            info.modes
                .iter()
                .find(|mode| mode.current)
                .map(|mode| mode.dimensions)
        })
        .unwrap_or((0, 0));
    Some(OutputGeometry {
        name: info
            .name
            .unwrap_or_else(|| format!("{}-{}", info.make, info.model)),
        x,
        y,
        width,
        height,
    })
}

pub fn find_output(output_state: &OutputState, name: &str) -> Option<WlOutput> {
    output_state.outputs().find(|output| {
        output_state
            .info(output)
            .and_then(|info| info.name)
            .is_some_and(|output_name| output_name == name)
    })
}

pub fn output_origin(output_state: &OutputState, output: &WlOutput) -> (i32, i32) {
    output_state
        .info(output)
        .map(|info| info.logical_position.unwrap_or(info.location))
        .unwrap_or((0, 0))
}

pub fn layout_bounds(outputs: &[OutputGeometry]) -> Option<LayoutBounds> {
    let min_x = outputs.iter().map(|output| output.x).min()?;
    let min_y = outputs.iter().map(|output| output.y).min()?;
    let max_x = outputs.iter().map(|output| output.x + output.width).max()?;
    let max_y = outputs
        .iter()
        .map(|output| output.y + output.height)
        .max()?;
    Some(LayoutBounds {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

impl State {
    fn outputs_changed(&mut self) {
        if !self.ready {
            return;
        }
        let outputs = output_geometries(&self.output_state);
        tracing::info!(?outputs, "outputs changed");
        self.emit(WaylandEvent::OutputsChanged { outputs });
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<State>, _: WlOutput) {
        self.outputs_changed();
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<State>, _: WlOutput) {
        self.outputs_changed();
    }

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<State>, _: WlOutput) {
        self.outputs_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, x: i32, y: i32, width: i32, height: i32) -> OutputGeometry {
        OutputGeometry {
            name: name.to_owned(),
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn bounds_cover_every_output() {
        let outputs = [
            output("DP-1", 0, 0, 2560, 1440),
            output("HDMI-A-1", -1920, 200, 1920, 1080),
        ];
        assert_eq!(
            layout_bounds(&outputs),
            Some(LayoutBounds {
                x: -1920,
                y: 0,
                width: 4480,
                height: 1440
            })
        );
        assert_eq!(layout_bounds(&[]), None);
    }
}
