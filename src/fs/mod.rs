use std::{
    collections::VecDeque,
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
    if destination.exists() {
        return Err(FsError::new(
            "rename",
            source,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the destination already exists",
            ),
        ));
    }
    fs::rename(source, &destination).map_err(|e| FsError::new("rename", source, e))?;
    Ok(destination)
}

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

pub fn move_entry(source: &Path, destination_directory: &Path) -> Result<PathBuf, FsError> {
    let destination = move_destination(source, destination_directory)?;
    gio::File::for_path(source)
        .move_(
            &gio::File::for_path(&destination),
            gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
            None::<&gio::Cancellable>,
            None,
        )
        .map_err(|error| FsError::new("move", source, io::Error::other(error.to_string())))?;
    Ok(destination)
}

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
