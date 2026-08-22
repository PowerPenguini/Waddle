use std::{
    collections::{BTreeSet, VecDeque},
    ffi::{OsStr, OsString},
    fmt, fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use gio::prelude::*;

use crate::transfer::Action;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: OsString,
    pub(crate) directory: bool,
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

#[derive(Clone, Debug, Default)]
pub struct TransferReport {
    pub completed: Vec<PathBuf>,
    pub failures: Vec<TransferFailure>,
    pub retained: Vec<PathBuf>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    kind: u32,
}

impl FileIdentity {
    fn read(path: &Path) -> io::Result<Self> {
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
    pub fn new(sources: Vec<PathBuf>, destination_directory: PathBuf, action: Action) -> Self {
        let mut roots = Vec::with_capacity(sources.len());
        let mut pending = VecDeque::with_capacity(sources.len());
        for source in sources {
            let Some(name) = source.file_name() else {
                continue;
            };
            let destination = destination_directory.join(name);
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
        Self {
            action,
            roots,
            pending,
            blocked: None,
            apply_remaining: None,
            failed_roots: BTreeSet::new(),
            retained_roots: BTreeSet::new(),
            failures: Vec::new(),
        }
    }

    pub fn run(mut self) -> TransferBatchOutcome {
        while let Some(pending) = self.pending.pop_front() {
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
                            if let Err(error) = self.resolve_blocked(blocked, choice) {
                                self.fail(root, error.0, error.1);
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
                    if let Err(error) = transfer_exact(&source, &destination, self.action) {
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
        TransferBatchOutcome::Complete(self.report())
    }

    pub fn resolve(mut self, choice: ConflictChoice, remaining: bool) -> Self {
        if remaining {
            self.apply_remaining = Some(choice);
        }
        if let Some(blocked) = self.blocked.take()
            && let Err(error) = self.resolve_blocked(blocked.clone(), choice)
        {
            if error.1.kind() == io::ErrorKind::AlreadyExists {
                self.pending.push_front(PendingTransfer::Entry {
                    source: blocked.source,
                    destination: blocked.destination,
                    root: blocked.root,
                });
                self.apply_remaining = None;
            } else {
                self.fail(blocked.root, error.0, error.1);
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
    ) -> Result<(), (PathBuf, io::Error)> {
        match choice {
            ConflictChoice::Skip => {
                self.retained_roots.insert(blocked.root);
                self.pending
                    .retain(|pending| pending.root() != blocked.root);
                Ok(())
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

    fn merge_directories(&mut self, blocked: BlockedTransfer) -> Result<(), (PathBuf, io::Error)> {
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
        Ok(())
    }

    fn fail(&mut self, root: usize, source: PathBuf, error: io::Error) {
        self.failed_roots.insert(root);
        self.failures.push(TransferFailure {
            source,
            error: error.to_string(),
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
            retained: self
                .retained_roots
                .into_iter()
                .filter_map(|index| self.roots.get(index).map(|root| root.source.clone()))
                .collect(),
        }
    }
}

impl FileEntry {
    pub fn is_directory(&self) -> bool {
        self.directory
    }
}

#[derive(Debug)]
pub struct FsError {
    action: &'static str,
    path: PathBuf,
    source: io::Error,
}

impl FsError {
    fn new(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self {
            action,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Could not {} {}: {}",
            self.action,
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for FsError {}

pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("The name cannot be empty.");
    }
    if matches!(name, "." | "..") {
        return Err("That name is reserved.");
    }
    if name.contains('/') || name.contains('\0') {
        return Err("The name cannot contain a slash or NUL character.");
    }
    Ok(())
}

pub fn read_directory(path: &Path) -> Result<Vec<FileEntry>, FsError> {
    let iter = fs::read_dir(path).map_err(|e| FsError::new("read", path, e))?;
    let mut entries = Vec::new();
    for result in iter {
        let entry = result.map_err(|e| FsError::new("read an entry in", path, e))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|e| FsError::new("inspect", entry.path(), e))?;
        let path = entry.path();
        let directory = if file_type.is_symlink() {
            path.is_dir()
        } else {
            file_type.is_dir()
        };
        let sort_name = name.to_string_lossy().to_lowercase();
        entries.push((
            directory,
            sort_name,
            FileEntry {
                path,
                name,
                directory,
            },
        ));
    }
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.name.cmp(&b.2.name))
    });
    Ok(entries.into_iter().map(|(_, _, entry)| entry).collect())
}

pub fn open_directory(path: &Path) -> Result<OpenedDirectory, FsError> {
    let canonical_path = path
        .canonicalize()
        .map_err(|error| FsError::new("open", path, error))?;
    let entries = read_directory(&canonical_path)?;
    Ok(OpenedDirectory {
        canonical_path,
        entries,
    })
}

pub fn search_directory(
    root: &Path,
    query: &str,
    max_results: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<SearchResults, FsError> {
    if query.is_empty() || max_results == 0 {
        return Ok(SearchResults {
            entries: Vec::new(),
            truncated: false,
        });
    }

    let query = query.to_lowercase();
    let root_device = fs::metadata(root)
        .map_err(|error| FsError::new("search", root, error))?
        .dev();
    let mut directories = VecDeque::from([root.to_path_buf()]);
    let mut matches = Vec::new();

    while let Some(directory) = directories.pop_front() {
        if cancelled() {
            break;
        }
        let iter = match fs::read_dir(&directory) {
            Ok(iter) => iter,
            Err(error) if directory == root => {
                return Err(FsError::new("search", root, error));
            }
            Err(_) => continue,
        };
        let mut children: Vec<_> = iter
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .collect();
        children.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

        for child in children {
            if cancelled() {
                return Ok(SearchResults {
                    entries: matches,
                    truncated: false,
                });
            }
            let Ok(file_type) = child.file_type() else {
                continue;
            };
            let path = child.path();
            let directory = if file_type.is_symlink() {
                path.is_dir()
            } else {
                file_type.is_dir()
            };
            if file_type.is_dir()
                && child
                    .metadata()
                    .is_ok_and(|metadata| metadata.dev() == root_device)
            {
                directories.push_back(path.clone());
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if !relative.to_string_lossy().to_lowercase().contains(&query) {
                continue;
            }
            if matches.len() == max_results {
                return Ok(SearchResults {
                    entries: matches,
                    truncated: true,
                });
            }
            matches.push(FileEntry {
                path,
                name: child.file_name(),
                directory,
            });
        }
    }

    Ok(SearchResults {
        entries: matches,
        truncated: false,
    })
}

pub fn read_child_folders(path: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            !entry.file_name().to_string_lossy().starts_with('.')
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect();
    folders.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    folders
}

pub fn read_entry_details(path: &Path) -> Result<String, FsError> {
    const ATTRIBUTES: &str = concat!(
        "standard::size,standard::type,unix::mode,unix::uid,unix::gid,",
        "owner::user,owner::group"
    );
    let info = gio::File::for_path(path)
        .query_info(
            ATTRIBUTES,
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| FsError::new("inspect", path, io::Error::other(error.to_string())))?;

    let permissions = if info.has_attribute("unix::mode") {
        format_permissions(info.attribute_uint32("unix::mode"), info.file_type())
    } else {
        "permissions unknown".to_owned()
    };
    let size = format_size(u64::try_from(info.size()).unwrap_or_default());
    let user = info
        .attribute_string("owner::user")
        .map(|value| value.to_string())
        .or_else(|| {
            info.has_attribute("unix::uid")
                .then(|| info.attribute_uint32("unix::uid").to_string())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let group = info
        .attribute_string("owner::group")
        .map(|value| value.to_string())
        .or_else(|| {
            info.has_attribute("unix::gid")
                .then(|| info.attribute_uint32("unix::gid").to_string())
        })
        .unwrap_or_else(|| "unknown".to_owned());

    Ok(format!("{permissions}  •  {size}  •  {user}:{group}"))
}

fn format_permissions(mode: u32, file_type: gio::FileType) -> String {
    let kind = match file_type {
        gio::FileType::Directory => 'd',
        gio::FileType::SymbolicLink => 'l',
        _ => '-',
    };
    let mut value = String::with_capacity(10);
    value.push(kind);
    value.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    value.push(match (mode & 0o100 != 0, mode & 0o4000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    value.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    value.push(match (mode & 0o010 != 0, mode & 0o2000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    value.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    value.push(match (mode & 0o001 != 0, mode & 0o1000 != 0) {
        (true, true) => 't',
        (false, true) => 'T',
        (true, false) => 'x',
        (false, false) => '-',
    });
    value
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let path = parent.join(name);
    fs::create_dir(&path).map_err(|e| FsError::new("create", &path, e))?;
    Ok(path)
}

pub fn rename_entry(source: &Path, new_name: &str) -> Result<PathBuf, FsError> {
    let destination = source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(new_name);
    rename_noreplace(source, &destination)
        .map_err(|error| FsError::new("rename", source, error))?;
    Ok(destination)
}

#[cfg(target_os = "linux")]
fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    renameat2(source, destination, libc::RENAME_NOREPLACE)
}

#[cfg(not(target_os = "linux"))]
fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination already exists",
        ));
    }
    fs::rename(source, destination)
}

#[cfg(target_os = "linux")]
fn rename_exchange(first: &Path, second: &Path) -> io::Result<()> {
    renameat2(first, second, libc::RENAME_EXCHANGE)
}

#[cfg(target_os = "linux")]
fn renameat2(source: &Path, destination: &Path, flags: libc::c_uint) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL")
    })?;
    // SAFETY: both paths are NUL-terminated for the duration of the syscall.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
pub fn move_destination(source: &Path, destination_directory: &Path) -> Result<PathBuf, FsError> {
    let source_metadata =
        fs::symlink_metadata(source).map_err(|error| FsError::new("inspect", source, error))?;
    let destination_metadata = fs::metadata(destination_directory)
        .map_err(|error| FsError::new("inspect", destination_directory, error))?;
    if !destination_metadata.is_dir() {
        return Err(FsError::new(
            "move into",
            destination_directory,
            io::Error::new(
                io::ErrorKind::NotADirectory,
                "the destination is not a folder",
            ),
        ));
    }

    let Some(name) = source.file_name() else {
        return Err(FsError::new(
            "move",
            source,
            io::Error::new(io::ErrorKind::InvalidInput, "the source has no file name"),
        ));
    };
    let destination = destination_directory.join(name);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(FsError::new(
            "move",
            source,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the destination already exists",
            ),
        ));
    }

    if source_metadata.file_type().is_dir() {
        let canonical_source = source
            .canonicalize()
            .map_err(|error| FsError::new("inspect", source, error))?;
        let canonical_directory = destination_directory
            .canonicalize()
            .map_err(|error| FsError::new("inspect", destination_directory, error))?;
        if canonical_directory == canonical_source
            || canonical_directory.starts_with(&canonical_source)
        {
            return Err(FsError::new(
                "move",
                source,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a folder cannot be moved into itself",
                ),
            ));
        }
    }

    Ok(destination)
}

#[cfg(test)]
pub fn move_entry(source: &Path, destination_directory: &Path) -> Result<PathBuf, FsError> {
    let destination = move_destination(source, destination_directory)?;
    move_exact(source, &destination).map_err(|error| FsError::new("move", source, error))?;
    Ok(destination)
}

#[cfg(test)]
pub fn copy_entry(source: &Path, destination_directory: &Path) -> Result<PathBuf, FsError> {
    let source_metadata =
        fs::symlink_metadata(source).map_err(|error| FsError::new("inspect", source, error))?;
    let destination_metadata = fs::metadata(destination_directory)
        .map_err(|error| FsError::new("inspect", destination_directory, error))?;
    if !destination_metadata.is_dir() {
        return Err(FsError::new(
            "copy into",
            destination_directory,
            io::Error::new(
                io::ErrorKind::NotADirectory,
                "the destination is not a folder",
            ),
        ));
    }

    if source_metadata.is_dir() {
        let canonical_source = source
            .canonicalize()
            .map_err(|error| FsError::new("inspect", source, error))?;
        let canonical_destination = destination_directory
            .canonicalize()
            .map_err(|error| FsError::new("inspect", destination_directory, error))?;
        if canonical_destination == canonical_source
            || canonical_destination.starts_with(&canonical_source)
        {
            return Err(FsError::new(
                "copy",
                source,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a folder cannot be copied into itself",
                ),
            ));
        }
    }

    let Some(name) = source.file_name() else {
        return Err(FsError::new(
            "copy",
            source,
            io::Error::new(io::ErrorKind::InvalidInput, "the source has no file name"),
        ));
    };
    let destination = available_copy_destination(destination_directory, name);
    if let Err(error) = copy_item(source, &destination) {
        remove_incomplete_copy(&destination);
        return Err(FsError::new("copy", source, error));
    }
    Ok(destination)
}

fn transfer_exact(source: &Path, destination: &Path, action: Action) -> io::Result<()> {
    match action {
        Action::Copy => copy_item(source, destination),
        Action::Move => move_exact(source, destination),
    }
}

fn move_exact(source: &Path, destination: &Path) -> io::Result<()> {
    match rename_noreplace(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            copy_item(source, destination)?;
            if let Err(error) = remove_item(source) {
                remove_incomplete_copy(destination);
                return Err(error);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn replace_exact(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
) -> io::Result<()> {
    if FileIdentity::read(destination)? != observed {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination changed while the conflict was open",
        ));
    }
    match action {
        Action::Move => match rename_exchange(source, destination) {
            Ok(()) => {
                if FileIdentity::read(source)? != observed {
                    rename_exchange(source, destination)?;
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "the destination changed while Replace was running",
                    ));
                }
                remove_item(source)
            }
            Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
                replace_by_staging(source, destination, observed, true)
            }
            Err(error) => Err(error),
        },
        Action::Copy => replace_by_staging(source, destination, observed, false),
    }
}

#[cfg(not(target_os = "linux"))]
fn replace_exact(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
) -> io::Result<()> {
    if FileIdentity::read(destination)? != observed {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination changed while the conflict was open",
        ));
    }
    let staging = staging_path(destination)?;
    transfer_exact(source, &staging, Action::Copy)?;
    remove_item(destination)?;
    fs::rename(&staging, destination)?;
    if action == Action::Move {
        remove_item(source)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn replace_by_staging(
    source: &Path,
    destination: &Path,
    observed: FileIdentity,
    remove_source: bool,
) -> io::Result<()> {
    let staging = staging_path(destination)?;
    if let Err(error) = copy_item(source, &staging) {
        remove_incomplete_copy(&staging);
        return Err(error);
    }
    if let Err(error) = rename_exchange(&staging, destination) {
        remove_incomplete_copy(&staging);
        return Err(error);
    }
    if FileIdentity::read(&staging)? != observed {
        rename_exchange(&staging, destination)?;
        remove_incomplete_copy(&staging);
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination changed while Replace was running",
        ));
    }
    remove_item(&staging)?;
    if remove_source {
        remove_item(source)?;
    }
    Ok(())
}

fn staging_path(destination: &Path) -> io::Result<PathBuf> {
    let directory = destination.parent().unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .unwrap_or_else(|| OsStr::new("item"));
    for nonce in 0_u64..10_000 {
        let mut candidate = OsString::from(".polarexp-replace-");
        candidate.push(std::process::id().to_string());
        candidate.push("-");
        candidate.push(nonce.to_string());
        candidate.push("-");
        candidate.push(name);
        let path = directory.join(candidate);
        if fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a replacement staging name",
    ))
}

fn remove_item(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
pub fn transfer_entries(
    sources: &[PathBuf],
    destination_directory: &Path,
    action: Action,
) -> TransferReport {
    let mut report = TransferReport::default();
    for source in sources {
        let result = match action {
            Action::Copy => copy_entry(source, destination_directory),
            Action::Move => move_entry(source, destination_directory),
        };
        match result {
            Ok(path) => report.completed.push(path),
            Err(error) => report.failures.push(TransferFailure {
                source: source.clone(),
                error: error.to_string(),
            }),
        }
    }
    report
}

fn available_copy_destination(directory: &Path, name: &OsStr) -> PathBuf {
    let direct = directory.join(name);
    if fs::symlink_metadata(&direct).is_err() {
        return direct;
    }
    for number in 1_u64.. {
        let mut candidate = OsString::from(name);
        if number == 1 {
            candidate.push(" copy");
        } else {
            candidate.push(format!(" copy {number}"));
        }
        let path = directory.join(candidate);
        if fs::symlink_metadata(&path).is_err() {
            return path;
        }
    }
    unreachable!()
}

fn copy_item(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return copy_symlink(source, destination);
    }
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_item(&entry.path(), &destination.join(entry.file_name()))?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
        return Ok(());
    }

    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output)?;
    fs::set_permissions(destination, metadata.permissions())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output).map(|_| ())
}

fn remove_incomplete_copy(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

pub fn delete_permanently(path: &Path) -> Result<(), FsError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| FsError::new("inspect", path, e))?;
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| FsError::new("delete", path, e))
}

pub fn display_name(name: &OsStr) -> String {
    name.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
