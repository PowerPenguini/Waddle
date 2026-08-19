use gio::prelude::*;
use slint::SharedString;

use crate::{DialogKind, fs};

use super::Explorer;
use crate::app::{state::PendingName, view::show_error_window};
use crate::fs::FileEntry;

impl Explorer {
    pub(super) fn show_rename_dialog(&self, index: i32) {
        if !self.mutations_allowed() {
            return;
        }
        if let Some(entry) = self.entry_at(index) {
            self.show_name_dialog(Some(entry));
        }
    }

    pub(super) fn show_name_dialog(&self, existing: Option<FileEntry>) {
        if !self.mutations_allowed() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let (title, value, pending) = match existing {
            Some(entry) => (
                "Rename",
                fs::display_name(&entry.name),
                PendingName::Rename(entry),
            ),
            None => ("New Folder", String::new(), PendingName::NewFolder),
        };
        self.state.lock().unwrap().pending_name = Some(pending);
        ui.set_dialog_title(title.into());
        ui.set_dialog_message(SharedString::default());
        ui.set_dialog_detail(SharedString::default());
        ui.set_dialog_input(value.into());
        ui.set_dialog_error(SharedString::default());
        ui.set_dialog_kind(DialogKind::Name);
    }

    pub(super) fn submit_name(&self, name: &str) {
        if let Err(message) = fs::validate_name(name) {
            if let Some(ui) = self.ui.upgrade() {
                ui.set_dialog_error(message.into());
            }
            return;
        }

        let (pending, current) = {
            let state = self.state.lock().unwrap();
            (state.pending_name.clone(), state.current.clone())
        };
        let Some(pending) = pending else {
            return;
        };
        let name = name.to_owned();
        let state = self.state.clone();
        let ui = self.ui.clone();
        let tasks = self.operation_tasks.clone();
        let navigation = self.navigation_context();
        if let Some(window) = ui.upgrade() {
            window.set_busy(true);
            window.set_dialog_error(SharedString::default());
        }

        tasks.execute(move || {
            let result = match pending {
                PendingName::NewFolder => fs::create_folder(&current, &name),
                PendingName::Rename(entry) => fs::rename_entry(&entry.path, &name),
            };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                match result {
                    Ok(path) => {
                        state.lock().unwrap().pending_name = None;
                        window.set_dialog_kind(DialogKind::None);
                        navigation.refresh(Some(path), true);
                    }
                    Err(error) => {
                        window.set_busy(false);
                        window.set_dialog_error(error.to_string().into());
                    }
                }
            });
        });
    }

    pub(super) fn show_trash_dialog(&self, index: i32) {
        if !self.mutations_allowed() {
            return;
        }
        let Some(entry) = self.entry_at(index) else {
            return;
        };
        self.show_trash_dialog_for_entries(vec![entry]);
    }

    pub(super) fn show_selected_trash_dialog(&self) {
        if !self.mutations_allowed() {
            return;
        }
        let entries = {
            let state = self.state.lock().unwrap();
            if state.selected_entries.len() > 1 {
                state
                    .selected_entries
                    .iter()
                    .filter_map(|index| state.entries.get(*index).cloned())
                    .collect::<Vec<_>>()
            } else {
                state
                    .selected_entry
                    .and_then(|index| state.entries.get(index).cloned())
                    .into_iter()
                    .collect()
            }
        };
        self.show_trash_dialog_for_entries(entries);
    }

    fn show_trash_dialog_for_entries(&self, entries: Vec<FileEntry>) {
        if entries.is_empty() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let message = deletion_confirmation(&entries);
        self.state.lock().unwrap().pending_delete = entries;
        ui.set_dialog_title("Move to Trash".into());
        ui.set_dialog_message(message.into());
        ui.set_dialog_detail(SharedString::default());
        ui.set_dialog_error(SharedString::default());
        ui.set_dialog_kind(DialogKind::Trash);
    }

    pub(super) fn confirm_dialog(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        match ui.get_dialog_kind() {
            DialogKind::Trash => self.move_pending_to_trash(),
            DialogKind::PermanentDelete => self.delete_pending_permanently(),
            DialogKind::Error => self.cancel_dialog(),
            _ => {}
        }
    }

    pub(super) fn move_pending_to_trash(&self) {
        let pending = self.state.lock().unwrap().pending_delete.clone();
        if pending.is_empty() {
            return;
        }
        let state = self.state.clone();
        let ui = self.ui.clone();
        let tasks = self.operation_tasks.clone();
        let navigation = self.navigation_context();
        if let Some(window) = ui.upgrade() {
            window.set_busy(true);
        }

        tasks.execute(move || {
            let failures = pending
                .iter()
                .filter_map(|entry| {
                    gio::File::for_path(&entry.path)
                        .trash(None::<&gio::Cancellable>)
                        .err()
                        .map(|error| (entry.clone(), error.to_string()))
                })
                .collect::<Vec<_>>();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                if failures.is_empty() {
                    state.lock().unwrap().pending_delete.clear();
                    window.set_dialog_kind(DialogKind::None);
                    navigation.refresh(None, true);
                } else {
                    let failed_entries = failures
                        .iter()
                        .map(|(entry, _)| entry.clone())
                        .collect::<Vec<_>>();
                    let detail = failures
                        .iter()
                        .map(|(entry, reason)| {
                            format!("{}: {reason}", fs::display_name(&entry.name))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    state.lock().unwrap().pending_delete = failed_entries;
                    window.set_busy(false);
                    window.set_dialog_title("Moving to Trash failed".into());
                    window.set_dialog_message(permanent_delete_confirmation(failures.len()).into());
                    window.set_dialog_detail(format!("{detail}\n\nThis cannot be undone.").into());
                    window.set_dialog_kind(DialogKind::PermanentDelete);
                }
            });
        });
    }

    pub(super) fn delete_pending_permanently(&self) {
        let pending = self.state.lock().unwrap().pending_delete.clone();
        if pending.is_empty() {
            return;
        }
        let state = self.state.clone();
        let ui = self.ui.clone();
        let tasks = self.operation_tasks.clone();
        let navigation = self.navigation_context();
        if let Some(window) = ui.upgrade() {
            window.set_busy(true);
        }

        tasks.execute(move || {
            let failures = pending
                .iter()
                .filter_map(|entry| {
                    fs::delete_permanently(&entry.path)
                        .err()
                        .map(|error| (entry.clone(), error.to_string()))
                })
                .collect::<Vec<_>>();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                if failures.is_empty() {
                    state.lock().unwrap().pending_delete.clear();
                    window.set_dialog_kind(DialogKind::None);
                    navigation.refresh(None, true);
                } else {
                    let detail = failures
                        .iter()
                        .map(|(entry, reason)| {
                            format!("{}: {reason}", fs::display_name(&entry.name))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    state.lock().unwrap().pending_delete =
                        failures.into_iter().map(|(entry, _)| entry).collect();
                    window.set_busy(false);
                    show_error_window(&window, detail);
                }
            });
        });
    }

    pub(super) fn cancel_dialog(&self) {
        if let Some(ui) = self.ui.upgrade() {
            if ui.get_busy() {
                return;
            }
            ui.set_dialog_kind(DialogKind::None);
            ui.set_dialog_error(SharedString::default());
        }
        let mut state = self.state.lock().unwrap();
        state.pending_name = None;
        state.pending_delete.clear();
    }
}

fn deletion_confirmation(entries: &[FileEntry]) -> String {
    if let [entry] = entries {
        format!("Move “{}” to Trash?", fs::display_name(&entry.name))
    } else {
        format!("Move {} selected items to Trash?", entries.len())
    }
}

fn permanent_delete_confirmation(count: usize) -> String {
    if count == 1 {
        "Permanently delete this item instead?".to_owned()
    } else {
        format!("Permanently delete these {count} items instead?")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{deletion_confirmation, permanent_delete_confirmation};
    use crate::fs::FileEntry;

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from("/tmp").join(name),
            name: name.into(),
            directory: false,
        }
    }

    #[test]
    fn deletion_prompts_distinguish_single_and_multiple_items() {
        assert_eq!(
            deletion_confirmation(&[entry("one")]),
            "Move “one” to Trash?"
        );
        assert_eq!(
            deletion_confirmation(&[entry("one"), entry("two")]),
            "Move 2 selected items to Trash?"
        );
        assert_eq!(
            permanent_delete_confirmation(2),
            "Permanently delete these 2 items instead?"
        );
    }
}
