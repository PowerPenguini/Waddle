use std::path::PathBuf;

use iced::{
    Task,
    widget::{self, Id},
};

use crate::journal;

use super::{
    App, COMMAND_ID, Completion, DisplayedLocation, FileOperationSession, FileOperationWork,
    GioTrashAdapter, InputMode, Message, NEW_FOLDER_ID, NavigationTransition, OPEN_WITH_ID,
    OperationKind, RENAME_ID, TransientPresentation, command, file_operation, open_with, places,
    presentation::command_failure_report, properties, recent, system_icon_task,
    transfer_integration, trash,
};

impl App {
    pub(super) fn begin_command(&mut self, prefix: char) -> Task<Message> {
        self.open_with.cancel();
        self.browser_input.enter(InputMode::Command);
        self.command.begin(prefix);
        self.sync_transient_presentation();
        widget::operation::focus(Id::new(COMMAND_ID))
    }

    pub(super) fn begin_open_with(&mut self) -> Task<Message> {
        self.begin_open_with_target(None)
    }

    fn begin_open_with_target(&mut self, target: Option<PathBuf>) -> Task<Message> {
        let Some(path) = target.or_else(|| self.selected_entry_path()) else {
            self.presentation
                .set_status("Select an entry or pass a path after --".to_owned());
            return Task::none();
        };
        self.command.close_output();
        if let Err(error) = self.open_with.begin(path) {
            self.presentation.set_status(error);
            return Task::none();
        }
        self.browser_input.enter(InputMode::OpenWith);
        self.sync_transient_presentation();
        widget::operation::focus(Id::new(OPEN_WITH_ID))
    }

    pub(super) fn submit_command(&mut self) -> Task<Message> {
        if self.browser_input.mode() != InputMode::Command {
            return Task::none();
        }
        self.browser_input.leave_mode();
        let action = self.command.submit(self.navigation.current().to_path_buf());
        self.apply_command_action(action)
    }

    fn apply_command_action(&mut self, action: command::CommandAction) -> Task<Message> {
        match action {
            command::CommandAction::None => Task::none(),
            command::CommandAction::Error(error) => {
                self.presentation.set_status(error);
                Task::none()
            }
            command::CommandAction::Quit => self.quit(),
            command::CommandAction::OutputChanged => {
                self.sync_transient_presentation();
                Task::none()
            }
            command::CommandAction::Refresh => self.live_refresh(),
            command::CommandAction::Diagnostics => {
                self.command.show_diagnostics(self.diagnostics.report());
                self.sync_transient_presentation();
                Task::none()
            }
            command::CommandAction::ChangeSettings { local, arguments } => {
                match self.view_preferences.apply_command(
                    self.navigation.current(),
                    local,
                    &arguments,
                ) {
                    Ok(applied) => {
                        self.refresh_theme();
                        let system_icons = self
                            .system_icons
                            .set_enabled(self.view_preferences.uses_system_icons());
                        if arguments.is_empty() || arguments == "all" {
                            self.show_command_detail(applied.status);
                            return Task::none();
                        }
                        self.presentation.set_status(applied.status);
                        if applied.tree_changed {
                            self.sync_tree_visibility();
                        }
                        let browse = if applied.browse_changed {
                            self.live_refresh()
                        } else if applied.tree_changed {
                            self.load_visible_thumbnails()
                        } else {
                            Task::none()
                        };
                        Task::batch([browse, system_icon_task(system_icons)])
                    }
                    Err(error) => {
                        self.presentation.set_status(error);
                        Task::none()
                    }
                }
            }
            command::CommandAction::ManageFavorite(arguments) => {
                let mut entries = self.recent.sidebar_entry().into_iter().collect::<Vec<_>>();
                entries.push(self.trash.sidebar_entry());
                match self.sidebar_tree.favorite_command(
                    self.navigation.current(),
                    &arguments,
                    entries,
                ) {
                    Ok(status) => {
                        if arguments.is_empty() || arguments == "list" {
                            self.show_command_detail(status);
                        } else {
                            self.presentation.set_status(status);
                        }
                    }
                    Err(error) => self.presentation.set_status(error),
                }
                Task::none()
            }
            command::CommandAction::ManageRecent(arguments) => {
                match self.recent.command(&arguments) {
                    Ok((effect, status)) => {
                        self.presentation.set_status(status);
                        self.install_locations();
                        match effect {
                            recent::Effect::Open => return self.open_recent(),
                            recent::Effect::Reload
                                if self.navigation.displayed_location()
                                    == DisplayedLocation::Recent =>
                            {
                                return self.open_recent();
                            }
                            recent::Effect::Disabled
                                if self.navigation.displayed_location()
                                    == DisplayedLocation::Recent =>
                            {
                                let current = self.navigation.current().to_path_buf();
                                return self.transition_navigation(NavigationTransition::Open {
                                    requested: current,
                                    remember: false,
                                    select: None,
                                });
                            }
                            recent::Effect::Reload
                            | recent::Effect::Disabled
                            | recent::Effect::Enabled => {}
                        }
                    }
                    Err(error) => self.presentation.set_status(error),
                }
                Task::none()
            }
            command::CommandAction::ManageVolume(arguments) => {
                self.presentation
                    .set_status("Waiting for desktop volume authorization…");
                Task::perform(
                    self.operations
                        .run_foreground(OperationKind::Background, move |_| {
                            places::run_volume_command(&arguments)
                        }),
                    |completion| match completion {
                        Completion::Finished(result) => Message::VolumeFinished(result),
                        Completion::Cancelled => Message::Noop,
                    },
                )
            }
            command::CommandAction::ShowProperties { target } => {
                self.show_properties_target(target)
            }
            command::CommandAction::ChangePermissions { mode, targets } => {
                self.run_permission_change(mode, targets)
            }
            command::CommandAction::OpenWith {
                application,
                default,
                target,
            } => {
                if application.is_empty() && !default {
                    self.begin_open_with_target(target)
                } else {
                    self.run_open_with(application, default, target)
                }
            }
            command::CommandAction::Execute(execution) => {
                let execution = execution.with_selected(
                    self.selected_entries()
                        .into_iter()
                        .map(|entry| entry.path)
                        .collect(),
                );
                self.presentation.set_status(execution.status());
                let adapter = self.command_adapter;
                Task::perform(
                    self.operations
                        .run_foreground(OperationKind::Command, move |_| {
                            Ok(execution.run(&adapter))
                        }),
                    |completion| match completion {
                        Completion::Finished(result) => Message::CommandFinished(result),
                        Completion::Cancelled => Message::Noop,
                    },
                )
            }
        }
    }

    pub(super) fn finish_command(
        &mut self,
        result: Result<command::Completion, String>,
    ) -> Task<Message> {
        if let Some((summary, detail)) = command_failure_report(&result) {
            self.diagnostics.record(summary, detail);
        }
        let consequences = self.command.complete(result, self.navigation.current());
        self.sync_transient_presentation();
        if let Some(error) = consequences.error {
            self.show_error(error);
            return Task::none();
        }
        if let Some(status) = consequences.status {
            self.presentation.set_status(status);
        }
        if !consequences.refresh {
            return Task::none();
        }
        let tree_refresh = self.invalidate_tree(vec![self.navigation.current().to_path_buf()]);
        if let Some(directory) = consequences.navigate {
            Task::batch([
                tree_refresh,
                self.transition_navigation(NavigationTransition::Open {
                    requested: directory,
                    remember: true,
                    select: None,
                }),
            ])
        } else {
            Task::batch([tree_refresh, self.refresh(None)])
        }
    }

    pub(super) fn transient_presentation(&self) -> TransientPresentation {
        let transfers = self.transfers.overview();
        if transfers.conflict_prompt.is_some() {
            TransientPresentation::conflict()
        } else if let Some(height) = self.open_with.preferred_height() {
            TransientPresentation::open_with(height)
        } else if let Some(output) = self.command.output() {
            TransientPresentation::command_output(&output.detail)
        } else if self.file_operations.prompt_active() {
            TransientPresentation::file_operation(self.file_operations.expanded_detail())
        } else if transfers.expanded && self.browser_input.mode() == InputMode::Browser {
            TransientPresentation::transfer_history()
        } else {
            TransientPresentation::standard()
        }
    }

    pub(super) fn sync_transient_presentation(&mut self) {
        let next = self.transient_presentation();
        if self.presentation.sync_transient(next) {
            self.refresh_status();
        }
    }

    pub(super) fn show_command_output(&mut self, summary: String, detail: String) {
        self.command.show_output(summary, detail);
        self.sync_transient_presentation();
    }

    pub(super) fn show_command_detail(&mut self, detail: String) {
        self.command.show_settings(detail);
        self.sync_transient_presentation();
    }

    pub(super) fn close_command_output(&mut self) {
        self.command.close_output();
        self.sync_transient_presentation();
    }

    pub(super) fn show_rename(&mut self, index: usize) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        self.file_operations.begin_rename(entry);
        self.browser_input.enter(InputMode::Rename);
        self.command.close_output();
        self.sync_transient_presentation();
        Task::batch([
            widget::operation::focus(Id::new(RENAME_ID)),
            widget::operation::select_all(Id::new(RENAME_ID)),
        ])
    }

    pub(super) fn rename_selected(&mut self) -> Task<Message> {
        let Some(index) = self.grid.selected_entry() else {
            return Task::none();
        };
        self.show_rename(index)
    }

    pub(super) fn cancel_rename(&mut self) {
        self.browser_input.leave_mode();
        if self.file_operations.cancel() {
            self.sync_transient_presentation();
        }
    }

    pub(super) fn show_new_folder(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(|session| session.begin_new_folder());
        widget::operation::focus(Id::new(NEW_FOLDER_ID))
    }

    pub(super) fn show_new_file(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(|session| session.begin_new_file());
        widget::operation::focus(Id::new(NEW_FOLDER_ID))
    }

    pub(super) fn show_properties(&mut self) -> Task<Message> {
        self.show_properties_target(None)
    }

    fn show_properties_target(&mut self, target: Option<PathBuf>) -> Task<Message> {
        let Some(path) = target.or_else(|| self.selected_entry_path()) else {
            self.presentation
                .set_status("Select an entry or pass a path to :properties".to_owned());
            return Task::none();
        };
        self.close_command_output();
        self.presentation
            .set_status("Reading Properties…".to_owned());
        Task::perform(
            self.operations
                .run_foreground(OperationKind::Details, move |_| properties::read(&path)),
            |completion| match completion {
                Completion::Finished(result) => Message::PropertiesFinished(result),
                Completion::Cancelled => {
                    Message::PropertiesFinished(Err("Properties request was replaced".to_owned()))
                }
            },
        )
    }

    pub(super) fn run_permission_change(
        &mut self,
        mode: String,
        mut targets: Vec<PathBuf>,
    ) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        if targets.is_empty() {
            targets = self
                .selected_entries()
                .into_iter()
                .map(|entry| entry.path)
                .collect();
        }
        if targets.is_empty() {
            self.presentation
                .set_status("Select entries or pass paths to :chmod".to_owned());
            return Task::none();
        }
        self.presentation
            .set_status("Changing permissions…".to_owned());
        Task::perform(
            self.operations
                .run_foreground(OperationKind::Mutation, move |_| {
                    properties::chmod(targets, &mode)
                }),
            |completion| match completion {
                Completion::Finished(result) => Message::MetadataFinished(result),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn run_open_with(
        &mut self,
        application: String,
        make_default: bool,
        target: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(path) = target.or_else(|| self.selected_entry_path()) else {
            self.presentation
                .set_status("Select an entry or pass a path after --".to_owned());
            return Task::none();
        };
        self.run_open_with_path(path, application, make_default)
    }

    fn selected_entry_path(&self) -> Option<PathBuf> {
        self.grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
            .map(|entry| entry.path.clone())
    }

    pub(super) fn choose_open_with(&mut self, application: String) -> Task<Message> {
        let Some(request) = self.open_with.choose(&application) else {
            return Task::none();
        };
        self.finish_open_with(request)
    }

    pub(super) fn submit_open_with(&mut self) -> Task<Message> {
        let Some(request) = self.open_with.submit_custom() else {
            return Task::none();
        };
        self.finish_open_with(request)
    }

    pub(super) fn cancel_open_with(&mut self) -> Task<Message> {
        self.open_with.cancel();
        self.browser_input.leave_mode();
        self.sync_transient_presentation();
        Task::none()
    }

    fn finish_open_with(&mut self, request: open_with::Request) -> Task<Message> {
        self.browser_input.leave_mode();
        self.sync_transient_presentation();
        self.run_open_with_path(request.path, request.application, false)
    }

    fn run_open_with_path(
        &mut self,
        path: PathBuf,
        application: String,
        make_default: bool,
    ) -> Task<Message> {
        self.presentation.set_status(if make_default {
            "Changing the default application…".to_owned()
        } else {
            "Opening with selected application…".to_owned()
        });
        let operation = if make_default {
            OperationKind::Mutation
        } else {
            OperationKind::Background
        };
        Task::perform(
            self.operations.run_foreground(operation, move |_| {
                open_with::launch(path, &application, make_default)
            }),
            |completion| match completion {
                Completion::Finished(result) => Message::MetadataFinished(result),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn submit_rename(&mut self) -> Task<Message> {
        if self.browser_input.mode() != InputMode::Rename {
            return Task::none();
        }
        self.submit_file_operation_name()
    }

    pub(super) fn submit_file_operation_name(&mut self) -> Task<Message> {
        let Some(work) = self
            .file_operations
            .submit_name(self.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.start_file_operation(work)
    }

    pub(super) fn show_trash_prompt(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let entries = self.selected_entries();
        if entries.is_empty() {
            return Task::none();
        }
        self.open_file_operation(move |session| {
            session.begin_trash(entries);
        });
        Task::none()
    }

    pub(super) fn selected_trash_entries(&self) -> Vec<trash::Entry> {
        let selected = self
            .grid
            .selected_items(self.navigation.entries())
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        self.navigation
            .trash_entries()
            .iter()
            .filter(|entry| selected.contains(&entry.file.path))
            .cloned()
            .collect()
    }

    pub(super) fn restore_selected_trash(&mut self) -> Task<Message> {
        if self.navigation.displayed_location() != DisplayedLocation::Trash
            || self.foreground_operation_active()
            || self.transfers.overview().conflict_prompt.is_some()
        {
            return Task::none();
        }
        let entries = self.selected_trash_entries();
        if entries.is_empty() {
            return Task::none();
        }
        self.presentation
            .set_status(format!("Restoring {} Trash items…", entries.len()));
        self.transfers
            .restore(entries, &self.operations)
            .map(transfer_integration::transfer_runtime_message)
    }

    pub(super) fn show_trash_delete_prompt(&mut self, empty: bool) -> Task<Message> {
        if self.navigation.displayed_location() != DisplayedLocation::Trash
            || self.foreground_operation_active()
        {
            return Task::none();
        }
        let entries = if empty {
            self.navigation.trash_entries().to_vec()
        } else {
            self.selected_trash_entries()
        };
        self.open_file_operation(move |session| {
            session.begin_trash_delete(entries, empty);
        });
        Task::none()
    }

    pub(super) fn confirm_prompt(&mut self) -> Task<Message> {
        if let Some(work) = self
            .file_operations
            .confirm(self.navigation.current().to_path_buf())
        {
            self.start_file_operation(work)
        } else {
            self.sync_transient_presentation();
            Task::none()
        }
    }

    pub(super) fn cancel_prompt(&mut self) -> Task<Message> {
        if self.file_operations.cancel() {
            self.sync_transient_presentation();
        }
        Task::none()
    }

    pub(super) fn prompt_blocks_action(&mut self) -> bool {
        if self.open_with.is_open() {
            let _ = self.cancel_open_with();
        }
        if !self.file_operations.prompt_active() {
            return false;
        }
        if self.file_operations.is_busy() {
            return true;
        }
        let _ = self.cancel_prompt();
        false
    }

    pub(super) fn open_file_operation(&mut self, open: impl FnOnce(&mut FileOperationSession)) {
        if self.browser_input.mode() == InputMode::Rename {
            self.browser_input.leave_mode();
            self.file_operations.cancel();
        }
        self.command.close_output();
        open(&mut self.file_operations);
        self.sync_transient_presentation();
    }

    pub(super) fn start_file_operation(&mut self, work: FileOperationWork) -> Task<Message> {
        Task::perform(
            self.operations
                .run_foreground(OperationKind::Mutation, move |_| {
                    Ok(work.run(&GioTrashAdapter))
                }),
            |completion| match completion {
                Completion::Finished(Ok(completion)) => Message::FileOperationFinished(completion),
                Completion::Finished(Err(error)) => Message::OperationError(error),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn finish_file_operation(
        &mut self,
        completion: file_operation::Completion,
    ) -> Task<Message> {
        let effects = self.file_operations.complete(completion);
        if let Some(detail) = effects.detail {
            self.command.show_settings(detail);
        }
        if effects.renamed {
            self.browser_input.leave_mode();
        }
        self.sync_transient_presentation();
        if let Some(status) = effects.status {
            self.presentation.set_status(status);
        }
        match effects.journal_action {
            Ok(Some(action)) => {
                if let Err(error) = self.journal.record(action) {
                    self.presentation.set_notice(format!(
                        "Operation completed but Undo was not saved: {error}"
                    ));
                }
            }
            Err(error) => {
                self.presentation.set_notice(format!(
                    "Operation completed but Undo is unavailable: {error}"
                ));
            }
            Ok(None) => {}
        }
        if effects.refresh {
            if !self.navigation.folder_displayed() {
                self.refresh_location()
            } else {
                Task::batch([
                    self.invalidate_tree(vec![self.navigation.current().to_path_buf()]),
                    self.refresh(effects.select),
                ])
            }
        } else {
            Task::none()
        }
    }

    pub(super) fn run_journal(&mut self, redo: bool) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let mut journal = self.journal.clone();
        Task::perform(
            self.operations
                .run_foreground(OperationKind::Mutation, move |_| {
                    let result = if redo { journal.redo() } else { journal.undo() }
                        .map_err(|error| error.to_string());
                    Ok((journal, result))
                }),
            |completion| match completion {
                Completion::Finished(Ok((journal, result))) => Message::JournalFinished {
                    journal: Box::new(journal),
                    result,
                },
                Completion::Finished(Err(error)) => Message::OperationError(error),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn finish_journal(
        &mut self,
        journal: journal::Journal,
        result: Result<journal::Effect, String>,
    ) -> Task<Message> {
        self.journal = journal;
        match result {
            Ok(effect) => {
                self.presentation.set_status(effect.status);
                let tree = self.invalidate_tree(effect.changed_folders);
                let refresh = self.refresh(effect.select);
                Task::batch([tree, refresh])
            }
            Err(error) => {
                self.presentation.set_status(error);
                Task::none()
            }
        }
    }
}
