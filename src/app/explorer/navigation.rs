use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use slint::ComponentHandle;

use crate::{AppWindow, fs};

use super::{Explorer, to_ui_view_mode};
use crate::app::{
    executor::TaskExecutor,
    settings,
    state::{ExplorerState, NavigationKind, PendingNavigation, ViewMode},
    tree::sync_tree,
    view::{
        clear_preview, show_error_window, sync_files, sync_navigation, sync_selection, sync_status,
    },
};

pub(super) trait DirectoryLoader: Send + Sync {
    fn open(&self, path: &Path) -> Result<fs::OpenedDirectory, fs::FsError>;
}

pub(super) struct FsDirectoryLoader;

impl DirectoryLoader for FsDirectoryLoader {
    fn open(&self, path: &Path) -> Result<fs::OpenedDirectory, fs::FsError> {
        fs::open_directory(path)
    }
}

#[derive(Clone)]
pub(super) struct NavigationContext {
    navigation_tasks: TaskExecutor,
    background_tasks: TaskExecutor,
    state: Arc<Mutex<ExplorerState>>,
    ui: slint::Weak<AppWindow>,
    loader: Arc<dyn DirectoryLoader>,
    in_flight: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Explorer {
    pub(super) fn navigation_context(&self) -> NavigationContext {
        NavigationContext {
            navigation_tasks: self.navigation_tasks.clone(),
            background_tasks: self.background_tasks.clone(),
            state: self.state.clone(),
            ui: self.ui.clone(),
            loader: self.directory_loader.clone(),
            in_flight: self.in_flight_navigation.clone(),
        }
    }

    pub(super) fn submit_location(&self, input: &str) {
        let input = PathBuf::from(input);
        let target = if input.is_absolute() {
            input
        } else {
            self.state.lock().unwrap().current.join(input)
        };
        self.navigate(target, true);
    }

    pub(super) fn navigate(&self, target: PathBuf, remember: bool) {
        self.navigate_select(target, remember, None);
    }

    pub(super) fn navigate_select(&self, target: PathBuf, remember: bool, select: Option<PathBuf>) {
        if !self.navigation_allowed() {
            return;
        }
        self.cancel_search_for_navigation();
        self.navigation_context().request(PendingNavigation {
            requested: target,
            kind: NavigationKind::Forward { remember },
            select,
        });
    }

    pub(super) fn set_view_mode(self: &std::rc::Rc<Self>, mode: ViewMode) {
        if !self.navigation_allowed() {
            return;
        }
        let changed = {
            let mut state = self.state.lock().unwrap();
            if state.view_mode == mode {
                false
            } else {
                state.view_mode = mode;
                state.begin_preview();
                if mode == ViewMode::Ranger
                    && state.selected_entry.is_none()
                    && !state.entries.is_empty()
                {
                    state.selected_entry = Some(0);
                }
                true
            }
        };
        if !changed {
            return;
        }

        if let Some(ui) = self.ui.upgrade() {
            let state = self.state.lock().unwrap();
            ui.set_view_mode(to_ui_view_mode(mode));
            sync_selection(&ui, &state);
            drop(state);
            if mode == ViewMode::Grid {
                clear_preview(&ui);
            }
        }
        let operations = self.operation_tasks.clone();
        operations.execute(move || {
            let _ = settings::save_view_mode(mode);
        });
        if mode == ViewMode::Ranger {
            Self::load_ranger_parent(
                self.background_tasks.clone(),
                self.state.clone(),
                self.ui.clone(),
            );
            self.schedule_preview();
        }
    }

    pub(super) fn go_back(&self) {
        if !self.navigation_allowed() {
            return;
        }
        self.cancel_search_for_navigation();
        let cancelled = {
            let mut state = self.state.lock().unwrap();
            state.cancel_navigation()
        };
        if cancelled {
            if let Some(ui) = self.ui.upgrade() {
                let state = self.state.lock().unwrap();
                ui.set_navigation_loading(false);
                sync_navigation(&ui, &state);
                sync_status(&ui, &state);
            }
            return;
        }

        let target = self.state.lock().unwrap().history.last().cloned();
        let Some(target) = target else {
            return;
        };
        self.navigation_context().request(PendingNavigation {
            requested: target.clone(),
            kind: NavigationKind::Back { expected: target },
            select: None,
        });
    }

    pub(super) fn go_forward(&self) {
        if !self.navigation_allowed() {
            return;
        }
        self.cancel_search_for_navigation();
        let target = self.state.lock().unwrap().forward_history.last().cloned();
        let Some(target) = target else {
            return;
        };
        self.navigation_context().request(PendingNavigation {
            requested: target.clone(),
            kind: NavigationKind::HistoryForward { expected: target },
            select: None,
        });
    }

    pub(super) fn go_parent(&self) {
        if !self.navigation_allowed() {
            return;
        }
        let target = {
            let state = self.state.lock().unwrap();
            let current = state
                .pending_navigation
                .as_ref()
                .map(|navigation| &navigation.requested)
                .unwrap_or(&state.current);
            current
                .parent()
                .map(|parent| (parent.to_path_buf(), current.clone()))
        };
        let Some((target, current)) = target else {
            return;
        };
        self.navigate_select(target, true, Some(current));
    }

    pub(super) fn refresh(&self, select: Option<PathBuf>) {
        self.navigation_context().refresh(select, false);
    }
}

impl NavigationContext {
    pub(super) fn refresh(&self, select: Option<PathBuf>, keep_operation_busy: bool) {
        let current = self.state.lock().unwrap().current.clone();
        self.request(PendingNavigation {
            requested: current,
            kind: NavigationKind::Refresh {
                keep_operation_busy,
            },
            select,
        });
    }

    pub(super) fn request(&self, navigation: PendingNavigation) {
        let requested = navigation.requested.clone();
        {
            let mut state = self.state.lock().unwrap();
            if state.pending_navigation.as_ref() == Some(&navigation) {
                return;
            }
            state.begin_navigation(navigation);
            if let Some(window) = self.ui.upgrade() {
                window.set_navigation_loading(true);
                sync_navigation(&window, &state);
                window.set_status_text(format!("Opening {}…", requested.display()).into());
            }
        }

        let should_queue = self.in_flight.lock().unwrap().insert(requested.clone());
        if !should_queue {
            return;
        }

        let context = self.clone();
        let worker_path = requested.clone();
        if !self.navigation_tasks.execute(move || {
            let result = context.loader.open(&worker_path);
            let callback_context = context.clone();
            let _ = slint::invoke_from_event_loop(move || {
                callback_context.finish(worker_path, result);
            });
        }) {
            self.in_flight.lock().unwrap().remove(&requested);
            let mut state = self.state.lock().unwrap();
            if state.take_navigation_for(&requested).is_some()
                && let Some(window) = self.ui.upgrade()
            {
                window.set_navigation_loading(false);
                sync_navigation(&window, &state);
                window.set_status_text("Could not queue the navigation request".into());
            }
        }
    }

    fn finish(&self, requested: PathBuf, result: Result<fs::OpenedDirectory, fs::FsError>) {
        self.in_flight.lock().unwrap().remove(&requested);
        let Some(window) = self.ui.upgrade() else {
            return;
        };
        let mut state = self.state.lock().unwrap();

        let tree_changed = match &result {
            Ok(opened) => {
                Explorer::cache_tree_navigation_success(&mut state, &requested, &opened.entries)
            }
            Err(_) => Explorer::cache_tree_navigation_failure(&mut state, &requested),
        };

        let Some(pending) = state.take_navigation_for(&requested) else {
            if tree_changed {
                sync_tree(&window, &mut state);
            }
            return;
        };
        window.set_navigation_loading(false);

        match result {
            Ok(opened) => {
                let keep_operation_busy = matches!(
                    &pending.kind,
                    NavigationKind::Refresh {
                        keep_operation_busy: true
                    }
                );
                if !state.commit_navigation(pending, opened.canonical_path, opened.entries) {
                    sync_navigation(&window, &state);
                    sync_status(&window, &state);
                    if tree_changed {
                        sync_tree(&window, &mut state);
                    }
                    return;
                }
                sync_navigation(&window, &state);
                sync_files(&window, &state);
                sync_tree(&window, &mut state);
                let ranger = state.view_mode == ViewMode::Ranger;
                drop(state);
                if keep_operation_busy {
                    window.set_busy(false);
                }
                Explorer::queue_current_details(
                    self.background_tasks.clone(),
                    self.state.clone(),
                    window.as_weak(),
                );
                if ranger {
                    Explorer::load_ranger_parent(
                        self.background_tasks.clone(),
                        self.state.clone(),
                        window.as_weak(),
                    );
                    Explorer::queue_current_preview(
                        self.background_tasks.clone(),
                        self.state.clone(),
                        window.as_weak(),
                    );
                }
            }
            Err(error) => {
                let keep_operation_busy = matches!(
                    &pending.kind,
                    NavigationKind::Refresh {
                        keep_operation_busy: true
                    }
                );
                sync_navigation(&window, &state);
                if tree_changed {
                    sync_tree(&window, &mut state);
                }
                drop(state);
                if keep_operation_busy {
                    window.set_busy(false);
                    show_error_window(&window, error.to_string());
                } else {
                    window.set_status_text(error.to_string().into());
                }
            }
        }
    }
}
