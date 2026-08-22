use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::fs::BrowseOptions;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Stored {
    global: BrowseOptions,
    overrides: BTreeMap<PathBuf, BrowseOptions>,
}

pub(super) struct Preferences {
    path: PathBuf,
    stored: Stored,
}

impl Preferences {
    pub(super) fn open_default() -> Self {
        let path = settings_path();
        let stored = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { path, stored }
    }

    pub(super) fn for_directory(&self, directory: &Path) -> BrowseOptions {
        self.stored
            .overrides
            .get(directory)
            .copied()
            .unwrap_or(self.stored.global)
    }

    pub(super) fn set_directory(&mut self, directory: PathBuf, options: BrowseOptions) {
        if options == self.stored.global {
            self.stored.overrides.remove(&directory);
        } else {
            self.stored.overrides.insert(directory, options);
        }
        let _ = self.save();
    }

    fn save(&self) -> Result<(), String> {
        let directory = self.path.parent().ok_or("settings path has no parent")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&self.stored).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, &self.path).map_err(|error| error.to_string())
    }
}

fn settings_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("polarexp/view.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".polarexp-view.json"),
        |home| PathBuf::from(home).join(".config/polarexp/view.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{SortKey, ViewMode};

    #[test]
    fn directory_override_round_trips_and_global_equivalent_removes_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("view.json");
        let mut preferences = Preferences {
            path: path.clone(),
            stored: Stored::default(),
        };
        let directory = PathBuf::from("/work");
        let override_options = BrowseOptions {
            view: ViewMode::List,
            sort: SortKey::Size,
            show_hidden: true,
            ..BrowseOptions::default()
        };
        preferences.set_directory(directory.clone(), override_options);
        let reopened = Preferences {
            path,
            stored: serde_json::from_slice(&fs::read(temp.path().join("view.json")).unwrap())
                .unwrap(),
        };
        assert_eq!(reopened.for_directory(&directory), override_options);
    }
}
