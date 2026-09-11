use std::collections::HashMap;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use opendesk_proto::transfer::{FileBegin, FileChunk, FileEnd, TransferEntry};

use super::{TransferError, io_error, is_safe_relative_path, top_level_paths};

struct ActiveTransfer {
    root: PathBuf,
    entries: Vec<TransferEntry>,
    files: HashMap<u32, std::fs::File>,
}

pub struct DropAccumulator {
    root_dir: PathBuf,
    active: HashMap<u64, ActiveTransfer>,
}

impl DropAccumulator {
    pub fn new(root_dir: PathBuf) -> DropAccumulator {
        DropAccumulator {
            root_dir,
            active: HashMap::new(),
        }
    }

    pub fn begin(&mut self, begin: FileBegin) -> Result<(), TransferError> {
        for entry in &begin.entries {
            if !is_safe_relative_path(&entry.relative_path) {
                return Err(TransferError::UnsafePath(entry.relative_path.clone()));
            }
        }
        let root = self
            .root_dir
            .join(format!("transfer_{}", begin.transfer_id));
        std::fs::create_dir_all(&root).map_err(|error| io_error(&root, error))?;
        let mut files = HashMap::new();
        for (index, entry) in begin.entries.iter().enumerate() {
            let target = root.join(&entry.relative_path);
            if entry.is_directory {
                std::fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
            }
            let file = std::fs::File::create(&target).map_err(|error| io_error(&target, error))?;
            files.insert(index as u32, file);
        }
        self.active.insert(
            begin.transfer_id,
            ActiveTransfer {
                root,
                entries: begin.entries,
                files,
            },
        );
        Ok(())
    }

    pub fn chunk(&mut self, chunk: FileChunk) -> Result<(), TransferError> {
        let transfer = self
            .active
            .get_mut(&chunk.transfer_id)
            .ok_or(TransferError::UnknownEntry(chunk.entry_index))?;
        let file = transfer
            .files
            .get_mut(&chunk.entry_index)
            .ok_or(TransferError::UnknownEntry(chunk.entry_index))?;
        file.seek(SeekFrom::Start(chunk.offset))
            .map_err(|error| io_error(&transfer.root, error))?;
        file.write_all(&chunk.bytes)
            .map_err(|error| io_error(&transfer.root, error))?;
        Ok(())
    }

    pub fn end(&mut self, end: FileEnd) -> Result<Vec<PathBuf>, TransferError> {
        let Some(mut transfer) = self.active.remove(&end.transfer_id) else {
            return Ok(Vec::new());
        };
        if !end.success {
            let _ = std::fs::remove_dir_all(&transfer.root);
            return Ok(Vec::new());
        }
        for file in transfer.files.values_mut() {
            file.flush()
                .map_err(|error| io_error(&transfer.root, error))?;
        }
        Ok(top_level_paths(&transfer.root, &transfer.entries))
    }

    pub fn cancel(&mut self, transfer_id: u64) {
        if let Some(transfer) = self.active.remove(&transfer_id) {
            let _ = std::fs::remove_dir_all(&transfer.root);
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }
}
