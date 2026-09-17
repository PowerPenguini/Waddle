//! Shared availability for keyboard dispatch, context menus, and execution.
//! A collection of paths is a valid source, but is not a destination folder.
use super::{App, DisplayedLocation};

pub(super) struct OperationAccess {
    pub entries: bool,
    pub destination: bool,
    pub trash: bool,
    pub history: bool,
    pub unavailable: &'static str,
}

impl App {
    pub(super) fn operation_access(&self) -> OperationAccess {
        let transfers = self.transfers.overview();
        let idle = !self.foreground_operation_active() && transfers.conflict_prompt.is_none();
        let ready = idle && !self.navigation.loading();
        let location = self.navigation.displayed_location();
        OperationAccess {
            entries: ready && location != DisplayedLocation::Trash,
            destination: ready
                && location == DisplayedLocation::Folder
                && !self.search.is_recursive(),
            trash: ready && location == DisplayedLocation::Trash,
            // History contains absolute paths and does not require a destination.
            // A background refresh must not swallow Undo/Redo.
            history: idle && !transfers.active,
            unavailable: if !idle {
                "Finish the current operation before changing files"
            } else if self.navigation.loading() {
                "Wait for the location to finish loading"
            } else if location == DisplayedLocation::Trash {
                "Use Delete to delete permanently from Trash"
            } else {
                "Select a file first"
            },
        }
    }
}
