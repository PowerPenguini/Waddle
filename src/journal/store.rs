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
        let Some(index) = self.stored.cursor.checked_sub(1) else {
            return Err(Error::message("Nothing to undo"));
        };
        let effect = apply(&mut self.stored.entries[index].action, Direction::Undo)?;
        self.stored.cursor = index;
        self.save()?;
        Ok(effect)
    }

    pub(crate) fn redo(&mut self) -> Result<Effect, Error> {
        let Some(entry) = self.stored.entries.get_mut(self.stored.cursor) else {
            return Err(Error::message("Nothing to redo"));
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

    fn save(&self) -> Result<(), Error> {
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
