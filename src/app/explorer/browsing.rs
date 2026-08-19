use std::{
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gio::prelude::*;
use slint::{SharedString, TimerMode};

use crate::{AppWindow, fs};

use super::{DETAILS_DEBOUNCE, Explorer, PREVIEW_DEBOUNCE};
use crate::app::{
    executor::TaskExecutor,
    state::{ExplorerState, ViewMode},
    view::{
        clear_preview, show_error_window, show_preview_error, sync_preview, sync_ranger_parent,
        sync_selection, sync_status,
    },
};
use crate::fs::FileEntry;

impl Explorer {
    pub(super) fn entry_clicked(self: &Rc<Self>, index: i32) {
        if !self.navigation_allowed() {
            return;
        }
        let entry = self.entry_at(index);
        let Some(entry) = entry else {
            return;
        };
        if entry.is_directory() {
            self.navigate(entry.path, true);
        } else {
            self.select_entry(index);
        }
    }

    pub(super) fn entry_double_clicked(&self, index: i32) {
        if !self.navigation_allowed() {
            return;
        }
        let Some(entry) = self.entry_at(index) else {
            return;
        };
        if entry.is_directory() {
            return;
        }

        self.open_entry(entry);
    }

    pub(super) fn open_entry(&self, entry: FileEntry) {
        let uri = gio::File::for_path(&entry.path).uri();
        let path = entry.path;
        let ui = self.ui.clone();
        self.background_tasks.execute(move || {
            let result = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
            if let Err(error) = result {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = ui.upgrade() {
                        show_error_window(
                            &window,
                            format!("Could not open {}: {error}", path.display()),
                        );
                    }
                });
            }
        });
    }

    pub(super) fn move_selection(self: &Rc<Self>, horizontal: i32, vertical: i32, columns: i32) {
        let (_selected, changed) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.selected_entry;
            let selected = state.move_selection(horizontal, vertical, columns);
            (selected, previous != selected)
        };
        if let Some(ui) = self.ui.upgrade() {
            let state = self.state.lock().unwrap();
            sync_selection(&ui, &state);
        }
        if changed {
            self.schedule_selected_details();
        }
    }

    pub(super) fn move_ranger_selection(self: &Rc<Self>, delta: i32) {
        let (_selected, changed) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.selected_entry;
            let selected = state.move_ranger_selection(delta);
            (selected, previous != selected)
        };
        if let Some(ui) = self.ui.upgrade() {
            let state = self.state.lock().unwrap();
            sync_selection(&ui, &state);
        }
        if changed {
            self.schedule_selected_details();
        }
        self.schedule_preview();
    }

    pub(super) fn activate_selected_entry(self: &Rc<Self>) {
        let selected = self.state.lock().unwrap().selected_entry;
        if let Some(index) = selected {
            self.activate_entry(index as i32);
        }
    }

    pub(super) fn activate_entry(self: &Rc<Self>, index: i32) {
        if !self.navigation_allowed() {
            return;
        }
        let Some(entry) = self.entry_at(index) else {
            return;
        };
        if entry.is_directory() {
            self.navigate(entry.path, true);
        } else {
            self.select_entry(index);
            self.entry_double_clicked(index);
        }
    }

    pub(super) fn select_entry(self: &Rc<Self>, index: i32) {
        let mut state = self.state.lock().unwrap();
        let selected = usize::try_from(index)
            .ok()
            .filter(|index| *index < state.entries.len());
        let changed = state.selected_entry != selected;
        state.select_only(selected);
        let ranger = state.view_mode == ViewMode::Ranger;
        if let Some(ui) = self.ui.upgrade() {
            sync_selection(&ui, &state);
        }
        drop(state);
        if changed {
            self.schedule_selected_details();
        }
        if ranger {
            self.schedule_preview();
        }
    }

    pub(super) fn toggle_visual_selection(self: &Rc<Self>) {
        if !self.navigation_allowed() {
            return;
        }
        {
            let mut state = self.state.lock().unwrap();
            state.toggle_visual_selection();
            if let Some(ui) = self.ui.upgrade() {
                sync_selection(&ui, &state);
            }
        }
        self.schedule_selected_details();
    }

    pub(super) fn cancel_visual_selection(self: &Rc<Self>) {
        {
            let mut state = self.state.lock().unwrap();
            state.cancel_visual_selection();
            if let Some(ui) = self.ui.upgrade() {
                sync_selection(&ui, &state);
            }
        }
        self.schedule_selected_details();
    }

    pub(super) fn update_drag_selection(
        &self,
        start_row: i32,
        start_column: i32,
        end_row: i32,
        end_column: i32,
        columns: i32,
    ) {
        if !self.navigation_allowed() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state.select_rectangle(start_row, start_column, end_row, end_column, columns);
        if let Some(ui) = self.ui.upgrade() {
            sync_selection(&ui, &state);
        }
    }

    pub(super) fn finish_drag_selection(self: &Rc<Self>) {
        self.schedule_selected_details();
    }

    pub(super) fn cancel_delete_operator(&self) {
        if let Some(ui) = self.ui.upgrade() {
            let state = self.state.lock().unwrap();
            sync_status(&ui, &state);
        }
    }

    pub(super) fn apply_delete_operator(&self, motion: &str, columns: i32) {
        if !self.mutations_allowed() {
            return;
        }
        let selected = {
            let mut state = self.state.lock().unwrap();
            let selected = state.select_delete_motion(motion, columns);
            if selected && let Some(ui) = self.ui.upgrade() {
                sync_selection(&ui, &state);
            }
            selected
        };
        if selected {
            self.show_selected_trash_dialog();
        } else {
            self.cancel_delete_operator();
        }
    }

    pub(super) fn schedule_selected_details(self: &Rc<Self>) {
        let state = self.state.clone();
        let ui = self.ui.clone();
        let request = {
            let mut state = state.lock().unwrap();
            let entry = state
                .selected_entry
                .and_then(|index| state.entries.get(index))
                .cloned();
            let generation = state.begin_details();
            if let Some(window) = ui.upgrade() {
                sync_status(&window, &state);
            }
            entry.map(|entry| (generation, entry))
        };
        let Some((generation, entry)) = request else {
            return;
        };

        let tasks = self.background_tasks.clone();
        self.details_timer
            .start(TimerMode::SingleShot, DETAILS_DEBOUNCE, move || {
                Self::queue_details(
                    tasks.clone(),
                    state.clone(),
                    ui.clone(),
                    generation,
                    entry.clone(),
                );
            });
    }

    pub(super) fn queue_current_details(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
    ) {
        let request = {
            let mut state = state.lock().unwrap();
            let entry = state
                .selected_entry
                .and_then(|index| state.entries.get(index))
                .cloned();
            let generation = state.begin_details();
            if let Some(window) = ui.upgrade() {
                sync_status(&window, &state);
            }
            entry.map(|entry| (generation, entry))
        };
        if let Some((generation, entry)) = request {
            Self::queue_details(tasks, state, ui, generation, entry);
        }
    }

    fn queue_details(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
        generation: u64,
        entry: FileEntry,
    ) {
        tasks.execute(move || {
            let path = entry.path.clone();
            if !state.lock().unwrap().accepts_details(generation, &path) {
                return;
            }
            let details =
                fs::read_entry_details(&path).unwrap_or_else(|_| "Details unavailable".to_owned());
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                let mut state = state.lock().unwrap();
                if !state.accepts_details(generation, &path) {
                    return;
                }
                state.selected_details = Some(details);
                sync_status(&window, &state);
            });
        });
    }

    pub(super) fn ranger_go_parent(&self) {
        let current = self.state.lock().unwrap().current.clone();
        let Some(parent) = current.parent().map(Path::to_path_buf) else {
            return;
        };
        self.navigate_select(parent, true, Some(current));
    }

    pub(super) fn ranger_parent_activated(&self, index: i32) {
        if !self.navigation_allowed() {
            return;
        }
        let Some(entry) = self.ranger_parent_entry_at(index) else {
            return;
        };
        if entry.is_directory() {
            self.navigate(entry.path, true);
        }
    }

    pub(super) fn entry_at(&self, index: i32) -> Option<FileEntry> {
        let index = usize::try_from(index).ok()?;
        self.state.lock().unwrap().entries.get(index).cloned()
    }

    pub(super) fn ranger_parent_entry_at(&self, index: i32) -> Option<FileEntry> {
        let index = usize::try_from(index).ok()?;
        self.state
            .lock()
            .unwrap()
            .parent_entries
            .get(index)
            .cloned()
    }

    pub(super) fn load_ranger_parent(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
    ) {
        let (current, parent) = {
            let state = state.lock().unwrap();
            (
                state.current.clone(),
                state.current.parent().map(Path::to_path_buf),
            )
        };
        let Some(parent) = parent else {
            let mut state = state.lock().unwrap();
            state.parent_entries.clear();
            state.selected_parent_entry = None;
            if let Some(window) = ui.upgrade() {
                sync_ranger_parent(&window, &state);
            }
            return;
        };

        tasks.execute(move || {
            let entries = fs::read_directory(&parent);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                let mut state = state.lock().unwrap();
                if state.current != current || state.view_mode != ViewMode::Ranger {
                    return;
                }
                match entries {
                    Ok(entries) => {
                        state.selected_parent_entry =
                            entries.iter().position(|entry| entry.path == current);
                        state.parent_entries = entries;
                    }
                    Err(_) => {
                        state.parent_entries.clear();
                        state.selected_parent_entry = None;
                    }
                }
                sync_ranger_parent(&window, &state);
            });
        });
    }

    pub(super) fn schedule_preview(self: &Rc<Self>) {
        let state = self.state.clone();
        let ui = self.ui.clone();
        let request = {
            let mut state = state.lock().unwrap();
            if state.view_mode != ViewMode::Ranger {
                return;
            }
            let entry = state
                .selected_entry
                .and_then(|index| state.entries.get(index))
                .cloned();
            let generation = state.begin_preview();
            entry.map(|entry| (generation, entry))
        };

        let Some((generation, entry)) = request else {
            if let Some(window) = ui.upgrade() {
                clear_preview(&window);
            }
            return;
        };
        if let Some(window) = ui.upgrade() {
            window.set_preview_title(fs::display_name(&entry.name).into());
            window.set_preview_metadata(SharedString::default());
            window.set_preview_text("Loading preview…".into());
            window.set_preview_kind(crate::PreviewKind::Empty);
            window.set_ranger_preview_files(slint::ModelRc::new(slint::VecModel::default()));
        }

        let tasks = self.background_tasks.clone();
        self.preview_timer
            .start(TimerMode::SingleShot, PREVIEW_DEBOUNCE, move || {
                Self::queue_preview(
                    tasks.clone(),
                    state.clone(),
                    ui.clone(),
                    generation,
                    entry.clone(),
                );
            });
    }

    pub(super) fn queue_current_preview(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
    ) {
        let request = {
            let mut state = state.lock().unwrap();
            let entry = state
                .selected_entry
                .and_then(|index| state.entries.get(index))
                .cloned();
            let generation = state.begin_preview();
            entry.map(|entry| (generation, entry))
        };
        let Some((generation, entry)) = request else {
            if let Some(window) = ui.upgrade() {
                clear_preview(&window);
            }
            return;
        };
        Self::show_preview_loading(&ui, &entry);
        Self::queue_preview(tasks, state, ui, generation, entry);
    }

    pub(super) fn show_preview_loading(ui: &slint::Weak<AppWindow>, entry: &FileEntry) {
        if let Some(window) = ui.upgrade() {
            window.set_preview_title(fs::display_name(&entry.name).into());
            window.set_preview_metadata(SharedString::default());
            window.set_preview_text("Loading preview…".into());
            window.set_preview_kind(crate::PreviewKind::Empty);
            window.set_ranger_preview_files(slint::ModelRc::new(slint::VecModel::default()));
        }
    }

    pub(super) fn queue_preview(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
        generation: u64,
        entry: FileEntry,
    ) {
        tasks.execute(move || {
            let path = entry.path.clone();
            {
                let state = state.lock().unwrap();
                if state.view_mode != ViewMode::Ranger || !state.accepts_preview(generation, &path)
                {
                    return;
                }
            }
            let result = fs::read_preview(&entry);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                let state = state.lock().unwrap();
                if state.view_mode != ViewMode::Ranger || !state.accepts_preview(generation, &path)
                {
                    return;
                }
                drop(state);
                match result {
                    Ok(preview) => sync_preview(&window, &entry, preview),
                    Err(error) => show_preview_error(&window, &entry, error.to_string()),
                }
            });
        });
    }
}
