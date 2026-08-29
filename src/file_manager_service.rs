use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use crate::launch::{self, Target};

pub(crate) const NAME: &str = "org.freedesktop.FileManager1";
pub(crate) const PATH: &str = "/org/freedesktop/FileManager1";

pub(crate) fn requested() -> bool {
    std::env::args_os().nth(1).as_deref() == Some("--file-manager-service".as_ref())
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let launcher = ProcessLauncher::new(std::env::current_exe()?);
    let connection = zbus::blocking::connection::Builder::session()?
        .name(NAME)?
        .serve_at(PATH, FileManager::new(launcher))?
        .build()?;
    connection.closed();
    Ok(())
}

trait Launcher: Send + Sync + 'static {
    fn launch(&self, target: &Target, startup_id: &str) -> Result<(), String>;
}

struct ProcessLauncher {
    executable: PathBuf,
}

impl ProcessLauncher {
    fn new(executable: PathBuf) -> Self {
        Self { executable }
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self, target: &Target, startup_id: &str) -> Result<(), String> {
        let mut command = Command::new(&self.executable);
        if target.selected.is_empty() {
            command.arg(&target.directory);
        } else {
            command.arg("--show-items").args(&target.selected);
        }
        if !startup_id.is_empty() {
            command
                .env("DESKTOP_STARTUP_ID", startup_id)
                .env("XDG_ACTIVATION_TOKEN", startup_id);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "could not launch {}: {error}",
                    display_executable(&self.executable)
                )
            })
    }
}

fn display_executable(path: &Path) -> String {
    path.display().to_string()
}

struct FileManager {
    launcher: Arc<dyn Launcher>,
}

impl FileManager {
    fn new(launcher: impl Launcher) -> Self {
        Self {
            launcher: Arc::new(launcher),
        }
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager {
    fn show_items(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        let targets = launch::show_items(uris.iter())
            .map_err(|error| zbus::fdo::Error::InvalidArgs(error.to_string()))?;
        for target in targets {
            self.launcher
                .launch(&target, &startup_id)
                .map_err(zbus::fdo::Error::Failed)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use gio::prelude::FileExt;

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingLauncher {
        launched: Arc<Mutex<Vec<(Target, String)>>>,
    }

    impl Launcher for RecordingLauncher {
        fn launch(&self, target: &Target, startup_id: &str) -> Result<(), String> {
            self.launched
                .lock()
                .unwrap()
                .push((target.clone(), startup_id.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn show_items_launches_one_window_per_parent_with_the_requested_selection() {
        let temp = tempfile::tempdir().unwrap();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        std::fs::create_dir_all(&first_dir).unwrap();
        std::fs::create_dir_all(&second_dir).unwrap();
        let first = first_dir.join("first file");
        let second = second_dir.join("drugi");
        std::fs::write(&first, "x").unwrap();
        std::fs::write(&second, "x").unwrap();
        let recorder = RecordingLauncher::default();
        let launched = recorder.launched.clone();
        let service = FileManager::new(recorder);

        service
            .show_items(
                vec![
                    gio::File::for_path(&first).uri().to_string(),
                    gio::File::for_path(&second).uri().to_string(),
                ],
                "activation-token".to_owned(),
            )
            .unwrap();

        assert_eq!(
            *launched.lock().unwrap(),
            vec![
                (
                    Target {
                        directory: first_dir,
                        selected: vec![first],
                    },
                    "activation-token".to_owned(),
                ),
                (
                    Target {
                        directory: second_dir,
                        selected: vec![second],
                    },
                    "activation-token".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn show_items_rejects_the_whole_call_before_launching_anything() {
        let temp = tempfile::tempdir().unwrap();
        let valid = temp.path().join("valid");
        std::fs::write(&valid, "x").unwrap();
        let recorder = RecordingLauncher::default();
        let launched = recorder.launched.clone();
        let service = FileManager::new(recorder);

        let result = service.show_items(
            vec![
                gio::File::for_path(valid).uri().to_string(),
                "file://remote/tmp/missing".to_owned(),
            ],
            String::new(),
        );

        assert!(matches!(result, Err(zbus::fdo::Error::InvalidArgs(_))));
        assert!(launched.lock().unwrap().is_empty());
    }
}
