pub mod client;
pub mod server;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum IpcRequest {
    Status,
    Discover,
    Pair { name: String },
    SubmitPin { pin: String },
    PeerSet { name: String, side: String },
    PeerRemove { name: String },
    Release,
    Enable,
    Disable,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum IpcResponse {
    Status(StatusReport),
    Discovered(Vec<DiscoveredPeerReport>),
    PinRequired,
    Ok,
    Error { message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct StatusReport {
    pub name: String,
    pub peer_id: String,
    pub state: String,
    pub enabled: bool,
    pub peers: Vec<PeerStatus>,
    pub pending_pin: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PeerStatus {
    pub name: String,
    pub side: Option<String>,
    pub connected: bool,
    pub address: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DiscoveredPeerReport {
    pub name: String,
    pub peer_id: String,
    pub address: String,
    pub version: String,
    pub paired: bool,
}

const SOCKET_ENV: &str = "OPENDESK_IPC_SOCKET";

pub fn socket_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os(SOCKET_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("XDG_RUNTIME_DIR is not set"))?;
    Ok(PathBuf::from(runtime_dir).join("opendesk").join("ipc.sock"))
}
