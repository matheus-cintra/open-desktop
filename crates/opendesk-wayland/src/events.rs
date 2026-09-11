use opendesk_proto::control::{OutputGeometry, Side};
use opendesk_proto::input::{Axis, AxisSource};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripSpec {
    pub side: Side,
    pub output: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarStyle {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Default for BarStyle {
    fn default() -> BarStyle {
        BarStyle {
            red: 0x5e,
            green: 0x81,
            blue: 0xac,
            alpha: 0xCC,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WaylandEvent {
    Ready {
        outputs: Vec<OutputGeometry>,
    },
    OutputsChanged {
        outputs: Vec<OutputGeometry>,
    },
    Keymap {
        xkb: String,
    },
    EdgeEntered {
        side: Side,
        output: String,
        position: f64,
    },
    EdgeLeft {
        side: Side,
    },
    RelativeMotion {
        dx: f64,
        dy: f64,
    },
    Button {
        code: u32,
        pressed: bool,
    },
    Axis {
        axis: Axis,
        value: f64,
        value120: i32,
        source: AxisSource,
    },
    Key {
        code: u32,
        pressed: bool,
    },
    Modifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
    HotkeyPressed,
    Fatal {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum WaylandCommand {
    ConfigureStrips {
        strips: Vec<StripSpec>,
    },
    SetReleaseHotkey {
        hotkey: HotkeySpec,
    },
    LockPointer,
    UnlockPointer {
        hint: Option<f64>,
    },
    StartGrab,
    StopGrab {
        hint: Option<f64>,
    },
    SetKeymap {
        xkb: String,
    },
    SetBarStyle {
        style: BarStyle,
    },
    ShowProgressBar {
        side: Side,
        position: f64,
        progress: f32,
    },
    HideProgressBar,
    ShowArrivalBar {
        side: Side,
        position: f64,
    },
    InjectAbsoluteMotion {
        x: f64,
        y: f64,
    },
    InjectMotion {
        dx: f64,
        dy: f64,
    },
    InjectButton {
        code: u32,
        pressed: bool,
    },
    InjectAxis {
        axis: Axis,
        value: f64,
        value120: i32,
        source: AxisSource,
    },
    InjectKey {
        code: u32,
        pressed: bool,
    },
    InjectModifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
    Shutdown,
}
