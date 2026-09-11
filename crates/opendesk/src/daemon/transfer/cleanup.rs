use std::path::Path;
use std::time::{Duration, SystemTime};

use tracing::debug;

pub fn remove_expired(root_dir: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(root_dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let expired = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map(|modified| now.duration_since(modified).unwrap_or_default() > keep)
            .unwrap_or(false);
        if expired && let Err(error) = std::fs::remove_dir_all(&path) {
            debug!(%error, path = %path.display(), "could not remove an expired transfer");
        }
    }
}
