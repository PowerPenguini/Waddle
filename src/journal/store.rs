use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Effect {
    pub(crate) status: String,
    pub(crate) changed_folders: Vec<PathBuf>,
    pub(crate) select: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct Journal {
    path: PathBuf,
    pub(super) stored: StoredJournal,
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

    pub(super) fn record_at(&mut self, action: Action, recorded_at: u64) -> Result<(), String> {
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
