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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attributes_digest: Option<u64>,
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
        Ok(*self
            == Self::read_with_permissions(path, permissions, self.attributes_digest.is_some())?)
    }

    fn read_with_permissions(
        path: &Path,
        permissions: bool,
        attributes: bool,
    ) -> Result<Self, Error> {
        let mut digest = Fnv::default();
        let mut attributes_digest = attributes.then(Fnv::default);
        hash_tree(
            path,
            Path::new(""),
            &mut digest,
            permissions,
            &mut attributes_digest,
        )?;
        Ok(Self {
            root: Fingerprint::read(path)?,
            digest: digest.0,
            attributes_digest: attributes_digest.map(|digest| digest.0),
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
    digest: &mut Fnv,
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
