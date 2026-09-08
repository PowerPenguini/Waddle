use iced::{
    Task,
    widget::{self, Id},
};

use crate::fs;

use super::{
    App, BrowserStatusModel, COMMAND_ID, DisplayedLocation, Message, NEW_FOLDER_ID, OPEN_WITH_ID,
    RENAME_ID, SEARCH_ID,
};

impl App {
    fn active_bottom_input(&self) -> Option<(&'static str, bool)> {
        self.transient_presentation().input().map(|input| {
            use super::transient::InputTarget;
            let id = match input.target {
                InputTarget::Search => SEARCH_ID,
                InputTarget::Command => COMMAND_ID,
                InputTarget::Rename => RENAME_ID,
                InputTarget::NewName => NEW_FOLDER_ID,
                InputTarget::OpenWith => OPEN_WITH_ID,
            };
            (id, input.empty)
        })
    }

    pub(super) fn active_bottom_input_empty(&self) -> bool {
        self.active_bottom_input().is_some_and(|(_, empty)| empty)
    }

    pub(super) fn bottom_input_active(&self) -> bool {
        self.active_bottom_input().is_some()
    }

    pub(super) fn refocus_bottom_input(&self) -> Task<Message> {
        self.focus_bottom_input(false)
    }

    pub(super) fn focus_bottom_input(&self, select_all: bool) -> Task<Message> {
        self.active_bottom_input()
            .map_or_else(Task::none, |(id, _)| {
                widget::operation::focus(Id::new(id)).chain(if select_all {
                    widget::operation::select_all(Id::new(id))
                } else {
                    Task::none()
                })
            })
    }

    pub(super) fn flash_copy_feedback(&mut self) {
        self.presentation.flash_copy_feedback();
    }

    pub(super) fn browser_status_model(&self) -> BrowserStatusModel<'_> {
        let transfers = self.transfers.overview();
        self.presentation.browser_status(
            transfers.conflict_prompt,
            transfers.active,
            transfers.retry,
        )
    }

    #[cfg(test)]
    pub(super) fn delete_operator_pending(&self) -> bool {
        self.browser_input.delete_pending()
    }

    pub(super) fn show_error(&mut self, message: String) {
        self.open_file_operation(move |session| session.show_error(message));
    }

    pub(super) fn show_warning(&mut self, message: String) {
        self.open_file_operation(move |session| session.show_warning(message));
    }

    pub(super) fn refresh_status(&mut self) {
        if self.browser_input.pending_sequence().is_some() {
            return;
        }
        if let Some(status) = self.transfers.pending_cut_status() {
            self.presentation.set_status(status);
            return;
        }
        let location = self.navigation.location_label();
        let status = if self.grid.selection_count() > 1 {
            format!("{} selected  •  {}", self.grid.selection_count(), location)
        } else if let Some(entry) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
        {
            let name = fs::display_name(&entry.name);
            if self.navigation.displayed_location() == DisplayedLocation::Trash {
                self.navigation
                    .trash_entries()
                    .iter()
                    .find(|trashed| trashed.file.path == entry.path)
                    .map_or_else(
                        || format!("{name}  •  Trash"),
                        |trashed| {
                            format!(
                                "{name}  •  originally {}",
                                trashed.receipt.original.display()
                            )
                        },
                    )
            } else {
                match self.grid.details() {
                    Some(details) => format!("{name}  •  {details}"),
                    None => format!("{name}  •  Loading details…"),
                }
            }
        } else {
            format!("{} items  •  {}", self.navigation.entries().len(), location)
        };
        self.presentation.set_status(status);
    }

    pub(super) fn status_height(&self) -> f32 {
        self.presentation.status_height(self.reduced_motion())
    }
}
