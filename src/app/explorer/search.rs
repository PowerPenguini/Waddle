use std::{
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use slint::TimerMode;

use crate::{AppWindow, fs, fs::FileEntry};

use super::{Explorer, RECURSIVE_SEARCH_DEBOUNCE};
use crate::app::{
    executor::TaskExecutor,
    state::{ExplorerState, ViewMode},
    view::{clear_preview, show_error_window, sync_files, sync_selection},
};

const MAX_RECURSIVE_RESULTS: usize = 1_000;

impl Explorer {
    pub(super) fn begin_search(&self) {
        self.search_timer.stop();
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        let restored = restore_directory_view(&mut state, true);
        state.cancel_visual_selection();
        state.search_origin = Some(state.selected_entry);
        state.search_draft.clear();
        if restored && let Some(window) = self.ui.upgrade() {
            sync_files(&window, &state);
        }
    }

    pub(super) fn update_search(self: &Rc<Self>, query: &str) {
        self.search_timer.stop();
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        if self.state.lock().unwrap().recursive_search_active {
            self.update_recursive_search(query, generation);
            return;
        }

        let (selected, changed, ranger) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.selected_entry;
            let restored = restore_directory_view(&mut state, true);
            state.search_draft.clear();
            state.search_draft.push_str(query);
            let origin = state.search_origin.unwrap_or(state.selected_entry);
            state.selected_entry = if query.is_empty() {
                origin
            } else {
                find_match(&state.entries, query, origin, false).or(origin)
            };
            let result = (
                state.selected_entry,
                restored || state.selected_entry != previous,
                state.view_mode == ViewMode::Ranger,
            );
            if restored && let Some(window) = self.ui.upgrade() {
                sync_files(&window, &state);
            }
            result
        };
        self.apply_search_selection(selected, changed, ranger);
    }

    pub(super) fn set_recursive_search_mode(self: &Rc<Self>, enabled: bool) {
        self.search_timer.stop();
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        if enabled {
            self.update_recursive_search("", generation);
            return;
        }

        let (selected, changed, ranger) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.selected_entry;
            let restored = restore_directory_view(&mut state, true);
            state.search_draft.clear();
            let result = (
                state.selected_entry,
                restored || state.selected_entry != previous,
                state.view_mode == ViewMode::Ranger,
            );
            if restored && let Some(window) = self.ui.upgrade() {
                sync_files(&window, &state);
            }
            result
        };
        self.apply_search_selection(selected, changed, ranger);
    }

    fn update_recursive_search(self: &Rc<Self>, query: &str, generation: u64) {
        let (root, ranger) = {
            let mut state = self.state.lock().unwrap();
            state.recursive_search_active = true;
            state.recursive_search_loading = !query.is_empty();
            state.recursive_search_truncated = false;
            state.search_draft.clear();
            state.search_draft.push_str(query);
            state.entries.clear();
            state.selected_entry = None;
            state.begin_details();
            (state.current.clone(), state.view_mode == ViewMode::Ranger)
        };
        if let Some(window) = self.ui.upgrade() {
            let state = self.state.lock().unwrap();
            sync_files(&window, &state);
            if ranger && query.is_empty() {
                drop(state);
                self.schedule_preview();
            } else if ranger {
                clear_preview(&window);
            }
        }
        if query.is_empty() {
            return;
        }

        let tasks = self.background_tasks.clone();
        let state = self.state.clone();
        let ui = self.ui.clone();
        let generation_tracker = self.search_generation.clone();
        let query = query.to_owned();
        self.search_timer.start(
            TimerMode::SingleShot,
            RECURSIVE_SEARCH_DEBOUNCE,
            move || {
                Self::queue_recursive_search(
                    tasks.clone(),
                    state.clone(),
                    ui.clone(),
                    generation_tracker.clone(),
                    generation,
                    root.clone(),
                    query.clone(),
                );
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn queue_recursive_search(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
        generation_tracker: Arc<AtomicU64>,
        generation: u64,
        root: PathBuf,
        query: String,
    ) {
        let callback_tasks = tasks.clone();
        tasks.execute(move || {
            if generation_tracker.load(Ordering::Relaxed) != generation {
                return;
            }
            let result = fs::search_directory(&root, &query, MAX_RECURSIVE_RESULTS, || {
                generation_tracker.load(Ordering::Relaxed) != generation
            });
            if generation_tracker.load(Ordering::Relaxed) != generation {
                return;
            }
            let _ = slint::invoke_from_event_loop(move || {
                if generation_tracker.load(Ordering::Relaxed) != generation {
                    return;
                }
                let Some(window) = ui.upgrade() else {
                    return;
                };
                let mut state_guard = state.lock().unwrap();
                if !state_guard.recursive_search_active
                    || state_guard.search_draft != query
                    || state_guard.current != root
                {
                    return;
                }
                match result {
                    Ok(results) => {
                        state_guard.recursive_search_loading = false;
                        state_guard.recursive_search_truncated = results.truncated;
                        state_guard.entries = results.entries;
                        state_guard.selected_entry = (!state_guard.entries.is_empty()).then_some(0);
                        state_guard.begin_details();
                        let ranger = state_guard.view_mode == ViewMode::Ranger;
                        sync_files(&window, &state_guard);
                        drop(state_guard);
                        Self::queue_current_details(
                            callback_tasks.clone(),
                            state.clone(),
                            ui.clone(),
                        );
                        if ranger {
                            Self::queue_current_preview(callback_tasks, state, ui);
                        }
                    }
                    Err(error) => {
                        restore_directory_view(&mut state_guard, true);
                        state_guard.search_origin = None;
                        state_guard.search_draft.clear();
                        sync_files(&window, &state_guard);
                        drop(state_guard);
                        window.set_search_active(false);
                        window.set_search_text("".into());
                        show_error_window(&window, error.to_string());
                    }
                }
            });
        });
    }

    pub(super) fn submit_search(self: &Rc<Self>) {
        if self.state.lock().unwrap().recursive_search_active {
            self.submit_recursive_search();
            return;
        }
        let (selected, changed, ranger) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.selected_entry;
            if state.search_draft.is_empty() {
                if !state.last_search.is_empty() {
                    let origin = state.search_origin.unwrap_or(state.selected_entry);
                    state.selected_entry =
                        find_match(&state.entries, &state.last_search, origin, false).or(origin);
                }
            } else {
                state.last_search = state.search_draft.clone();
            }
            state.search_origin = None;
            state.search_draft.clear();
            (
                state.selected_entry,
                state.selected_entry != previous,
                state.view_mode == ViewMode::Ranger,
            )
        };
        self.apply_search_selection(selected, changed, ranger);
    }

    fn submit_recursive_search(self: &Rc<Self>) {
        let entry = {
            let state = self.state.lock().unwrap();
            state
                .selected_entry
                .and_then(|index| state.entries.get(index).cloned())
        };
        let Some(entry) = entry else {
            if let Some(window) = self.ui.upgrade() {
                window.set_search_active(true);
            }
            return;
        };

        self.search_timer.stop();
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        {
            let mut state = self.state.lock().unwrap();
            restore_directory_view(&mut state, true);
            state.search_origin = None;
            state.search_draft.clear();
            if let Some(window) = self.ui.upgrade() {
                sync_files(&window, &state);
                window.set_search_text("".into());
            }
        }
        self.schedule_selected_details();
        if self.state.lock().unwrap().view_mode == ViewMode::Ranger {
            self.schedule_preview();
        }
        if entry.is_directory() {
            self.navigate(entry.path, true);
        } else {
            self.open_entry(entry);
        }
    }

    pub(super) fn cancel_search(self: &Rc<Self>) {
        self.search_timer.stop();
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        let (selected, changed, ranger) = {
            let mut state = self.state.lock().unwrap();
            let previous = state.selected_entry;
            let restored = restore_directory_view(&mut state, true);
            if let Some(origin) = state.search_origin.take() {
                state.selected_entry = origin;
            }
            state.search_draft.clear();
            let result = (
                state.selected_entry,
                restored || state.selected_entry != previous,
                state.view_mode == ViewMode::Ranger,
            );
            if restored && let Some(window) = self.ui.upgrade() {
                sync_files(&window, &state);
            }
            result
        };
        self.apply_search_selection(selected, changed, ranger);
    }

    pub(super) fn cancel_search_for_navigation(&self) {
        self.search_timer.stop();
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        restore_directory_view(&mut state, true);
        if let Some(origin) = state.search_origin.take() {
            state.selected_entry = origin;
        }
        state.search_draft.clear();
        if let Some(window) = self.ui.upgrade() {
            sync_files(&window, &state);
            window.set_search_active(false);
            window.set_search_text("".into());
        }
    }

    pub(super) fn repeat_search(self: &Rc<Self>, reverse: bool) {
        let (selected, changed, ranger) = {
            let mut state = self.state.lock().unwrap();
            if state.last_search.is_empty() {
                return;
            }
            let previous = state.selected_entry;
            state.selected_entry = find_match(
                &state.entries,
                &state.last_search,
                state.selected_entry,
                reverse,
            )
            .or(state.selected_entry);
            (
                state.selected_entry,
                state.selected_entry != previous,
                state.view_mode == ViewMode::Ranger,
            )
        };
        self.apply_search_selection(selected, changed, ranger);
    }

    fn apply_search_selection(
        self: &Rc<Self>,
        _selected: Option<usize>,
        changed: bool,
        ranger: bool,
    ) {
        if let Some(ui) = self.ui.upgrade() {
            let state = self.state.lock().unwrap();
            sync_selection(&ui, &state);
        }
        if changed {
            self.schedule_selected_details();
        }
        if ranger {
            self.schedule_preview();
        }
    }
}

fn restore_directory_view(state: &mut ExplorerState, restore_selection: bool) -> bool {
    if !state.recursive_search_active {
        return false;
    }
    state.entries = state.directory_entries.clone();
    if restore_selection && let Some(origin) = state.search_origin {
        state.selected_entry = origin;
    }
    state.recursive_search_active = false;
    state.recursive_search_loading = false;
    state.recursive_search_truncated = false;
    state.begin_details();
    true
}

fn find_match(
    entries: &[FileEntry],
    query: &str,
    anchor: Option<usize>,
    reverse: bool,
) -> Option<usize> {
    if entries.is_empty() || query.is_empty() {
        return None;
    }
    let query = query.to_lowercase();
    let len = entries.len();
    let first = match (anchor, reverse) {
        (Some(index), false) => (index + 1) % len,
        (Some(index), true) => index.checked_sub(1).unwrap_or(len - 1),
        (None, false) => 0,
        (None, true) => len - 1,
    };
    (0..len)
        .map(|offset| {
            if reverse {
                (first + len - offset) % len
            } else {
                (first + offset) % len
            }
        })
        .find(|index| {
            entries[*index]
                .name
                .to_string_lossy()
                .to_lowercase()
                .contains(&query)
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{find_match, restore_directory_view};
    use crate::app::state::ExplorerState;
    use crate::fs::FileEntry;

    fn entries() -> Vec<FileEntry> {
        ["alpha", "Beta", "alphabet", "gamma"]
            .into_iter()
            .map(|name| FileEntry {
                path: PathBuf::from(name),
                name: name.into(),
                directory: false,
            })
            .collect()
    }

    #[test]
    fn search_wraps_and_ignores_case() {
        let entries = entries();
        assert_eq!(find_match(&entries, "ALP", Some(0), false), Some(2));
        assert_eq!(find_match(&entries, "beta", Some(2), false), Some(1));
    }

    #[test]
    fn reverse_search_wraps_backwards() {
        let entries = entries();
        assert_eq!(find_match(&entries, "alp", Some(2), true), Some(0));
        assert_eq!(find_match(&entries, "alp", Some(0), true), Some(2));
    }

    #[test]
    fn leaving_recursive_search_restores_directory_entries_and_selection() {
        let mut state = ExplorerState::new(PathBuf::from("/root"), Vec::new());
        state.directory_entries = entries();
        state.entries = vec![FileEntry {
            path: PathBuf::from("nested/result"),
            name: "result".into(),
            directory: false,
        }];
        state.selected_entry = Some(0);
        state.search_origin = Some(Some(2));
        state.recursive_search_active = true;
        state.recursive_search_loading = true;
        state.recursive_search_truncated = true;

        assert!(restore_directory_view(&mut state, true));

        assert_eq!(state.entries.len(), 4);
        assert_eq!(state.selected_entry, Some(2));
        assert!(!state.recursive_search_active);
        assert!(!state.recursive_search_loading);
        assert!(!state.recursive_search_truncated);
    }
}
