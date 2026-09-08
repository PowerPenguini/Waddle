use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

mod effects;
mod fingerprint;
mod removal;
mod store;
mod trash_receipt;

use effects::{Direction, apply};
use fingerprint::{DirectoryIdentity, Fingerprint, TreeFingerprint};
pub(crate) use store::{Effect, Journal};
pub(crate) use trash_receipt::trash;

#[cfg(test)]
use trash_receipt::percent_decode_path;

#[cfg(test)]
mod tests;

const VERSION: u32 = 1;
const MAX_OPERATIONS: usize = 100;
const MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug)]
pub(crate) enum Error {
    Io {
        context: String,
        source: io::Error,
    },
    Json {
        context: &'static str,
        source: serde_json::Error,
    },
    Desktop {
        context: String,
        source: gio::glib::Error,
    },
    Message(String),
}

impl Error {
    fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    fn json(context: &'static str, source: serde_json::Error) -> Self {
        Self::Json { context, source }
    }

    fn desktop(context: impl Into<String>, source: gio::glib::Error) -> Self {
        Self::Desktop {
            context: context.into(),
            source,
        }
    }

    fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(formatter, "{context}: {source}"),
            Self::Json { context, source } => write!(formatter, "{context}: {source}"),
            Self::Desktop { context, source } => write!(formatter, "{context}: {source}"),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Desktop { source, .. } => Some(source),
            Self::Message(_) => None,
        }
    }
}

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
    // A started Redo retained below newer actions when their recording forks history.
    #[serde(default)]
    redo_pending: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Action {
    Rename {
        #[serde(with = "crate::path_serde")]
        before: PathBuf,
        #[serde(with = "crate::path_serde")]
        after: PathBuf,
        fingerprint: Fingerprint,
    },
    NewFolder {
        #[serde(with = "crate::path_serde")]
        path: PathBuf,
        fingerprint: Fingerprint,
        #[serde(default)]
        identity: Option<DirectoryIdentity>,
    },
    NewFile {
        #[serde(with = "crate::path_serde")]
        path: PathBuf,
        fingerprint: Fingerprint,
    },
    Transfer {
        kind: TransferKind,
        items: Vec<TransferItem>,
        #[serde(default)]
        transfer: crate::fs::JournalTransfer,
    },
    Trash {
        items: Vec<TrashItem>,
        #[serde(default)]
        transfer: crate::fs::JournalTransfer,
    },
    Restore {
        items: Vec<TrashItem>,
        #[serde(default)]
        transfer: crate::fs::JournalTransfer,
        #[serde(default = "legacy_transfer_requires_refusal")]
        replaced_existing: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) enum TransferKind {
    Copy,
    Move,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct TransferItem {
    #[serde(with = "crate::path_serde")]
    source: PathBuf,
    #[serde(with = "crate::path_serde")]
    destination: PathBuf,
    source_fingerprint: TreeFingerprint,
    result_fingerprint: TreeFingerprint,
    #[serde(default = "legacy_transfer_requires_refusal")]
    replaced_existing: bool,
    #[serde(default)]
    undone: bool,
    #[serde(default)]
    removal: Option<removal::RemovalPlan>,
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
    #[serde(with = "crate::path_serde")]
    original: PathBuf,
    #[serde(with = "crate::path_serde")]
    trashed: PathBuf,
    #[serde(with = "crate::path_serde")]
    info: PathBuf,
    fingerprint: TreeFingerprint,
    // A completed physical restore awaiting the rest of the batch or metadata cleanup.
    #[serde(default)]
    restore_pending: bool,
    // A completed physical Trash awaiting the remaining entries in this batch.
    #[serde(default)]
    trash_pending: bool,
}

impl Action {
    pub(super) fn has_partial_effects(&self) -> bool {
        match self {
            Self::Transfer { items, .. } => {
                items.iter().any(|item| item.removal.is_some())
                    || items
                        .first()
                        .is_some_and(|first| items.iter().any(|item| item.undone != first.undone))
            }
            Self::Trash { items, .. } | Self::Restore { items, .. } => items
                .iter()
                .any(|item| item.restore_pending || item.trash_pending),
            _ => false,
        }
    }

    pub(crate) fn rename(before: PathBuf, after: PathBuf) -> Result<Self, Error> {
        Ok(Self::Rename {
            fingerprint: Fingerprint::read(&after)?,
            before,
            after,
        })
    }

    pub(crate) fn new_folder(path: PathBuf) -> Result<Self, Error> {
        Ok(Self::NewFolder {
            fingerprint: Fingerprint::read(&path)?,
            identity: Some(DirectoryIdentity::read(&path)?),
            path,
        })
    }

    pub(crate) fn new_file(path: PathBuf) -> Result<Self, Error> {
        Ok(Self::NewFile {
            fingerprint: Fingerprint::read(&path)?,
            path,
        })
    }

    pub(crate) fn transfer(
        kind: TransferKind,
        receipts: &[crate::fs::TransferReceipt],
    ) -> Result<Option<Self>, Error> {
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
                    undone: false,
                    removal: None,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Some(Self::Transfer {
            kind,
            items,
            transfer: Default::default(),
        }))
    }

    pub(crate) fn trash(receipts: &[TrashReceipt]) -> Result<Option<Self>, Error> {
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
                    restore_pending: false,
                    trash_pending: false,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Some(Self::Trash {
            items,
            transfer: Default::default(),
        }))
    }

    pub(crate) fn restore(
        receipts: &[TrashReceipt],
        replaced_existing: bool,
    ) -> Result<Option<Self>, Error> {
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
                    restore_pending: false,
                    trash_pending: false,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Some(Self::Restore {
            items,
            transfer: Default::default(),
            replaced_existing,
        }))
    }
}
