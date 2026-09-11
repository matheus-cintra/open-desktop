mod cleanup;
mod receive;
mod send;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use opendesk_proto::transfer::{DragInfo, TransferEntry};

pub use cleanup::remove_expired;
pub use receive::DropAccumulator;
pub use send::run_transfer;

pub struct TransferPlan {
    pub drag: DragInfo,
    sources: Vec<Option<PathBuf>>,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("unsafe relative path `{0}` in the transfer")]
    UnsafePath(String),
    #[error("chunk for entry {0} which is not an open file")]
    UnknownEntry(u32),
    #[error("no entries to transfer")]
    Empty,
}

fn io_error(path: &Path, source: std::io::Error) -> TransferError {
    TransferError::Io {
        path: path.display().to_string(),
        source,
    }
}

pub fn plan_transfer(paths: &[PathBuf], transfer_id: u64) -> Result<TransferPlan, TransferError> {
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| TransferError::UnsafePath(path.display().to_string()))?;
        add_entry(path, &name, &mut entries, &mut sources)?;
    }
    if entries.is_empty() {
        return Err(TransferError::Empty);
    }
    Ok(TransferPlan {
        drag: DragInfo {
            transfer_id,
            entries,
        },
        sources,
    })
}

fn add_entry(
    path: &Path,
    relative_path: &str,
    entries: &mut Vec<TransferEntry>,
    sources: &mut Vec<Option<PathBuf>>,
) -> Result<(), TransferError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.is_dir() {
        entries.push(TransferEntry {
            relative_path: relative_path.to_owned(),
            size: 0,
            is_directory: true,
        });
        sources.push(None);
        let mut children: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|error| io_error(path, error))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        children.sort();
        for child in children {
            if let Some(name) = child
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            {
                let nested = format!("{relative_path}/{name}");
                add_entry(&child, &nested, entries, sources)?;
            }
        }
        return Ok(());
    }
    if metadata.is_file() {
        entries.push(TransferEntry {
            relative_path: relative_path.to_owned(),
            size: metadata.len(),
            is_directory: false,
        });
        sources.push(Some(path.to_owned()));
    }
    Ok(())
}

pub fn is_safe_relative_path(relative_path: &str) -> bool {
    if relative_path.is_empty() {
        return false;
    }
    let candidate = Path::new(relative_path);
    candidate.is_relative()
        && candidate
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

pub fn top_level_paths(root: &Path, entries: &[TransferEntry]) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|entry| !entry.relative_path.contains('/'))
        .map(|entry| root.join(&entry.relative_path))
        .collect()
}
