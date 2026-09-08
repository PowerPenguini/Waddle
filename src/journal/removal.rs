use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsString,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use super::{Error, TreeFingerprint};
use serde::{Deserialize, Serialize};

/// A persisted list of entries still owned by a partially completed Copy Undo.
/// Directories are removed only after their recorded children; never recursively.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RemovalPlan {
    remaining: VecDeque<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Entry {
    #[serde(with = "crate::path_serde")]
    relative: PathBuf,
    device: u64,
    inode: u64,
    kind: u32,
    contents: Option<TreeFingerprint>,
}

impl RemovalPlan {
    pub(super) fn capture(root: &Path) -> Result<Self, Error> {
        let mut remaining = VecDeque::new();
        capture(root, Path::new(""), &mut remaining)?;
        Ok(Self { remaining })
    }

    pub(super) fn verify(&self, root: &Path) -> Result<(), Error> {
        let mut children: BTreeMap<&Path, BTreeSet<OsString>> = BTreeMap::new();
        for entry in &self.remaining {
            if let Some(name) = entry.relative.file_name() {
                children
                    .entry(entry.relative.parent().unwrap())
                    .or_default()
                    .insert(name.to_owned());
            }
        }
        for entry in &self.remaining {
            let path = entry.path(root);
            entry.verify(&path)?;
            if entry.kind == libc::S_IFDIR {
                let actual = fs::read_dir(&path)
                    .map_err(|e| Error::io("could not inspect remaining Undo directory", e))?
                    .map(|e| e.map(|e| e.file_name()))
                    .collect::<Result<BTreeSet<_>, _>>()
                    .map_err(|e| Error::io("could not inspect remaining Undo children", e))?;
                if actual
                    != children
                        .remove(entry.relative.as_path())
                        .unwrap_or_default()
                {
                    return Err(changed(&path));
                }
            }
        }
        Ok(())
    }

    pub(super) fn remove(&mut self, root: &Path) -> Result<(), Error> {
        self.verify(root)?;
        while let Some(entry) = self.remaining.front() {
            let path = entry.path(root);
            entry.verify(&path)?;
            let result = if entry.kind == libc::S_IFDIR {
                fs::remove_dir(&path)
            } else {
                fs::remove_file(&path)
            };
            result
                .map_err(|e| Error::io(format!("could not undo Copy at {}", path.display()), e))?;
            self.remaining.pop_front();
        }
        Ok(())
    }
}

impl Entry {
    fn path(&self, root: &Path) -> PathBuf {
        if self.relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&self.relative)
        }
    }

    fn verify(&self, path: &Path) -> Result<(), Error> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|e| Error::io("could not verify remaining Undo entry", e))?;
        if self.device != metadata.dev()
            || self.inode != metadata.ino()
            || self.kind != metadata.mode() & libc::S_IFMT
        {
            return Err(changed(path));
        }
        if let Some(contents) = &self.contents
            && TreeFingerprint::read_without_permissions(path)? != *contents
        {
            return Err(changed(path));
        }
        Ok(())
    }
}

fn capture(root: &Path, relative: &Path, entries: &mut VecDeque<Entry>) -> Result<(), Error> {
    let path = if relative.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let metadata =
        fs::symlink_metadata(&path).map_err(|e| Error::io("could not plan Copy Undo", e))?;
    if metadata.is_dir() {
        for child in
            fs::read_dir(&path).map_err(|e| Error::io("could not plan directory Undo", e))?
        {
            let child = child.map_err(|e| Error::io("could not plan child Undo", e))?;
            capture(root, &relative.join(child.file_name()), entries)?;
        }
    }
    entries.push_back(Entry {
        relative: relative.to_owned(),
        device: metadata.dev(),
        inode: metadata.ino(),
        kind: metadata.mode() & libc::S_IFMT,
        contents: if metadata.is_dir() {
            None
        } else {
            Some(TreeFingerprint::read_without_permissions(&path)?)
        },
    });
    Ok(())
}

fn changed(path: &Path) -> Error {
    Error::message(format!(
        "Refused Undo: {} changed since Copy cleanup began",
        path.display()
    ))
}
