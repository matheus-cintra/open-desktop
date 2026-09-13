use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::transfer::{DragInfo, FileBegin, FileChunk, FileEnd};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown side `{0}`, expected left, right, top or bottom")]
pub struct UnknownSide(pub String);

impl FromStr for Side {
    type Err = UnknownSide;

    fn from_str(text: &str) -> Result<Side, UnknownSide> {
        match text.trim().to_ascii_lowercase().as_str() {
            "left" => Ok(Side::Left),
            "right" => Ok(Side::Right),
            "top" => Ok(Side::Top),
            "bottom" => Ok(Side::Bottom),
            other => Err(UnknownSide(other.to_owned())),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub [u8; 16]);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("peer id must be 32 hexadecimal characters")]
pub struct InvalidPeerId;

impl PeerId {
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn parse_hex(text: &str) -> Result<PeerId, InvalidPeerId> {
        if text.len() != 32 {
            return Err(InvalidPeerId);
        }
        let mut bytes = [0u8; 16];
        for (index, slot) in bytes.iter_mut().enumerate() {
            let pair = text.get(index * 2..index * 2 + 2).ok_or(InvalidPeerId)?;
            *slot = u8::from_str_radix(pair, 16).map_err(|_| InvalidPeerId)?;
        }
        Ok(PeerId(bytes))
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct Token(pub [u8; 32]);

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(..)")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OutputGeometry {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    ProtocolVersion { expected: u16, received: u16 },
    Unauthorized,
    Busy,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenyReason {
    AlreadyControlling,
    AlreadyControlled,
    Disabled,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseReason {
    EdgeCrossed,
    Hotkey,
    Disconnect,
    Disabled,
    DragCancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ControlMessage {
    Hello {
        protocol_version: u16,
        peer_id: PeerId,
        name: String,
        token: Option<Token>,
        udp_port: u16,
        layout: Vec<OutputGeometry>,
    },
    HelloAck {
        peer_id: PeerId,
        name: String,
        udp_port: u16,
        layout: Vec<OutputGeometry>,
    },
    HelloRejected {
        reason: RejectReason,
    },
    PairRequest {
        peer_id: PeerId,
        name: String,
    },
    PairPin {
        pin: String,
    },
    PairAccepted {
        token: Token,
    },
    PairRejected {
        remaining_attempts: u8,
    },
    LayoutChanged {
        layout: Vec<OutputGeometry>,
    },
    RequestControl {
        side: Side,
        fraction: f32,
        drag: Option<DragInfo>,
    },
    ControlGranted {
        session_id: u32,
    },
    ControlDenied {
        reason: DenyReason,
    },
    ReleaseControl {
        fraction: Option<f32>,
        reason: ReleaseReason,
    },
    Keymap {
        xkb: String,
    },
    Key {
        code: u32,
        pressed: bool,
    },
    Button {
        code: u32,
        pressed: bool,
    },
    Modifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
    ClipboardSet {
        mime: String,
        bytes: Vec<u8>,
    },
    FileBegin(FileBegin),
    FileChunk(FileChunk),
    FileEnd(FileEnd),
    DragCancel {
        transfer_id: u64,
    },
    Ping {
        nonce: u32,
    },
    Pong {
        nonce: u32,
    },
    ReturnDrag {
        drag: DragInfo,
    },
    Capabilities {
        macos: bool,
        file_drag: bool,
    },
    Map(crate::map::MapControl),
    PhysicalKey {
        usage: u16,
        pressed: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_round_trips_through_text() {
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            assert_eq!(side.as_str().parse::<Side>(), Ok(side));
            assert_eq!(side.opposite().opposite(), side);
        }
        assert_eq!(
            "diagonal".parse::<Side>(),
            Err(UnknownSide("diagonal".to_owned()))
        );
    }

    #[test]
    fn peer_id_round_trips_through_hex() {
        let peer_id = PeerId([0xab; 16]);
        assert_eq!(PeerId::parse_hex(&peer_id.to_hex()), Ok(peer_id));
        assert_eq!(PeerId::parse_hex("abc"), Err(InvalidPeerId));
        assert_eq!(PeerId::parse_hex(&"zz".repeat(16)), Err(InvalidPeerId));
    }
}
