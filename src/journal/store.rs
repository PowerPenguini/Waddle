use std::{
    fs, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    Action, Direction, Entry, Error, MAX_AGE_SECONDS, MAX_OPERATIONS, StoredJournal, VERSION, apply,
};

#[derive(Clone, Debug)]
pub(crate) struct Effect {
    pub(crate) status: String,
    pub(crate) changed_folders: Vec<PathBuf>,
    pub(crate) select: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct Journal {
    path: Option<PathBuf>,
    pub(super) stored: StoredJournal,
}

impl Journal {
    #[cfg(not(test))]
    pub(crate) fn open_default() -> Result<Self, Error> {
        Self::open(default_path())
    }

    #[cfg(not(test))]
    pub(crate) fn empty_default() -> Self {
        Self {
            path: Some(default_path()),
            stored: StoredJournal::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self {
            path: None,
            stored: StoredJournal::default(),
        }
    }

    pub(crate) fn open(path: PathBuf) -> Result<Self, Error> {
        let mut stored = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<StoredJournal>(&bytes)
                .map_err(|error| Error::json("could not decode operation journal", error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => StoredJournal::default(),
            Err(error) => return Err(Error::io("could not read operation journal", error)),
        };
        if stored.version != VERSION {
            return Err(Error::message(format!(
                "operation journal version {} is unsupported",
                stored.version
            )));
        }
        stored.cursor = stored.cursor.min(stored.entries.len());
        let mut journal = Self {
            path: Some(path),
            stored,
        };
        journal.prune(now_seconds());
        Ok(journal)
    }

    #[cfg(test)]
    pub(crate) fn uses_default_storage(&self) -> bool {
        self.path
            .as_ref()
            .is_some_and(|path| *path == default_path())
    }

    pub(crate) fn record(&mut self, action: Action) -> Result<(), Error> {
        self.record_at(action, now_seconds())
    }

    pub(super) fn record_at(&mut self, action: Action, recorded_at: u64) -> Result<(), Error> {
        let _lock = self.lock_and_reload()?;
        self.stored.entries.truncate(self.stored.cursor);
        self.stored.entries.push(Entry {
            recorded_at,
            action,
        });
        self.stored.cursor = self.stored.entries.len();
        self.prune(recorded_at);
        self.save()
    }

    pub(crate) fn undo(&mut self) -> Result<Effect, Error> {
        let _lock = self.lock_and_reload()?;
        self.prune(now_seconds());
        if self
            .stored
            .entries
            .get(self.stored.cursor)
            .is_some_and(|entry| entry.action.has_partial_effects())
        {
            return Err(Error::message(
                "Redo partially completed; retry Redo before Undo",
            ));
        }
        let Some(index) = self.stored.cursor.checked_sub(1) else {
            return Err(Error::message("Nothing to undo"));
        };
        let effect = apply(&mut self.stored.entries[index].action, Direction::Undo);
        if effect.is_ok() {
            self.stored.cursor = index;
        }
        self.save()?;
        effect
    }

    pub(crate) fn redo(&mut self) -> Result<Effect, Error> {
        let _lock = self.lock_and_reload()?;
        self.prune(now_seconds());
        if self
            .stored
            .cursor
            .checked_sub(1)
            .and_then(|index| self.stored.entries.get(index))
            .is_some_and(|entry| entry.action.has_partial_effects())
        {
            return Err(Error::message(
                "Undo partially completed; retry Undo before Redo",
            ));
        }
        let Some(entry) = self.stored.entries.get_mut(self.stored.cursor) else {
            return Err(Error::message("Nothing to redo"));
        };
        let effect = apply(&mut entry.action, Direction::Redo);
        if effect.is_ok() {
            self.stored.cursor += 1;
        }
        self.save()?;
        effect
    }

    // Hold a stable sidecar lock across reload, filesystem effects and commit.
    // Locking the journal itself would not survive its atomic replacement.
    fn lock_and_reload(&mut self) -> Result<Option<fs::File>, Error> {
        let Some(path) = self.path.as_ref() else {
            return Ok(None);
        };
        let directory = path
            .parent()
            .ok_or_else(|| Error::message("operation journal path has no parent"))?;
        fs::create_dir_all(directory)
            .map_err(|error| Error::io("could not create operation journal directory", error))?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|error| Error::io("could not open operation journal lock", error))?;
        lock.lock()
            .map_err(|error| Error::io("could not lock operation journal", error))?;
        // Do not prune using the wall clock here: record_at supplies its own clock.
        self.stored = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| Error::json("could not decode operation journal", error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => StoredJournal::default(),
            Err(error) => return Err(Error::io("could not read operation journal", error)),
        };
        if self.stored.version != VERSION {
            return Err(Error::message(format!(
                "operation journal version {} is unsupported",
                self.stored.version
            )));
        }
        self.stored.cursor = self.stored.cursor.min(self.stored.entries.len());
        Ok(Some(lock))
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

    pub(super) fn save(&self) -> Result<(), Error> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        let Some(directory) = path.parent() else {
            return Err(Error::message("operation journal path has no parent"));
        };
        fs::create_dir_all(directory)
            .map_err(|error| Error::io("could not create operation journal directory", error))?;
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.stored)
            .map_err(|error| Error::json("could not encode operation journal", error))?;
        fs::write(&temporary, bytes)
            .map_err(|error| Error::io("could not write operation journal", error))?;
        fs::rename(&temporary, path)
            .map_err(|error| Error::io("could not commit operation journal", error))
    }
}

fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("waddle/operations.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".waddle-operations.json"),
        |home| PathBuf::from(home).join(".local/state/waddle/operations.json"),
    )
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod regressions {
    use super::*;

    #[test]
    fn two_windows_keep_both_recorded_operations() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history.json");
        let mut window_a = Journal::open(path.clone()).unwrap();
        let mut window_b = Journal::open(path.clone()).unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        window_a.record(Action::new_file(a).unwrap()).unwrap();
        window_b.record(Action::new_file(b).unwrap()).unwrap();
        let reopened = Journal::open(path).unwrap();
        assert_eq!(
            reopened.stored.entries.len(),
            2,
            "second window overwrote the first window's operation"
        );
    }

    #[test]
    fn concurrent_windows_serialize_records_and_share_the_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("history.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers = (0..2)
            .map(|window| {
                let path = path.clone();
                let barrier = barrier.clone();
                let directory = temp.path().to_path_buf();
                std::thread::spawn(move || {
                    let mut journal = Journal::open(path).unwrap();
                    barrier.wait();
                    for index in 0..10 {
                        let file = directory.join(format!("{window}-{index}"));
                        fs::write(&file, "").unwrap();
                        journal.record(Action::new_file(file).unwrap()).unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let mut first = Journal::open(path.clone()).unwrap();
        let mut second = Journal::open(path.clone()).unwrap();
        assert_eq!(first.stored.entries.len(), 20);
        first.undo().unwrap();
        second.undo().unwrap();
        first.redo().unwrap();
        assert_eq!(Journal::open(path).unwrap().stored.cursor, 19);
    }
}
