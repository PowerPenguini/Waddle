use std::{fs, path::PathBuf};

use iced::{Point, Size, window};
use serde::{Deserialize, Serialize};

use crate::launch;

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
    requested: Option<launch::Target>,
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
                .join(format!("waddle-startup-test-{}.json", std::process::id())),
            stored: Stored::default(),
            requested: None,
            error: None,
        }
    }

    pub(super) fn initial_directory(&self, remember_last: bool) -> PathBuf {
        self.requested
            .as_ref()
            .map(|target| target.directory.clone())
            .or_else(|| {
                remember_last
                    .then(|| self.stored.last_directory.clone())
                    .flatten()
            })
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
    }

    pub(super) fn initial_selection(&self) -> Vec<PathBuf> {
        self.requested
            .as_ref()
            .map(|target| target.selected.clone())
            .unwrap_or_default()
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
            platform_specific: window::settings::PlatformSpecific {
                application_id: "io.github.powerpenguini.Waddle".to_owned(),
                ..window::settings::PlatformSpecific::default()
            },
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

#[cfg(test)]
fn resolve_location(argument: &str) -> Result<launch::Target, String> {
    launch::location(argument.as_ref())
}

#[cfg(not(test))]
fn requested_location() -> (Option<launch::Target>, Option<String>) {
    let mut arguments = std::env::args_os().skip(1);
    let Some(argument) = arguments.next() else {
        return (None, None);
    };
    let result = if argument == "--show-items" {
        launch::show_items(arguments).and_then(|mut targets| {
            if targets.len() == 1 {
                Ok(targets.remove(0))
            } else {
                Err("one Waddle window can reveal items from only one folder".to_owned())
            }
        })
    } else {
        launch::location(&argument)
    };
    match result {
        Ok(path) => (Some(path), None),
        Err(error) => (None, Some(error)),
    }
}

#[cfg(not(test))]
fn state_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("waddle/startup.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".waddle-startup.json"),
        |home| PathBuf::from(home).join(".local/state/waddle/startup.json"),
    )
}

#[cfg(test)]
mod tests {
    use gio::prelude::FileExt;

    use super::*;

    #[test]
    fn local_paths_and_file_uris_preserve_the_file_to_reveal() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.txt");
        fs::write(&file, "x").unwrap();
        let path_target = resolve_location(file.to_str().unwrap()).unwrap();
        let uri_target = resolve_location(&gio::File::for_path(&file).uri()).unwrap();

        assert_eq!(path_target.directory, temp.path());
        assert_eq!(path_target.selected.as_slice(), std::slice::from_ref(&file));
        assert_eq!(uri_target.directory, temp.path());
        assert_eq!(uri_target.selected, [file]);
    }

    #[test]
    fn directory_locations_do_not_request_a_selection() {
        let temp = tempfile::tempdir().unwrap();
        let target = resolve_location(temp.path().to_str().unwrap()).unwrap();

        assert_eq!(target.directory, temp.path());
        assert!(target.selected.is_empty());
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
        assert_eq!(reopened.initial_directory(true), temp.path());
        assert_eq!(
            reopened.initial_directory(false),
            std::env::current_dir().unwrap()
        );
        assert_eq!(reopened.window_settings().size, Size::new(900.0, 700.0));
    }

    #[test]
    fn window_application_id_matches_the_desktop_file() {
        assert_eq!(
            State::open_default()
                .window_settings()
                .platform_specific
                .application_id,
            "io.github.powerpenguini.Waddle"
        );
    }

    #[test]
    fn explicit_location_overrides_startup_behavior() {
        let temp = tempfile::tempdir().unwrap();
        let state = State {
            path: temp.path().join("startup.json"),
            stored: Stored {
                last_directory: Some(PathBuf::from("/")),
                ..Stored::default()
            },
            requested: Some(launch::Target {
                directory: temp.path().to_path_buf(),
                selected: Vec::new(),
            }),
            error: None,
        };

        assert_eq!(state.initial_directory(true), temp.path());
        assert_eq!(state.initial_directory(false), temp.path());
    }
}
