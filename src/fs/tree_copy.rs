use super::*;

pub(super) fn copy_item_with_warnings(
    source: &Path,
    destination: &Path,
) -> io::Result<Vec<String>> {
    let mut context = CopyContext::default();
    context.copy(source, destination)?;
    Ok(context.warnings)
}

#[derive(Default)]
struct CopyContext {
    hardlinks: HashMap<(u64, u64), PathBuf>,
    warnings: Vec<String>,
}

impl CopyContext {
    fn copy(&mut self, source: &Path, destination: &Path) -> io::Result<()> {
        let metadata = fs::symlink_metadata(source)?;
        if metadata.file_type().is_symlink() {
            copy_symlink(source, destination)?;
            self.metadata(source, destination, &metadata, true);
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

        let hardlink_key = (metadata.dev(), metadata.ino());
        if metadata.nlink() > 1
            && let Some(existing) = self.hardlinks.get(&hardlink_key)
        {
            match fs::hard_link(existing, destination) {
                Ok(()) => return Ok(()),
                Err(error) => self.warnings.push(format!(
                    "hardlink relationship for {}: {error}",
                    source.display()
                )),
            }
        }

        let mut input = fs::File::open(source)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let sparse = metadata.len() > 0 && metadata.blocks().saturating_mul(512) < metadata.len();
        if sparse {
            match copy_sparse(&mut input, &mut output, metadata.len()) {
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
                    io::copy(&mut input, &mut output)?;
                }
                Err(error) => return Err(error),
            }
        } else {
            io::copy(&mut input, &mut output)?;
        }
        if metadata.nlink() > 1 {
            self.hardlinks
                .insert(hardlink_key, destination.to_path_buf());
        }
        self.metadata(source, destination, &metadata, false);
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
fn copy_sparse(input: &mut fs::File, output: &mut fs::File, length: u64) -> io::Result<()> {
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
        io::copy(&mut input.take((hole - data) as u64), output)?;
        offset = hole;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn copy_sparse(input: &mut fs::File, output: &mut fs::File, _: u64) -> io::Result<()> {
    io::copy(input, output).map(|_| ())
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

#[cfg(target_os = "linux")]
fn copy_xattrs(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL")
    })?;
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
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}
