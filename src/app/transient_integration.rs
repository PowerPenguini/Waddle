//! App adapter for Transient presentation. Domain edits finish before animation,
//! restored browser status, or Iced focus observes the resolved presentation.

use super::{App, transient};
use iced::Task;
use transient::Dismiss;

impl App {
    pub(super) fn show_command_output(&mut self, summary: String, detail: String) {
        self.command.show_output(summary, detail);
        self.sync_transient_presentation();
    }

    pub(super) fn show_command_detail(&mut self, detail: String) {
        self.command.show_settings(detail);
        self.sync_transient_presentation();
    }

    pub(super) fn close_command_output(&mut self) {
        self.change_transient(|sessions| sessions.dismiss(Dismiss::CommandOutput));
    }

    pub(super) fn finish_presentation_update(
        &mut self,
        previous: transient::Resolved,
        task: Task<super::Message>,
    ) -> Task<super::Message> {
        self.sync_transient_presentation();
        self.finish_focus_update(previous, task)
    }

    pub(super) fn transient_presentation(&self) -> transient::Resolved {
        let transfers = self.transfers.overview();
        transient::Sources {
            command: &self.command,
            file_operation: &self.file_operations,
            open_with: &self.open_with,
            mode: self.browser_input.mode(),
            search_empty: self.search.query().is_empty(),
            conflict: transfers.conflict_prompt.is_some(),
            history_open: transfers.expanded,
        }
        .resolve()
    }

    pub(super) fn change_transient<R>(
        &mut self,
        edit: impl FnOnce(&mut transient::Sessions<'_>) -> R,
    ) -> R {
        let result = edit(&mut transient::Sessions::new(
            &mut self.command,
            &mut self.file_operations,
            &mut self.open_with,
            &mut self.browser_input,
        ));
        self.sync_transient_presentation();
        result
    }

    fn sync_transient_presentation(&mut self) {
        let next = self.transient_presentation();
        if self.presentation.sync_transient(next) {
            self.refresh_status();
        }
    }
}
