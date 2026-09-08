use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

use crate::transfer::Action;

use super::{
    FsError,
    browse::validate_name,
    source_tree::SourceTree,
    transfer_batch::FileIdentity,
    tree_copy::{CopyLinks, copy_item_with_warnings, remove_incomplete_copy},
};

pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let path = parent.join(name);
    fs::create_dir(&path).map_err(|e| FsError::new("create", &path, e))?;
    Ok(path)
}

pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    validate_name(name).map_err(|message| {
        FsError::new(
            "create",
            parent.join(name),
            io::Error::new(io::ErrorKind::InvalidInput, message),
        )
    })?;
    let destination = parent.join(name);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| FsError::new("create", &destination, error))?;
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

pub(super) fn transfer_exact(
    source: &Path,
    destination: &Path,
    action: Action,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &mut CopyLinks,
) -> io::Result<Vec<String>> {
    match action {
        Action::Copy => copy_revealed(source, destination, progress, links),
        Action::Move => move_exact_with_progress(source, destination, progress, links),
    }
}

fn copy_revealed(
    source: &Path,
    destination: &Path,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &mut CopyLinks,
) -> io::Result<Vec<String>> {
    let staging = staging_path(destination)?;
    let warnings = copy_item_with_warnings(source, &staging, progress, links)?;
    if let Err(error) = rename_noreplace(&staging, destination) {
        remove_incomplete_copy(&staging);
        return Err(error);
    }
    Ok(warnings.publish(&staging, destination, links))
}

pub(super) fn move_exact(source: &Path, destination: &Path) -> io::Result<Vec<String>> {
    move_exact_with_progress(
        source,
        destination,
        &mut |_| Ok(()),
        &mut CopyLinks::default(),
    )
}

fn move_exact_with_progress(
    source: &Path,
    destination: &Path,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &mut CopyLinks,
) -> io::Result<Vec<String>> {
    match rename_noreplace(source, destination) {
        Ok(()) => {
            let _ = progress(tree_bytes(destination).unwrap_or_default());
            Ok(Vec::new())
        }
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            let snapshot = SourceTree::read(source)?;
            let staging = staging_path(destination)?;
            let warnings = copy_item_with_warnings(source, &staging, progress, links)?;
            if let Err(error) = snapshot.verify(source) {
                remove_incomplete_copy(&staging);
                return Err(error);
            }
            if let Err(error) = rename_noreplace(&staging, destination) {
                remove_incomplete_copy(&staging);
                return Err(error);
            }
            if let Err(error) = snapshot.remove_copied(source) {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "the complete destination was kept, but the source could not be fully removed: {error}"
                    ),
                ));
            }
            Ok(warnings.publish(&staging, destination, links))
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
pub(super) fn replace_exact_with_progress(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &mut CopyLinks,
) -> io::Result<Vec<String>> {
    if FileIdentity::read(destination)? != observed {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination changed while the conflict was open",
        ));
    }
    check_replace_cleanup(destination)?;
    match action {
        Action::Move => {
            let backup = staging_path(destination)?;
            match rename_exchange(source, destination) {
                Ok(()) => {
                    if FileIdentity::read(source)? != observed {
                        rename_exchange(source, destination)?;
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "the destination changed while Replace was running",
                        ));
                    }
                    // Move the old destination away from the source before destructive
                    // cleanup. A failure must never make Retry move old data as input.
                    if let Err(error) = rename_noreplace(source, &backup) {
                        rename_exchange(source, destination).map_err(|rollback| {
                        io::Error::new(error.kind(), format!(
                            "could not retain the old destination: {error}; could not restore the source: {rollback}"
                        ))
                    })?;
                        return Err(error);
                    }
                    let warnings = cleanup_replaced(&backup).into_iter().collect();
                    let _ = progress(tree_bytes(destination).unwrap_or_default());
                    Ok(warnings)
                }
                Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
                    replace_by_staging(source, destination, observed, true, progress, links)
                }
                Err(error) => Err(error),
            }
        }
        Action::Copy => replace_by_staging(source, destination, observed, false, progress, links),
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn replace_exact_with_progress(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &mut CopyLinks,
) -> io::Result<Vec<String>> {
    if FileIdentity::read(destination)? != observed {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the destination changed while the conflict was open",
        ));
    }
    let staging = staging_path(destination)?;
    let warnings = transfer_exact(source, &staging, Action::Copy, progress, links)?;
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
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &mut CopyLinks,
) -> io::Result<Vec<String>> {
    let snapshot = remove_source
        .then(|| SourceTree::read(source))
        .transpose()?;
    let staging = staging_path(destination)?;
    let warnings = copy_item_with_warnings(source, &staging, progress, links)?;
    if let Some(snapshot) = &snapshot
        && let Err(error) = snapshot.verify(source)
    {
        remove_incomplete_copy(&staging);
        return Err(error);
    }
    if let Err(error) = check_replace_cleanup(destination) {
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
    // Publication is committed. Recursive deletion may fail after removing some
    // old entries, so exchanging it back would fabricate a destructive rollback.
    let cleanup_warning = cleanup_replaced(&staging);
    if let Some(snapshot) = snapshot {
        snapshot.remove_copied(source)?;
    }
    let mut warnings = warnings.publish(&staging, destination, links);
    warnings.extend(cleanup_warning);
    Ok(warnings)
}

#[cfg(target_os = "linux")]
fn check_replace_cleanup(path: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    if !fs::symlink_metadata(path)?.is_dir() {
        return Ok(());
    }
    let mut entries = fs::read_dir(path)?.peekable();
    if entries.peek().is_some() {
        let path_c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        // SAFETY: path_c is valid for this call. Use effective credentials and ACLs.
        if unsafe {
            libc::faccessat(
                libc::AT_FDCWD,
                path_c.as_ptr(),
                libc::W_OK | libc::X_OK,
                libc::AT_EACCESS,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    for entry in entries {
        check_replace_cleanup(&entry?.path())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn cleanup_replaced(path: &Path) -> Option<String> {
    remove_item(path).err().map(|error| format!(
        "replacement completed, but old destination cleanup failed: {error}; remaining entries kept at {}",
        path.display()
    ))
}

fn staging_path(destination: &Path) -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);
    let directory = destination.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..10_000 {
        // Every concurrent operation in this process gets a distinct candidate,
        // including before either operation has created its staging entry.
        let nonce = NEXT_STAGING.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".waddle-replace-{}-{nonce}", std::process::id()));
        if path == destination {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
            Ok(_) => {}
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

/// Keep filesystem relationships across all entries of one history operation.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct JournalTransfer {
    links: CopyLinks,
}

impl JournalTransfer {
    pub(crate) fn apply(
        &mut self,
        action: Action,
        source: &Path,
        destination: &Path,
    ) -> Result<(), String> {
        transfer_exact(
            source,
            destination,
            action,
            &mut |_| Ok(()),
            &mut self.links,
        )
        .map(drop)
        .map_err(|error| format!("could not transfer entry: {error}"))
    }
}

#[cfg(test)]
pub(crate) fn journal_copy(source: &Path, destination: &Path) -> Result<(), String> {
    JournalTransfer::default().apply(Action::Copy, source, destination)
}

pub(crate) fn tree_bytes(path: &Path) -> io::Result<u64> {
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

pub(super) fn available_copy_destination(
    directory: &Path,
    name: &OsStr,
    is_directory: bool,
) -> io::Result<PathBuf> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let direct = directory.join(name);
    match fs::symlink_metadata(&direct) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(direct),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let directory_c = CString::new(directory.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "directory contains NUL"))?;
    // SAFETY: directory_c is a valid NUL-terminated path for the duration of pathconf.
    let limit = unsafe { libc::pathconf(directory_c.as_ptr(), libc::_PC_NAME_MAX) };
    let limit = usize::try_from(limit)
        .ok()
        .filter(|limit| *limit > 0)
        .unwrap_or(255);
    for number in 1_u64.. {
        let suffix = if number == 1 {
            " copy".to_owned()
        } else {
            format!(" copy {number}")
        };
        let extension = (!is_directory)
            .then(|| Path::new(name).extension())
            .flatten()
            .filter(|extension| extension.as_bytes().len() + 1 + suffix.len() < limit);
        let stem = extension
            .and_then(|_| Path::new(name).file_stem())
            .unwrap_or(name);
        let ending = extension.map_or(0, |extension| extension.as_bytes().len() + 1);
        let Some(available) = limit
            .checked_sub(suffix.len() + ending)
            .filter(|length| *length > 0)
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "copy suffix exceeds the filesystem name limit",
            ));
        };
        let bytes = stem.as_bytes();
        let mut length = bytes.len().min(available);
        if let Some(text) = stem.to_str() {
            while !text.is_char_boundary(length) {
                length -= 1;
            }
        }
        let mut candidate = OsString::from(OsStr::from_bytes(&bytes[..length]));
        candidate.push(suffix);
        if let Some(extension) = extension {
            candidate.push(".");
            candidate.push(extension);
        }
        let path = directory.join(candidate);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
            Ok(_) => {}
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

#[cfg(all(test, target_os = "linux"))]
pub(super) fn replace_exact(
    source: &Path,
    destination: &Path,
    action: Action,
    observed: FileIdentity,
) -> io::Result<Vec<String>> {
    replace_exact_with_progress(
        source,
        destination,
        action,
        observed,
        &mut |_| Ok(()),
        &mut CopyLinks::default(),
    )
}
