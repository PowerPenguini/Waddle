use std::{
    fs,
    path::{Path, PathBuf},
};

use gio::prelude::{MountExt, MountOperationExt, VolumeExt, VolumeMonitorExt};
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
                self.favorites.push(Favorite {
                    path: current.to_path_buf(),
                    label: label.clone(),
                });
                self.save()?;
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
                let removed = self.favorites.remove(index - 1);
                self.save()?;
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
        let favorite = self.favorites.remove(from);
        self.favorites.insert(to, favorite);
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        let directory = self.path.parent().ok_or("Favorites path has no parent")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&self.favorites).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, &self.path).map_err(|error| error.to_string())
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
                volume
                    .mount_future(gio::MountMountFlags::NONE, Some(&operation))
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(format!("Mounted {}", volume.name()))
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
