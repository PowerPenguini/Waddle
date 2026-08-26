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
        custom: &'a str,
        error: &'a str,
    },
}

#[derive(Clone, Debug)]
struct State {
    path: PathBuf,
    target_name: String,
    applications: Vec<Application>,
    custom: String,
    error: String,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Session {
    state: Option<State>,
}

impl Session {
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

    pub(super) fn change_custom(&mut self, value: String) {
        if let Some(state) = self.state.as_mut() {
            state.custom = value;
            state.error.clear();
        }
    }

    pub(super) fn choose(&mut self, application: &str) -> Option<Request> {
        let state = self.state.as_ref()?;
        let application = state
            .applications
            .iter()
            .find(|candidate| candidate.id == application)?
            .id
            .clone();
        let state = self.state.take()?;
        Some(Request {
            path: state.path,
            application,
        })
    }

    pub(super) fn submit_custom(&mut self) -> Option<Request> {
        let state = self.state.as_mut()?;
        let application = state.custom.trim().to_owned();
        if application.is_empty() {
            state.error = "Enter an application name or desktop ID".to_owned();
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
            custom: String::new(),
            error: String::new(),
        });
    }
}

pub(super) fn applications_for(path: &Path) -> Result<Vec<Application>, String> {
    let metadata = fs::symlink_metadata(path)
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
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    let content_type = content_type(&path, metadata.is_dir());
    let requested = requested.trim();
    if requested.is_empty() {
        return Err("application name or desktop ID is required".to_owned());
    }
    let matches = |application: &gio::AppInfo| {
        app_id(application).eq_ignore_ascii_case(requested)
            || application.name().eq_ignore_ascii_case(requested)
    };
    let application = gio::AppInfo::all_for_type(&content_type)
        .into_iter()
        .find(matches)
        .or_else(|| gio::AppInfo::all().into_iter().find(matches))
        .ok_or_else(|| format!("application not found: {requested}"))?;
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
        } = session.view()
        else {
            panic!("Open With session should be visible");
        };
        assert_eq!(target_name, "document.txt");
        assert_eq!(applications.len(), 2);
        assert_eq!(custom, "");
        assert_eq!(error, "");

        session.change_custom("org.example.Custom.desktop".to_owned());
        assert_eq!(
            session.submit_custom(),
            Some(Request {
                path: PathBuf::from("/work/document.txt"),
                application: "org.example.Custom.desktop".to_owned(),
            })
        );
        assert!(!session.is_open());
    }

    #[test]
    fn known_option_uses_the_retained_target_and_empty_custom_input_stays_open() {
        let mut session = Session::default();
        session.open(
            PathBuf::from("/work/image.png"),
            vec![application("org.example.Viewer.desktop", "Viewer", false)],
        );

        assert!(session.submit_custom().is_none());
        assert!(matches!(
            session.view(),
            View::Open { error, .. } if !error.is_empty()
        ));
        assert_eq!(
            session.choose("org.example.Viewer.desktop"),
            Some(Request {
                path: PathBuf::from("/work/image.png"),
                application: "org.example.Viewer.desktop".to_owned(),
            })
        );
    }
}
