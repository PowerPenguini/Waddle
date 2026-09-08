use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use super::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl DirectoryIdentity {
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path)
            .map_err(|error| Error::io(format!("could not verify {}", path.display()), error))?;
        if !metadata.is_dir() {
            return Err(Error::message(format!(
                "{} is no longer a folder",
                path.display()
            )));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Fingerprint {
    kind: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct TreeFingerprint {
    root: Fingerprint,
    digest: u64,
}

impl TreeFingerprint {
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        Self::read_with_permissions(path, true)
    }

    pub(super) fn read_without_permissions(path: &Path) -> Result<Self, Error> {
        Self::read_with_permissions(path, false)
    }

    fn read_with_permissions(path: &Path, permissions: bool) -> Result<Self, Error> {
        let mut digest = Fnv::default();
        hash_tree(path, Path::new(""), &mut digest, permissions)?;
        Ok(Self {
            root: Fingerprint::read(path)?,
            digest: digest.0,
        })
    }
}

struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

fn hash_tree(
    path: &Path,
    relative: &Path,
    digest: &mut Fnv,
    permissions: bool,
) -> Result<(), Error> {
    use std::{io::Read, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt};

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| Error::io(format!("could not fingerprint {}", path.display()), error))?;
    digest.write(relative.as_os_str().as_bytes());
    digest.write(
        &(if permissions {
            metadata.mode()
        } else {
            metadata.mode() & libc::S_IFMT
        })
        .to_le_bytes(),
    );
    digest.write(&metadata.size().to_le_bytes());
    digest.write(&metadata.mtime().to_le_bytes());
    digest.write(&metadata.mtime_nsec().to_le_bytes());
    if metadata.file_type().is_symlink() {
        digest.write(
            fs::read_link(path)
                .map_err(|error| Error::io("could not read symbolic link", error))?
                .as_os_str()
                .as_bytes(),
        );
    } else if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .map_err(|error| Error::io(format!("could not fingerprint {}", path.display()), error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| Error::io("could not read directory entry", error))?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            hash_tree(
                &entry.path(),
                &relative.join(entry.file_name()),
                digest,
                permissions,
            )?;
        }
    } else if metadata.is_file() {
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| {
                Error::io(format!("could not fingerprint {}", path.display()), error)
            })?;
        if !file
            .metadata()
            .map_err(|error| Error::io("could not inspect file", error))?
            .is_file()
        {
            return Err(Error::message("cannot fingerprint a special file"));
        }
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| Error::io("could not read file for fingerprint", error))?;
            if read == 0 {
                break;
            }
            digest.write(&buffer[..read]);
        }
    } else {
        return Err(Error::message(format!(
            "cannot fingerprint special file {}",
            path.display()
        )));
    }
    Ok(())
}

impl Fingerprint {
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path)
            .map_err(|error| Error::io(format!("could not verify {}", path.display()), error))?;
        Ok(Self {
            kind: metadata.mode() & libc::S_IFMT,
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }
}
