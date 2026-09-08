use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Seek, SeekFrom},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct CopyLinks(HashMap<(u64, u64), CopiedLink>);

#[derive(Clone, Debug)]
struct CopiedLink {
    path: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    mode: u32,
    attributes: Option<ExtendedAttributes>,
}

impl CopiedLink {
    fn usable(&self, source_path: &Path, source: &fs::Metadata) -> bool {
        self.size == source.len()
            && self.mode == source.mode()
            && self.modified == (source.mtime(), source.mtime_nsec())
            && self.attributes.as_ref().is_some_and(|attributes| {
                read_xattrs(source_path).is_ok_and(|current| current == *attributes)
                    && read_xattrs(&self.path).is_ok_and(|current| current == *attributes)
            })
            && fs::symlink_metadata(&self.path).is_ok_and(|m| {
                m.is_file()
                    && m.dev() == self.device
                    && m.ino() == self.inode
                    && m.len() == self.size
                    && m.mode() == self.mode
                    && (m.mtime(), m.mtime_nsec()) == self.modified
            })
    }
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
        progress,
    };
    context.copy(source, destination)?;
    Ok(PreparedCopy {
        warnings: context.warnings,
        links: context.hardlinks,
    })
}

struct CopyContext<'a> {
    hardlinks: CopyLinks,
    warnings: Vec<String>,
    bytes: u64,
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
            self.metadata(source, destination, &metadata, true);
            self.advance(metadata.len())?;
            return Ok(());
        }
        if metadata.is_dir() {
            fs::create_dir(destination)?;
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
            && existing.usable(source, &metadata)
        {
            match fs::hard_link(&existing.path, destination) {
                Ok(()) => {
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
            .open(destination)?;
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
        if !symlink {
            record_metadata_result(
                "permissions",
                fs::set_permissions(destination, metadata.permissions()),
                &mut self.warnings,
            );
        }
        record_metadata_result(
            "extended attributes and ACLs",
            copy_xattrs(source, destination),
            &mut self.warnings,
        );
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
fn read_xattrs(source: &Path) -> io::Result<ExtendedAttributes> {
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
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "destination contains NUL"))?;
    for (name, value) in read_xattrs(source)? {
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
fn read_xattrs(_: &Path) -> io::Result<ExtendedAttributes> {
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
