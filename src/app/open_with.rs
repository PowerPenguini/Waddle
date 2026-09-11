use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use gio::prelude::AppInfoExt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Application {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Request {
    pub(super) path: PathBuf,
    pub(super) application: String,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum View<'a> {
    Closed,
    Open {
        target_name: &'a str,
        applications: &'a [Application],
        selected: usize,
        editing: bool,
        custom: &'a str,
        error: &'a str,
    },
}

#[derive(Clone, Debug)]
struct State {
    path: PathBuf,
    target_name: String,
    applications: Vec<Application>,
    selected: usize,
    editing: bool,
    custom: String,
    error: String,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Session {
    state: Option<State>,
}

impl Session {
    #[cfg(test)]
    pub(super) fn with_applications(path: PathBuf, applications: Vec<Application>) -> Self {
        let mut session = Self::default();
        session.open(path, applications);
        session
    }

    pub(super) fn begin(&mut self, path: PathBuf) -> Result<(), String> {
        let applications = applications_for(&path)?;
        self.open(path, applications);
        Ok(())
    }

    pub(super) fn view(&self) -> View<'_> {
        self.state
            .as_ref()
            .map_or(View::Closed, |state| View::Open {
                target_name: &state.target_name,
                applications: &state.applications,
                selected: state.selected,
                editing: state.editing,
                custom: &state.custom,
                error: &state.error,
            })
    }

    pub(super) fn is_open(&self) -> bool {
        self.state.is_some()
    }

    pub(super) fn preferred_height(&self) -> Option<f32> {
        self.state.as_ref().map(|state| {
            let visible_rows = state.applications.len().clamp(1, 5) as f32;
            91.0 + visible_rows * 29.0
        })
    }

    pub(super) fn move_selection(&mut self, delta: i32) {
        if let Some(state) = self.state.as_mut().filter(|state| !state.editing) {
            state.selected = state
                .selected
                .saturating_add_signed(delta as isize)
                .min(state.applications.len());
            state.error.clear();
        }
    }

    pub(super) fn leave_custom(&mut self) -> bool {
        if let Some(state) = self.state.as_mut().filter(|state| state.editing) {
            state.editing = false;
            state.error.clear();
            true
        } else {
            false
        }
    }

    pub(super) fn change_custom(&mut self, value: String) {
        if let Some(state) = self.state.as_mut().filter(|state| state.editing) {
            state.custom = value;
            state.error.clear();
        }
    }

    pub(super) fn submit(&mut self) -> Option<Request> {
        let state = self.state.as_mut()?;
        if !state.editing && state.selected == state.applications.len() {
            state.editing = true;
            return None;
        }
        let application = if state.editing {
            state.custom.trim().to_owned()
        } else {
            state.applications.get(state.selected)?.id.clone()
        };
        if application.is_empty() {
            state.error = "Enter an application name, desktop ID, or executable path".to_owned();
            return None;
        }
        let state = self.state.take()?;
        Some(Request {
            path: state.path,
            application,
        })
    }

    pub(super) fn cancel(&mut self) -> bool {
        self.state.take().is_some()
    }

    fn open(&mut self, path: PathBuf, applications: Vec<Application>) {
        let target_name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.state = Some(State {
            path,
            target_name,
            applications,
            selected: 0,
            editing: false,
            custom: String::new(),
            error: String::new(),
        });
    }
}

pub(super) fn applications_for(path: &Path) -> Result<Vec<Application>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    Ok(applications_for_type(&content_type(
        path,
        metadata.is_dir(),
    )))
}

pub(super) fn applications_for_type(content_type: &str) -> Vec<Application> {
    let default_id = gio::AppInfo::default_for_type(content_type, false).map(|app| app_id(&app));
    let mut seen = HashSet::new();
    let mut applications = gio::AppInfo::all_for_type(content_type)
        .into_iter()
        .filter_map(|application| {
            let id = app_id(&application);
            seen.insert(id.to_lowercase()).then(|| Application {
                default: default_id
                    .as_deref()
                    .is_some_and(|default| default.eq_ignore_ascii_case(&id)),
                id,
                name: application.name().to_string(),
            })
        })
        .collect::<Vec<_>>();
    applications.sort_by(|left, right| {
        right
            .default
            .cmp(&left.default)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    applications
}

pub(super) fn launch(path: PathBuf, requested: &str, make_default: bool) -> Result<String, String> {
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    let content_type = content_type(&path, metadata.is_dir());
    let requested = requested.trim();
    if requested.is_empty() {
        return Err("application name, desktop ID, or executable path is required".to_owned());
    }
    let matches = |application: &gio::AppInfo| {
        app_id(application).eq_ignore_ascii_case(requested)
            || application.name().eq_ignore_ascii_case(requested)
    };
    let application = gio::AppInfo::all_for_type(&content_type)
        .into_iter()
        .find(matches)
        .or_else(|| gio::AppInfo::all().into_iter().find(matches));
    let Some(application) = application else {
        if make_default {
            return Err("Setting a default requires an installed application".to_owned());
        }
        let mut child = std::process::Command::new(application_path(&path, requested))
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not launch {requested}: {error}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(format!("Opened with {requested}"));
    };
    if make_default {
        application
            .set_as_default_for_type(&content_type)
            .map_err(|error| format!("Could not set the default application: {error}"))?;
        Ok(format!(
            "{} is now the default for {}",
            application.name(),
            gio::content_type_get_mime_type(&content_type).unwrap_or(content_type)
        ))
    } else {
        application
            .launch(
                &[gio::File::for_path(&path)],
                None::<&gio::AppLaunchContext>,
            )
            .map_err(|error| format!("Could not launch {}: {error}", application.name()))?;
        Ok(format!("Opened with {}", application.name()))
    }
}

fn application_path(target: &Path, requested: &str) -> PathBuf {
    if let Some(relative) = requested.strip_prefix("~/") {
        return gio::glib::home_dir().join(relative);
    }
    let path = PathBuf::from(requested);
    if path.is_relative() && requested.contains('/') {
        target.parent().unwrap_or(Path::new(".")).join(path)
    } else {
        path
    }
}

fn app_id(application: &gio::AppInfo) -> String {
    application
        .id()
        .map_or_else(|| application.name().to_string(), |id| id.to_string())
}

fn content_type(path: &Path, directory: bool) -> gio::glib::GString {
    if directory {
        return "inode/directory".into();
    }
    gio::content_type_guess(Some(path), None).0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application(id: &str, name: &str, default: bool) -> Application {
        Application {
            id: id.to_owned(),
            name: name.to_owned(),
            default,
        }
    }

    #[test]
    fn keyboard_selection_includes_custom_last_and_preserves_its_input() {
        let mut session = Session::with_applications(
            "/work/file.txt".into(),
            vec![
                application("editor.desktop", "Editor", true),
                application("viewer.desktop", "Viewer", false),
            ],
        );
        session.move_selection(-1);
        assert!(matches!(session.view(), View::Open { selected: 0, .. }));
        session.move_selection(1);
        assert_eq!(
            session.clone().submit().unwrap().application,
            "viewer.desktop"
        );
        session.move_selection(10);
        assert!(matches!(
            session.view(),
            View::Open {
                selected: 2,
                editing: false,
                ..
            }
        ));
        assert!(session.submit().is_none());
        session.change_custom("/opt/my editor".into());
        session.move_selection(-1);
        assert!(matches!(
            session.view(),
            View::Open {
                selected: 2,
                editing: true,
                ..
            }
        ));
        assert!(session.leave_custom());
        session.move_selection(-1);
        session.move_selection(1);
        assert!(session.submit().is_none());
        assert_eq!(
            session.submit().unwrap(),
            Request {
                path: "/work/file.txt".into(),
                application: "/opt/my editor".into(),
            }
        );
    }

    #[test]
    fn empty_application_list_still_offers_custom() {
        let mut session = Session::with_applications("/work/file.txt".into(), vec![]);
        session.move_selection(1);
        session.move_selection(-1);
        assert!(session.submit().is_none());
        assert!(matches!(
            session.view(),
            View::Open {
                selected: 0,
                editing: true,
                ..
            }
        ));
    }

    #[test]
    fn executable_path_with_spaces_receives_target_as_one_literal_argument() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("my editor");
        let target = directory.path().join("file ; $HOME.txt");
        fs::write(&target, "example").unwrap();
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s' \"$1\" > \"$0.received\"\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        launch(target.clone(), "./my editor", false).unwrap();
        let received = directory.path().join("my editor.received");
        for _ in 0..100 {
            if fs::read_to_string(&received).ok().as_deref() == target.to_str() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("executable did not receive the literal target path");
    }

    #[test]
    fn application_paths_expand_home_and_resolve_relative_to_target_folder() {
        assert_eq!(
            application_path(Path::new("/work/file.txt"), "~/bin/editor"),
            gio::glib::home_dir().join("bin/editor")
        );
        assert_eq!(
            application_path(Path::new("/work/file.txt"), "./editor"),
            PathBuf::from("/work/./editor")
        );
        assert_eq!(
            application_path(Path::new("/work/file.txt"), "editor"),
            PathBuf::from("editor")
        );
        assert_eq!(
            application_path(Path::new("/work/file.txt"), "/opt/editor"),
            PathBuf::from("/opt/editor")
        );
    }

    #[test]
    fn session_exposes_options_and_keeps_a_manual_application_input() {
        let mut session = Session::default();
        session.open(
            PathBuf::from("/work/document.txt"),
            vec![
                application("org.example.Editor.desktop", "Editor", true),
                application("org.example.Viewer.desktop", "Viewer", false),
            ],
        );

        let View::Open {
            target_name,
            applications,
            custom,
            error,
            ..
        } = session.view()
        else {
            panic!("Open With session should be visible");
        };
        assert_eq!(target_name, "document.txt");
        assert_eq!(applications.len(), 2);
        assert_eq!(custom, "");
        assert_eq!(error, "");

        session.move_selection(2);
        assert!(session.submit().is_none());
        session.change_custom("org.example.Custom.desktop".to_owned());
        assert_eq!(
            session.submit(),
            Some(Request {
                path: PathBuf::from("/work/document.txt"),
                application: "org.example.Custom.desktop".to_owned(),
            })
        );
        assert!(!session.is_open());
    }

    #[test]
    fn typed_option_uses_the_retained_target_and_empty_input_stays_open() {
        let mut session = Session::default();
        session.open(
            PathBuf::from("/work/image.png"),
            vec![application("org.example.Viewer.desktop", "Viewer", false)],
        );

        session.move_selection(1);
        assert!(session.submit().is_none());
        assert!(session.submit().is_none());
        assert!(matches!(
            session.view(),
            View::Open { error, .. } if !error.is_empty()
        ));
        session.change_custom("  Viewer  ".to_owned());
        assert_eq!(
            session.submit(),
            Some(Request {
                path: PathBuf::from("/work/image.png"),
                application: "Viewer".to_owned(),
            })
        );
    }
}
