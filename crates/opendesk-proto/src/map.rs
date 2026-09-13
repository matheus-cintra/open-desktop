//! Shared logical desktop geometry. Names are labels, never routing identities.
use crate::control::{PeerId, Side};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Revision {
    pub counter: u64,
    pub author: PeerId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub peer: PeerId,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesktopMap {
    pub group: PeerId,
    pub revision: Revision,
    pub screens: Vec<Screen>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlEpoch {
    pub claim: Revision,
    pub session: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MapControl {
    Clock {
        counter: u64,
    },
    Sync(Option<DesktopMap>),
    Synced {
        revision: Revision,
    },
    Identify,
    Availability {
        ready: bool,
        input_status: String,
    },
    Claim {
        revision: Revision,
    },
    Revoke {
        epoch: ControlEpoch,
    },
    ReturnDrag {
        epoch: ControlEpoch,
        side: Side,
        fraction: f32,
        drag: crate::transfer::DragInfo,
    },
    Prepare {
        epoch: ControlEpoch,
        revision: Revision,
        drag: Option<crate::transfer::DragInfo>,
        side: Side,
        fraction: f32,
    },
    Prepared {
        epoch: ControlEpoch,
        accepted: bool,
    },
    Commit {
        epoch: ControlEpoch,
    },
    Committed {
        epoch: ControlEpoch,
    },
    Release {
        epoch: ControlEpoch,
    },
    Released {
        epoch: ControlEpoch,
    },
    Edge {
        epoch: ControlEpoch,
        side: Side,
        fraction: f32,
    },
    Renew {
        epoch: ControlEpoch,
    },
    Renewed {
        epoch: ControlEpoch,
    },
    Input {
        epoch: ControlEpoch,
        message: Box<crate::control::ControlMessage>,
    },
}
