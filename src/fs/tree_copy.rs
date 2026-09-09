use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Seek, SeekFrom},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, Default)]
pub(crate) struct CopyLinks(HashMap<(u64, u64), CopiedLink>);

// JSON object keys cannot represent device/inode tuples. Store an entry list.
impl Serialize for CopyLinks {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter())
    }
}

impl<'de> Deserialize<'de> for CopyLinks {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = Vec::<((u64, u64), CopiedLink)>::deserialize(deserializer)?;
        Ok(Self(entries.into_iter().collect()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CopiedLink {
    #[serde(with = "crate::path_serde")]
    path: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    mode: u32,
    attributes: Option<ExtendedAttributes>,
    #[serde(default)]
    source_changed: Option<(i64, i64)>,
    #[serde(default)]
    changed: Option<(i64, i64)>,
}

impl CopiedLink {
    fn usable(
        &self,
        source_path: &Path,
        source: &fs::Metadata,
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<bool> {
        let Ok(destination) = fs::symlink_metadata(&self.path) else {
            return Ok(false);
        };
        let metadata_match = self.size == source.len()
            && self.mode == source.mode()
            && self.modified == (source.mtime(), source.mtime_nsec())
            && self.attributes.as_ref().is_some_and(|attributes| {
                read_xattrs(source_path).is_ok_and(|current| current == *attributes)
                    && read_xattrs(&self.path).is_ok_and(|current| current == *attributes)
            })
            && destination.is_file()
            && destination.dev() == self.device
            && destination.ino() == self.inode
            && destination.len() == self.size
            && destination.mode() == self.mode
            && (destination.mtime(), destination.mtime_nsec()) == self.modified;
        if !metadata_match {
            return Ok(false);
        }
        if self.source_changed == Some(change_time(source))
            && self.changed == Some(change_time(&destination))
        {
            return Ok(true);
        }
        // Contents can change without changing size/mtime. Link count changes
        // also update ctime, so compare bytes instead of discarding valid links.
        match same_contents(source_path, source, &self.path, &destination, check) {
            Ok(equal) => Ok(equal),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Err(error),
            Err(_) => Ok(false),
        }
    }
}

fn change_time(metadata: &fs::Metadata) -> (i64, i64) {
    (metadata.ctime(), metadata.ctime_nsec())
}

fn same_contents(
    source: &Path,
    source_metadata: &fs::Metadata,
    destination: &Path,
    destination_metadata: &fs::Metadata,
    check: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;
    let open = |path: &Path| {
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
    };
    let unchanged = |file: &fs::File, expected: &fs::Metadata| {
        file.metadata().is_ok_and(|current| {
            current.is_file()
                && current.dev() == expected.dev()
                && current.ino() == expected.ino()
                && current.len() == expected.len()
                && change_time(&current) == change_time(expected)
        })
    };
    check()?;
    let mut left = open(source)?;
    let mut right = open(destination)?;
    if !unchanged(&left, source_metadata) || !unchanged(&right, destination_metadata) {
        return Ok(false);
    }
    let mut left_bytes = [0; 64 * 1024];
    let mut right_bytes = [0; 64 * 1024];
    let mut remaining = source_metadata.len();
    while remaining > 0 {
        check()?;
        let count = remaining.min(left_bytes.len() as u64) as usize;
        left.read_exact(&mut left_bytes[..count])?;
        right.read_exact(&mut right_bytes[..count])?;
        if left_bytes[..count] != right_bytes[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    Ok(unchanged(&left, source_metadata) && unchanged(&right, destination_metadata))
}

pub(super) struct PreparedCopy {
    warnings: Vec<String>,
    links: CopyLinks,
}

impl PreparedCopy {
    pub(super) fn publish(
        mut self,
        staging: &Path,
        destination: &Path,
        links: &mut CopyLinks,
    ) -> Vec<String> {
        for link in self.links.0.values_mut() {
            if let Ok(relative) = link.path.strip_prefix(staging) {
                link.path = if relative.as_os_str().is_empty() {
                    destination.to_path_buf()
                } else {
                    destination.join(relative)
                };
                link.changed = fs::symlink_metadata(&link.path)
                    .ok()
                    .map(|m| change_time(&m));
            }
        }
        *links = self.links;
        self.warnings
    }
}

pub(super) fn copy_item_with_warnings(
    source: &Path,
    destination: &Path,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
    links: &CopyLinks,
) -> io::Result<PreparedCopy> {
    let mut context = CopyContext {
        hardlinks: links.clone(),
        warnings: Vec::new(),
        bytes: 0,
        created: false,
        progress,
    };
    if let Err(error) = context.copy(source, destination) {
        // Creation can lose a race to an unrelated entry. Only clean a root
        // that this copy actually created; create_new/mkdir/link are exclusive.
        if context.created {
            remove_incomplete_copy(destination);
        }
        return Err(error);
    }
    Ok(PreparedCopy {
        warnings: context.warnings,
        links: context.hardlinks,
    })
}

struct CopyContext<'a> {
    hardlinks: CopyLinks,
    warnings: Vec<String>,
    bytes: u64,
    created: bool,
    progress: &'a mut dyn FnMut(u64) -> io::Result<()>,
}

impl CopyContext<'_> {
    fn advance(&mut self, bytes: u64) -> io::Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        (self.progress)(self.bytes)
    }

    fn copy(&mut self, source: &Path, destination: &Path) -> io::Result<()> {
        (self.progress)(self.bytes)?;
        let metadata = fs::symlink_metadata(source)?;
        if metadata.file_type().is_symlink() {
            copy_symlink(source, destination)?;
            self.created = true;
            self.metadata(source, destination, &metadata, true);
            self.advance(metadata.len())?;
            return Ok(());
        }
        if metadata.is_dir() {
            use std::os::unix::fs::DirBuilderExt;
            // Keep unpublished contents private even under a permissive umask
            // or an inherited default ACL. Source permissions are applied last.
            fs::DirBuilder::new().mode(0o700).create(destination)?;
            self.created = true;
            let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                self.copy(&entry.path(), &destination.join(entry.file_name()))?;
            }
            self.metadata(source, destination, &metadata, false);
            return Ok(());
        }
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "copying special files is unsupported",
            ));
        }

        let hardlink_key = (metadata.dev(), metadata.ino());
        if let Some(existing) = self.hardlinks.0.get(&hardlink_key)
            && existing.usable(source, &metadata, &mut || (self.progress)(self.bytes))?
        {
            match fs::hard_link(&existing.path, destination) {
                Ok(()) => {
                    self.created = true;
                    let existing = self.hardlinks.0.get_mut(&hardlink_key).unwrap();
                    existing.changed = fs::symlink_metadata(destination)
                        .ok()
                        .map(|m| change_time(&m));
                    existing.source_changed = Some(change_time(&metadata));
                    self.advance(metadata.len())?;
                    return Ok(());
                }
                Err(error) => self.warnings.push(format!(
                    "hardlink relationship for {}: {error}",
                    source.display()
                )),
            }
        }

        use std::os::unix::fs::OpenOptionsExt;

        // Do not block if the source was replaced by a FIFO after inspection.
        let mut input = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(source)?;
        if !input.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "copying special files is unsupported",
            ));
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(destination)?;
        self.created = true;
        let sparse = metadata.len() > 0 && metadata.blocks().saturating_mul(512) < metadata.len();
        let base = self.bytes;
        let mut copied = 0;
        let mut report = |position| {
            // Sparse fallback may restart the same file. Do not count its
            // earlier extents twice or move the displayed progress backwards.
            copied = copied.max(position);
            (self.progress)(base.saturating_add(copied))
        };
        if sparse {
            match copy_sparse(&mut input, &mut output, metadata.len(), &mut report) {
                Ok(()) => {}
                Err(error)
                    if matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOTSUP)) =>
                {
                    self.warnings.push(format!(
                        "sparse layout for {}: {error}; copied densely",
                        source.display()
                    ));
                    input.seek(SeekFrom::Start(0))?;
                    output.set_len(0)?;
                    output.seek(SeekFrom::Start(0))?;
                    copy_stream(&mut input, &mut output, &mut report)?;
                }
                Err(error) => return Err(error),
            }
        } else {
            copy_stream(&mut input, &mut output, &mut report)?;
        }
        self.bytes = base.saturating_add(copied);
        self.metadata(source, destination, &metadata, false);
        // Buffered writes can succeed even when the device later reports ENOSPC
        // or EIO. Surface those errors while this is still an unpublished copy,
        // before Replace discards old data or Move removes the source.
        output.sync_all()?;
        if metadata.nlink() > 1 {
            let copied = fs::symlink_metadata(destination)?;
            self.hardlinks.0.insert(
                hardlink_key,
                CopiedLink {
                    path: destination.to_path_buf(),
                    device: copied.dev(),
                    inode: copied.ino(),
                    size: copied.len(),
                    modified: (copied.mtime(), copied.mtime_nsec()),
                    mode: copied.mode(),
                    attributes: read_xattrs(destination).ok(),
                    source_changed: Some(change_time(&metadata)),
                    changed: Some(change_time(&copied)),
                },
            );
        }
        Ok(())
    }

    fn metadata(
        &mut self,
        source: &Path,
        destination: &Path,
        metadata: &fs::Metadata,
        symlink: bool,
    ) {
        // Reconcile inherited ACLs while the new entry is still private.
        // Opening the source's group mask first could briefly enable an
        // inherited named-user entry that the source never granted.
        record_metadata_result(
            "extended attributes and ACLs",
            copy_xattrs(source, destination),
            &mut self.warnings,
        );
        if !symlink {
            record_metadata_result(
                "permissions",
                fs::set_permissions(destination, metadata.permissions()),
                &mut self.warnings,
            );
        }
        record_metadata_result(
            "timestamps",
            set_times(
                destination,
                metadata.atime(),
                metadata.atime_nsec(),
                metadata.mtime(),
                metadata.mtime_nsec(),
            ),
            &mut self.warnings,
        );
    }
}

pub(super) fn record_metadata_result(
    label: &str,
    result: io::Result<()>,
    warnings: &mut Vec<String>,
) {
    if let Err(error) = result {
        warnings.push(format!("{label}: {error}"));
    }
}

#[cfg(target_os = "linux")]
fn copy_sparse(
    input: &mut fs::File,
    output: &mut fs::File,
    length: u64,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
) -> io::Result<()> {
    let mut offset = 0_i64;
    output.set_len(length)?;
    while offset < length as i64 {
        // SAFETY: lseek only reads and updates the valid file descriptor's offset.
        let data = unsafe { libc::lseek(input.as_raw_fd(), offset, libc::SEEK_DATA) };
        if data < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENXIO) {
                break;
            }
            return Err(error);
        }
        // SAFETY: as above, with SEEK_HOLE on the same valid descriptor.
        let hole = unsafe { libc::lseek(input.as_raw_fd(), data, libc::SEEK_HOLE) };
        if hole < 0 {
            return Err(io::Error::last_os_error());
        }
        input.seek(SeekFrom::Start(data as u64))?;
        output.seek(SeekFrom::Start(data as u64))?;
        progress(data as u64)?;
        copy_stream(
            &mut input.take((hole - data) as u64),
            output,
            &mut |copied| progress(data as u64 + copied),
        )?;
        offset = hole;
    }
    progress(length)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn copy_sparse(
    input: &mut fs::File,
    output: &mut fs::File,
    _: u64,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
) -> io::Result<()> {
    copy_stream(input, output, progress)
}

fn copy_stream(
    input: &mut impl Read,
    output: &mut fs::File,
    progress: &mut dyn FnMut(u64) -> io::Result<()>,
) -> io::Result<()> {
    let mut copied = 0_u64;
    loop {
        // Bound each kernel-assisted copy so a large file produces live updates.
        let bytes = io::copy(&mut input.take(1024 * 1024), output)?;
        if bytes == 0 {
            return Ok(());
        }
        copied = copied.saturating_add(bytes);
        progress(copied)?;
    }
}

#[cfg(target_os = "linux")]
pub(super) fn set_times(
    path: &Path,
    atime_seconds: i64,
    atime_nanoseconds: i64,
    mtime_seconds: i64,
    mtime_nanoseconds: i64,
) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let times = [
        libc::timespec {
            tv_sec: atime_seconds,
            tv_nsec: atime_nanoseconds,
        },
        libc::timespec {
            tv_sec: mtime_seconds,
            tv_nsec: mtime_nanoseconds,
        },
    ];
    // SAFETY: path and times point to valid memory for the duration of the call.
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn set_times(_: &Path, _: i64, _: i64, _: i64, _: i64) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "timestamp preservation is unsupported",
    ))
}

type ExtendedAttributes = Vec<(std::ffi::CString, Vec<u8>)>;

#[cfg(target_os = "linux")]
pub(crate) fn read_xattrs(source: &Path) -> io::Result<ExtendedAttributes> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    // SAFETY: source is a valid NUL-terminated path and the null buffer requests its size.
    let size = unsafe { libc::llistxattr(source.as_ptr(), std::ptr::null_mut(), 0) };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut names = vec![0_u8; size as usize];
    if size > 0 {
        // SAFETY: names has the exact capacity reported by llistxattr.
        let read =
            unsafe { libc::llistxattr(source.as_ptr(), names.as_mut_ptr().cast(), names.len()) };
        if read < 0 {
            return Err(io::Error::last_os_error());
        }
        names.truncate(read as usize);
    }
    let mut attributes = Vec::new();
    for bytes in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = CString::new(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "xattr name contains NUL"))?;
        // SAFETY: source and name are valid and the null buffer requests the value size.
        let value_size =
            unsafe { libc::lgetxattr(source.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if value_size < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut value = vec![0_u8; value_size as usize];
        if value_size > 0 {
            // SAFETY: value has the capacity reported by lgetxattr.
            let read = unsafe {
                libc::lgetxattr(
                    source.as_ptr(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            if read < 0 {
                return Err(io::Error::last_os_error());
            }
            value.truncate(read as usize);
        }
        attributes.push((name, value));
    }
    attributes.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(attributes)
}

#[cfg(target_os = "linux")]
fn copy_xattrs(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let attributes = read_xattrs(source)?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "destination contains NUL"))?;
    // Creation can inherit ACLs from the destination parent. chmod only adjusts
    // their mask; it does not remove named users or a directory's default ACL.
    // Absence on the source is meaningful too. Leave unrelated filesystem-
    // assigned attributes (such as security labels) alone.
    for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        if !attributes
            .iter()
            .any(|(present, _)| present.as_c_str() == name)
        {
            // SAFETY: both arguments are live NUL-terminated strings. The l*
            // variant changes this entry without following symbolic links.
            if unsafe { libc::lremovexattr(destination.as_ptr(), name.as_ptr()) } != 0 {
                let error = io::Error::last_os_error();
                if !matches!(error.raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)) {
                    return Err(error);
                }
            }
        }
    }
    for (name, value) in attributes {
        // SAFETY: destination, name, and value are valid for the duration of the call.
        let result = unsafe {
            libc::lsetxattr(
                destination.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn read_xattrs(_: &Path) -> io::Result<ExtendedAttributes> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "extended attributes are unsupported",
    ))
}

#[cfg(not(target_os = "linux"))]
fn copy_xattrs(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "extended attributes are unsupported",
    ))
}

#[cfg(all(test, target_os = "linux"))]
pub(super) fn set_xattr(path: &Path, name: &str, value: &[u8]) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = CString::new(name).unwrap();
    // SAFETY: all pointers and lengths describe live buffers for this call.
    let result = unsafe {
        libc::lsetxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(super) fn get_xattr(path: &Path, name: &str) -> io::Result<Vec<u8>> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = CString::new(name).unwrap();
    // SAFETY: the null buffer requests the value size.
    let size = unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut value = vec![0_u8; size as usize];
    // SAFETY: value has the capacity reported by lgetxattr.
    let read = unsafe {
        libc::lgetxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
        )
    };
    if read < 0 {
        return Err(io::Error::last_os_error());
    }
    value.truncate(read as usize);
    Ok(value)
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

pub(super) fn remove_incomplete_copy(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        use std::os::unix::fs::PermissionsExt;

        // These directories belong to an unpublished copy. Preserved source
        // permissions may prevent cleanup; never change the source or follow links.
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(metadata.mode() | 0o700));
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                remove_incomplete_copy(&entry.path());
            }
        }
        let _ = fs::remove_dir(path);
    } else {
        let _ = fs::remove_file(path);
    }
}
