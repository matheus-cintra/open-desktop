#[cfg(test)]
mod common;

use std::os::unix::fs::PermissionsExt;

use opendesk_core::peers::{PeerRecord, PeerStore, PeerStoreError};
use opendesk_core::pin::{generate_peer_id, generate_token};

use common::TempDir;

fn record(name: &str) -> PeerRecord {
    PeerRecord {
        id: generate_peer_id(),
        name: name.to_owned(),
        token: generate_token(),
    }
}

#[test]
fn missing_file_loads_an_empty_store() {
    let directory = TempDir::new("peers-missing");
    let store = PeerStore::load(&directory.path().join("peers.toml")).unwrap();
    assert!(store.records().is_empty());
}

#[test]
fn save_writes_owner_only_and_reloads() {
    let directory = TempDir::new("peers-roundtrip");
    let path = directory.path().join("nested").join("peers.toml");
    let mut store = PeerStore::default();
    let notebook = record("notebook");
    store.upsert(notebook.clone());
    store.upsert(record("tablet"));
    store.save(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains(&format!("id = \"{}\"", notebook.id.to_hex())),
        "{text}"
    );
    assert!(text.contains("[[peer]]"), "{text}");
    let reloaded = PeerStore::load(&path).unwrap();
    assert_eq!(reloaded, store);
    assert_eq!(reloaded.find_by_id(notebook.id), Some(&notebook));
    assert_eq!(reloaded.find_by_name("notebook"), Some(&notebook));
}

#[test]
fn save_resets_permissions_on_an_existing_file() {
    let directory = TempDir::new("peers-chmod");
    let path = directory.path().join("peers.toml");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    PeerStore::default().save(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn upsert_replaces_by_id_and_remove_reports_presence() {
    let mut store = PeerStore::default();
    let mut notebook = record("notebook");
    store.upsert(notebook.clone());
    notebook.name = "renamed".to_owned();
    store.upsert(notebook.clone());
    assert_eq!(store.records(), &[notebook.clone()]);
    assert!(store.remove(notebook.id));
    assert!(!store.remove(notebook.id));
    assert!(store.records().is_empty());
}

#[test]
fn malformed_token_is_a_parse_error() {
    let directory = TempDir::new("peers-bad-token");
    let path = directory.path().join("peers.toml");
    std::fs::write(
        &path,
        format!(
            "[[peer]]\nid = \"{}\"\nname = \"x\"\ntoken = \"abc\"\n",
            "00".repeat(16)
        ),
    )
    .unwrap();
    let result = PeerStore::load(&path);
    assert!(
        matches!(result, Err(PeerStoreError::Parse { .. })),
        "{result:?}"
    );
    if let Err(PeerStoreError::Parse { message, .. }) = result {
        assert!(message.contains("64 hexadecimal"), "{message}");
    }
}

#[test]
fn default_path_is_a_sibling_of_the_config() {
    let path = PeerStore::default_path().unwrap();
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("peers.toml")
    );
}
