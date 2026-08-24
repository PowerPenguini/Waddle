use super::*;

impl App {
    pub(super) fn browser_status_model(&self) -> BrowserStatusModel<'_> {
        if let Some(prompt) = self.transfers.conflict_prompt() {
            BrowserStatusModel {
                presentation: BrowserStatusPresentation::Conflict,
                text: prompt,
                retry: false,
                history: false,
            }
        } else if self.transfers.active() {
            BrowserStatusModel {
                presentation: BrowserStatusPresentation::Transfer,
                text: "",
                retry: false,
                history: true,
            }
        } else {
            let retry = self.transfers.has_retry();
            BrowserStatusModel {
                presentation: BrowserStatusPresentation::General,
                text: self.status_notice.as_deref().unwrap_or(&self.status),
                retry,
                history: retry,
            }
        }
    }

    #[cfg(test)]
    pub(super) fn delete_operator_pending(&self) -> bool {
        self.browser_input.delete_pending()
    }

    pub(super) fn show_error(&mut self, message: String) {
        self.busy = false;
        self.open_file_operation(move |session| session.show_error(message));
    }

    pub(super) fn refresh_status(&mut self) {
        if self.browser_input.pending_sequence().is_some() {
            return;
        }
        if let Some(status) = self.transfers.pending_cut_status() {
            self.status = status;
            return;
        }
        let location = self.navigation.location_label();
        self.status = if self.grid.selection_count() > 1 {
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
    }

    pub(super) fn status_height(&self) -> f32 {
        if self.reduced_motion() {
            return self.desired_expanded_height().unwrap_or(STATUS_HEIGHT);
        }
        self.output_expansion.interpolate(
            STATUS_HEIGHT,
            self.expanded_bar_height,
            self.animation_now,
        )
    }
}
