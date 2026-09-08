use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{
    Error, TreeFingerprint,
    effects::{ensure_absent, rename_noreplace, verify_tree},
    removal::RemovalPlan,
};

/// A prepared result whose identity is persisted before its atomic publication.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Publication {
    #[serde(with = "crate::path_serde")]
    staging: PathBuf,
    device: u64,
    inode: u64,
    kind: u32,
    fingerprint: TreeFingerprint,
    cleanup: Option<RemovalPlan>,
}

impl Publication {
    pub(super) fn capture(staging: &Path, cleanup: Option<RemovalPlan>) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(staging)
            .map_err(|e| Error::io("could not identify prepared history result", e))?;
        Ok(Self {
            staging: staging.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
            kind: metadata.mode() & libc::S_IFMT,
            fingerprint: TreeFingerprint::read(staging)?,
            cleanup,
        })
    }

    pub(super) fn verify(&self, path: &Path) -> Result<(), Error> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| Error::io("could not inspect prepared history result", e))?;
        if metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.mode() & libc::S_IFMT != self.kind
        {
            return Err(Error::message(format!(
                "Refused recovery: {} is not the recorded result",
                path.display()
            )));
        }
        verify_tree(path, &self.fingerprint)
    }

    pub(super) fn finish(
        &mut self,
        source: &Path,
        destination: &Path,
    ) -> Result<TreeFingerprint, Error> {
        match fs::symlink_metadata(destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.verify(&self.staging)?;
                ensure_absent(destination)?;
                rename_noreplace(&self.staging, destination)?;
            }
            Err(error) => return Err(Error::io("could not inspect history destination", error)),
            Ok(_) => {}
        }
        // Never infer completion just from existence or matching bytes: an
        // external file can have both. Require our pre-publication inode too.
        self.verify(destination)?;
        if self.cleanup.is_some() && self.staging == source {
            // A same-filesystem rename consumed the original pathname. A new
            // entry there belongs to someone else, even if it is a hardlink.
            ensure_absent(source)?;
        } else if let Some(cleanup) = &mut self.cleanup {
            cleanup.remove(source)?;
        }
        Ok(self.fingerprint.clone())
    }
}
