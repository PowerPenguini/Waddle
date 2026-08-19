use std::{path::Path, rc::Rc};

use slint::{DataTransfer, ModelRc, SharedString, VecModel};

use crate::{AppWindow, DialogKind, EntryIcon, FileItem as UiFileItem, PreviewKind, fs};

use super::state::{DraggedEntry, ExplorerState};

pub(super) fn sync_navigation(ui: &AppWindow, state: &ExplorerState) {
    let location = state
        .pending_navigation
        .as_ref()
        .map(|navigation| &navigation.requested)
        .unwrap_or(&state.current);
    ui.set_location_text(location.to_string_lossy().into_owned().into());
    ui.set_can_go_back(state.pending_navigation.is_some() || !state.history.is_empty());
    ui.set_can_go_forward(state.pending_navigation.is_none() && !state.forward_history.is_empty());
    ui.set_can_go_parent(location.parent().is_some());
}

pub(super) fn sync_files(ui: &AppWindow, state: &ExplorerState) {
    let relative_to = state
        .recursive_search_active
        .then_some(state.current.as_path());
    ui.set_files(ModelRc::new(VecModel::from(file_items(
        &state.entries,
        relative_to,
    ))));
    sync_selection(ui, state);
    ui.set_recursive_search_active(state.recursive_search_active);
    ui.set_search_loading(state.recursive_search_loading);
    ui.set_search_result_count(if state.recursive_search_active {
        state.entries.len() as i32
    } else {
        -1
    });
    ui.set_search_results_truncated(state.recursive_search_truncated);
    sync_status(ui, state);
}

pub(super) fn sync_selection(ui: &AppWindow, state: &ExplorerState) {
    ui.set_selected_entry(state.selected_entry.map_or(-1, |index| index as i32));
    let use_extended_selection = state.selected_entries.len() > 1;
    ui.set_selected_entries(ModelRc::new(VecModel::from(
        (0..state.entries.len())
            .map(|index| {
                if use_extended_selection {
                    state.selected_entries.contains(&index)
                } else {
                    state.selected_entry == Some(index)
                }
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_visual_selection_active(
        state.visual_selection_anchor.is_some() || state.selected_entries.len() > 1,
    );
    sync_status(ui, state);
}

pub(super) fn sync_status(ui: &AppWindow, state: &ExplorerState) {
    let text = if state.selected_entries.len() > 1 {
        format!(
            "{} selected  •  {}",
            state.selected_entries.len(),
            state.current.display()
        )
    } else {
        state
            .selected_entry
            .and_then(|index| state.entries.get(index))
            .map(|entry| {
                let name = fs::display_name(&entry.name);
                match &state.selected_details {
                    Some(details) => format!("{name}  •  {details}"),
                    None => format!("{name}  •  Loading details…"),
                }
            })
            .unwrap_or_else(|| {
                format!(
                    "{} items  •  {}",
                    state.entries.len(),
                    state.current.display()
                )
            })
    };
    ui.set_status_text(text.into());
}

pub(super) fn sync_ranger_parent(ui: &AppWindow, state: &ExplorerState) {
    ui.set_ranger_parent_files(ModelRc::new(VecModel::from(file_items(
        &state.parent_entries,
        None,
    ))));
    ui.set_ranger_parent_selected(state.selected_parent_entry.map_or(-1, |index| index as i32));
}

pub(super) fn clear_preview(ui: &AppWindow) {
    ui.set_preview_title(SharedString::default());
    ui.set_preview_metadata(SharedString::default());
    ui.set_preview_text("Select an item to preview".into());
    ui.set_preview_kind(PreviewKind::Empty);
    ui.set_ranger_preview_files(ModelRc::new(VecModel::default()));
}

pub(super) fn sync_preview(ui: &AppWindow, entry: &fs::FileEntry, preview: fs::PreviewData) {
    ui.set_preview_title(fs::display_name(&entry.name).into());
    ui.set_ranger_preview_files(ModelRc::new(VecModel::default()));
    match preview {
        fs::PreviewData::Directory(entries) => {
            ui.set_preview_metadata(format!("{} items", entries.len()).into());
            ui.set_preview_text(if entries.is_empty() {
                "This folder is empty".into()
            } else {
                SharedString::default()
            });
            ui.set_ranger_preview_files(ModelRc::new(VecModel::from(file_items(&entries, None))));
            ui.set_preview_kind(PreviewKind::Directory);
        }
        fs::PreviewData::Text {
            metadata,
            mut text,
            truncated,
        } => {
            if truncated {
                text.push_str("\n\n… preview truncated at 64 KiB");
            }
            ui.set_preview_metadata(metadata.into());
            ui.set_preview_text(text.into());
            ui.set_preview_kind(PreviewKind::Text);
        }
        fs::PreviewData::Metadata(metadata) => {
            ui.set_preview_metadata(metadata.into());
            ui.set_preview_text("Binary file — text preview unavailable".into());
            ui.set_preview_kind(PreviewKind::Metadata);
        }
    }
}

pub(super) fn show_preview_error(ui: &AppWindow, entry: &fs::FileEntry, message: String) {
    ui.set_preview_title(fs::display_name(&entry.name).into());
    ui.set_preview_metadata(SharedString::default());
    ui.set_preview_text(message.into());
    ui.set_preview_kind(PreviewKind::Error);
    ui.set_ranger_preview_files(ModelRc::new(VecModel::default()));
}

fn file_items(entries: &[fs::FileEntry], relative_to: Option<&Path>) -> Vec<UiFileItem> {
    entries
        .iter()
        .map(|entry| {
            let directory = entry.is_directory();
            let mut drag_data = DataTransfer::default();
            drag_data.set_user_data(Rc::new(DraggedEntry {
                path: entry.path.clone(),
            }));
            UiFileItem {
                name: relative_to
                    .and_then(|root| entry.path.strip_prefix(root).ok())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| fs::display_name(&entry.name))
                    .into(),
                directory,
                icon: if directory {
                    EntryIcon::Folder
                } else {
                    EntryIcon::File
                },
                drag_data,
            }
        })
        .collect()
}

pub(super) fn show_error_window(ui: &AppWindow, message: String) {
    ui.set_busy(false);
    ui.set_dialog_title("Something went wrong".into());
    ui.set_dialog_message(SharedString::default());
    ui.set_dialog_detail(message.into());
    ui.set_dialog_error(SharedString::default());
    ui.set_dialog_kind(DialogKind::Error);
}
