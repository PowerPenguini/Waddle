use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::fs::{BrowseOptions, SortKey, ViewMode};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum PreferenceOverride {
    #[default]
    Auto,
    On,
    Off,
}

impl PreferenceOverride {
    pub(super) fn resolve(self, system: bool) -> bool {
        match self {
            Self::Auto => system,
            Self::On => true,
            Self::Off => false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct Stored {
    global: BrowseOptions,
    overrides: BTreeMap<PathBuf, BrowseOptions>,
    single_click_activation: bool,
    high_contrast: PreferenceOverride,
    reduced_motion: PreferenceOverride,
    reduced_transparency: PreferenceOverride,
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

    #[cfg(test)]
    pub(super) fn empty_at(path: PathBuf) -> Self {
        Self {
            path,
            stored: Stored::default(),
        }
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

    pub(super) fn single_click_activation(&self) -> bool {
        self.stored.single_click_activation
    }

    pub(super) fn toggle_single_click_activation(&mut self) {
        self.stored.single_click_activation = !self.stored.single_click_activation;
        let _ = self.save();
    }

    pub(super) fn high_contrast(&self) -> PreferenceOverride {
        self.stored.high_contrast
    }

    pub(super) fn reduced_motion(&self) -> PreferenceOverride {
        self.stored.reduced_motion
    }

    pub(super) fn reduced_transparency(&self) -> PreferenceOverride {
        self.stored.reduced_transparency
    }

    pub(super) fn apply_command(
        &mut self,
        directory: &Path,
        local: bool,
        arguments: &str,
    ) -> Result<String, String> {
        let arguments = arguments.trim();
        if arguments.is_empty() {
            return Ok(self.describe(directory, local));
        }
        if arguments == "all" {
            return Ok(format!(
                "{}\n\nview: grid or list\nsort: name, modified, size, or type\ndirection: ascending or descending\nfolders-first: keep folders before files\nhidden: show dot-prefixed entries\nclick: single or double activation (global)\nhigh-contrast: auto, true, or false\nreduced-motion: auto, true, or false\nreduced-transparency: auto, true, or false",
                self.describe(directory, local)
            ));
        }
        let previous = self.stored.clone();
        let mut options = if local {
            self.for_directory(directory)
        } else {
            self.stored.global
        };
        let mut click = self.stored.single_click_activation;
        let mut visual = (
            self.stored.high_contrast,
            self.stored.reduced_motion,
            self.stored.reduced_transparency,
        );
        for argument in arguments.split_whitespace() {
            apply_option(
                &mut options,
                &mut click,
                argument,
                local,
                self.stored.global,
                &mut visual,
            )?;
        }
        if local {
            if options == self.stored.global {
                self.stored.overrides.remove(directory);
            } else {
                self.stored
                    .overrides
                    .insert(directory.to_path_buf(), options);
            }
        } else {
            self.stored.global = options;
            self.stored.single_click_activation = click;
            self.stored.high_contrast = visual.0;
            self.stored.reduced_motion = visual.1;
            self.stored.reduced_transparency = visual.2;
        }
        if let Err(error) = self.save() {
            self.stored = previous;
            return Err(error);
        }
        Ok(self.describe(directory, local))
    }

    fn describe(&self, directory: &Path, local: bool) -> String {
        let options = if local {
            self.for_directory(directory)
        } else {
            self.stored.global
        };
        let scope = if local { "local" } else { "global" };
        format!(
            "{scope}: view={} sort={} direction={} folders-first={} hidden={} click={} high-contrast={} reduced-motion={} reduced-transparency={}",
            match options.view {
                ViewMode::Grid => "grid",
                ViewMode::List => "list",
            },
            match options.sort {
                SortKey::Name => "name",
                SortKey::Modified => "modified",
                SortKey::Size => "size",
                SortKey::Type => "type",
            },
            if options.descending {
                "descending"
            } else {
                "ascending"
            },
            options.folders_first,
            options.show_hidden,
            if self.stored.single_click_activation {
                "single"
            } else {
                "double"
            },
            override_label(self.stored.high_contrast),
            override_label(self.stored.reduced_motion),
            override_label(self.stored.reduced_transparency),
        )
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

fn apply_option(
    options: &mut BrowseOptions,
    click: &mut bool,
    argument: &str,
    local: bool,
    global: BrowseOptions,
    visual: &mut (PreferenceOverride, PreferenceOverride, PreferenceOverride),
) -> Result<(), String> {
    if let Some(name) = argument.strip_suffix('&') {
        if !local {
            return Err("option& is only valid with :setlocal".to_owned());
        }
        return match name {
            "view" => {
                options.view = global.view;
                Ok(())
            }
            "sort" => {
                options.sort = global.sort;
                Ok(())
            }
            "direction" => {
                options.descending = global.descending;
                Ok(())
            }
            "folders-first" => {
                options.folders_first = global.folders_first;
                Ok(())
            }
            "hidden" => {
                options.show_hidden = global.show_hidden;
                Ok(())
            }
            "click" => Err("click activation is global and cannot be reset locally".to_owned()),
            _ => Err(format!("unknown setting: {name}")),
        };
    }
    let (name, value) = argument
        .split_once('=')
        .ok_or_else(|| format!("expected option=value, got: {argument}"))?;
    match (name, value) {
        ("view", "grid") => options.view = ViewMode::Grid,
        ("view", "list") => options.view = ViewMode::List,
        ("sort", "name") => options.sort = SortKey::Name,
        ("sort", "modified") => options.sort = SortKey::Modified,
        ("sort", "size") => options.sort = SortKey::Size,
        ("sort", "type") => options.sort = SortKey::Type,
        ("direction", "ascending" | "asc") => options.descending = false,
        ("direction", "descending" | "desc") => options.descending = true,
        ("folders-first", value) => options.folders_first = parse_bool(name, value)?,
        ("hidden", value) => options.show_hidden = parse_bool(name, value)?,
        ("click", _) if local => return Err("click activation is a global setting".to_owned()),
        ("click", "single") => *click = true,
        ("click", "double") => *click = false,
        ("high-contrast", value) if !local => visual.0 = parse_override(name, value)?,
        ("reduced-motion", value) if !local => visual.1 = parse_override(name, value)?,
        ("reduced-transparency", value) if !local => visual.2 = parse_override(name, value)?,
        ("high-contrast" | "reduced-motion" | "reduced-transparency", _) if local => {
            return Err(format!("{name} is a global setting"));
        }
        ("view" | "sort" | "direction" | "click", _) => {
            return Err(format!("invalid value for {name}: {value}"));
        }
        _ => return Err(format!("unknown setting: {name}")),
    }
    Ok(())
}

fn parse_override(name: &str, value: &str) -> Result<PreferenceOverride, String> {
    match value {
        "auto" => Ok(PreferenceOverride::Auto),
        "true" | "on" | "yes" => Ok(PreferenceOverride::On),
        "false" | "off" | "no" => Ok(PreferenceOverride::Off),
        _ => Err(format!("invalid override for {name}: {value}")),
    }
}

fn override_label(value: PreferenceOverride) -> &'static str {
    match value {
        PreferenceOverride::Auto => "auto",
        PreferenceOverride::On => "true",
        PreferenceOverride::Off => "false",
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        _ => Err(format!("invalid boolean for {name}: {value}")),
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

    #[test]
    fn click_activation_mode_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("view.json");
        let mut preferences = Preferences {
            path: path.clone(),
            stored: Stored::default(),
        };
        assert!(!preferences.single_click_activation());

        preferences.toggle_single_click_activation();
        let reopened = Preferences {
            path,
            stored: serde_json::from_slice(&fs::read(temp.path().join("view.json")).unwrap())
                .unwrap(),
        };
        assert!(reopened.single_click_activation());
    }

    #[test]
    fn setting_commands_are_atomic_and_local_reset_uses_global_value() {
        let temp = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::empty_at(temp.path().join("view.json"));
        let directory = Path::new("/work");
        preferences
            .apply_command(
                directory,
                false,
                "view=list sort=size hidden=true click=single",
            )
            .unwrap();
        assert_eq!(preferences.for_directory(directory).view, ViewMode::List);
        assert!(preferences.single_click_activation());

        preferences
            .apply_command(directory, true, "view=grid sort=type")
            .unwrap();
        assert_eq!(preferences.for_directory(directory).view, ViewMode::Grid);
        preferences.apply_command(directory, true, "view&").unwrap();
        assert_eq!(preferences.for_directory(directory).view, ViewMode::List);
        assert_eq!(preferences.for_directory(directory).sort, SortKey::Type);

        let before = preferences.for_directory(directory);
        assert!(
            preferences
                .apply_command(directory, true, "sort=nope")
                .is_err()
        );
        assert_eq!(preferences.for_directory(directory), before);
    }

    #[test]
    fn accessibility_overrides_resolve_against_system_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::empty_at(temp.path().join("view.json"));
        preferences
            .apply_command(
                Path::new("/work"),
                false,
                "high-contrast=true reduced-motion=false reduced-transparency=auto",
            )
            .unwrap();

        assert!(preferences.high_contrast().resolve(false));
        assert!(!preferences.reduced_motion().resolve(true));
        assert!(preferences.reduced_transparency().resolve(true));
        assert!(!preferences.reduced_transparency().resolve(false));
    }
}
