use slint::ComponentHandle;

use crate::fs;

use super::Explorer;
use crate::app::{tree::invalidate_tree_folders, view::show_error_window};

impl Explorer {
    pub(super) fn copy_selected_entry(&self, index: i32) {
        if !self.navigation_allowed() {
            return;
        }
        let Some(entry) = self.entry_at(index) else {
            return;
        };
        let name = fs::display_name(&entry.name);
        self.state.lock().unwrap().copied_entry = Some(entry.path);
        if let Some(window) = self.ui.upgrade() {
            window.set_status_text(format!("Copied {name}").into());
        }
    }

    pub(super) fn paste_copied_entry(&self) {
        if !self.mutations_allowed() {
            return;
        }
        let (source, destination) = {
            let state = self.state.lock().unwrap();
            (state.copied_entry.clone(), state.current.clone())
        };
        let Some(source) = source else {
            return;
        };

        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.display().to_string());
        let state = self.state.clone();
        let ui = self.ui.clone();
        let tasks = self.operation_tasks.clone();
        let background_tasks = self.background_tasks.clone();
        let navigation = self.navigation_context();
        if let Some(window) = ui.upgrade() {
            window.set_busy(true);
            window.set_status_text(format!("Copying {name}…").into());
        }

        tasks.execute(move || {
            let result = fs::copy_entry(&source, &destination);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                match result {
                    Ok(copied_path) => {
                        let reloads = {
                            let mut state = state.lock().unwrap();
                            invalidate_tree_folders(
                                &mut state.roots,
                                std::slice::from_ref(&destination),
                            )
                        };
                        for (node_id, path) in reloads {
                            Self::load_folder_children(
                                background_tasks.clone(),
                                state.clone(),
                                window.as_weak(),
                                node_id,
                                path,
                            );
                        }
                        navigation.refresh(Some(copied_path), true);
                    }
                    Err(error) => show_error_window(&window, error.to_string()),
                }
            });
        });
    }
}
