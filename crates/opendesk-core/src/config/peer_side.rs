use opendesk_proto::control::Side;
use serde::{Deserialize, Deserializer, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerSide {
    Side(Side),
    All,
}

impl PeerSide {
    pub fn covers(self, side: Side) -> bool {
        match self {
            Self::All => true,
            Self::Side(candidate) => candidate == side,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Side(side) => side.as_str(),
            Self::All => "all",
        }
    }
}

impl std::fmt::Display for PeerSide {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for PeerSide {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.trim().eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        text.parse::<Side>()
            .map(Self::Side)
            .map_err(|error| error.to_string())
    }
}

pub fn serialize<S: Serializer>(side: &PeerSide, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(side.as_str())
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PeerSide, D::Error> {
    let text = String::deserialize(deserializer)?;
    text.parse().map_err(serde::de::Error::custom)
}
