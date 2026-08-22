use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Fingerprint {
    kind: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
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
}
