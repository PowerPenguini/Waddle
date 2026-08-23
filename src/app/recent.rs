use std::{ffi::OsString, fs, path::PathBuf};

use gio::prelude::FileExt;
use serde::{Deserialize, Serialize};

use crate::fs::FileEntry;

use super::{places, state::NodeKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Effect {
    Open,
    Reload,
    Disabled,
    Enabled,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Preferences {
    enabled: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Recent {
    history_path: PathBuf,
    preferences_path: PathBuf,
    preferences: Preferences,
}

impl Recent {
    pub(super) fn open_default() -> Self {
        Self::open_at(history_path(), preferences_path())
    }

    fn open_at(history_path: PathBuf, preferences_path: PathBuf) -> Self {
        let preferences = fs::read(&preferences_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            history_path,
            preferences_path,
            preferences,
        }
    }

    pub(super) fn sidebar_entry(&self) -> Option<places::Entry> {
        self.preferences.enabled.then(|| places::Entry {
            path: self.history_path.clone(),
            label: "Recent".to_owned(),
            kind: NodeKind::Recent,
            favorite_index: None,
        })
    }

    pub(super) fn entries(&self) -> Result<Vec<FileEntry>, String> {
        if !self.preferences.enabled || !self.history_path.exists() {
            return Ok(Vec::new());
        }
        let mut bookmarks = gio::glib::BookmarkFile::new();
        bookmarks
            .load_from_file(&self.history_path)
            .map_err(|error| format!("Could not read shared Recent history: {error}"))?;
        let mut entries = bookmarks
            .uris()
            .into_iter()
            .filter_map(|uri| {
                let path = gio::File::for_uri(uri.as_str()).path()?;
                let metadata = fs::metadata(&path).ok()?;
                let name = path.file_name().map(OsString::from)?;
                Some(FileEntry {
                    path,
                    name,
                    directory: metadata.is_dir(),
                    metadata: crate::fs::entry_metadata(&metadata),
                })
            })
            .collect::<Vec<_>>();
        entries.reverse();
        Ok(entries)
    }

    pub(super) fn watch_paths(&self, displayed: &[FileEntry]) -> Vec<PathBuf> {
        let mut paths = self
            .history_path
            .parent()
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>();
        paths.extend(
            displayed
                .iter()
                .filter_map(|entry| entry.path.parent().map(PathBuf::from)),
        );
        paths
    }

    pub(super) fn command(&mut self, arguments: &str) -> Result<(Effect, String), String> {
        match arguments.trim() {
            "" | "open" if self.preferences.enabled => {
                Ok((Effect::Open, "Opened shared Recent history".to_owned()))
            }
            "" | "open" => Err("Recent is disabled; use :recent enable".to_owned()),
            "clear" => {
                self.clear()?;
                Ok((Effect::Reload, "Shared Recent history cleared".to_owned()))
            }
            "disable" => {
                self.preferences.enabled = false;
                self.save_preferences()?;
                Ok((Effect::Disabled, "Recent disabled in PolarExp".to_owned()))
            }
            "enable" => {
                self.preferences.enabled = true;
                self.save_preferences()?;
                Ok((Effect::Enabled, "Recent enabled in PolarExp".to_owned()))
            }
            command => Err(format!(
                "unknown Recent command: {command}; expected open, clear, enable, or disable"
            )),
        }
    }

    fn clear(&self) -> Result<(), String> {
        if let Some(parent) = self.history_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        gio::glib::BookmarkFile::new()
            .to_file(&self.history_path)
            .map_err(|error| format!("Could not clear shared Recent history: {error}"))
    }

    fn save_preferences(&self) -> Result<(), String> {
        let parent = self
            .preferences_path
            .parent()
            .ok_or("Recent preferences path has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = self.preferences_path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&self.preferences).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, &self.preferences_path).map_err(|error| error.to_string())
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

fn history_path() -> PathBuf {
    data_home().join("recently-used.xbel")
}

fn preferences_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("polarexp/recent.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".polarexp-recent.json"),
        |home| PathBuf::from(home).join(".config/polarexp/recent.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_history_skips_missing_local_entries_and_can_be_disabled() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("kept.txt");
        fs::write(&existing, "kept").unwrap();
        let missing = temp.path().join("missing.txt");
        let history = temp.path().join("recently-used.xbel");
        let preferences = temp.path().join("config/recent.json");
        let mut bookmarks = gio::glib::BookmarkFile::new();
        let existing_uri = gio::File::for_path(&existing).uri();
        let missing_uri = gio::File::for_path(&missing).uri();
        bookmarks.set_title(Some(&existing_uri), "kept");
        bookmarks.add_application(&existing_uri, Some("PolarExp test"), Some("polarexp %u"));
        bookmarks.set_title(Some(&missing_uri), "missing");
        bookmarks.add_application(&missing_uri, Some("PolarExp test"), Some("polarexp %u"));
        bookmarks.to_file(&history).unwrap();

        let mut recent = Recent::open_at(history, preferences.clone());
        let entries = recent.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, existing);

        assert_eq!(recent.command("disable").unwrap().0, Effect::Disabled);
        assert!(recent.sidebar_entry().is_none());
        assert!(recent.entries().unwrap().is_empty());
        assert!(
            !Recent::open_at(temp.path().join("unused"), preferences)
                .preferences
                .enabled
        );
    }

    #[test]
    fn clear_writes_an_empty_valid_shared_bookmark_file() {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("recently-used.xbel");
        let preferences = temp.path().join("recent.json");
        let mut bookmarks = gio::glib::BookmarkFile::new();
        bookmarks.set_title(Some("file:///tmp/item"), "item");
        bookmarks.add_application(
            "file:///tmp/item",
            Some("PolarExp test"),
            Some("polarexp %u"),
        );
        bookmarks.to_file(&history).unwrap();
        let mut recent = Recent::open_at(history.clone(), preferences);

        assert_eq!(recent.command("clear").unwrap().0, Effect::Reload);
        let mut cleared = gio::glib::BookmarkFile::new();
        cleared.load_from_file(history).unwrap();
        assert!(cleared.uris().is_empty());
    }
}
