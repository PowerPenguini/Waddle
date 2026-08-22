use std::{
    ffi::OsString,
    fs,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use gio::prelude::{FileEnumeratorExt, FileExt};

use crate::{fs::FileEntry, journal};

use super::{places, state::NodeKind};

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub(super) file: FileEntry,
    pub(super) receipt: journal::TrashReceipt,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RestoreReport {
    pub(super) restored: Vec<journal::TrashReceipt>,
    pub(super) failures: Vec<(PathBuf, String)>,
    pub(super) retained: usize,
    pub(super) warnings: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct DeleteReport {
    pub(super) deleted: usize,
    pub(super) failures: Vec<(FileEntry, String)>,
}

#[derive(Clone, Debug)]
pub(super) struct Trash {
    physical_root: Option<PathBuf>,
}

impl Trash {
    pub(super) fn open_default() -> Self {
        Self {
            physical_root: None,
        }
    }

    #[cfg(test)]
    fn at(root: PathBuf) -> Self {
        Self {
            physical_root: Some(root),
        }
    }

    pub(super) fn sidebar_entry(&self) -> places::Entry {
        places::Entry {
            path: data_home().join("Trash"),
            label: "Trash".to_owned(),
            kind: NodeKind::Trash,
            favorite_index: None,
        }
    }

    pub(super) fn entries(&self) -> Result<Vec<Entry>, String> {
        match &self.physical_root {
            Some(root) => physical_entries(root),
            None => desktop_entries(),
        }
    }
}

fn physical_entries(root: &Path) -> Result<Vec<Entry>, String> {
    let info_directory = root.join("info");
    let files_directory = root.join("files");
    let read = match fs::read_dir(&info_directory) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not read Trash metadata: {error}")),
    };
    let mut entries = read
        .filter_map(Result::ok)
        .filter_map(|metadata| {
            let info = metadata.path();
            if info.extension()? != "trashinfo" {
                return None;
            }
            let original = original_path(&info)?;
            let name = info.file_stem()?.to_owned();
            let trashed = files_directory.join(&name);
            let file_metadata = fs::symlink_metadata(&trashed).ok()?;
            let directory = file_metadata.file_type().is_dir();
            let display_name = original
                .file_name()
                .map_or_else(|| name.clone(), OsString::from);
            Some((
                metadata.metadata().ok()?.modified().ok(),
                Entry {
                    file: FileEntry {
                        path: trashed.clone(),
                        name: display_name,
                        directory,
                    },
                    receipt: journal::TrashReceipt {
                        original,
                        trashed,
                        info,
                    },
                },
            ))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

fn desktop_entries() -> Result<Vec<Entry>, String> {
    let trash = gio::File::for_uri("trash:///");
    let enumerator = trash
        .enumerate_children(
            "standard::type,standard::target-uri,trash::orig-path,trash::deletion-date",
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| format!("Could not open desktop Trash: {error}"))?;
    let mut entries = Vec::new();
    loop {
        let info = enumerator
            .next_file(None::<&gio::Cancellable>)
            .map_err(|error| format!("Could not read a desktop Trash entry: {error}"))?;
        let Some(info) = info else {
            break;
        };
        let Some(target_uri) = info.attribute_string("standard::target-uri") else {
            continue;
        };
        let Some(trashed) = gio::File::for_uri(target_uri.as_str()).path() else {
            continue;
        };
        let Some(original_bytes) = info.attribute_byte_string("trash::orig-path") else {
            continue;
        };
        let original = PathBuf::from(OsString::from_vec(original_bytes.as_bytes().to_vec()));
        let Some(physical_name) = trashed.file_name() else {
            continue;
        };
        let Some(trash_root) = trashed.parent().and_then(Path::parent) else {
            continue;
        };
        let mut info_name = physical_name.to_os_string();
        info_name.push(".trashinfo");
        let info_path = trash_root.join("info").join(info_name);
        let Some(display_name) = original.file_name().map(OsString::from) else {
            continue;
        };
        let deleted = info
            .attribute_string("trash::deletion-date")
            .map_or_else(String::new, |value| value.to_string());
        entries.push((
            deleted,
            Entry {
                file: FileEntry {
                    path: trashed.clone(),
                    name: display_name,
                    directory: info.file_type() == gio::FileType::Directory,
                },
                receipt: journal::TrashReceipt {
                    original,
                    trashed,
                    info: info_path,
                },
            },
        ));
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0.clone()));
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

pub(super) fn restore_batch(entries: &[Entry]) -> crate::fs::TransferBatch {
    crate::fs::TransferBatch::new_mapped(
        entries.iter().map(|entry| {
            (
                entry.receipt.trashed.clone(),
                entry.receipt.original.clone(),
            )
        }),
        crate::transfer::Action::Move,
    )
}

pub(super) fn finish_restore(
    report: crate::fs::TransferReport,
    entries: &[Entry],
) -> RestoreReport {
    let mut restored = Vec::new();
    let mut warnings = report
        .warnings
        .into_iter()
        .map(|warning| warning.detail)
        .collect::<Vec<_>>();
    for receipt in report.receipts {
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.receipt.trashed == receipt.source)
        else {
            continue;
        };
        if let Err(error) = fs::remove_file(&entry.receipt.info) {
            warnings.push(format!(
                "Restored {}, but could not remove Trash metadata: {error}",
                receipt.destination.display()
            ));
        }
        restored.push(journal::TrashReceipt {
            original: receipt.destination,
            trashed: receipt.source,
            info: entry.receipt.info.clone(),
        });
    }
    RestoreReport {
        restored,
        failures: report
            .failures
            .into_iter()
            .map(|failure| (failure.source, failure.error))
            .collect(),
        retained: report.retained.len(),
        warnings,
    }
}

pub(super) fn delete(entries: Vec<Entry>) -> DeleteReport {
    let mut report = DeleteReport::default();
    for entry in entries {
        match crate::fs::delete_permanently(&entry.receipt.trashed) {
            Ok(()) => match fs::remove_file(&entry.receipt.info) {
                Ok(()) => report.deleted += 1,
                Err(error) => report.failures.push((
                    entry.file,
                    format!("item deleted, but Trash metadata remains: {error}"),
                )),
            },
            Err(error) => report.failures.push((entry.file, error.to_string())),
        }
    }
    report
}

fn original_path(info: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(info).ok()?;
    let encoded = contents
        .lines()
        .find_map(|line| line.strip_prefix("Path="))?;
    Some(PathBuf::from(percent_decode(encoded)?))
}

fn percent_decode(value: &str) -> Option<OsString> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Some(OsString::from_vec(decoded))
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map_or_else(
        || {
            std::env::var_os("HOME").map_or_else(
                || PathBuf::from(".local/share"),
                |home| PathBuf::from(home).join(".local/share"),
            )
        },
        PathBuf::from,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_lists_original_locations_and_skips_orphaned_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Trash");
        fs::create_dir_all(root.join("files")).unwrap();
        fs::create_dir_all(root.join("info")).unwrap();
        fs::write(root.join("files/report.txt.2"), "report").unwrap();
        fs::write(
            root.join("info/report.txt.2.trashinfo"),
            "[Trash Info]\nPath=/home/user/Reports/report%20final.txt\nDeletionDate=2026-08-22T10:00:00\n",
        )
        .unwrap();
        fs::write(
            root.join("info/missing.trashinfo"),
            "[Trash Info]\nPath=/home/user/missing\n",
        )
        .unwrap();

        let entries = Trash::at(root).entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].file.name,
            std::ffi::OsStr::new("report final.txt")
        );
        assert_eq!(
            entries[0].receipt.original,
            Path::new("/home/user/Reports/report final.txt")
        );
    }

    #[test]
    fn completed_restore_removes_metadata_and_keeps_an_undo_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let trashed = temp.path().join("Trash/files/item");
        let info = temp.path().join("Trash/info/item.trashinfo");
        let original = temp.path().join("restored/item");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::create_dir_all(original.parent().unwrap()).unwrap();
        fs::write(&trashed, "content").unwrap();
        fs::write(&info, "[Trash Info]\nPath=/unused\n").unwrap();
        let entry = Entry {
            file: FileEntry {
                path: trashed.clone(),
                name: OsString::from("item"),
                directory: false,
            },
            receipt: journal::TrashReceipt {
                original: original.clone(),
                trashed: trashed.clone(),
                info: info.clone(),
            },
        };
        let crate::fs::TransferBatchOutcome::Complete(report) =
            restore_batch(std::slice::from_ref(&entry)).run()
        else {
            panic!("unexpected conflict");
        };

        let restored = finish_restore(report, &[entry]);
        assert_eq!(restored.restored.len(), 1);
        assert_eq!(restored.restored[0].original, original);
        assert!(!info.exists());
        assert!(!trashed.exists());
    }
}
