use std::{ffi::CString, io, mem::MaybeUninit, os::unix::ffi::OsStrExt, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StorageUsage {
    pub(crate) used_bytes: u64,
    pub(crate) available_bytes: u64,
}

impl StorageUsage {
    pub(crate) fn used_bytes(self) -> u64 {
        self.used_bytes
    }

    pub(crate) fn used_fraction(self) -> f32 {
        let accounted_bytes = self.used_bytes.saturating_add(self.available_bytes);
        if accounted_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f32 / accounted_bytes as f32
        }
    }
}

pub(crate) fn storage_usage(path: &Path) -> Result<StorageUsage, String> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("Could not read storage usage for {}", path.display()))?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a NUL-terminated CString and `stats` points to writable,
    // correctly aligned storage which is initialized by a successful statvfs call.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    // SAFETY: statvfs returned success, so it initialized the output structure.
    let stats = unsafe { stats.assume_init() };
    let block_size = if stats.f_frsize == 0 {
        stats.f_bsize
    } else {
        stats.f_frsize
    };
    Ok(StorageUsage {
        used_bytes: stats
            .f_blocks
            .saturating_sub(stats.f_bfree)
            .saturating_mul(block_size),
        available_bytes: stats.f_bavail.saturating_mul(block_size),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_usage_for_an_existing_filesystem() {
        let temp = tempfile::tempdir().unwrap();
        let usage = storage_usage(temp.path()).unwrap();

        assert!(usage.used_bytes().saturating_add(usage.available_bytes) > 0);
        assert!((0.0..=1.0).contains(&usage.used_fraction()));
    }

    #[test]
    fn empty_filesystem_has_zero_usage() {
        let usage = StorageUsage {
            used_bytes: 0,
            available_bytes: 0,
        };

        assert_eq!(usage.used_bytes(), 0);
        assert_eq!(usage.used_fraction(), 0.0);
    }
}
