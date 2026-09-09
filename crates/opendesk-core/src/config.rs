use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use opendesk_proto::control::Side;
use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::hotkey::Hotkey;

pub const CONFIG_ENV: &str = "OPENDESK_CONFIG";
const FALLBACK_NAME: &str = "opendesk";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine the user's home directory")]
    NoHomeDirectory,
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub name: String,
    pub port: u16,
    pub edge_threshold_px: f64,
    pub edge_cancel_px: f64,
    pub motion_rate_hz: u32,
    pub release_hotkey: Hotkey,
    pub clipboard_max_bytes: u64,
    #[serde(with = "home_relative_path")]
    pub dnd_dir: PathBuf,
    pub dnd_timeout_s: u64,
    pub dnd_keep_days: u32,
    pub bar_color: Rgba,
}

impl Default for General {
    fn default() -> General {
        General {
            name: local_hostname(),
            port: opendesk_proto::DEFAULT_PORT,
            edge_threshold_px: 60.0,
            edge_cancel_px: 8.0,
            motion_rate_hz: 0,
            release_hotkey: Hotkey {
                ctrl: true,
                alt: true,
                shift: false,
                logo: false,
                key: "escape".to_owned(),
            },
            clipboard_max_bytes: 10 * 1024 * 1024,
            dnd_dir: home_relative_path::expand("~/.cache/opendesk/dnd"),
            dnd_timeout_s: 120,
            dnd_keep_days: 7,
            bar_color: Rgba {
                red: 0x5e,
                green: 0x81,
                blue: 0xac,
                alpha: 0xCC,
            },
        }
    }
}

fn local_hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            std::env::var("HOSTNAME")
                .ok()
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| FALLBACK_NAME.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConfig {
    pub name: String,
    #[serde(with = "side_as_word")]
    pub side: Side,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<SocketAddr>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default, rename = "peer", skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<PeerConfig>,
}

impl Config {
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        if let Some(path) = std::env::var_os(CONFIG_ENV).filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(path));
        }
        dirs::config_dir()
            .map(|directory| directory.join("opendesk").join("config.toml"))
            .ok_or(ConfigError::NoHomeDirectory)
    }

    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config::default());
            }
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        toml::from_str(&content).map_err(|error| ConfigError::Parse {
            path: path.to_owned(),
            message: error.to_string(),
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        let write_error = |source| ConfigError::Write {
            path: path.to_owned(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(write_error)?;
        }
        std::fs::write(path, content).map_err(write_error)
    }

    pub fn peer_for_side(&self, side: Side) -> Option<&PeerConfig> {
        self.peers.iter().find(|peer| peer.side == side)
    }

    pub fn side_for_peer(&self, name: &str) -> Option<Side> {
        self.peers
            .iter()
            .find(|peer| peer.name == name)
            .map(|peer| peer.side)
    }

    pub fn set_peer_side(&mut self, name: &str, side: Side) {
        self.peers
            .retain(|peer| peer.side != side || peer.name == name);
        match self.peers.iter_mut().find(|peer| peer.name == name) {
            Some(peer) => peer.side = side,
            None => self.peers.push(PeerConfig {
                name: name.to_owned(),
                side,
                addr: None,
            }),
        }
    }

    pub fn remove_peer(&mut self, name: &str) -> bool {
        let before = self.peers.len();
        self.peers.retain(|peer| peer.name != name);
        self.peers.len() != before
    }
}

pub mod side_as_word {
    use opendesk_proto::control::Side;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(side: &Side, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(side.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Side, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

pub mod home_relative_path {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn expand(text: &str) -> PathBuf {
        match text
            .strip_prefix("~/")
            .and_then(|tail| dirs::home_dir().map(|home| home.join(tail)))
        {
            Some(expanded) => expanded,
            None => PathBuf::from(text),
        }
    }

    pub fn collapse(path: &Path) -> String {
        let collapsed = dirs::home_dir().and_then(|home| {
            path.strip_prefix(&home)
                .ok()
                .map(|tail| Path::new("~").join(tail))
        });
        collapsed
            .unwrap_or_else(|| path.to_owned())
            .to_string_lossy()
            .into_owned()
    }

    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&collapse(path))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(expand(&text))
    }
}
