use std::{fs, path::PathBuf};

use gio::prelude::FileExt;
use iced::{Point, Size, window};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct Stored {
    last_directory: Option<PathBuf>,
    width: f32,
    height: f32,
    position: Option<(f32, f32)>,
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            last_directory: None,
            width: 820.0,
            height: 560.0,
            position: None,
        }
    }
}

pub(super) struct State {
    path: PathBuf,
    stored: Stored,
    requested: Option<PathBuf>,
    error: Option<String>,
}

impl State {
    #[cfg(not(test))]
    pub(super) fn open_default() -> Self {
        let path = state_path();
        let stored = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let (requested, error) = requested_location();
        Self {
            path,
            stored,
            requested,
            error,
        }
    }

    #[cfg(test)]
    pub(super) fn open_default() -> Self {
        Self {
            path: std::env::temp_dir()
                .join(format!("polarexp-startup-test-{}.json", std::process::id())),
            stored: Stored::default(),
            requested: None,
            error: None,
        }
    }

    pub(super) fn initial_directory(&self) -> PathBuf {
        self.requested
            .clone()
            .or_else(|| self.stored.last_directory.clone())
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn window_settings(&self) -> window::Settings {
        window::Settings {
            size: Size::new(self.stored.width.max(660.0), self.stored.height.max(420.0)),
            position: self
                .stored
                .position
                .map_or(window::Position::Default, |(x, y)| {
                    window::Position::Specific(Point::new(x, y))
                }),
            min_size: Some(Size::new(660.0, 420.0)),
            transparent: true,
            blur: true,
            exit_on_close_request: false,
            ..window::Settings::default()
        }
    }

    pub(super) fn remember_directory(&mut self, path: PathBuf) {
        self.stored.last_directory = Some(path);
        let _ = self.save();
    }

    pub(super) fn remember_size(&mut self, size: Size) {
        self.stored.width = size.width;
        self.stored.height = size.height;
        let _ = self.save();
    }

    pub(super) fn remember_position(&mut self, position: Point) {
        self.stored.position = Some((position.x, position.y));
        let _ = self.save();
    }

    fn save(&self) -> Result<(), String> {
        let directory = self
            .path
            .parent()
            .ok_or("startup state path has no parent")?;
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

fn resolve_location(argument: &str) -> Result<PathBuf, String> {
    let path = if argument.starts_with("file://") {
        gio::File::for_uri(argument)
            .path()
            .ok_or_else(|| "the file URI is not a local path".to_owned())?
    } else {
        let path = PathBuf::from(argument);
        if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(path)
        }
    };
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        canonical
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| format!("{} has no parent directory", canonical.display()))
    }
}

#[cfg(not(test))]
fn requested_location() -> (Option<PathBuf>, Option<String>) {
    let Some(argument) = std::env::args_os().nth(1) else {
        return (None, None);
    };
    let argument = argument.to_string_lossy();
    match resolve_location(&argument) {
        Ok(path) => (Some(path), None),
        Err(error) => (None, Some(error)),
    }
}

#[cfg(not(test))]
fn state_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("polarexp/startup.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".polarexp-startup.json"),
        |home| PathBuf::from(home).join(".local/state/polarexp/startup.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_and_file_uris_resolve_files_to_their_parent() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.txt");
        fs::write(&file, "x").unwrap();
        assert_eq!(
            resolve_location(file.to_str().unwrap()).unwrap(),
            temp.path()
        );
        assert_eq!(
            resolve_location(&gio::File::for_path(&file).uri()).unwrap(),
            temp.path()
        );
    }

    #[test]
    fn geometry_and_last_directory_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("startup.json");
        let mut state = State {
            path: path.clone(),
            stored: Stored::default(),
            requested: None,
            error: None,
        };
        state.remember_directory(temp.path().to_path_buf());
        state.remember_size(Size::new(900.0, 700.0));
        state.remember_position(Point::new(12.0, 24.0));

        let stored = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let reopened = State {
            path: temp.path().join("startup.json"),
            stored,
            requested: None,
            error: None,
        };
        assert_eq!(reopened.initial_directory(), temp.path());
        assert_eq!(reopened.window_settings().size, Size::new(900.0, 700.0));
    }
}
