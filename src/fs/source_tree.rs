use std::{fs, io, os::unix::fs::MetadataExt, path::Path};

/// The entries a cross-device Move is allowed to remove. Never discover new
/// children during cleanup: they may contain data the copy did not include.
pub(super) struct SourceTree {
    metadata: fs::Metadata,
    children: Vec<(std::ffi::OsString, SourceTree)>,
}

impl SourceTree {
    pub(super) fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        let mut children = Vec::new();
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                children.push((entry.file_name(), Self::read(&entry.path())?));
            }
        }
        Ok(Self { metadata, children })
    }

    pub(super) fn verify(&self, path: &Path) -> io::Result<()> {
        let current = fs::symlink_metadata(path)?;
        self.check(path, &current, true)?;
        for (name, child) in &self.children {
            child.verify(&path.join(name))?;
        }
        Ok(())
    }

    fn check(&self, path: &Path, current: &fs::Metadata, full: bool) -> io::Result<()> {
        let old = &self.metadata;
        let identity = old.dev() == current.dev()
            && old.ino() == current.ino()
            && old.file_type() == current.file_type();
        let contents = old.len() == current.len()
            && old.mtime() == current.mtime()
            && old.mtime_nsec() == current.mtime_nsec();
        let changed = old.ctime() != current.ctime() || old.ctime_nsec() != current.ctime_nsec();
        if !identity || (full && (!contents || changed)) || (!full && !old.is_dir() && !contents) {
            return Err(io::Error::other(format!(
                "source changed during Move; retained {}",
                path.display()
            )));
        }
        Ok(())
    }

    pub(super) fn remove_copied(&self, path: &Path) -> io::Result<()> {
        // Removing children changes directory timestamps and unlinking one hardlink
        // changes its siblings' ctime. The full snapshot was verified before this.
        self.check(path, &fs::symlink_metadata(path)?, false)?;
        if self.metadata.is_dir() {
            for (name, child) in &self.children {
                child.remove_copied(&path.join(name))?;
            }
            // A late addition makes this fail safely instead of deleting it.
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    }
}
