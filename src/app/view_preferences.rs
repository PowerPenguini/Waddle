use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::fs::{BrowseOptions, SortKey, ViewMode};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum PreferenceOverride {
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum StartupBehavior {
    #[default]
    Last,
    Cwd,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BrowsePatch {
    view: Option<ViewMode>,
    sort: Option<SortKey>,
    descending: Option<bool>,
    folders_first: Option<bool>,
    show_hidden: Option<bool>,
}

impl BrowsePatch {
    fn apply(self, mut options: BrowseOptions) -> BrowseOptions {
        if let Some(view) = self.view {
            options.view = view;
        }
        if let Some(sort) = self.sort {
            options.sort = sort;
        }
        if let Some(descending) = self.descending {
            options.descending = descending;
        }
        if let Some(folders_first) = self.folders_first {
            options.folders_first = folders_first;
        }
        if let Some(show_hidden) = self.show_hidden {
            options.show_hidden = show_hidden;
        }
        options
    }

    fn record_difference(&mut self, before: BrowseOptions, after: BrowseOptions) {
        if before.view != after.view {
            self.view = Some(after.view);
        }
        if before.sort != after.sort {
            self.sort = Some(after.sort);
        }
        if before.descending != after.descending {
            self.descending = Some(after.descending);
        }
        if before.folders_first != after.folders_first {
            self.folders_first = Some(after.folders_first);
        }
        if before.show_hidden != after.show_hidden {
            self.show_hidden = Some(after.show_hidden);
        }
    }

    fn remove_values_matching(&mut self, base: BrowseOptions) {
        if self.view == Some(base.view) {
            self.view = None;
        }
        if self.sort == Some(base.sort) {
            self.sort = None;
        }
        if self.descending == Some(base.descending) {
            self.descending = None;
        }
        if self.folders_first == Some(base.folders_first) {
            self.folders_first = None;
        }
        if self.show_hidden == Some(base.show_hidden) {
            self.show_hidden = None;
        }
    }

    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GlobalSettings {
    browse: BrowseOptions,
    single_click_activation: bool,
    high_contrast: PreferenceOverride,
    reduced_motion: PreferenceOverride,
    reduced_transparency: PreferenceOverride,
    startup: StartupBehavior,
    tree_visible: bool,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            browse: BrowseOptions::default(),
            single_click_activation: false,
            high_contrast: PreferenceOverride::Auto,
            reduced_motion: PreferenceOverride::Auto,
            reduced_transparency: PreferenceOverride::Auto,
            startup: StartupBehavior::Last,
            tree_visible: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Config {
    global: GlobalSettings,
    locals: BTreeMap<PathBuf, BrowsePatch>,
}

#[derive(Clone, Debug)]
struct Session {
    global: GlobalSettings,
    locals: BTreeMap<PathBuf, BrowsePatch>,
}

#[derive(Clone, Debug)]
pub(super) struct Applied {
    pub(super) status: String,
    pub(super) browse_changed: bool,
    pub(super) tree_changed: bool,
}

pub(super) struct Preferences {
    path: PathBuf,
    configured: Config,
    session: Session,
    error: Option<String>,
}

impl Preferences {
    pub(super) fn open_default() -> Self {
        let path = settings_path();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self::open(path, home.as_deref())
    }

    fn open(path: PathBuf, home: Option<&Path>) -> Self {
        let (configured, error) = match load_config(&path, home) {
            Ok(config) => (config, None),
            Err(error) => (Config::default(), Some(error)),
        };
        Self {
            path,
            session: Session {
                global: configured.global,
                locals: BTreeMap::new(),
            },
            configured,
            error,
        }
    }

    #[cfg(test)]
    pub(super) fn empty_at(path: PathBuf) -> Self {
        Self {
            path,
            configured: Config::default(),
            session: Session {
                global: GlobalSettings::default(),
                locals: BTreeMap::new(),
            },
            error: None,
        }
    }

    #[cfg(test)]
    fn open_at(path: PathBuf, home: Option<&Path>) -> Self {
        Self::open(path, home)
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn for_directory(&self, directory: &Path) -> BrowseOptions {
        let configured = self
            .configured
            .locals
            .get(directory)
            .copied()
            .unwrap_or_default()
            .apply(self.session.global.browse);
        self.session
            .locals
            .get(directory)
            .copied()
            .unwrap_or_default()
            .apply(configured)
    }

    pub(super) fn update_directory(
        &mut self,
        directory: &Path,
        change: impl FnOnce(&mut BrowseOptions),
    ) {
        let before = self.for_directory(directory);
        let mut after = before;
        change(&mut after);
        if before == after {
            return;
        }

        let mut patch = self
            .session
            .locals
            .get(directory)
            .copied()
            .unwrap_or_default();
        patch.record_difference(before, after);
        let configured = self
            .configured
            .locals
            .get(directory)
            .copied()
            .unwrap_or_default()
            .apply(self.session.global.browse);
        patch.remove_values_matching(configured);
        if patch.is_empty() {
            self.session.locals.remove(directory);
        } else {
            self.session.locals.insert(directory.to_path_buf(), patch);
        }
    }

    pub(super) fn single_click_activation(&self) -> bool {
        self.session.global.single_click_activation
    }

    pub(super) fn high_contrast(&self) -> PreferenceOverride {
        self.session.global.high_contrast
    }

    pub(super) fn reduced_motion(&self) -> PreferenceOverride {
        self.session.global.reduced_motion
    }

    pub(super) fn reduced_transparency(&self) -> PreferenceOverride {
        self.session.global.reduced_transparency
    }

    pub(super) fn remember_last_directory_on_startup(&self) -> bool {
        self.session.global.startup == StartupBehavior::Last
    }

    pub(super) fn tree_visible(&self) -> bool {
        self.session.global.tree_visible
    }

    pub(super) fn toggle_tree(&mut self) -> bool {
        self.session.global.tree_visible = !self.session.global.tree_visible;
        self.session.global.tree_visible
    }

    pub(super) fn apply_command(
        &mut self,
        directory: &Path,
        local: bool,
        arguments: &str,
    ) -> Result<Applied, String> {
        let arguments = arguments.trim();
        if arguments.is_empty() || arguments == "all" {
            let mut status = self.describe(directory, local);
            if arguments == "all" {
                status.push_str(SETTING_REFERENCE);
            }
            return Ok(Applied {
                status,
                browse_changed: false,
                tree_changed: false,
            });
        }

        let before_browse = self.for_directory(directory);
        let before_tree = self.tree_visible();
        let mut global = self.session.global;
        let mut local_patch = self
            .session
            .locals
            .get(directory)
            .copied()
            .unwrap_or_default();
        for argument in arguments.split_whitespace() {
            if local {
                apply_local_option(&mut local_patch, argument)?;
            } else {
                apply_global_option(&mut global, argument, false)?;
            }
        }

        if local {
            let configured = self
                .configured
                .locals
                .get(directory)
                .copied()
                .unwrap_or_default()
                .apply(self.session.global.browse);
            local_patch.remove_values_matching(configured);
            if local_patch.is_empty() {
                self.session.locals.remove(directory);
            } else {
                self.session
                    .locals
                    .insert(directory.to_path_buf(), local_patch);
            }
        } else {
            self.session.global = global;
        }

        Ok(Applied {
            status: self.describe(directory, local),
            browse_changed: before_browse != self.for_directory(directory),
            tree_changed: before_tree != self.tree_visible(),
        })
    }

    fn describe(&self, directory: &Path, local: bool) -> String {
        let options = if local {
            self.for_directory(directory)
        } else {
            self.session.global.browse
        };
        let scope = if local {
            "session local"
        } else {
            "session global"
        };
        format!(
            "{scope}: view={} sort={} direction={} folders-first={} hidden={} click={} high-contrast={} reduced-motion={} reduced-transparency={} tree={} startup={} (config-only)  •  config={}",
            view_label(options.view),
            sort_label(options.sort),
            direction_label(options.descending),
            options.folders_first,
            options.show_hidden,
            if self.session.global.single_click_activation {
                "single"
            } else {
                "double"
            },
            override_label(self.session.global.high_contrast),
            override_label(self.session.global.reduced_motion),
            override_label(self.session.global.reduced_transparency),
            self.session.global.tree_visible,
            startup_label(self.session.global.startup),
            self.path.display(),
        )
    }
}

const SETTING_REFERENCE: &str = "\n\nview: grid or list\nsort: name, modified, size, or type\ndirection: ascending or descending\nfolders-first: keep folders before files\nhidden: show dot-prefixed entries\nclick: single or double activation (global session)\nhigh-contrast: auto, true, or false (global session)\nreduced-motion: auto, true, or false (global session)\nreduced-transparency: auto, true, or false (global session)\ntree: show the sidebar tree (global session)\nstartup: last or cwd (waddlerc only)";

fn load_config(path: &Path, home: Option<&Path>) -> Result<Config, String> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    parse_config(path, &source, home)
}

fn parse_config(path: &Path, source: &str, home: Option<&Path>) -> Result<Config, String> {
    let mut config = Config::default();
    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('"') || line.starts_with('#') {
            continue;
        }
        let words = shlex::split(line)
            .ok_or_else(|| config_error(path, line_number, "unclosed quote or escape"))?;
        if words.is_empty() {
            continue;
        }
        match words[0].as_str() {
            "set" if words.len() > 1 => {
                for argument in &words[1..] {
                    apply_global_option(&mut config.global, argument, true)
                        .map_err(|error| config_error(path, line_number, error))?;
                }
            }
            "setlocal" if words.len() > 2 => {
                let directory = expand_config_path(&words[1], home)
                    .map_err(|error| config_error(path, line_number, error))?;
                let patch = config.locals.entry(directory).or_default();
                for argument in &words[2..] {
                    apply_local_option(patch, argument)
                        .map_err(|error| config_error(path, line_number, error))?;
                }
            }
            "set" => return Err(config_error(path, line_number, "set requires an option")),
            "setlocal" => {
                return Err(config_error(
                    path,
                    line_number,
                    "setlocal requires PATH and an option",
                ));
            }
            directive => {
                return Err(config_error(
                    path,
                    line_number,
                    format!("unknown directive: {directive}"),
                ));
            }
        }
    }
    Ok(config)
}

fn config_error(path: &Path, line: usize, error: impl std::fmt::Display) -> String {
    format!("{}:{line}: {error}", path.display())
}

fn expand_config_path(value: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let path = if value == "~" {
        home.ok_or("HOME is unavailable for ~")?.to_path_buf()
    } else if let Some(relative) = value.strip_prefix("~/") {
        home.ok_or("HOME is unavailable for ~")?.join(relative)
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err(format!(
            "setlocal path must be absolute or start with ~/: {value}"
        ));
    }
    Ok(path)
}

fn apply_global_option(
    settings: &mut GlobalSettings,
    argument: &str,
    from_config: bool,
) -> Result<(), String> {
    let (name, value) = split_option(argument)?;
    match (name, value) {
        ("view", "grid") => settings.browse.view = ViewMode::Grid,
        ("view", "list") => settings.browse.view = ViewMode::List,
        ("sort", "name") => settings.browse.sort = SortKey::Name,
        ("sort", "modified") => settings.browse.sort = SortKey::Modified,
        ("sort", "size") => settings.browse.sort = SortKey::Size,
        ("sort", "type") => settings.browse.sort = SortKey::Type,
        ("direction", "ascending" | "asc") => settings.browse.descending = false,
        ("direction", "descending" | "desc") => settings.browse.descending = true,
        ("folders-first", value) => settings.browse.folders_first = parse_bool(name, value)?,
        ("hidden", value) => settings.browse.show_hidden = parse_bool(name, value)?,
        ("click", "single") => settings.single_click_activation = true,
        ("click", "double") => settings.single_click_activation = false,
        ("high-contrast", value) => settings.high_contrast = parse_override(name, value)?,
        ("reduced-motion", value) => settings.reduced_motion = parse_override(name, value)?,
        ("reduced-transparency", value) => {
            settings.reduced_transparency = parse_override(name, value)?;
        }
        ("tree", value) => settings.tree_visible = parse_bool(name, value)?,
        ("startup", "last") if from_config => settings.startup = StartupBehavior::Last,
        ("startup", "cwd") if from_config => settings.startup = StartupBehavior::Cwd,
        ("startup", _) if !from_config => {
            return Err("startup is config-only; edit waddlerc".to_owned());
        }
        ("view" | "sort" | "direction" | "click" | "startup", _) => {
            return Err(format!("invalid value for {name}: {value}"));
        }
        _ => return Err(format!("unknown setting: {name}")),
    }
    Ok(())
}

fn apply_local_option(patch: &mut BrowsePatch, argument: &str) -> Result<(), String> {
    if let Some(name) = argument.strip_suffix('&') {
        return match name {
            "view" => {
                patch.view = None;
                Ok(())
            }
            "sort" => {
                patch.sort = None;
                Ok(())
            }
            "direction" => {
                patch.descending = None;
                Ok(())
            }
            "folders-first" => {
                patch.folders_first = None;
                Ok(())
            }
            "hidden" => {
                patch.show_hidden = None;
                Ok(())
            }
            "click"
            | "high-contrast"
            | "reduced-motion"
            | "reduced-transparency"
            | "tree"
            | "startup" => Err(format!("{name} is a global setting")),
            _ => Err(format!("unknown setting: {name}")),
        };
    }
    let (name, value) = split_option(argument)?;
    match (name, value) {
        ("view", "grid") => patch.view = Some(ViewMode::Grid),
        ("view", "list") => patch.view = Some(ViewMode::List),
        ("sort", "name") => patch.sort = Some(SortKey::Name),
        ("sort", "modified") => patch.sort = Some(SortKey::Modified),
        ("sort", "size") => patch.sort = Some(SortKey::Size),
        ("sort", "type") => patch.sort = Some(SortKey::Type),
        ("direction", "ascending" | "asc") => patch.descending = Some(false),
        ("direction", "descending" | "desc") => patch.descending = Some(true),
        ("folders-first", value) => patch.folders_first = Some(parse_bool(name, value)?),
        ("hidden", value) => patch.show_hidden = Some(parse_bool(name, value)?),
        (
            "click"
            | "high-contrast"
            | "reduced-motion"
            | "reduced-transparency"
            | "tree"
            | "startup",
            _,
        ) => return Err(format!("{name} is a global setting")),
        ("view" | "sort" | "direction", _) => {
            return Err(format!("invalid value for {name}: {value}"));
        }
        _ => return Err(format!("unknown setting: {name}")),
    }
    Ok(())
}

fn split_option(argument: &str) -> Result<(&str, &str), String> {
    argument
        .split_once('=')
        .ok_or_else(|| format!("expected option=value, got: {argument}"))
}

fn parse_override(name: &str, value: &str) -> Result<PreferenceOverride, String> {
    match value {
        "auto" => Ok(PreferenceOverride::Auto),
        "true" | "on" | "yes" => Ok(PreferenceOverride::On),
        "false" | "off" | "no" => Ok(PreferenceOverride::Off),
        _ => Err(format!("invalid override for {name}: {value}")),
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        _ => Err(format!("invalid boolean for {name}: {value}")),
    }
}

fn view_label(value: ViewMode) -> &'static str {
    match value {
        ViewMode::Grid => "grid",
        ViewMode::List => "list",
    }
}

fn sort_label(value: SortKey) -> &'static str {
    match value {
        SortKey::Name => "name",
        SortKey::Modified => "modified",
        SortKey::Size => "size",
        SortKey::Type => "type",
    }
}

fn direction_label(descending: bool) -> &'static str {
    if descending {
        "descending"
    } else {
        "ascending"
    }
}

fn override_label(value: PreferenceOverride) -> &'static str {
    match value {
        PreferenceOverride::Auto => "auto",
        PreferenceOverride::On => "true",
        PreferenceOverride::Off => "false",
    }
}

fn startup_label(value: StartupBehavior) -> &'static str {
    match value {
        StartupBehavior::Last => "last",
        StartupBehavior::Cwd => "cwd",
    }
}

fn settings_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("waddle/waddlerc");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from("waddlerc"),
        |home| PathBuf::from(home).join(".config/waddle/waddlerc"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waddlerc_parses_global_local_comments_quotes_and_tilde() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let path = temp.path().join("waddlerc");
        fs::write(
            &path,
            "\" Waddle config\nset view=list tree=false startup=cwd # inline\nsetlocal \"~/My Files\" sort=size hidden=false\n",
        )
        .unwrap();

        let preferences = Preferences::open_at(path, Some(&home));
        assert!(preferences.error().is_none());
        assert!(!preferences.tree_visible());
        assert!(!preferences.remember_last_directory_on_startup());
        let options = preferences.for_directory(&home.join("My Files"));
        assert_eq!(options.view, ViewMode::List);
        assert_eq!(options.sort, SortKey::Size);
        assert!(!options.show_hidden);
    }

    #[test]
    fn invalid_config_is_atomic_and_reports_the_line() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("waddlerc");
        fs::write(&path, "set view=list\nset sort=nope\n").unwrap();

        let preferences = Preferences::open_at(path.clone(), Some(temp.path()));
        assert_eq!(
            preferences.for_directory(Path::new("/work")).view,
            ViewMode::Grid
        );
        assert!(preferences.error().is_some_and(|error| {
            error.contains(&format!("{}:2", path.display())) && error.contains("invalid value")
        }));
    }

    #[test]
    fn runtime_settings_layer_over_config_without_writing_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("waddlerc");
        let source = "set view=list folders-first=true\nsetlocal /work hidden=false\n";
        fs::write(&path, source).unwrap();
        let mut preferences = Preferences::open_at(path.clone(), Some(temp.path()));

        preferences
            .apply_command(Path::new("/work"), false, "view=grid folders-first=false")
            .unwrap();
        preferences
            .apply_command(Path::new("/work"), true, "sort=size hidden=true")
            .unwrap();
        let options = preferences.for_directory(Path::new("/work"));
        assert_eq!(options.view, ViewMode::Grid);
        assert_eq!(options.sort, SortKey::Size);
        assert!(options.show_hidden);
        assert!(!options.folders_first);

        preferences
            .apply_command(Path::new("/work"), true, "hidden&")
            .unwrap();
        assert!(!preferences.for_directory(Path::new("/work")).show_hidden);
        assert_eq!(fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn global_settings_report_the_global_layer_not_the_current_local_override() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("waddlerc");
        fs::write(&path, "set view=grid\nsetlocal /work view=list\n").unwrap();
        let mut preferences = Preferences::open_at(path, Some(temp.path()));

        let applied = preferences
            .apply_command(Path::new("/work"), false, "sort=size")
            .unwrap();

        assert!(
            applied
                .status
                .contains("session global: view=grid sort=size")
        );
        assert_eq!(
            preferences.for_directory(Path::new("/work")).view,
            ViewMode::List
        );
    }

    #[test]
    fn directory_controls_create_session_local_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::empty_at(temp.path().join("waddlerc"));
        preferences.update_directory(Path::new("/work"), |options| {
            options.view = ViewMode::List;
        });
        assert_eq!(
            preferences.for_directory(Path::new("/work")).view,
            ViewMode::List
        );
        assert_eq!(
            preferences.for_directory(Path::new("/other")).view,
            ViewMode::Grid
        );
        assert!(!temp.path().join("waddlerc").exists());
    }

    #[test]
    fn setting_commands_are_atomic_and_startup_is_config_only() {
        let temp = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::empty_at(temp.path().join("waddlerc"));
        preferences
            .apply_command(Path::new("/work"), false, "view=list sort=size")
            .unwrap();
        let before = preferences.for_directory(Path::new("/work"));
        assert!(
            preferences
                .apply_command(Path::new("/work"), false, "view=grid sort=nope")
                .is_err()
        );
        assert_eq!(preferences.for_directory(Path::new("/work")), before);
        assert!(
            preferences
                .apply_command(Path::new("/work"), false, "startup=cwd")
                .is_err()
        );
    }

    #[test]
    fn accessibility_overrides_resolve_against_system_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let mut preferences = Preferences::empty_at(temp.path().join("waddlerc"));
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
