use std::{collections::HashSet, path::PathBuf};

use iced::Task;

use crate::journal;

use super::{
    App, Completion, DisplayedLocation, FileOperationSession, FileOperationWork, InputMode,
    Message, NavigationTransition, OperationKind, command, file_operation, open_with, places,
    presentation::command_failure_report, properties, recent, system_icon_task,
    transfer_integration, transient::Dismiss, trash,
};

impl App {
    pub(super) fn begin_command(&mut self, prefix: char) -> Task<Message> {
        self.change_transient(|sessions| sessions.begin_command(prefix));
        self.refocus_bottom_input()
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
        if let Err(error) = self.change_transient(|sessions| sessions.begin_open_with(path)) {
            self.presentation.set_status(error);
            return Task::none();
        }
        self.release_location_focus()
    }

    pub(super) fn submit_command(&mut self) -> Task<Message> {
        let current = self.navigation.current().to_path_buf();
        let action = self.change_transient(|sessions| sessions.submit_command(current));
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
            command::CommandAction::OutputChanged => Task::none(),
            command::CommandAction::Refresh => self.refresh_location(),
            command::CommandAction::Diagnostics => {
                self.command.show_diagnostics(self.diagnostics.report());
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
                        let icon_size = if applied.icon_size_changed {
                            self.sync_icon_size()
                        } else {
                            Task::none()
                        };
                        let browse = if applied.browse_changed {
                            self.refresh_location()
                        } else if applied.tree_changed {
                            self.load_visible_thumbnails()
                        } else {
                            Task::none()
                        };
                        Task::batch([browse, icon_size, system_icon_task(system_icons)])
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
                let request = self.command.output_revision();
                let adapter = self.command_adapter;
                Task::perform(
                    self.operations
                        .run_foreground(OperationKind::Command, move |_| {
                            Ok(execution.run(&adapter))
                        }),
                    move |completion| match completion {
                        Completion::Finished(result) => {
                            Message::CommandFinished { request, result }
                        }
                        Completion::Cancelled => Message::Noop,
                    },
                )
            }
        }
    }

    pub(super) fn finish_command(
        &mut self,
        request: u64,
        result: Result<command::Completion, String>,
    ) -> Task<Message> {
        if let Some((summary, detail)) = command_failure_report(&result) {
            self.diagnostics.record(summary, detail);
        }
        let consequences =
            self.command
                .complete_request(request, result, self.navigation.current());
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
            Task::batch([tree_refresh, self.refresh_location()])
        }
    }

    pub(super) fn show_rename(&mut self, index: usize) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        self.change_transient(|sessions| sessions.begin_rename(entry));
        self.focus_bottom_input(true)
    }

    pub(super) fn rename_selected(&mut self) -> Task<Message> {
        let Some(index) = self.grid.selected_entry() else {
            return Task::none();
        };
        self.show_rename(index)
    }

    pub(super) fn cancel_rename(&mut self) {
        self.change_transient(|sessions| sessions.dismiss(Dismiss::FileOperation));
    }

    pub(super) fn show_new_folder(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(|session| session.begin_new_folder());
        self.refocus_bottom_input()
    }

    pub(super) fn show_new_file(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(|session| session.begin_new_file());
        self.refocus_bottom_input()
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
        let request = self.command.output_revision();
        self.presentation
            .set_status("Reading Properties…".to_owned());
        Task::perform(
            // Selection details must not cancel this explicit request.
            // The Command session revision handles superseded Properties output.
            self.operations
                .run_foreground(OperationKind::Background, move |_| properties::read(&path)),
            move |completion| Message::PropertiesFinished {
                request,
                result: match completion {
                    Completion::Finished(result) => result,
                    Completion::Cancelled => Err("Properties request was replaced".to_owned()),
                },
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
        let request = self.command.output_revision();
        Task::perform(
            self.operations
                .run_foreground(OperationKind::Mutation, move |_| {
                    properties::chmod(targets, &mode)
                }),
            move |completion| match completion {
                Completion::Finished(result) => Message::MetadataFinished { request, result },
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

    pub(super) fn submit_open_with(&mut self) -> Task<Message> {
        let Some(request) = self.change_transient(|sessions| sessions.submit_open_with()) else {
            return self.refocus_bottom_input();
        };
        self.finish_open_with(request)
    }

    pub(super) fn cancel_open_with(&mut self) -> Task<Message> {
        if self.open_with.leave_custom() {
            return iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::unfocus(),
            );
        }
        self.change_transient(|sessions| sessions.dismiss(Dismiss::OpenWith));
        Task::none()
    }

    fn finish_open_with(&mut self, request: open_with::Request) -> Task<Message> {
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
        let request = self.command.output_revision();
        Task::perform(
            self.operations.run_foreground(operation, move |_| {
                open_with::launch(path, &application, make_default)
            }),
            move |completion| match completion {
                Completion::Finished(result) => Message::MetadataFinished { request, result },
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn submit_rename(&mut self) -> Task<Message> {
        if self.browser_input.mode() != InputMode::Rename {
            return Task::none();
        }
        if self.file_operations.rename_is_unchanged() {
            self.cancel_rename();
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

    pub(super) fn trash_selected(&mut self) -> Task<Message> {
        if self.navigation.displayed_location() == DisplayedLocation::Trash {
            return self.show_trash_delete_prompt(false);
        }
        if !self.mutations_allowed() {
            return Task::none();
        }
        let entries = self.selected_entries();
        if entries.is_empty() {
            return Task::none();
        }
        self.change_transient(|sessions| sessions.prepare_trash());
        self.transfers
            .trash(entries, &self.operations)
            .map(transfer_integration::transfer_runtime_message)
    }

    pub(super) fn selected_trash_entries(&self) -> Vec<trash::Entry> {
        let selected = self
            .grid
            .selected_items(self.navigation.entries())
            .into_iter()
            .map(|entry| entry.path)
            .collect::<HashSet<_>>();
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
        let current = self.navigation.current().to_path_buf();
        let confirmation =
            self.change_transient(|sessions| sessions.confirm_file_operation(current));
        match confirmation {
            Some(work) => self.start_file_operation(work),
            None => Task::none(),
        }
    }

    pub(super) fn cancel_prompt(&mut self) -> Task<Message> {
        self.change_transient(|sessions| sessions.dismiss(Dismiss::FileOperation));
        Task::none()
    }

    pub(super) fn prompt_blocks_action(&mut self) -> bool {
        self.change_transient(|sessions| sessions.blocks_action())
    }

    pub(super) fn open_file_operation(&mut self, open: impl FnOnce(&mut FileOperationSession)) {
        self.change_transient(|sessions| sessions.open_file_operation(open));
    }

    pub(super) fn start_file_operation(&mut self, work: FileOperationWork) -> Task<Message> {
        Task::perform(
            self.operations
                .run_foreground(OperationKind::Mutation, move |_| Ok(work.run())),
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
        let effects =
            self.change_transient(|sessions| sessions.complete_file_operation(completion));
        if let Some(status) = effects.status {
            self.presentation.set_status_notice(status);
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
            if effects.preserve_interaction || !self.navigation.folder_displayed() {
                self.refresh_location()
            } else {
                let refresh = if self.navigation.defer_refresh() {
                    Task::none()
                } else {
                    self.refresh(effects.select)
                };
                Task::batch([
                    self.invalidate_tree(vec![self.navigation.current().to_path_buf()]),
                    refresh,
                ])
            }
        } else {
            Task::none()
        }
    }

    pub(super) fn run_journal(&mut self, redo: bool) -> Task<Message> {
        let transfers = self.transfers.overview();
        if self.foreground_operation_active()
            || transfers.active
            || transfers.conflict_prompt.is_some()
            || self.search.is_recursive()
            || !self.navigation.folder_displayed()
        {
            return Task::none();
        }
        self.presentation
            .set_status(if redo { "Redoing…" } else { "Undoing…" });
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
                let refresh = if self.navigation.defer_refresh() {
                    Task::none()
                } else if self.navigation.folder_displayed() {
                    self.refresh(effect.select)
                } else {
                    self.refresh_location()
                };
                Task::batch([tree, refresh])
            }
            Err(error) => {
                // Undo/Redo can fail after applying some filesystem changes.
                // Keep the failure visible while refreshing the affected views.
                self.presentation.set_notice(error);
                let tree = self.invalidate_tree(self.sidebar_tree.expanded_paths());
                Task::batch([tree, self.refresh_location()])
            }
        }
    }
}
