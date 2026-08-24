use super::*;

pub(crate) fn trash(path: &Path) -> Result<TrashReceipt, String> {
    gio::File::for_path(path)
        .trash(None::<&gio::Cancellable>)
        .map_err(|error| format!("could not move {} to Trash: {error}", path.display()))?;
    locate_trash(path).ok_or_else(|| {
        format!(
            "moved {} to Trash, but its recovery metadata could not be located",
            path.display()
        )
    })
}

fn locate_trash(original: &Path) -> Option<TrashReceipt> {
    locate_desktop_trash(original).or_else(|| locate_home_trash(original))
}

fn locate_desktop_trash(original: &Path) -> Option<TrashReceipt> {
    use std::os::unix::ffi::OsStringExt;

    let trash = gio::File::for_uri("trash:///");
    let enumerator = trash
        .enumerate_children(
            "standard::target-uri,trash::orig-path,trash::deletion-date",
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    let mut matches = Vec::new();
    while let Some(info) = enumerator.next_file(None::<&gio::Cancellable>).ok()? {
        let Some(encoded) = info.attribute_byte_string("trash::orig-path") else {
            continue;
        };
        let candidate = PathBuf::from(std::ffi::OsString::from_vec(encoded.as_bytes().to_vec()));
        if candidate != original {
            continue;
        }
        let Some(target_uri) = info.attribute_string("standard::target-uri") else {
            continue;
        };
        let Some(trashed) = gio::File::for_uri(target_uri.as_str()).path() else {
            continue;
        };
        let Some(trash_root) = trashed.parent().and_then(Path::parent) else {
            continue;
        };
        let Some(trashed_name) = trashed.file_name() else {
            continue;
        };
        let mut info_name = trashed_name.to_os_string();
        info_name.push(".trashinfo");
        let deletion_date = info
            .attribute_string("trash::deletion-date")
            .map_or_else(String::new, |value| value.to_string());
        matches.push((
            deletion_date,
            TrashReceipt {
                original: original.to_path_buf(),
                info: trash_root.join("info").join(info_name),
                trashed,
            },
        ));
    }
    matches.sort_by_key(|entry| entry.0.clone());
    matches.pop().map(|(_, receipt)| receipt)
}

fn locate_home_trash(original: &Path) -> Option<TrashReceipt> {
    let data_home = std::env::var_os("XDG_DATA_HOME").map_or_else(
        || {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share")
        },
        PathBuf::from,
    );
    let trash = data_home.join("Trash");
    let info_directory = trash.join("info");
    let mut matches = fs::read_dir(&info_directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let info = entry.path();
            if info.extension()? != "trashinfo" {
                return None;
            }
            let contents = fs::read_to_string(&info).ok()?;
            let encoded = contents
                .lines()
                .find_map(|line| line.strip_prefix("Path="))?;
            if percent_decode_path(encoded).as_deref() != Some(original.as_os_str()) {
                return None;
            }
            let name = info.file_stem()?;
            let trashed = trash.join("files").join(name);
            fs::symlink_metadata(&trashed).ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((
                modified,
                TrashReceipt {
                    original: original.to_path_buf(),
                    trashed,
                    info,
                },
            ))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(modified, _)| *modified);
    matches.pop().map(|(_, receipt)| receipt)
}

#[cfg(unix)]
pub(super) fn percent_decode_path(value: &str) -> Option<std::ffi::OsString> {
    use std::os::unix::ffi::OsStringExt;

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
    Some(std::ffi::OsString::from_vec(decoded))
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
