use serde::{Deserialize, Serialize};

pub const CHUNK_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TransferEntry {
    pub relative_path: String,
    pub size: u64,
    pub is_directory: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DragInfo {
    pub transfer_id: u64,
    pub entries: Vec<TransferEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileBegin {
    pub transfer_id: u64,
    pub entries: Vec<TransferEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileChunk {
    pub transfer_id: u64,
    pub entry_index: u32,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileEnd {
    pub transfer_id: u64,
    pub success: bool,
}
