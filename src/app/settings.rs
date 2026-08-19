use std::{fs, io, path::PathBuf};

use super::state::ViewMode;

fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("polarexp/settings.conf"))
}

pub(super) fn load_view_mode() -> ViewMode {
    let Some(path) = path() else {
        return ViewMode::Grid;
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return ViewMode::Grid;
    };
    contents
        .lines()
        .find_map(|line| line.strip_prefix("view-mode="))
        .and_then(|value| match value.trim() {
            "grid" => Some(ViewMode::Grid),
            "ranger" => Some(ViewMode::Ranger),
            _ => None,
        })
        .unwrap_or(ViewMode::Grid)
}

pub(super) fn save_view_mode(mode: ViewMode) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::other("no configuration directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let value = match mode {
        ViewMode::Grid => "grid",
        ViewMode::Ranger => "ranger",
    };
    fs::write(path, format!("view-mode={value}\n"))
}

#[cfg(test)]
pub(super) fn parse_view_mode(contents: &str) -> ViewMode {
    contents
        .lines()
        .find_map(|line| line.strip_prefix("view-mode="))
        .and_then(|value| match value.trim() {
            "grid" => Some(ViewMode::Grid),
            "ranger" => Some(ViewMode::Ranger),
            _ => None,
        })
        .unwrap_or(ViewMode::Grid)
}
