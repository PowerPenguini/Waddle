use std::path::{Path, PathBuf};

use slint::{ComponentHandle, DataTransfer};

use crate::fs;

use super::Explorer;
use crate::app::{
    tree::{dragged_path, invalidate_tree_folders},
    view::show_error_window,
};
use crate::fs::FileEntry;

impl Explorer {
    pub(super) fn entry_drop_allowed(&self, data: &DataTransfer, index: i32) -> bool {
        if !self.mutations_allowed() {
            return false;
        }
        let Some(destination) = self.entry_at(index).filter(FileEntry::is_directory) else {
            return false;
        };
        Self::drop_allowed(data, &destination.path)
    }

    pub(super) fn ranger_parent_drop_allowed(&self, data: &DataTransfer, index: i32) -> bool {
        if !self.mutations_allowed() {
            return false;
        }
        let Some(destination) = self
            .ranger_parent_entry_at(index)
            .filter(FileEntry::is_directory)
        else {
            return false;
        };
        Self::drop_allowed(data, &destination.path)
    }

    pub(super) fn tree_drop_allowed(&self, data: &DataTransfer, index: i32) -> bool {
        if !self.mutations_allowed() {
            return false;
        }
        self.tree_path_at(index)
            .is_some_and(|destination| Self::drop_allowed(data, &destination))
    }

    pub(super) fn drop_allowed(data: &DataTransfer, destination: &Path) -> bool {
        dragged_path(data).is_some_and(|source| {
            source != destination
                && source.parent() != Some(destination)
                && !destination.starts_with(&source)
        })
    }

    pub(super) fn drop_on_entry(&self, data: DataTransfer, index: i32) -> bool {
        let Some(destination) = self.entry_at(index).filter(FileEntry::is_directory) else {
            return false;
        };
        self.begin_drop(data, destination.path)
    }

    pub(super) fn drop_on_ranger_parent(&self, data: DataTransfer, index: i32) -> bool {
        let Some(destination) = self
            .ranger_parent_entry_at(index)
            .filter(FileEntry::is_directory)
        else {
            return false;
        };
        self.begin_drop(data, destination.path)
    }

    pub(super) fn drop_on_tree(&self, data: DataTransfer, index: i32) -> bool {
        let Some(destination) = self.tree_path_at(index) else {
            return false;
        };
        self.begin_drop(data, destination)
    }

    pub(super) fn begin_drop(&self, data: DataTransfer, destination: PathBuf) -> bool {
        if !self.mutations_allowed() {
            return false;
        }
        let Some(source) = dragged_path(&data) else {
            return false;
        };
        if !Self::drop_allowed(&data, &destination) {
            return false;
        }

        let source_parent = source.parent().map(Path::to_path_buf);
        let source_name = source
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
            window.set_status_text(format!("Moving {source_name}…").into());
        }

        tasks.execute(move || {
            let result = fs::move_entry(&source, &destination);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                match result {
                    Ok(moved_path) => {
                        let mut changed_folders = vec![destination];
                        if let Some(source_parent) = source_parent {
                            changed_folders.push(source_parent);
                        }
                        let reloads = {
                            let mut state = state.lock().unwrap();
                            invalidate_tree_folders(&mut state.roots, &changed_folders)
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
                        navigation.refresh(Some(moved_path), true);
                    }
                    Err(error) => show_error_window(&window, error.to_string()),
                }
            });
        });
        true
    }
}
