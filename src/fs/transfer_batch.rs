use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIdentity {
    device: u64,
    inode: u64,
    kind: u32,
}

impl FileIdentity {
    pub(super) fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            kind: metadata.mode() & libc::S_IFMT,
        })
    }
}

#[derive(Clone, Debug)]
struct TransferRoot {
    source: PathBuf,
    destination: PathBuf,
}

#[derive(Clone, Debug)]
enum PendingTransfer {
    Entry {
        source: PathBuf,
        destination: PathBuf,
        root: usize,
    },
    RemoveSourceDirectory {
        source: PathBuf,
        root: usize,
    },
}

impl PendingTransfer {
    fn root(&self) -> usize {
        match self {
            Self::Entry { root, .. } | Self::RemoveSourceDirectory { root, .. } => *root,
        }
    }
}

#[derive(Clone, Debug)]
struct BlockedTransfer {
    source: PathBuf,
    destination: PathBuf,
    root: usize,
    destination_identity: FileIdentity,
    directories: bool,
}

#[derive(Clone, Debug)]
pub struct TransferBatch {
    action: Action,
    roots: Vec<TransferRoot>,
    pending: VecDeque<PendingTransfer>,
    blocked: Option<BlockedTransfer>,
    apply_remaining: Option<ConflictChoice>,
    failed_roots: BTreeSet<usize>,
    retained_roots: BTreeSet<usize>,
    failures: Vec<TransferFailure>,
    warnings: Vec<TransferWarning>,
    root_bytes: Vec<u64>,
    progressed_roots: BTreeSet<usize>,
    cancelled: bool,
}

#[derive(Clone, Debug)]
pub enum TransferBatchOutcome {
    Conflict {
        batch: Box<TransferBatch>,
        conflict: TransferConflict,
    },
    Complete(TransferReport),
}

impl TransferBatch {
    pub fn try_new(
        sources: Vec<PathBuf>,
        destination_directory: PathBuf,
        action: Action,
    ) -> Result<Self, FsError> {
        validate_transfer(&sources, &destination_directory, action)?;
        Ok(Self::new(sources, destination_directory, action))
    }

    pub fn new(sources: Vec<PathBuf>, destination_directory: PathBuf, action: Action) -> Self {
        Self::new_mapped(
            sources.into_iter().filter_map(|source| {
                let name = source.file_name()?.to_owned();
                Some((source, destination_directory.join(name)))
            }),
            action,
        )
    }

    pub(crate) fn new_mapped(
        entries: impl IntoIterator<Item = (PathBuf, PathBuf)>,
        action: Action,
    ) -> Self {
        let mut roots = Vec::new();
        let mut pending = VecDeque::new();
        for (source, destination) in entries {
            let root = roots.len();
            roots.push(TransferRoot {
                source: source.clone(),
                destination: destination.clone(),
            });
            pending.push_back(PendingTransfer::Entry {
                source,
                destination,
                root,
            });
        }
        let root_bytes = roots
            .iter()
            .map(|root| tree_bytes(&root.source).unwrap_or_default())
            .collect();
        Self {
            action,
            roots,
            pending,
            blocked: None,
            apply_remaining: None,
            failed_roots: BTreeSet::new(),
            retained_roots: BTreeSet::new(),
            failures: Vec::new(),
            warnings: Vec::new(),
            root_bytes,
            progressed_roots: BTreeSet::new(),
            cancelled: false,
        }
    }

    #[cfg(test)]
    pub fn run(self) -> TransferBatchOutcome {
        self.run_with(|| false, |_| {})
    }

    pub fn run_with(
        mut self,
        cancelled: impl Fn() -> bool,
        mut progress: impl FnMut(TransferProgress),
    ) -> TransferBatchOutcome {
        self.publish_progress(&mut progress);
        while let Some(pending) = self.pending.pop_front() {
            if cancelled() {
                self.pending.push_front(pending);
                self.cancelled = true;
                return TransferBatchOutcome::Complete(self.cancel());
            }
            let root = pending.root();
            match pending {
                PendingTransfer::RemoveSourceDirectory { source, root } => {
                    if let Err(error) = fs::remove_dir(&source) {
                        self.fail(root, source, error);
                    }
                }
                PendingTransfer::Entry {
                    source,
                    destination,
                    root,
                } => {
                    let destination_metadata = match fs::symlink_metadata(&destination) {
                        Ok(metadata) => Some(metadata),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                        Err(error) => {
                            self.fail(root, source, error);
                            continue;
                        }
                    };
                    if let Some(destination_metadata) = destination_metadata {
                        let source_metadata = match fs::symlink_metadata(&source) {
                            Ok(metadata) => metadata,
                            Err(error) => {
                                self.fail(root, source, error);
                                continue;
                            }
                        };
                        let blocked = BlockedTransfer {
                            source,
                            destination,
                            root,
                            destination_identity: FileIdentity {
                                device: destination_metadata.dev(),
                                inode: destination_metadata.ino(),
                                kind: destination_metadata.mode() & libc::S_IFMT,
                            },
                            directories: source_metadata.is_dir()
                                && !source_metadata.file_type().is_symlink()
                                && destination_metadata.is_dir()
                                && !destination_metadata.file_type().is_symlink(),
                        };
                        let same_directory_copy = self.action == Action::Copy
                            && blocked.source.parent() == blocked.destination.parent();
                        if let Some(choice) = self
                            .apply_remaining
                            .or(same_directory_copy.then_some(ConflictChoice::KeepBoth))
                        {
                            match self.resolve_blocked(blocked.clone(), choice) {
                                Ok(warnings) => self.record_warnings(
                                    &blocked.source,
                                    &blocked.destination,
                                    warnings,
                                ),
                                Err(error) => self.fail(root, error.0, error.1),
                            }
                            continue;
                        }
                        let conflict = TransferConflict {
                            source: blocked.source.clone(),
                            destination: blocked.destination.clone(),
                            directories: blocked.directories,
                        };
                        self.blocked = Some(blocked);
                        return TransferBatchOutcome::Conflict {
                            batch: Box::new(self),
                            conflict,
                        };
                    }
                    match transfer_exact(&source, &destination, self.action) {
                        Ok(warnings) => self.record_warnings(&source, &destination, warnings),
                        Err(error) => {
                            if error.kind() == io::ErrorKind::AlreadyExists {
                                self.pending.push_front(PendingTransfer::Entry {
                                    source,
                                    destination,
                                    root,
                                });
                                continue;
                            }
                            self.fail(root, source, error);
                        }
                    }
                }
            }
            self.publish_completed_root(root, &mut progress);
        }
        self.publish_progress(&mut progress);
        TransferBatchOutcome::Complete(self.report())
    }

    pub fn resolve(mut self, choice: ConflictChoice, remaining: bool) -> Self {
        if remaining {
            self.apply_remaining = Some(choice);
        }
        if let Some(blocked) = self.blocked.take() {
            match self.resolve_blocked(blocked.clone(), choice) {
                Ok(warnings) => {
                    self.record_warnings(&blocked.source, &blocked.destination, warnings)
                }
                Err(error) if error.1.kind() == io::ErrorKind::AlreadyExists => {
                    self.pending.push_front(PendingTransfer::Entry {
                        source: blocked.source,
                        destination: blocked.destination,
                        root: blocked.root,
                    });
                    self.apply_remaining = None;
                }
                Err(error) => self.fail(blocked.root, error.0, error.1),
            }
        }
        self
    }

    pub fn cancel(mut self) -> TransferReport {
        if let Some(blocked) = self.blocked.take() {
            self.retained_roots.insert(blocked.root);
        }
        for pending in &self.pending {
            self.retained_roots.insert(pending.root());
        }
        self.report()
    }

    fn resolve_blocked(
        &mut self,
        blocked: BlockedTransfer,
        choice: ConflictChoice,
    ) -> Result<Vec<String>, (PathBuf, io::Error)> {
        match choice {
            ConflictChoice::Skip => {
                self.retained_roots.insert(blocked.root);
                self.pending
                    .retain(|pending| pending.root() != blocked.root);
                Ok(Vec::new())
            }
            ConflictChoice::KeepBoth => {
                let Some(name) = blocked.destination.file_name() else {
                    return Err((
                        blocked.source,
                        io::Error::new(io::ErrorKind::InvalidInput, "destination has no name"),
                    ));
                };
                let directory = blocked
                    .destination
                    .parent()
                    .unwrap_or_else(|| Path::new("."));
                let destination = available_copy_destination(directory, name);
                if self.roots[blocked.root].source == blocked.source {
                    self.roots[blocked.root].destination = destination.clone();
                }
                transfer_exact(&blocked.source, &destination, self.action)
                    .map_err(|error| (blocked.source, error))
            }
            ConflictChoice::Replace if blocked.directories => self.merge_directories(blocked),
            ConflictChoice::Replace => replace_exact(
                &blocked.source,
                &blocked.destination,
                self.action,
                blocked.destination_identity,
            )
            .map_err(|error| (blocked.source, error)),
        }
    }

    fn merge_directories(
        &mut self,
        blocked: BlockedTransfer,
    ) -> Result<Vec<String>, (PathBuf, io::Error)> {
        if FileIdentity::read(&blocked.destination)
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        {
            return transfer_exact(&blocked.source, &blocked.destination, self.action)
                .map_err(|error| (blocked.source, error));
        }
        if FileIdentity::read(&blocked.destination).ok() != Some(blocked.destination_identity) {
            return Err((
                blocked.source,
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "the destination changed while the conflict was open",
                ),
            ));
        }
        let mut children = fs::read_dir(&blocked.source)
            .map_err(|error| (blocked.source.clone(), error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| (blocked.source.clone(), error))?;
        children.sort_by_key(std::fs::DirEntry::file_name);
        if self.action == Action::Move {
            self.pending
                .push_front(PendingTransfer::RemoveSourceDirectory {
                    source: blocked.source.clone(),
                    root: blocked.root,
                });
        }
        for child in children.into_iter().rev() {
            self.pending.push_front(PendingTransfer::Entry {
                destination: blocked.destination.join(child.file_name()),
                source: child.path(),
                root: blocked.root,
            });
        }
        Ok(Vec::new())
    }

    fn fail(&mut self, root: usize, source: PathBuf, error: io::Error) {
        self.failed_roots.insert(root);
        self.failures.push(TransferFailure {
            source,
            error: error.to_string(),
        });
    }

    fn record_warnings(&mut self, source: &Path, destination: &Path, warnings: Vec<String>) {
        self.warnings
            .extend(warnings.into_iter().map(|detail| TransferWarning {
                source: source.to_path_buf(),
                destination: destination.to_path_buf(),
                detail,
            }));
    }

    fn publish_completed_root(&mut self, root: usize, progress: &mut impl FnMut(TransferProgress)) {
        let still_pending = self.pending.iter().any(|pending| pending.root() == root)
            || self
                .blocked
                .as_ref()
                .is_some_and(|blocked| blocked.root == root);
        if !still_pending {
            self.progressed_roots.insert(root);
            self.publish_progress(progress);
        }
    }

    fn publish_progress(&self, progress: &mut impl FnMut(TransferProgress)) {
        progress(TransferProgress {
            completed_entries: self.progressed_roots.len() as u64,
            total_entries: self.roots.len() as u64,
            completed_bytes: self
                .progressed_roots
                .iter()
                .filter_map(|index| self.root_bytes.get(*index))
                .sum(),
            total_bytes: self.root_bytes.iter().sum(),
        });
    }

    fn report(self) -> TransferReport {
        TransferReport {
            completed: self
                .roots
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    !self.failed_roots.contains(index) && !self.retained_roots.contains(index)
                })
                .map(|(_, root)| root.destination.clone())
                .collect(),
            failures: self.failures,
            warnings: self.warnings,
            receipts: self
                .roots
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    !self.failed_roots.contains(index) && !self.retained_roots.contains(index)
                })
                .map(|(_, root)| TransferReceipt {
                    source: root.source.clone(),
                    destination: root.destination.clone(),
                })
                .collect(),
            cancelled: self.cancelled,
            retained: self
                .retained_roots
                .into_iter()
                .filter_map(|index| self.roots.get(index).map(|root| root.source.clone()))
                .collect(),
        }
    }
}

fn validate_transfer(
    sources: &[PathBuf],
    destination_directory: &Path,
    action: Action,
) -> Result<(), FsError> {
    if sources.is_empty() {
        return Err(FsError::new(
            "paste into",
            destination_directory,
            io::Error::new(io::ErrorKind::InvalidInput, "there are no source entries"),
        ));
    }
    let destination_metadata = fs::metadata(destination_directory)
        .map_err(|error| FsError::new("inspect", destination_directory, error))?;
    if !destination_metadata.is_dir() {
        return Err(FsError::new(
            "paste into",
            destination_directory,
            io::Error::new(
                io::ErrorKind::NotADirectory,
                "the destination is not a folder",
            ),
        ));
    }
    require_access(destination_directory, libc::W_OK | libc::X_OK, "write into")?;
    let canonical_destination = destination_directory
        .canonicalize()
        .map_err(|error| FsError::new("inspect", destination_directory, error))?;
    let mut unique = BTreeSet::new();
    for source in sources {
        if !unique.insert(source) {
            return Err(FsError::new(
                "paste",
                source,
                io::Error::new(io::ErrorKind::InvalidInput, "the source is duplicated"),
            ));
        }
        let metadata =
            fs::symlink_metadata(source).map_err(|error| FsError::new("inspect", source, error))?;
        if source.file_name().is_none() {
            return Err(FsError::new(
                "paste",
                source,
                io::Error::new(io::ErrorKind::InvalidInput, "the source has no file name"),
            ));
        }
        let crosses_filesystems = metadata.dev() != destination_metadata.dev();
        if !metadata.file_type().is_symlink() && (action == Action::Copy || crosses_filesystems) {
            let read_mode = if metadata.is_dir() {
                libc::R_OK | libc::X_OK
            } else {
                libc::R_OK
            };
            require_access(source, read_mode, "read")?;
        }
        if action == Action::Move {
            let parent = source.parent().ok_or_else(|| {
                FsError::new(
                    "move",
                    source,
                    io::Error::new(io::ErrorKind::InvalidInput, "the source has no parent"),
                )
            })?;
            require_access(parent, libc::W_OK | libc::X_OK, "move from")?;
            let canonical_parent = parent
                .canonicalize()
                .map_err(|error| FsError::new("inspect", parent, error))?;
            if canonical_parent == canonical_destination {
                return Err(FsError::new(
                    "move",
                    source,
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the source is already in the destination folder",
                    ),
                ));
            }
        }
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let canonical_source = source
                .canonicalize()
                .map_err(|error| FsError::new("inspect", source, error))?;
            if canonical_destination == canonical_source
                || canonical_destination.starts_with(&canonical_source)
            {
                return Err(FsError::new(
                    "paste",
                    source,
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "a folder cannot be pasted into itself",
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn require_access(path: &Path, mode: i32, operation: &'static str) -> Result<(), FsError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let path_bytes = path.as_os_str().as_bytes();
    let path_c = CString::new(path_bytes).map_err(|_| {
        FsError::new(
            operation,
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "the path contains a null byte"),
        )
    })?;
    // SAFETY: `path_c` is a valid, null-terminated path and `mode` contains access flags only.
    let result =
        unsafe { libc::faccessat(libc::AT_FDCWD, path_c.as_ptr(), mode, libc::AT_EACCESS) };
    if result == 0 {
        Ok(())
    } else {
        Err(FsError::new(operation, path, io::Error::last_os_error()))
    }
}
