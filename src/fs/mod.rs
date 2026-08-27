use std::{ffi::OsString, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

mod browse;
mod mutation;
mod transfer_batch;
mod tree_copy;

#[allow(unused_imports)]
pub use browse::{
    FsError, open_directory_with, read_child_folders_with_hidden, read_directory_with,
    read_entry_details, search_directory_with_hidden, validate_name,
};
pub use mutation::{create_file, create_folder, delete_permanently, display_name, rename_entry};
pub use transfer_batch::{TransferBatch, TransferBatchOutcome};

pub(crate) use browse::format_size;
pub(crate) use mutation::{journal_copy, journal_move, journal_remove};

#[cfg(test)]
use mutation::{move_exact, rename_noreplace};
#[cfg(all(test, target_os = "linux"))]
use tree_copy::{get_xattr, set_xattr};
#[cfg(test)]
use tree_copy::{record_metadata_result, set_times};

#[cfg(test)]
pub use browse::{read_child_folders, read_directory, search_directory};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum SortKey {
    Name,
    Modified,
    Size,
    #[default]
    Type,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrowseOptions {
    pub view: ViewMode,
    pub sort: SortKey,
    pub descending: bool,
    pub show_hidden: bool,
}

impl Default for BrowseOptions {
    fn default() -> Self {
        Self {
            view: ViewMode::Grid,
            sort: SortKey::Type,
            descending: false,
            show_hidden: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: OsString,
    pub(crate) directory: bool,
    pub(crate) metadata: EntryMetadata,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EntryMetadata {
    pub(crate) size: Option<u64>,
    pub(crate) modified: Option<i64>,
}

pub(crate) fn entry_metadata(metadata: &fs::Metadata) -> EntryMetadata {
    EntryMetadata {
        size: Some(metadata.len()),
        modified: Some(std::os::unix::fs::MetadataExt::mtime(metadata)),
    }
}

#[derive(Debug)]
pub struct OpenedDirectory {
    pub canonical_path: PathBuf,
    pub entries: Vec<FileEntry>,
}

#[derive(Clone, Debug)]
pub struct SearchResults {
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct TransferFailure {
    pub source: PathBuf,
    pub error: String,
}

#[derive(Clone, Debug)]
pub struct TransferWarning {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct TransferReceipt {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub replaced_existing: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TransferReport {
    pub completed: Vec<PathBuf>,
    pub failures: Vec<TransferFailure>,
    pub retained: Vec<PathBuf>,
    pub warnings: Vec<TransferWarning>,
    pub receipts: Vec<TransferReceipt>,
    pub cancelled: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransferProgress {
    pub completed_entries: u64,
    pub total_entries: u64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictChoice {
    Replace,
    Skip,
    KeepBoth,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferConflict {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub directories: bool,
}
