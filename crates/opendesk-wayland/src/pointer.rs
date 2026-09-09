use opendesk_proto::input::{Axis, AxisSource};
use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::protocol::wl_pointer::{self, ButtonState, WlPointer};
use wayland_client::{Connection, QueueHandle, WEnum};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

use crate::events::WaylandEvent;
use crate::state::State;

const VALUE120_PER_STEP: i32 = 120;

#[derive(Default)]
pub struct PointerTracking {
    pub focused_strip: Option<usize>,
    frame: AxisFrame,
}

#[derive(Default)]
struct AxisFrame {
    source: Option<AxisSource>,
    vertical: AxisAccumulator,
    horizontal: AxisAccumulator,
}

#[derive(Default)]
struct AxisAccumulator {
    value: Option<f64>,
    value120: Option<i32>,
}

impl AxisFrame {
    fn accumulator(&mut self, axis: WEnum<wl_pointer::Axis>) -> Option<&mut AxisAccumulator> {
        match axis {
            WEnum::Value(wl_pointer::Axis::VerticalScroll) => Some(&mut self.vertical),
            WEnum::Value(wl_pointer::Axis::HorizontalScroll) => Some(&mut self.horizontal),
            _ => None,
        }
    }

    fn drain(&mut self) -> Vec<WaylandEvent> {
        let source = self.source.take().unwrap_or(AxisSource::Wheel);
        let pairs = [
            (Axis::Vertical, std::mem::take(&mut self.vertical)),
            (Axis::Horizontal, std::mem::take(&mut self.horizontal)),
        ];
        pairs
            .into_iter()
            .filter(|(_, accumulator)| {
                accumulator.value.is_some() || accumulator.value120.is_some()
            })
            .map(|(axis, accumulator)| WaylandEvent::Axis {
                axis,
                value: accumulator.value.unwrap_or(0.0),
                value120: accumulator.value120.unwrap_or(0),
                source,
            })
            .collect()
    }
}

fn axis_source(source: WEnum<wl_pointer::AxisSource>) -> AxisSource {
    match source {
        WEnum::Value(wl_pointer::AxisSource::Finger) => AxisSource::Finger,
        WEnum::Value(wl_pointer::AxisSource::Continuous) => AxisSource::Continuous,
        WEnum::Value(wl_pointer::AxisSource::WheelTilt) => AxisSource::WheelTilt,
        _ => AxisSource::Wheel,
    }
}

pub struct PointerData;

impl Dispatch2<WlPointer, State> for PointerData {
    fn event(
        &self,
        state: &mut State,
        _: &WlPointer,
        event: wl_pointer::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                state.devices.enter_serial = serial;
                let Some(index) = state.strips.index_of_surface(&surface) else {
                    return;
                };
                state.pointer.focused_strip = Some(index);
                state.show_cursor();
                let Some(entered) =
                    state
                        .strips
                        .entered(index, &state.output_state, (surface_x, surface_y))
                else {
                    return;
                };
                state.emit(entered);
            }
            wl_pointer::Event::Leave { surface, .. } => {
                let Some(index) = state.strips.index_of_surface(&surface) else {
                    return;
                };
                if state.pointer.focused_strip == Some(index) {
                    state.pointer.focused_strip = None;
                }
                if let Some(strip) = state.strips.get(index) {
                    state.emit(WaylandEvent::EdgeLeft {
                        side: strip.spec.side,
                    });
                }
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                if state.grab.active {
                    state.emit(WaylandEvent::Button {
                        code: button,
                        pressed: button_state == WEnum::Value(ButtonState::Pressed),
                    });
                }
            }
            wl_pointer::Event::AxisSource {
                axis_source: source,
            } => {
                state.pointer.frame.source = Some(axis_source(source));
            }
            wl_pointer::Event::Axis { axis, value, .. } => {
                if let Some(accumulator) = state.pointer.frame.accumulator(axis) {
                    accumulator.value = Some(accumulator.value.unwrap_or(0.0) + value);
                }
            }
            wl_pointer::Event::AxisDiscrete { axis, discrete } => {
                if let Some(accumulator) = state.pointer.frame.accumulator(axis) {
                    accumulator.value120 = Some(discrete.saturating_mul(VALUE120_PER_STEP));
                }
            }
            wl_pointer::Event::AxisValue120 { axis, value120 } => {
                if let Some(accumulator) = state.pointer.frame.accumulator(axis) {
                    accumulator.value120 = Some(value120);
                }
            }
            wl_pointer::Event::Frame => {
                let events = state.pointer.frame.drain();
                if state.grab.active {
                    for event in events {
                        state.emit(event);
                    }
                }
            }
            _ => {}
        }
    }
}

impl State {
    pub fn show_cursor(&self) {
        if let Some(device) = self.devices.cursor_shape_device.as_ref() {
            device.set_shape(self.devices.enter_serial, Shape::Default);
        }
    }

    pub fn hide_cursor(&self) {
        if let Some(pointer) = self.devices.pointer.as_ref() {
            pointer.set_cursor(self.devices.enter_serial, None, 0, 0);
        }
    }
}
