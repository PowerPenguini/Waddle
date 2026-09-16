use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use super::Error;

pub(super) fn file_identity(path: &Path) -> Result<(u64, u64), Error> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| Error::io(format!("could not verify {}", path.display()), error))?;
    Ok((metadata.dev(), metadata.ino()))
}

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
pub(crate) struct MetadataFingerprint {
    mode: u32,
    attributes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_control: Option<super::access_control::AccessControl>,
}

impl MetadataFingerprint {
    pub(super) fn mode(&self) -> u32 {
        self.mode & 0o7777
    }

    pub(super) fn creation_mode(&self) -> u32 {
        if self.access_control.is_some() {
            // Keep inherited named users masked until the saved ACL is restored.
            self.mode() & 0o700
        } else {
            self.mode()
        }
    }

    pub(super) fn restore_access_control(&self, path: &Path) -> Result<(), Error> {
        if let Some(saved) = &self.access_control {
            saved.restore(path)?;
        }
        Ok(())
    }

    pub(super) fn matches(&self, path: &Path) -> Result<bool, Error> {
        let current = Self::read(path)?;
        // Older records have the same digest but no ACL values for replay.
        Ok(self.mode == current.mode && self.attributes == current.attributes)
    }

    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path)
            .map_err(|error| Error::io(format!("could not inspect {}", path.display()), error))?;
        Ok(Self {
            mode: metadata.mode(),
            attributes: attribute_digest(path, true)?,
            access_control: super::access_control::AccessControl::read(path)?,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attributes_digest: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directory_stable_digest: Option<u64>,
}

impl TreeFingerprint {
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        Self::read_with_permissions(path, true, true)
    }

    pub(super) fn read_without_permissions(path: &Path) -> Result<Self, Error> {
        Self::read_with_permissions(path, false, true)
    }

    pub(super) fn matches(&self, path: &Path, permissions: bool) -> Result<bool, Error> {
        // Older journal records did not capture attributes. Keep their original
        // checks instead of invalidating all pre-upgrade Undo/Redo operations.
        let mut current =
            Self::read_with_permissions(path, permissions, self.attributes_digest.is_some())?;
        if let Some(expected) = self.directory_stable_digest {
            return Ok(current.directory_stable_digest == Some(expected)
                && current.attributes_digest == self.attributes_digest);
        }
        current.directory_stable_digest = None;
        Ok(*self == current)
    }

    fn read_with_permissions(
        path: &Path,
        permissions: bool,
        attributes: bool,
    ) -> Result<Self, Error> {
        let mut digests = [Fnv::default(), Fnv::default()];
        let mut attributes_digest = attributes.then(Fnv::default);
        hash_tree(
            path,
            Path::new(""),
            &mut digests,
            permissions,
            &mut attributes_digest,
        )?;
        Ok(Self {
            root: Fingerprint::read(path)?,
            digest: digests[0].0,
            attributes_digest: attributes_digest.map(|digest| digest.0),
            directory_stable_digest: Some(digests[1].0),
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
    fn write_field(&mut self, bytes: &[u8]) {
        self.write(&(bytes.len() as u64).to_le_bytes());
        self.write(bytes);
    }

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
    digests: &mut [Fnv; 2],
    permissions: bool,
    attributes_digest: &mut Option<Fnv>,
) -> Result<(), Error> {
    use std::{io::Read, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt};

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| Error::io(format!("could not fingerprint {}", path.display()), error))?;
    if let Some(attributes) = attributes_digest {
        attributes.write_field(relative.as_os_str().as_bytes());
        attributes.write(&attribute_digest(path, permissions)?.to_le_bytes());
    }
    for (index, digest) in digests.iter_mut().enumerate() {
        digest.write(relative.as_os_str().as_bytes());
        digest.write(
            &(if permissions {
                metadata.mode()
            } else {
                metadata.mode() & libc::S_IFMT
            })
            .to_le_bytes(),
        );
        // Child operations change directory timestamps and storage size even
        // when Undo restores every entry. Keep the original digest for older
        // journals, and verify new records by contents and meaningful metadata.
        if index == 0 || !metadata.is_dir() {
            digest.write(&metadata.size().to_le_bytes());
            digest.write(&metadata.mtime().to_le_bytes());
            digest.write(&metadata.mtime_nsec().to_le_bytes());
        }
    }
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(path)
            .map_err(|error| Error::io("could not read symbolic link", error))?;
        for digest in digests.iter_mut() {
            digest.write(target.as_os_str().as_bytes());
        }
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
                digests,
                permissions,
                attributes_digest,
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
            for digest in digests.iter_mut() {
                digest.write(&buffer[..read]);
            }
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
    pub(super) fn modified(&self) -> Result<std::time::SystemTime, Error> {
        use std::time::{Duration, UNIX_EPOCH};

        if !(0..1_000_000_000).contains(&self.modified_nanoseconds) {
            return Err(Error::message("invalid recorded modification time"));
        }
        let seconds = Duration::from_secs(self.modified_seconds.unsigned_abs());
        let whole = if self.modified_seconds < 0 {
            UNIX_EPOCH.checked_sub(seconds)
        } else {
            UNIX_EPOCH.checked_add(seconds)
        };
        whole
            .and_then(|time| {
                time.checked_add(Duration::from_nanos(self.modified_nanoseconds as u64))
            })
            .ok_or_else(|| Error::message("recorded modification time is out of range"))
    }

    pub(super) fn is_directory(&self) -> bool {
        self.kind == libc::S_IFDIR
    }

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

/// Fingerprint only metadata that is stable across content reads and unlinking
/// siblings. Permission-repair paths deliberately omit ACLs, just as they omit
/// mode bits, while still protecting user attributes such as tags and comments.
pub(super) fn attribute_digest(path: &Path, permissions: bool) -> Result<u64, Error> {
    let attributes = match crate::fs::read_xattrs(path) {
        Ok(attributes) => attributes,
        // The Linux reader handles unsupported enumeration itself. An error
        // after listing names means known metadata could not be verified;
        // treating it as empty could authorize deleting an externally edited copy.
        Err(error)
            if cfg!(not(target_os = "linux"))
                && error.kind() == std::io::ErrorKind::Unsupported =>
        {
            Vec::new()
        }
        Err(error) => {
            return Err(Error::io(
                format!("could not fingerprint attributes of {}", path.display()),
                error,
            ));
        }
    };
    let mut digest = Fnv::default();
    for (name, value) in attributes {
        if !permissions
            && matches!(
                name.to_bytes(),
                b"system.posix_acl_access" | b"system.posix_acl_default"
            )
        {
            continue;
        }
        digest.write_field(name.to_bytes());
        digest.write_field(&value);
    }
    Ok(digest.0)
}
