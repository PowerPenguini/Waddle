use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use gio::prelude::*;
use serde::{Deserialize, Serialize};

mod effects;
mod fingerprint;
mod store;
mod trash_receipt;

use effects::{Direction, apply};
use fingerprint::{Fingerprint, TreeFingerprint};
pub(crate) use store::{Effect, Journal};
pub(crate) use trash_receipt::trash;

#[cfg(test)]
use trash_receipt::percent_decode_path;

#[cfg(test)]
mod tests;

const VERSION: u32 = 1;
const MAX_OPERATIONS: usize = 100;
const MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredJournal {
    version: u32,
    cursor: usize,
    entries: Vec<Entry>,
}

impl Default for StoredJournal {
    fn default() -> Self {
        Self {
            version: VERSION,
            cursor: 0,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Entry {
    recorded_at: u64,
    action: Action,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Action {
    Rename {
        before: PathBuf,
        after: PathBuf,
        fingerprint: Fingerprint,
    },
    NewFolder {
        path: PathBuf,
        fingerprint: Fingerprint,
    },
    NewFile {
        path: PathBuf,
        fingerprint: Fingerprint,
    },
    Transfer {
        kind: TransferKind,
        items: Vec<TransferItem>,
    },
    Trash {
        items: Vec<TrashItem>,
    },
    Restore {
        items: Vec<TrashItem>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) enum TransferKind {
    Copy,
    Move,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct TransferItem {
    source: PathBuf,
    destination: PathBuf,
    source_fingerprint: TreeFingerprint,
    result_fingerprint: TreeFingerprint,
    #[serde(default = "legacy_transfer_requires_refusal")]
    replaced_existing: bool,
}

fn legacy_transfer_requires_refusal() -> bool {
    true
}

#[derive(Clone, Debug)]
pub(crate) struct TrashReceipt {
    pub(crate) original: PathBuf,
    pub(crate) trashed: PathBuf,
    pub(crate) info: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct TrashItem {
    original: PathBuf,
    trashed: PathBuf,
    info: PathBuf,
    fingerprint: TreeFingerprint,
}

impl Action {
    pub(crate) fn rename(before: PathBuf, after: PathBuf) -> Result<Self, String> {
        Ok(Self::Rename {
            fingerprint: Fingerprint::read(&after)?,
            before,
            after,
        })
    }

    pub(crate) fn new_folder(path: PathBuf) -> Result<Self, String> {
        Ok(Self::NewFolder {
            fingerprint: Fingerprint::read(&path)?,
            path,
        })
    }

    pub(crate) fn new_file(path: PathBuf) -> Result<Self, String> {
        Ok(Self::NewFile {
            fingerprint: Fingerprint::read(&path)?,
            path,
        })
    }

    pub(crate) fn transfer(
        kind: TransferKind,
        receipts: &[crate::fs::TransferReceipt],
    ) -> Result<Option<Self>, String> {
        if receipts.is_empty() {
            return Ok(None);
        }
        let items = receipts
            .iter()
            .map(|receipt| {
                let result_fingerprint = TreeFingerprint::read(&receipt.destination)?;
                let source_fingerprint = match kind {
                    TransferKind::Copy => TreeFingerprint::read(&receipt.source)?,
                    TransferKind::Move => result_fingerprint.clone(),
                };
                Ok(TransferItem {
                    source: receipt.source.clone(),
                    destination: receipt.destination.clone(),
                    source_fingerprint,
                    result_fingerprint,
                    replaced_existing: receipt.replaced_existing,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(Self::Transfer { kind, items }))
    }

    pub(crate) fn trash(receipts: &[TrashReceipt]) -> Result<Option<Self>, String> {
        if receipts.is_empty() {
            return Ok(None);
        }
        let items = receipts
            .iter()
            .map(|receipt| {
                Ok(TrashItem {
                    original: receipt.original.clone(),
                    trashed: receipt.trashed.clone(),
                    info: receipt.info.clone(),
                    fingerprint: TreeFingerprint::read(&receipt.trashed)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(Self::Trash { items }))
    }

    pub(crate) fn restore(receipts: &[TrashReceipt]) -> Result<Option<Self>, String> {
        if receipts.is_empty() {
            return Ok(None);
        }
        let items = receipts
            .iter()
            .map(|receipt| {
                Ok(TrashItem {
                    original: receipt.original.clone(),
                    trashed: receipt.trashed.clone(),
                    info: receipt.info.clone(),
                    fingerprint: TreeFingerprint::read(&receipt.original)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(Self::Restore { items }))
    }
}
