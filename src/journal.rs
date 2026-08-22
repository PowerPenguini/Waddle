use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use gio::prelude::*;
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Fingerprint {
    kind: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct TreeFingerprint {
    root: Fingerprint,
    digest: u64,
}

impl TreeFingerprint {
    fn read(path: &Path) -> Result<Self, String> {
        let mut digest = Fnv::default();
        hash_tree(path, Path::new(""), &mut digest)?;
        Ok(Self {
            root: Fingerprint::read(path)?,
            digest: digest.0,
        })
    }
}

struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

fn hash_tree(path: &Path, relative: &Path, digest: &mut Fnv) -> Result<(), String> {
    use std::{io::Read, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt};

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not fingerprint {}: {error}", path.display()))?;
    digest.write(relative.as_os_str().as_bytes());
    digest.write(&metadata.mode().to_le_bytes());
    digest.write(&metadata.size().to_le_bytes());
    digest.write(&metadata.mtime().to_le_bytes());
    digest.write(&metadata.mtime_nsec().to_le_bytes());
    if metadata.file_type().is_symlink() {
        digest.write(
            fs::read_link(path)
                .map_err(|error| error.to_string())?
                .as_os_str()
                .as_bytes(),
        );
    } else if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .map_err(|error| format!("could not fingerprint {}: {error}", path.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            hash_tree(&entry.path(), &relative.join(entry.file_name()), digest)?;
        }
    } else {
        let mut file = fs::File::open(path)
            .map_err(|error| format!("could not fingerprint {}: {error}", path.display()))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            digest.write(&buffer[..read]);
        }
    }
    Ok(())
}

impl Fingerprint {
    fn read(path: &Path) -> Result<Self, String> {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("could not verify {}: {error}", path.display()))?;
        Ok(Self {
            kind: metadata.mode() & libc::S_IFMT,
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Effect {
    pub(crate) status: String,
    pub(crate) changed_folders: Vec<PathBuf>,
    pub(crate) select: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct Journal {
    path: PathBuf,
    stored: StoredJournal,
}

impl Journal {
    pub(crate) fn open_default() -> Result<Self, String> {
        Self::open(default_path())
    }

    pub(crate) fn empty_default() -> Self {
        Self {
            path: default_path(),
            stored: StoredJournal::default(),
        }
    }

    pub(crate) fn open(path: PathBuf) -> Result<Self, String> {
        let mut stored = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<StoredJournal>(&bytes)
                .map_err(|error| format!("could not read operation journal: {error}"))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => StoredJournal::default(),
            Err(error) => return Err(format!("could not read operation journal: {error}")),
        };
        if stored.version != VERSION {
            return Err(format!(
                "operation journal version {} is unsupported",
                stored.version
            ));
        }
        stored.cursor = stored.cursor.min(stored.entries.len());
        let mut journal = Self { path, stored };
        journal.prune(now_seconds());
        Ok(journal)
    }

    pub(crate) fn record(&mut self, action: Action) -> Result<(), String> {
        self.record_at(action, now_seconds())
    }

    fn record_at(&mut self, action: Action, recorded_at: u64) -> Result<(), String> {
        self.stored.entries.truncate(self.stored.cursor);
        self.stored.entries.push(Entry {
            recorded_at,
            action,
        });
        self.stored.cursor = self.stored.entries.len();
        self.prune(recorded_at);
        self.save()
    }

    pub(crate) fn undo(&mut self) -> Result<Effect, String> {
        let Some(index) = self.stored.cursor.checked_sub(1) else {
            return Err("Nothing to undo".to_owned());
        };
        let effect = apply(&mut self.stored.entries[index].action, Direction::Undo)?;
        self.stored.cursor = index;
        self.save()?;
        Ok(effect)
    }

    pub(crate) fn redo(&mut self) -> Result<Effect, String> {
        let Some(entry) = self.stored.entries.get_mut(self.stored.cursor) else {
            return Err("Nothing to redo".to_owned());
        };
        let effect = apply(&mut entry.action, Direction::Redo)?;
        self.stored.cursor += 1;
        self.save()?;
        Ok(effect)
    }

    fn prune(&mut self, now: u64) {
        let oldest = now.saturating_sub(MAX_AGE_SECONDS);
        let expired = self
            .stored
            .entries
            .partition_point(|entry| entry.recorded_at < oldest);
        if expired > 0 {
            self.stored.entries.drain(..expired);
            self.stored.cursor = self.stored.cursor.saturating_sub(expired);
        }
        if self.stored.entries.len() > MAX_OPERATIONS {
            let excess = self.stored.entries.len() - MAX_OPERATIONS;
            self.stored.entries.drain(..excess);
            self.stored.cursor = self.stored.cursor.saturating_sub(excess);
        }
    }

    fn save(&self) -> Result<(), String> {
        let Some(directory) = self.path.parent() else {
            return Err("operation journal path has no parent".to_owned());
        };
        fs::create_dir_all(directory)
            .map_err(|error| format!("could not create operation journal directory: {error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.stored)
            .map_err(|error| format!("could not encode operation journal: {error}"))?;
        fs::write(&temporary, bytes)
            .map_err(|error| format!("could not write operation journal: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("could not commit operation journal: {error}"))
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Undo,
    Redo,
}

fn apply(action: &mut Action, direction: Direction) -> Result<Effect, String> {
    match action {
        Action::Rename {
            before,
            after,
            fingerprint,
        } => {
            let (source, destination, label) = match direction {
                Direction::Undo => (after.as_path(), before.as_path(), "Undid rename"),
                Direction::Redo => (before.as_path(), after.as_path(), "Redid rename"),
            };
            verify(source, fingerprint)?;
            ensure_absent(destination)?;
            rename_noreplace(source, destination)?;
            *fingerprint = Fingerprint::read(destination)?;
            Ok(Effect {
                status: label.to_owned(),
                changed_folders: parent_folders(source, destination),
                select: Some(destination.to_path_buf()),
            })
        }
        Action::NewFolder { path, fingerprint } => match direction {
            Direction::Undo => {
                let mut entries = fs::read_dir(&*path)
                    .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
                if entries.next().is_some() {
                    return Err(format!(
                        "Refused Undo: {} is no longer empty",
                        path.display()
                    ));
                }
                verify(path, fingerprint)?;
                fs::remove_dir(&*path)
                    .map_err(|error| format!("could not undo New Folder: {error}"))?;
                Ok(Effect {
                    status: "Undid New Folder".to_owned(),
                    changed_folders: path.parent().map(Path::to_path_buf).into_iter().collect(),
                    select: None,
                })
            }
            Direction::Redo => {
                ensure_absent(path)?;
                fs::create_dir(&*path)
                    .map_err(|error| format!("could not redo New Folder: {error}"))?;
                *fingerprint = Fingerprint::read(path)?;
                Ok(Effect {
                    status: "Redid New Folder".to_owned(),
                    changed_folders: path.parent().map(Path::to_path_buf).into_iter().collect(),
                    select: Some(path.clone()),
                })
            }
        },
        Action::Transfer { kind, items } => apply_transfer(*kind, items, direction),
        Action::Trash { items } => apply_trash(items, direction),
        Action::Restore { items } => {
            let mut effect = apply_trash(
                items,
                match direction {
                    Direction::Undo => Direction::Redo,
                    Direction::Redo => Direction::Undo,
                },
            )?;
            effect.status = match direction {
                Direction::Undo => "Undid Restore",
                Direction::Redo => "Redid Restore",
            }
            .to_owned();
            Ok(effect)
        }
    }
}

fn apply_trash(items: &mut [TrashItem], direction: Direction) -> Result<Effect, String> {
    match direction {
        Direction::Undo => {
            for item in items.iter() {
                verify_tree(&item.trashed, &item.fingerprint)?;
                ensure_absent(&item.original)?;
            }
            let mut moved: Vec<TrashItem> = Vec::new();
            for item in items.iter() {
                if let Err(error) = crate::fs::journal_move(&item.trashed, &item.original) {
                    for previous in moved.iter().rev() {
                        let _ = crate::fs::journal_move(&previous.original, &previous.trashed);
                    }
                    return Err(error);
                }
                moved.push(item.clone());
            }
            for item in items.iter() {
                fs::remove_file(&item.info).map_err(|error| {
                    format!("restored the item but could not remove Trash metadata: {error}")
                })?;
            }
            Ok(trash_effect(items, Direction::Undo))
        }
        Direction::Redo => {
            for item in items.iter() {
                verify_tree(&item.original, &item.fingerprint)?;
            }
            let mut receipts = Vec::new();
            for item in items.iter() {
                match trash(&item.original) {
                    Ok(receipt) => receipts.push(receipt),
                    Err(error) => {
                        for receipt in receipts.iter().rev() {
                            let _ = crate::fs::journal_move(&receipt.trashed, &receipt.original);
                            let _ = fs::remove_file(&receipt.info);
                        }
                        return Err(error);
                    }
                }
            }
            for (item, receipt) in items.iter_mut().zip(receipts) {
                item.trashed = receipt.trashed;
                item.info = receipt.info;
                item.fingerprint = TreeFingerprint::read(&item.trashed)?;
            }
            Ok(trash_effect(items, Direction::Redo))
        }
    }
}

fn trash_effect(items: &[TrashItem], direction: Direction) -> Effect {
    Effect {
        status: match direction {
            Direction::Undo => "Undid Trash",
            Direction::Redo => "Redid Trash",
        }
        .to_owned(),
        changed_folders: items
            .iter()
            .filter_map(|item| item.original.parent().map(Path::to_path_buf))
            .fold(Vec::new(), |mut folders, folder| {
                if !folders.contains(&folder) {
                    folders.push(folder);
                }
                folders
            }),
        select: matches!(direction, Direction::Undo)
            .then(|| items.first().map(|item| item.original.clone()))
            .flatten(),
    }
}

fn apply_transfer(
    kind: TransferKind,
    items: &mut [TransferItem],
    direction: Direction,
) -> Result<Effect, String> {
    match direction {
        Direction::Undo => {
            for item in items.iter() {
                verify_tree(&item.destination, &item.result_fingerprint)?;
                if matches!(kind, TransferKind::Move) {
                    ensure_absent(&item.source)?;
                }
            }
            let mut completed = Vec::new();
            for item in items.iter().rev() {
                let result = match kind {
                    TransferKind::Copy => crate::fs::journal_remove(&item.destination),
                    TransferKind::Move => crate::fs::journal_move(&item.destination, &item.source),
                };
                if let Err(error) = result {
                    rollback_transfer(kind, items, &completed, Direction::Undo);
                    return Err(error);
                }
                completed.push(item.source.clone());
            }
            Ok(transfer_effect(kind, items, Direction::Undo))
        }
        Direction::Redo => {
            for item in items.iter() {
                verify_tree(&item.source, &item.source_fingerprint)?;
                ensure_absent(&item.destination)?;
            }
            let mut completed = Vec::new();
            for item in items.iter_mut() {
                let result = match kind {
                    TransferKind::Copy => crate::fs::journal_copy(&item.source, &item.destination),
                    TransferKind::Move => crate::fs::journal_move(&item.source, &item.destination),
                };
                if let Err(error) = result {
                    rollback_transfer(kind, items, &completed, Direction::Redo);
                    return Err(error);
                }
                item.result_fingerprint = TreeFingerprint::read(&item.destination)?;
                completed.push(item.destination.clone());
            }
            Ok(transfer_effect(kind, items, Direction::Redo))
        }
    }
}

fn rollback_transfer(
    kind: TransferKind,
    items: &[TransferItem],
    completed: &[PathBuf],
    direction: Direction,
) {
    for path in completed.iter().rev() {
        let Some(item) = items
            .iter()
            .find(|item| item.source == *path || item.destination == *path)
        else {
            continue;
        };
        match (kind, direction) {
            (TransferKind::Copy, Direction::Redo) => {
                let _ = crate::fs::journal_remove(&item.destination);
            }
            (TransferKind::Move, Direction::Undo) => {
                let _ = crate::fs::journal_move(&item.source, &item.destination);
            }
            (TransferKind::Move, Direction::Redo) => {
                let _ = crate::fs::journal_move(&item.destination, &item.source);
            }
            (TransferKind::Copy, Direction::Undo) => {}
        }
    }
}

fn transfer_effect(kind: TransferKind, items: &[TransferItem], direction: Direction) -> Effect {
    let verb = match (kind, direction) {
        (TransferKind::Copy, Direction::Undo) => "Undid Copy",
        (TransferKind::Copy, Direction::Redo) => "Redid Copy",
        (TransferKind::Move, Direction::Undo) => "Undid Move",
        (TransferKind::Move, Direction::Redo) => "Redid Move",
    };
    let mut changed_folders = Vec::new();
    for item in items {
        for path in [&item.source, &item.destination] {
            if let Some(parent) = path.parent()
                && !changed_folders.iter().any(|existing| existing == parent)
            {
                changed_folders.push(parent.to_path_buf());
            }
        }
    }
    let select = items.first().map(|item| match (kind, direction) {
        (TransferKind::Copy, Direction::Undo) => item.source.clone(),
        (_, Direction::Undo) => item.source.clone(),
        (_, Direction::Redo) => item.destination.clone(),
    });
    Effect {
        status: verb.to_owned(),
        changed_folders,
        select,
    }
}

fn verify_tree(path: &Path, expected: &TreeFingerprint) -> Result<(), String> {
    if &TreeFingerprint::read(path)? == expected {
        Ok(())
    } else {
        Err(format!(
            "Refused operation: {} or its contents changed after it was recorded",
            path.display()
        ))
    }
}

fn verify(path: &Path, expected: &Fingerprint) -> Result<(), String> {
    if &Fingerprint::read(path)? == expected {
        Ok(())
    } else {
        Err(format!(
            "Refused operation: {} changed after it was recorded",
            path.display()
        ))
    }
}

fn ensure_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(format!("Refused operation: {} now exists", path.display())),
        Err(error) => Err(format!("could not inspect {}: {error}", path.display())),
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| "source path contains NUL".to_owned())?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| "destination path contains NUL".to_owned())?;
    // SAFETY: both C strings remain valid for the duration of the syscall.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "could not move entry: {}",
            io::Error::last_os_error()
        ))
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    ensure_absent(destination)?;
    fs::rename(source, destination).map_err(|error| format!("could not move entry: {error}"))
}

fn parent_folders(first: &Path, second: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for path in [first, second] {
        if let Some(parent) = path.parent()
            && !folders.iter().any(|existing| existing == parent)
        {
            folders.push(parent.to_path_buf());
        }
    }
    folders
}

fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("polarexp/operations.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".polarexp-operations.json"),
        |home| PathBuf::from(home).join(".local/state/polarexp/operations.json"),
    )
}

pub(crate) fn trash(path: &Path) -> Result<TrashReceipt, String> {
    gio::File::for_path(path)
        .trash(None::<&gio::Cancellable>)
        .map_err(|error| format!("could not move {} to Trash: {error}", path.display()))?;
    locate_trash(path).ok_or_else(|| {
        format!(
            "moved {} to Trash, but its recovery metadata could not be located",
            path.display()
        )
    })
}

fn locate_trash(original: &Path) -> Option<TrashReceipt> {
    locate_desktop_trash(original).or_else(|| locate_home_trash(original))
}

fn locate_desktop_trash(original: &Path) -> Option<TrashReceipt> {
    use std::os::unix::ffi::OsStringExt;

    let trash = gio::File::for_uri("trash:///");
    let enumerator = trash
        .enumerate_children(
            "standard::target-uri,trash::orig-path,trash::deletion-date",
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    let mut matches = Vec::new();
    while let Some(info) = enumerator.next_file(None::<&gio::Cancellable>).ok()? {
        let Some(encoded) = info.attribute_byte_string("trash::orig-path") else {
            continue;
        };
        let candidate = PathBuf::from(std::ffi::OsString::from_vec(encoded.as_bytes().to_vec()));
        if candidate != original {
            continue;
        }
        let Some(target_uri) = info.attribute_string("standard::target-uri") else {
            continue;
        };
        let Some(trashed) = gio::File::for_uri(target_uri.as_str()).path() else {
            continue;
        };
        let Some(trash_root) = trashed.parent().and_then(Path::parent) else {
            continue;
        };
        let Some(trashed_name) = trashed.file_name() else {
            continue;
        };
        let mut info_name = trashed_name.to_os_string();
        info_name.push(".trashinfo");
        let deletion_date = info
            .attribute_string("trash::deletion-date")
            .map_or_else(String::new, |value| value.to_string());
        matches.push((
            deletion_date,
            TrashReceipt {
                original: original.to_path_buf(),
                info: trash_root.join("info").join(info_name),
                trashed,
            },
        ));
    }
    matches.sort_by_key(|entry| entry.0.clone());
    matches.pop().map(|(_, receipt)| receipt)
}

fn locate_home_trash(original: &Path) -> Option<TrashReceipt> {
    let data_home = std::env::var_os("XDG_DATA_HOME").map_or_else(
        || {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share")
        },
        PathBuf::from,
    );
    let trash = data_home.join("Trash");
    let info_directory = trash.join("info");
    let mut matches = fs::read_dir(&info_directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let info = entry.path();
            if info.extension()? != "trashinfo" {
                return None;
            }
            let contents = fs::read_to_string(&info).ok()?;
            let encoded = contents
                .lines()
                .find_map(|line| line.strip_prefix("Path="))?;
            if percent_decode_path(encoded).as_deref() != Some(original.as_os_str()) {
                return None;
            }
            let name = info.file_stem()?;
            let trashed = trash.join("files").join(name);
            fs::symlink_metadata(&trashed).ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((
                modified,
                TrashReceipt {
                    original: original.to_path_buf(),
                    trashed,
                    info,
                },
            ))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(modified, _)| *modified);
    matches.pop().map(|(_, receipt)| receipt)
}

#[cfg(unix)]
fn percent_decode_path(value: &str) -> Option<std::ffi::OsString> {
    use std::os::unix::ffi::OsStringExt;

    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Some(std::ffi::OsString::from_vec(decoded))
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_and_new_folder_round_trip_across_a_reopened_journal() {
        let temp = tempfile::tempdir().unwrap();
        let journal_path = temp.path().join("state/operations.json");
        let before = temp.path().join("before");
        let after = temp.path().join("after");
        fs::write(&after, "content").unwrap();
        let mut journal = Journal::open(journal_path.clone()).unwrap();
        journal
            .record(Action::rename(before.clone(), after.clone()).unwrap())
            .unwrap();

        let mut journal = Journal::open(journal_path.clone()).unwrap();
        journal.undo().unwrap();
        assert!(before.exists());
        assert!(!after.exists());
        journal.redo().unwrap();
        assert!(!before.exists());
        assert!(after.exists());

        let folder = temp.path().join("folder");
        fs::create_dir(&folder).unwrap();
        journal
            .record(Action::new_folder(folder.clone()).unwrap())
            .unwrap();
        journal.undo().unwrap();
        assert!(!folder.exists());
        journal.redo().unwrap();
        assert!(folder.is_dir());
    }

    #[test]
    fn unsafe_inverse_is_refused_and_redo_is_cleared_by_new_work() {
        let temp = tempfile::tempdir().unwrap();
        let journal_path = temp.path().join("operations.json");
        let before = temp.path().join("before");
        let after = temp.path().join("after");
        fs::write(&after, "original").unwrap();
        let mut journal = Journal::open(journal_path).unwrap();
        journal
            .record(Action::rename(before.clone(), after.clone()).unwrap())
            .unwrap();
        fs::write(&after, "changed size").unwrap();
        assert!(journal.undo().unwrap_err().contains("changed"));

        fs::remove_file(&after).unwrap();
        fs::write(&after, "original").unwrap();
        journal.stored.entries[0].action = Action::rename(before, after).unwrap();
        journal.undo().unwrap();
        let folder = temp.path().join("new");
        fs::create_dir(&folder).unwrap();
        journal.record(Action::new_folder(folder).unwrap()).unwrap();
        assert_eq!(journal.redo().unwrap_err(), "Nothing to redo");
    }

    #[test]
    fn journal_keeps_only_one_hundred_operations_and_thirty_days() {
        let temp = tempfile::tempdir().unwrap();
        let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
        let now = 4_000_000;
        for index in 0..105 {
            let folder = temp.path().join(format!("folder-{index}"));
            fs::create_dir(&folder).unwrap();
            journal
                .record_at(Action::new_folder(folder).unwrap(), now)
                .unwrap();
        }
        assert_eq!(journal.stored.entries.len(), 100);

        let current = temp.path().join("current");
        fs::create_dir(&current).unwrap();
        journal
            .record_at(
                Action::new_folder(current).unwrap(),
                now + MAX_AGE_SECONDS + 1,
            )
            .unwrap();
        assert_eq!(journal.stored.entries.len(), 1);
    }

    #[test]
    fn new_folder_undo_refuses_non_empty_directory() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("folder");
        fs::create_dir(&folder).unwrap();
        let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
        journal
            .record(Action::new_folder(folder.clone()).unwrap())
            .unwrap();
        fs::write(folder.join("later"), "work").unwrap();

        assert!(journal.undo().unwrap_err().contains("no longer empty"));
        assert!(folder.join("later").exists());
    }

    #[test]
    fn copy_and_move_undo_redo_are_restart_safe() {
        let temp = tempfile::tempdir().unwrap();
        let journal_path = temp.path().join("operations.json");
        let source = temp.path().join("source");
        let copied = temp.path().join("copied");
        fs::write(&source, "content").unwrap();
        crate::fs::journal_copy(&source, &copied).unwrap();
        let copy_receipts = [crate::fs::TransferReceipt {
            source: source.clone(),
            destination: copied.clone(),
        }];
        let mut journal = Journal::open(journal_path.clone()).unwrap();
        journal
            .record(
                Action::transfer(TransferKind::Copy, &copy_receipts)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        drop(journal);

        let mut journal = Journal::open(journal_path.clone()).unwrap();
        journal.undo().unwrap();
        assert!(source.exists());
        assert!(!copied.exists());
        journal.redo().unwrap();
        assert_eq!(fs::read_to_string(&copied).unwrap(), "content");

        let moved = temp.path().join("moved");
        crate::fs::journal_move(&source, &moved).unwrap();
        let move_receipts = [crate::fs::TransferReceipt {
            source: source.clone(),
            destination: moved.clone(),
        }];
        journal
            .record(
                Action::transfer(TransferKind::Move, &move_receipts)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        journal.undo().unwrap();
        assert!(source.exists());
        assert!(!moved.exists());
        journal.redo().unwrap();
        assert!(!source.exists());
        assert!(moved.exists());
    }

    #[test]
    fn copy_undo_refuses_a_changed_result_tree() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let copied = temp.path().join("copied");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), "content").unwrap();
        crate::fs::journal_copy(&source, &copied).unwrap();
        let receipts = [crate::fs::TransferReceipt {
            source,
            destination: copied.clone(),
        }];
        let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
        journal
            .record(
                Action::transfer(TransferKind::Copy, &receipts)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        fs::write(copied.join("later"), "user work").unwrap();

        assert!(journal.undo().unwrap_err().contains("contents changed"));
        assert!(copied.join("later").exists());
    }

    #[test]
    fn trash_undo_restores_only_the_verified_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        let trashed = temp.path().join("Trash/files/original");
        let info = temp.path().join("Trash/info/original.trashinfo");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::write(&trashed, "content").unwrap();
        fs::write(&info, "[Trash Info]\nPath=/original\n").unwrap();
        let receipt = TrashReceipt {
            original: original.clone(),
            trashed: trashed.clone(),
            info: info.clone(),
        };
        let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
        journal
            .record(
                Action::trash(std::slice::from_ref(&receipt))
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        fs::write(&trashed, "changed after trash").unwrap();
        assert!(journal.undo().unwrap_err().contains("changed"));
        assert!(!original.exists());

        fs::write(&trashed, "content").unwrap();
        journal.stored.entries[0].action = Action::trash(std::slice::from_ref(&receipt))
            .unwrap()
            .unwrap();
        journal.undo().unwrap();
        assert_eq!(fs::read_to_string(original).unwrap(), "content");
        assert!(!trashed.exists());
        assert!(!info.exists());
    }

    #[test]
    fn restore_action_persists_the_restored_identity_for_later_undo() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("restored");
        fs::write(&original, "content").unwrap();
        let receipt = TrashReceipt {
            original,
            trashed: temp.path().join("Trash/files/restored"),
            info: temp.path().join("Trash/info/restored.trashinfo"),
        };
        let path = temp.path().join("operations.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        journal
            .record(
                Action::restore(std::slice::from_ref(&receipt))
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
        drop(journal);

        let reopened = Journal::open(path).unwrap();
        assert!(matches!(
            reopened.stored.entries[0].action,
            Action::Restore { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn trash_info_paths_decode_spaces_and_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let decoded = percent_decode_path("/tmp/a%20name-%FF").unwrap();
        assert_eq!(decoded.as_bytes(), b"/tmp/a name-\xff");
    }
}
