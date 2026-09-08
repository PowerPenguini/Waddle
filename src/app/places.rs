use std::{
    fs,
    path::{Path, PathBuf},
};

use gio::prelude::{FileExt, MountExt, MountOperationExt, VolumeExt, VolumeMonitorExt};
use serde::{Deserialize, Serialize};

use super::tree::NodeKind;

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub path: PathBuf,
    pub label: String,
    pub kind: NodeKind,
    pub favorite_index: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Favorite {
    #[serde(with = "crate::path_serde")]
    path: PathBuf,
    label: String,
}

#[derive(Debug)]
pub(super) struct Places {
    path: PathBuf,
    favorites: Vec<Favorite>,
}

impl Places {
    pub(super) fn open_default() -> Self {
        let path = config_path();
        let favorites = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { path, favorites }
    }

    #[cfg(test)]
    pub(super) fn empty_at(path: PathBuf) -> Self {
        Self {
            path,
            favorites: Vec::new(),
        }
    }

    pub(super) fn entries(&self) -> Vec<Entry> {
        let mut entries = Vec::new();
        if let Some(home) = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
        {
            entries.push(Entry {
                path: home,
                label: "Home".to_owned(),
                kind: NodeKind::Home,
                favorite_index: None,
            });
        }
        for (directory, label, kind) in [
            (
                gio::glib::UserDirectory::Desktop,
                "Desktop",
                NodeKind::Desktop,
            ),
            (
                gio::glib::UserDirectory::Documents,
                "Documents",
                NodeKind::Documents,
            ),
            (
                gio::glib::UserDirectory::Downloads,
                "Downloads",
                NodeKind::Downloads,
            ),
            (gio::glib::UserDirectory::Music, "Music", NodeKind::Music),
            (
                gio::glib::UserDirectory::Pictures,
                "Pictures",
                NodeKind::Pictures,
            ),
            (gio::glib::UserDirectory::Videos, "Videos", NodeKind::Videos),
        ] {
            if let Some(path) = gio::glib::user_special_dir(directory).filter(|path| path.is_dir())
            {
                if kind == NodeKind::Desktop
                    && entries.iter().any(|entry| {
                        entry.kind == NodeKind::Home
                            && (entry.path == path
                                || entry
                                    .path
                                    .canonicalize()
                                    .ok()
                                    .zip(path.canonicalize().ok())
                                    .is_some_and(|(home, desktop)| home == desktop))
                    })
                {
                    continue;
                }
                entries.push(Entry {
                    path,
                    label: label.to_owned(),
                    kind,
                    favorite_index: None,
                });
            }
        }
        entries.extend(
            self.favorites
                .iter()
                .enumerate()
                .filter(|(_, favorite)| favorite.path.is_dir())
                .map(|(index, favorite)| Entry {
                    path: favorite.path.clone(),
                    label: favorite.label.clone(),
                    kind: NodeKind::Favorite,
                    favorite_index: Some(index),
                }),
        );
        entries
    }

    pub(super) fn command(&mut self, current: &Path, arguments: &str) -> Result<String, String> {
        let mut parts = arguments.trim().splitn(2, char::is_whitespace);
        match parts.next().unwrap_or_default() {
            "add" => {
                let label = parts
                    .next()
                    .map(str::trim)
                    .filter(|label| !label.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        current.file_name().map_or_else(
                            || current.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        )
                    });
                if self
                    .favorites
                    .iter()
                    .any(|favorite| favorite.path == current)
                {
                    return Err("the current folder is already a Favorite".to_owned());
                }
                let mut favorites = self.favorites.clone();
                favorites.push(Favorite {
                    path: current.to_path_buf(),
                    label: label.clone(),
                });
                self.commit(favorites)?;
                Ok(format!("Added Favorite: {label}"))
            }
            "remove" => {
                let index = parts
                    .next()
                    .ok_or("expected :favorite remove INDEX")?
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Favorite index must be a number".to_owned())?;
                if index == 0 || index > self.favorites.len() {
                    return Err("Favorite index is out of range".to_owned());
                }
                let mut favorites = self.favorites.clone();
                let removed = favorites.remove(index - 1);
                self.commit(favorites)?;
                Ok(format!("Removed Favorite: {}", removed.label))
            }
            "list" | "" => Ok(if self.favorites.is_empty() {
                "No Favorites. Use :favorite add [LABEL] in a folder.".to_owned()
            } else {
                self.favorites
                    .iter()
                    .enumerate()
                    .map(|(index, favorite)| {
                        format!(
                            "{}. {}  {}",
                            index + 1,
                            favorite.label,
                            favorite.path.display()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
            command => Err(format!("unknown favorite command: {command}")),
        }
    }

    pub(super) fn reorder(&mut self, from: usize, to: usize) -> Result<(), String> {
        if from >= self.favorites.len() || to >= self.favorites.len() || from == to {
            return Ok(());
        }
        let mut favorites = self.favorites.clone();
        let favorite = favorites.remove(from);
        favorites.insert(to, favorite);
        self.commit(favorites)
    }

    fn commit(&mut self, favorites: Vec<Favorite>) -> Result<(), String> {
        let directory = self.path.parent().ok_or("Favorites path has no parent")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&favorites).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, &self.path).map_err(|error| error.to_string())?;
        self.favorites = favorites;
        Ok(())
    }
}

fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("waddle/favorites.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".waddle-favorites.json"),
        |home| PathBuf::from(home).join(".config/waddle/favorites.json"),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VolumeAction {
    Mount,
    Unmount,
    Eject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MountedVolume {
    pub(super) label: String,
}

pub(super) fn volume_id(volume: &gio::Volume) -> String {
    volume
        .uuid()
        .map(|uuid| format!("uuid:{uuid}"))
        .or_else(|| {
            volume
                .identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE)
                .map(|device| format!("device:{device}"))
        })
        .unwrap_or_else(|| format!("name:{}", volume.name()))
}

pub(super) fn mount_id(mount: &gio::Mount) -> String {
    format!("mount:{}", mount.root().uri())
}

async fn mount_volume_object(
    volume: gio::Volume,
    operation: &gio::MountOperation,
) -> Result<MountedVolume, String> {
    let label = volume.name().to_string();
    volume
        .mount_future(gio::MountMountFlags::NONE, Some(operation))
        .await
        .map_err(|error| error.to_string())?;
    Ok(MountedVolume { label })
}

pub(super) fn mount_volume(id: &str) -> Result<MountedVolume, String> {
    let id = id.to_owned();
    let context = gio::glib::MainContext::new();
    context.block_on(async move {
        let monitor = gio::VolumeMonitor::get();
        let volume = monitor
            .volumes()
            .into_iter()
            .find(|volume| volume_id(volume) == id)
            .ok_or("volume is no longer available")?;
        let operation = gio::MountOperation::new();
        operation.set_password_save(gio::PasswordSave::ForSession);
        mount_volume_object(volume, &operation).await
    })
}

pub(super) fn unmount_volume(id: &str) -> Result<(), String> {
    let id = id.to_owned();
    let context = gio::glib::MainContext::new();
    context.block_on(async move {
        let monitor = gio::VolumeMonitor::get();
        let mount = monitor
            .volumes()
            .into_iter()
            .find(|volume| volume_id(volume) == id)
            .and_then(|volume| volume.get_mount())
            .or_else(|| {
                monitor
                    .mounts()
                    .into_iter()
                    .find(|mount| mount_id(mount) == id)
            })
            .ok_or("mounted volume is no longer available")?;
        let operation = gio::MountOperation::new();
        operation.set_password_save(gio::PasswordSave::ForSession);
        if mount.can_unmount() {
            mount
                .unmount_with_operation_future(gio::MountUnmountFlags::NONE, Some(&operation))
                .await
        } else if mount.can_eject() {
            mount
                .eject_with_operation_future(gio::MountUnmountFlags::NONE, Some(&operation))
                .await
        } else {
            return Err("this volume cannot be unmounted".to_owned());
        }
        .map_err(|error| error.to_string())
    })
}

fn parse_volume_action(arguments: &str) -> Result<(VolumeAction, String), String> {
    let (action, name) = arguments
        .trim()
        .split_once(char::is_whitespace)
        .ok_or("expected :volume mount|unmount|eject NAME")?;
    let action = match action {
        "mount" => VolumeAction::Mount,
        "unmount" => VolumeAction::Unmount,
        "eject" => VolumeAction::Eject,
        _ => return Err(format!("unknown volume action: {action}")),
    };
    let name = name.trim();
    if name.is_empty() {
        return Err("volume name cannot be empty".to_owned());
    }
    Ok((action, name.to_owned()))
}

pub(super) fn run_volume_command(arguments: &str) -> Result<String, String> {
    let (action, name) = parse_volume_action(arguments)?;
    let context = gio::glib::MainContext::new();
    context.block_on(async move {
        let monitor = gio::VolumeMonitor::get();
        let operation = gio::MountOperation::new();
        operation.set_password_save(gio::PasswordSave::ForSession);
        match action {
            VolumeAction::Mount => {
                let volume = monitor
                    .volumes()
                    .into_iter()
                    .find(|volume| volume.name().eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("volume not found: {name}"))?;
                mount_volume_object(volume, &operation)
                    .await
                    .map(|mounted| format!("Mounted {}", mounted.label))
            }
            VolumeAction::Unmount | VolumeAction::Eject => {
                let mount = monitor
                    .mounts()
                    .into_iter()
                    .find(|mount| mount.name().eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("mounted volume not found: {name}"))?;
                if action == VolumeAction::Eject {
                    mount
                        .eject_with_operation_future(gio::MountUnmountFlags::NONE, Some(&operation))
                        .await
                } else {
                    mount
                        .unmount_with_operation_future(
                            gio::MountUnmountFlags::NONE,
                            Some(&operation),
                        )
                        .await
                }
                .map_err(|error| error.to_string())?;
                Ok(format!(
                    "{} {}",
                    if action == VolumeAction::Eject {
                        "Ejected"
                    } else {
                        "Unmounted"
                    },
                    mount.name()
                ))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunt_favorites_preserve_non_utf8_folder_paths() {
        use std::os::unix::ffi::OsStringExt;
        let temp = tempfile::tempdir().unwrap();
        let folder = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"folder-\xff".to_vec()));
        fs::create_dir(&folder).unwrap();
        let path = temp.path().join("favorites.json");
        let mut places = Places::empty_at(path.clone());
        places
            .command(&folder, "add My folder")
            .expect("a valid Unix path must remain a saveable Favorite");
        let reopened = Places {
            path: path.clone(),
            favorites: serde_json::from_slice(&fs::read(path).unwrap()).unwrap(),
        };
        assert!(
            reopened
                .entries()
                .iter()
                .any(|entry| entry.kind == NodeKind::Favorite
                    && entry.path == folder
                    && entry.label == "My folder")
        );
    }

    #[test]
    fn hunt_failed_favorite_save_preserves_state_and_allows_retry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("favorites.json");
        let mut places = Places::empty_at(path.clone());
        for name in ["one", "two", "three"] {
            fs::create_dir(temp.path().join(name)).unwrap();
        }
        places.command(&temp.path().join("one"), "add One").unwrap();
        places.command(&temp.path().join("two"), "add Two").unwrap();
        let before = places.command(temp.path(), "list").unwrap();
        let disk_before = fs::read(&path).unwrap();
        // An occupied staging path deterministically makes writes fail, including as root.
        let blocked = path.with_extension("json.tmp");
        fs::create_dir(&blocked).unwrap();
        assert!(
            places
                .command(&temp.path().join("three"), "add Three")
                .is_err()
        );
        assert_eq!(
            places.command(temp.path(), "list").unwrap(),
            before,
            "failed Add must not create an unsaved Favorite"
        );
        assert!(places.command(temp.path(), "remove 1").is_err());
        assert_eq!(
            places.command(temp.path(), "list").unwrap(),
            before,
            "failed Remove must preserve the Favorite"
        );
        assert!(places.reorder(0, 1).is_err());
        assert_eq!(
            places.command(temp.path(), "list").unwrap(),
            before,
            "failed Reorder must preserve order"
        );
        assert_eq!(fs::read(&path).unwrap(), disk_before);
        fs::remove_dir(blocked).unwrap();
        places
            .command(&temp.path().join("three"), "add Three")
            .expect("retry should work after repairing storage");
        places.reorder(2, 0).unwrap();
        places.command(temp.path(), "remove 2").unwrap();
        let favorites: Vec<Favorite> = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            favorites
                .iter()
                .map(|favorite| favorite.label.as_str())
                .collect::<Vec<_>>(),
            ["Three", "Two"]
        );
    }

    #[test]
    fn favorites_keep_custom_labels_and_persist_drag_order() {
        let temp = tempfile::tempdir().unwrap();
        let one = temp.path().join("one");
        let two = temp.path().join("two");
        fs::create_dir(&one).unwrap();
        fs::create_dir(&two).unwrap();
        let path = temp.path().join("favorites.json");
        let mut places = Places::empty_at(path.clone());
        places.command(&one, "add First label").unwrap();
        places.command(&two, "add Second label").unwrap();
        places.reorder(1, 0).unwrap();

        let favorites: Vec<Favorite> = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(favorites[0].label, "Second label");
        assert_eq!(favorites[1].path, one);
    }

    #[test]
    fn volume_commands_require_a_known_action_and_name() {
        assert_eq!(
            parse_volume_action("unmount Backup Drive").unwrap(),
            (VolumeAction::Unmount, "Backup Drive".to_owned())
        );
        assert!(parse_volume_action("format disk").is_err());
        assert!(parse_volume_action("mount").is_err());
    }
}
