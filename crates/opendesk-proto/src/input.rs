use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisSource {
    Wheel,
    Finger,
    Continuous,
    WheelTilt,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    Motion {
        dx: f64,
        dy: f64,
    },
    Axis {
        axis: Axis,
        value: f64,
        value120: i32,
        source: AxisSource,
    },
    Keepalive,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputHeader {
    pub epoch: Option<crate::map::ControlEpoch>,
    pub session_id: u32,
    pub sequence: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct InputDatagram {
    pub header: InputHeader,
    pub event: InputEvent,
}

pub fn is_newer(sequence: u32, last_seen: u32) -> bool {
    sequence.wrapping_sub(last_seen).cast_signed() > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_comparison_survives_wraparound() {
        assert!(is_newer(1, 0));
        assert!(!is_newer(0, 1));
        assert!(!is_newer(5, 5));
        assert!(is_newer(0, u32::MAX));
        assert!(!is_newer(u32::MAX, 0));
    }
}
