use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::Context;
use opendesk_core::pin::generate_peer_id;
use opendesk_proto::control::PeerId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    peer_id: String,
}

pub fn load_or_create(path: &Path) -> anyhow::Result<PeerId> {
    if path.exists() {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading identity file {}", path.display()))?;
        let file: IdentityFile = toml::from_str(&text)
            .with_context(|| format!("parsing identity file {}", path.display()))?;
        return PeerId::parse_hex(&file.peer_id)
            .with_context(|| format!("invalid peer id in {}", path.display()));
    }
    let peer_id = generate_peer_id();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let text = toml::to_string(&IdentityFile {
        peer_id: peer_id.to_hex(),
    })
    .context("serializing identity file")?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating identity file {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("writing identity file {}", path.display()))?;
    Ok(peer_id)
}
