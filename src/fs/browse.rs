use super::*;

#[cfg(target_os = "linux")]
use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};

impl FileEntry {
    pub fn is_directory(&self) -> bool {
        self.directory
    }

    pub fn is_hidden(&self) -> bool {
        self.name.as_encoded_bytes().first() == Some(&b'.')
    }

    pub fn type_label(&self) -> String {
        if self.is_directory() {
            "Folder".to_owned()
        } else {
            self.path
                .extension()
                .map(|extension| extension.to_string_lossy().to_uppercase())
                .unwrap_or_else(|| "File".to_owned())
        }
    }

    fn type_sort_key(&self) -> (u8, String) {
        let rank = if self.is_directory() {
            0
        } else if self.path.extension().is_some() {
            1
        } else {
            2
        };
        (rank, self.type_label().to_lowercase())
    }
}

#[derive(Debug)]
pub struct FsError {
    action: &'static str,
    path: PathBuf,
    source: io::Error,
}

impl FsError {
    pub(super) fn new(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self {
            action,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Could not {} {}: {}",
            self.action,
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for FsError {}

pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("The name cannot be empty.");
    }
    if matches!(name, "." | "..") {
        return Err("That name is reserved.");
    }
    if name.contains('/') || name.contains('\0') {
        return Err("The name cannot contain a slash or NUL character.");
    }
    Ok(())
}

#[cfg(test)]
pub fn read_directory(path: &Path) -> Result<Vec<FileEntry>, FsError> {
    read_directory_with(path, BrowseOptions::default())
}

pub fn read_directory_with(path: &Path, options: BrowseOptions) -> Result<Vec<FileEntry>, FsError> {
    let iter = fs::read_dir(path).map_err(|e| FsError::new("read", path, e))?;
    let mut entries = Vec::new();
    for result in iter {
        let entry = result.map_err(|e| FsError::new("read an entry in", path, e))?;
        let name = entry.file_name();
        if !options.show_hidden && name.to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|e| FsError::new("inspect", entry.path(), e))?;
        let path = entry.path();
        let directory = if file_type.is_symlink() {
            path.is_dir()
        } else {
            file_type.is_dir()
        };
        let metadata = read_entry_metadata(&entry);
        let sort_name = name.to_string_lossy().into_owned();
        let sort_size = metadata.size.unwrap_or_default();
        let sort_modified = metadata.modified.unwrap_or_default();
        let file_entry = FileEntry {
            path,
            name,
            directory,
            metadata,
        };
        let sort_type = file_entry.type_sort_key();
        entries.push((sort_name, sort_size, sort_modified, sort_type, file_entry));
    }
    entries.sort_by(|a, b| {
        if options.sort == SortKey::Size {
            return b
                .4
                .is_directory()
                .cmp(&a.4.is_directory())
                .then_with(|| {
                    if a.4.is_directory() {
                        natural_cmp(&a.0, &b.0)
                    } else if options.descending {
                        a.1.cmp(&b.1).reverse()
                    } else {
                        a.1.cmp(&b.1)
                    }
                })
                .then_with(|| natural_cmp(&a.0, &b.0))
                .then_with(|| a.4.name.cmp(&b.4.name));
        }
        let ordered = match options.sort {
            SortKey::Name => natural_cmp(&a.0, &b.0),
            SortKey::Modified => a.2.cmp(&b.2),
            SortKey::Type => a.3.cmp(&b.3).then_with(|| natural_cmp(&a.0, &b.0)),
            SortKey::Size => unreachable!("size sorting returns above"),
        };
        (if options.descending {
            ordered.reverse()
        } else {
            ordered
        })
        .then_with(|| a.4.name.cmp(&b.4.name))
    });
    Ok(entries
        .into_iter()
        .map(|(_, _, _, _, entry)| entry)
        .collect())
}

#[cfg(target_os = "linux")]
fn read_entry_metadata(entry: &fs::DirEntry) -> EntryMetadata {
    let path = entry.path();
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return EntryMetadata::default();
    };
    let mut metadata = MaybeUninit::<libc::statx>::zeroed();
    let flags = libc::AT_NO_AUTOMOUNT | libc::AT_SYMLINK_NOFOLLOW | libc::AT_STATX_DONT_SYNC;
    // SAFETY: path is NUL-terminated and metadata points to writable storage for one statx value.
    let result = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            path.as_ptr(),
            flags,
            libc::STATX_BASIC_STATS,
            metadata.as_mut_ptr(),
        )
    };
    if result != 0 {
        return EntryMetadata::default();
    }
    // SAFETY: statx returned success and initialized the output value.
    let metadata = unsafe { metadata.assume_init() };
    statx_entry_metadata(&metadata)
}

#[cfg(target_os = "linux")]
pub(super) fn statx_entry_metadata(metadata: &libc::statx) -> EntryMetadata {
    if metadata.stx_attributes & libc::STATX_ATTR_AUTOMOUNT as u64 != 0 {
        return EntryMetadata::default();
    }
    EntryMetadata {
        size: (metadata.stx_mask & libc::STATX_SIZE != 0).then_some(metadata.stx_size),
        modified: (metadata.stx_mask & libc::STATX_MTIME != 0).then_some(metadata.stx_mtime.tv_sec),
    }
}

#[cfg(not(target_os = "linux"))]
fn read_entry_metadata(entry: &fs::DirEntry) -> EntryMetadata {
    entry
        .metadata()
        .ok()
        .as_ref()
        .map_or_else(EntryMetadata::default, entry_metadata)
}

pub fn open_directory_with(
    path: &Path,
    options: BrowseOptions,
) -> Result<OpenedDirectory, FsError> {
    let canonical_path = path
        .canonicalize()
        .map_err(|error| FsError::new("open", path, error))?;
    let entries = read_directory_with(&canonical_path, options)?;
    Ok(OpenedDirectory {
        canonical_path,
        entries,
    })
}

#[cfg(test)]
pub fn search_directory(
    root: &Path,
    query: &str,
    max_results: usize,
    cancelled: impl FnMut() -> bool,
) -> Result<SearchResults, FsError> {
    search_directory_with_hidden(root, query, max_results, false, cancelled)
}

pub fn search_directory_with_hidden(
    root: &Path,
    query: &str,
    max_results: usize,
    show_hidden: bool,
    mut cancelled: impl FnMut() -> bool,
) -> Result<SearchResults, FsError> {
    if query.is_empty() || max_results == 0 {
        return Ok(SearchResults {
            entries: Vec::new(),
            truncated: false,
        });
    }

    let query = query.to_lowercase();
    let root_device = fs::metadata(root)
        .map_err(|error| FsError::new("search", root, error))?
        .dev();
    let mut directories = VecDeque::from([root.to_path_buf()]);
    let mut matches = Vec::new();

    while let Some(directory) = directories.pop_front() {
        if cancelled() {
            break;
        }
        let iter = match fs::read_dir(&directory) {
            Ok(iter) => iter,
            Err(error) if directory == root => {
                return Err(FsError::new("search", root, error));
            }
            Err(_) => continue,
        };
        let mut children: Vec<_> = iter
            .filter_map(Result::ok)
            .filter(|entry| show_hidden || !entry.file_name().to_string_lossy().starts_with('.'))
            .collect();
        children.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());

        for child in children {
            if cancelled() {
                return Ok(SearchResults {
                    entries: matches,
                    truncated: false,
                });
            }
            let Ok(file_type) = child.file_type() else {
                continue;
            };
            let path = child.path();
            let directory = if file_type.is_symlink() {
                path.is_dir()
            } else {
                file_type.is_dir()
            };
            if file_type.is_dir()
                && child
                    .metadata()
                    .is_ok_and(|metadata| metadata.dev() == root_device)
            {
                directories.push_back(path.clone());
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if !relative.to_string_lossy().to_lowercase().contains(&query) {
                continue;
            }
            if matches.len() == max_results {
                return Ok(SearchResults {
                    entries: matches,
                    truncated: true,
                });
            }
            matches.push(FileEntry {
                path,
                name: child.file_name(),
                directory,
                metadata: EntryMetadata::default(),
            });
        }
    }

    Ok(SearchResults {
        entries: matches,
        truncated: false,
    })
}

#[cfg(test)]
pub fn read_child_folders(path: &Path) -> Vec<PathBuf> {
    read_child_folders_with_hidden(path, false)
}

pub fn read_child_folders_with_hidden(path: &Path, show_hidden: bool) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            (show_hidden || !entry.file_name().to_string_lossy().starts_with('.'))
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect();
    folders.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    folders
}

fn natural_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.to_lowercase();
    let right = right.to_lowercase();
    let mut left = left.as_bytes().iter().copied().peekable();
    let mut right = right.as_bytes().iter().copied().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let mut a_digits = Vec::new();
                let mut b_digits = Vec::new();
                while left.peek().is_some_and(u8::is_ascii_digit) {
                    a_digits.push(left.next().unwrap());
                }
                while right.peek().is_some_and(u8::is_ascii_digit) {
                    b_digits.push(right.next().unwrap());
                }
                let a_trimmed = a_digits
                    .iter()
                    .skip_while(|digit| **digit == b'0')
                    .collect::<Vec<_>>();
                let b_trimmed = b_digits
                    .iter()
                    .skip_while(|digit| **digit == b'0')
                    .collect::<Vec<_>>();
                let ordering = a_trimmed
                    .len()
                    .cmp(&b_trimmed.len())
                    .then_with(|| a_trimmed.cmp(&b_trimmed))
                    .then_with(|| a_digits.len().cmp(&b_digits.len()));
                if !ordering.is_eq() {
                    return ordering;
                }
            }
            (Some(a), Some(b)) => {
                left.next();
                right.next();
                if a != b {
                    return a.cmp(&b);
                }
            }
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
        }
    }
}

pub fn read_entry_details(path: &Path) -> Result<String, FsError> {
    const ATTRIBUTES: &str = concat!(
        "standard::size,standard::type,unix::mode,unix::uid,unix::gid,",
        "owner::user,owner::group"
    );
    let info = gio::File::for_path(path)
        .query_info(
            ATTRIBUTES,
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| FsError::new("inspect", path, io::Error::other(error.to_string())))?;

    let permissions = if info.has_attribute("unix::mode") {
        format_permissions(info.attribute_uint32("unix::mode"), info.file_type())
    } else {
        "permissions unknown".to_owned()
    };
    let size = format_size(u64::try_from(info.size()).unwrap_or_default());
    let user = info
        .attribute_string("owner::user")
        .map(|value| value.to_string())
        .or_else(|| {
            info.has_attribute("unix::uid")
                .then(|| info.attribute_uint32("unix::uid").to_string())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let group = info
        .attribute_string("owner::group")
        .map(|value| value.to_string())
        .or_else(|| {
            info.has_attribute("unix::gid")
                .then(|| info.attribute_uint32("unix::gid").to_string())
        })
        .unwrap_or_else(|| "unknown".to_owned());

    Ok(format!("{permissions}  •  {size}  •  {user}:{group}"))
}

fn format_permissions(mode: u32, file_type: gio::FileType) -> String {
    let kind = match file_type {
        gio::FileType::Directory => 'd',
        gio::FileType::SymbolicLink => 'l',
        _ => '-',
    };
    let mut value = String::with_capacity(10);
    value.push(kind);
    value.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    value.push(match (mode & 0o100 != 0, mode & 0o4000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    value.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    value.push(match (mode & 0o010 != 0, mode & 0o2000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    value.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    value.push(match (mode & 0o001 != 0, mode & 0o1000 != 0) {
        (true, true) => 't',
        (false, true) => 'T',
        (true, false) => 'x',
        (false, false) => '-',
    });
    value
}

pub(crate) fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
