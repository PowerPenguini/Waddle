use super::*;

pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let path = parent.join(name);
    fs::create_dir(&path).map_err(|e| FsError::new("create", &path, e))?;
    Ok(path)
}

pub fn create_file(parent: &Path, name: &str, template: Option<&Path>) -> Result<PathBuf, FsError> {
    validate_name(name).map_err(|message| {
        FsError::new(
            "create",
            parent.join(name),
            io::Error::new(io::ErrorKind::InvalidInput, message),
        )
    })?;
    let destination = parent.join(name);
    if let Some(template) = template {
        let metadata = fs::symlink_metadata(template)
            .map_err(|error| FsError::new("read template", template, error))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            return Err(FsError::new(
                "create from template",
                template,
                io::Error::new(io::ErrorKind::InvalidInput, "template is a directory"),
            ));
        }
        copy_revealed(template, &destination)
            .map(drop)
            .map_err(|error| FsError::new("create from template", &destination, error))?;
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| FsError::new("create", &destination, error))?;
    }
    Ok(destination)
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
pub(super) fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    renameat2(source, destination, libc::RENAME_NOREPLACE)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
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

pub(super) fn transfer_exact(
    source: &Path,
    destination: &Path,
    action: Action,
) -> io::Result<Vec<String>> {
    match action {
        Action::Copy => copy_revealed(source, destination),
        Action::Move => move_exact(source, destination),
    }
}

fn copy_revealed(source: &Path, destination: &Path) -> io::Result<Vec<String>> {
    let staging = staging_path(destination)?;
    let warnings = match copy_item_with_warnings(source, &staging) {
        Ok(warnings) => warnings,
        Err(error) => {
            remove_incomplete_copy(&staging);
            return Err(error);
        }
    };
    if let Err(error) = rename_noreplace(&staging, destination) {
        remove_incomplete_copy(&staging);
        return Err(error);
    }
    Ok(warnings)
}

pub(super) fn move_exact(source: &Path, destination: &Path) -> io::Result<Vec<String>> {
    match rename_noreplace(source, destination) {
        Ok(()) => Ok(Vec::new()),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            let staging = staging_path(destination)?;
            let warnings = match copy_item_with_warnings(source, &staging) {
                Ok(warnings) => warnings,
                Err(error) => {
                    remove_incomplete_copy(&staging);
                    return Err(error);
                }
            };
            if let Err(error) = rename_noreplace(&staging, destination) {
                remove_incomplete_copy(&staging);
                return Err(error);
            }
            if let Err(error) = remove_item(source) {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "the complete destination was kept, but the source could not be fully removed: {error}"
                    ),
                ));
            }
            Ok(warnings)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
pub(super) fn replace_exact(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
) -> io::Result<Vec<String>> {
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
                remove_item(source).map(|()| Vec::new())
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
pub(super) fn replace_exact(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
) -> io::Result<Vec<String>> {
    if FileIdentity::read(destination)? != observed {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination changed while the conflict was open",
        ));
    }
    let staging = staging_path(destination)?;
    let warnings = transfer_exact(source, &staging, Action::Copy)?;
    remove_item(destination)?;
    fs::rename(&staging, destination)?;
    if action == Action::Move {
        remove_item(source)?;
    }
    Ok(warnings)
}

#[cfg(target_os = "linux")]
fn replace_by_staging(
    source: &Path,
    destination: &Path,
    observed: FileIdentity,
    remove_source: bool,
) -> io::Result<Vec<String>> {
    let staging = staging_path(destination)?;
    let warnings = match copy_item_with_warnings(source, &staging) {
        Ok(warnings) => warnings,
        Err(error) => {
            remove_incomplete_copy(&staging);
            return Err(error);
        }
    };
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
    Ok(warnings)
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

pub(crate) fn journal_copy(source: &Path, destination: &Path) -> Result<(), String> {
    copy_revealed(source, destination)
        .map(drop)
        .map_err(|error| format!("could not redo Copy: {error}"))
}

pub(super) fn tree_bytes(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::read_dir(path)?.try_fold(0_u64, |total, entry| {
            tree_bytes(&entry?.path()).map(|bytes| total.saturating_add(bytes))
        })
    } else {
        Ok(metadata.len())
    }
}

pub(crate) fn journal_move(source: &Path, destination: &Path) -> Result<(), String> {
    move_exact(source, destination)
        .map(drop)
        .map_err(|error| format!("could not move entry: {error}"))
}

pub(crate) fn journal_remove(path: &Path) -> Result<(), String> {
    remove_item(path).map_err(|error| format!("could not remove {}: {error}", path.display()))
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

pub(super) fn available_copy_destination(directory: &Path, name: &OsStr) -> PathBuf {
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
