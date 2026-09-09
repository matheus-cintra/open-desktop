use opendesk_proto::input::{Axis, AxisSource};
use wayland_client::protocol::wl_pointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Proxy, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

use crate::globals::NoEvents;
use crate::outputs::LayoutBounds;
use crate::state::State;

const VALUE120_PER_STEP: i32 = 120;
const WITH_OUTPUT_SINCE_VERSION: u32 = 2;

pub struct VirtualPointer {
    object: ZwlrVirtualPointerV1,
}

impl VirtualPointer {
    pub fn new(
        manager: &ZwlrVirtualPointerManagerV1,
        seat: &WlSeat,
        queue_handle: &QueueHandle<State>,
    ) -> VirtualPointer {
        let object = if manager.version() >= WITH_OUTPUT_SINCE_VERSION {
            manager.create_virtual_pointer_with_output(Some(seat), None, queue_handle, NoEvents)
        } else {
            manager.create_virtual_pointer(Some(seat), queue_handle, NoEvents)
        };
        VirtualPointer { object }
    }

    pub fn absolute_motion(&self, time: u32, x: f64, y: f64, bounds: LayoutBounds) {
        let clamp = |value: f64, origin: i32, extent: i32| -> u32 {
            let relative = (value - f64::from(origin)).round();
            let limit = f64::from(extent.max(1) - 1);
            let clamped = relative.clamp(0.0, limit);
            u32::try_from(clamped as i64).unwrap_or(0)
        };
        let x_extent = u32::try_from(bounds.width.max(1)).unwrap_or(1);
        let y_extent = u32::try_from(bounds.height.max(1)).unwrap_or(1);
        self.object.motion_absolute(
            time,
            clamp(x, bounds.x, bounds.width),
            clamp(y, bounds.y, bounds.height),
            x_extent,
            y_extent,
        );
        self.object.frame();
    }

    pub fn motion(&self, time: u32, dx: f64, dy: f64) {
        self.object.motion(time, dx, dy);
        self.object.frame();
    }

    pub fn button(&self, time: u32, code: u32, pressed: bool) {
        let state = if pressed {
            wl_pointer::ButtonState::Pressed
        } else {
            wl_pointer::ButtonState::Released
        };
        self.object.button(time, code, state);
        self.object.frame();
    }

    pub fn axis(&self, time: u32, axis: Axis, value: f64, value120: i32, source: AxisSource) {
        let wl_axis = match axis {
            Axis::Vertical => wl_pointer::Axis::VerticalScroll,
            Axis::Horizontal => wl_pointer::Axis::HorizontalScroll,
        };
        let wl_source = match source {
            AxisSource::Wheel => wl_pointer::AxisSource::Wheel,
            AxisSource::Finger => wl_pointer::AxisSource::Finger,
            AxisSource::Continuous => wl_pointer::AxisSource::Continuous,
            AxisSource::WheelTilt => wl_pointer::AxisSource::WheelTilt,
        };
        self.object.axis_source(wl_source);
        if value120 == 0 {
            self.object.axis(time, wl_axis, value);
        } else {
            let discrete = discrete_steps(value120);
            self.object.axis_discrete(time, wl_axis, value, discrete);
        }
        self.object.frame();
    }
}

fn discrete_steps(value120: i32) -> i32 {
    let rounded = (f64::from(value120) / f64::from(VALUE120_PER_STEP)).round();
    if rounded == 0.0 {
        value120.signum()
    } else {
        rounded as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value120_maps_to_wheel_steps() {
        assert_eq!(discrete_steps(120), 1);
        assert_eq!(discrete_steps(-240), -2);
        assert_eq!(discrete_steps(30), 1);
        assert_eq!(discrete_steps(-30), -1);
        assert_eq!(discrete_steps(180), 2);
    }
}
