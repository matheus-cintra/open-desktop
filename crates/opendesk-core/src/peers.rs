use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use opendesk_proto::control::{PeerId, Token};
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigError};

const OWNER_ONLY: u32 = 0o600;

#[derive(Debug, thiserror::Error)]
pub enum PeerStoreError {
    #[error(transparent)]
    Config(#[from] ConfigError),
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
    #[error("failed to serialize peer store: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecord {
    #[serde(with = "peer_id_as_hex")]
    pub id: PeerId,
    pub name: String,
    #[serde(with = "token_as_hex")]
    pub token: Token,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerStore {
    #[serde(default, rename = "peer", skip_serializing_if = "Vec::is_empty")]
    records: Vec<PeerRecord>,
}

impl PeerStore {
    pub fn default_path() -> Result<PathBuf, PeerStoreError> {
        Ok(Config::default_path()?.with_file_name("peers.toml"))
    }

    pub fn load(path: &Path) -> Result<PeerStore, PeerStoreError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PeerStore::default());
            }
            Err(source) => {
                return Err(PeerStoreError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        toml::from_str(&content).map_err(|error| PeerStoreError::Parse {
            path: path.to_owned(),
            message: error.to_string(),
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), PeerStoreError> {
        use std::io::Write;

        let content = toml::to_string_pretty(self)?;
        let write_error = |source| PeerStoreError::Write {
            path: path.to_owned(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(write_error)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(OWNER_ONLY)
            .open(path)
            .map_err(write_error)?;
        file.set_permissions(std::fs::Permissions::from_mode(OWNER_ONLY))
            .map_err(write_error)?;
        file.write_all(content.as_bytes()).map_err(write_error)
    }

    pub fn find_by_id(&self, id: PeerId) -> Option<&PeerRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&PeerRecord> {
        self.records.iter().find(|record| record.name == name)
    }

    pub fn upsert(&mut self, record: PeerRecord) {
        match self
            .records
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            Some(existing) => *existing = record,
            None => self.records.push(record),
        }
    }

    pub fn remove(&mut self, id: PeerId) -> bool {
        let before = self.records.len();
        self.records.retain(|record| record.id != id);
        self.records.len() != before
    }

    pub fn records(&self) -> &[PeerRecord] {
        &self.records
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex<const LENGTH: usize>(text: &str) -> Option<[u8; LENGTH]> {
    if text.len() != LENGTH * 2 {
        return None;
    }
    let mut bytes = [0u8; LENGTH];
    for (index, slot) in bytes.iter_mut().enumerate() {
        let pair = text.get(index * 2..index * 2 + 2)?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

mod peer_id_as_hex {
    use opendesk_proto::control::PeerId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(id: &PeerId, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&id.to_hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PeerId, D::Error> {
        let text = String::deserialize(deserializer)?;
        PeerId::parse_hex(&text).map_err(serde::de::Error::custom)
    }
}

mod token_as_hex {
    use opendesk_proto::control::Token;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(token: &Token, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::encode_hex(&token.0))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Token, D::Error> {
        let text = String::deserialize(deserializer)?;
        super::decode_hex::<32>(&text)
            .map(Token)
            .ok_or_else(|| serde::de::Error::custom("token must be 64 hexadecimal characters"))
    }
}
