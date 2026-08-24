use super::*;

impl App {
    pub(super) fn begin_command(&mut self, prefix: char) -> Task<Message> {
        self.browser_input.enter(InputMode::Command);
        self.command.begin(prefix);
        self.sync_bottom_bar();
        widget::operation::focus(Id::new(COMMAND_ID))
    }

    pub(super) fn submit_command(&mut self) -> Task<Message> {
        if self.browser_input.mode() != InputMode::Command {
            return Task::none();
        }
        self.browser_input.leave_mode();
        match self.command.submit(self.navigation.current().to_path_buf()) {
            CommandSubmission::None => Task::none(),
            CommandSubmission::Quit => self.quit(),
            CommandSubmission::Updated => {
                self.sync_bottom_bar();
                Task::none()
            }
            CommandSubmission::Refresh => self.live_refresh(),
            CommandSubmission::Diagnostics => {
                self.command.show_diagnostics(self.diagnostics.report());
                self.sync_bottom_bar();
                Task::none()
            }
            CommandSubmission::Settings { local, arguments } => {
                match self.view_preferences.apply_command(
                    self.navigation.current(),
                    local,
                    &arguments,
                ) {
                    Ok(applied) => {
                        if arguments.is_empty() || arguments == "all" {
                            self.command.show_settings(applied.status);
                            self.sync_bottom_bar();
                            return Task::none();
                        }
                        self.status = applied.status;
                        if applied.tree_changed {
                            self.sync_tree_visibility();
                        }
                        if applied.browse_changed {
                            self.live_refresh()
                        } else if applied.tree_changed {
                            self.load_visible_thumbnails()
                        } else {
                            Task::none()
                        }
                    }
                    Err(error) => {
                        self.status = error;
                        Task::none()
                    }
                }
            }
            CommandSubmission::Favorite(arguments) => {
                match self.places.command(self.navigation.current(), &arguments) {
                    Ok(status) => {
                        self.install_locations();
                        if arguments.is_empty() || arguments == "list" {
                            self.command.show_settings(status);
                            self.sync_bottom_bar();
                        } else {
                            self.status = status;
                        }
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            CommandSubmission::Recent(arguments) => {
                match self.recent.command(&arguments) {
                    Ok((effect, status)) => {
                        self.status = status;
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
                                return self.navigate(current, false, None);
                            }
                            recent::Effect::Reload
                            | recent::Effect::Disabled
                            | recent::Effect::Enabled => {}
                        }
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            CommandSubmission::Volume(arguments) => {
                self.busy = true;
                self.status = "Waiting for desktop volume authorization…".to_owned();
                Task::perform(
                    self.operations.run(OperationKind::Background, move |_| {
                        places::run_volume_command(&arguments)
                    }),
                    |completion| match completion {
                        Completion::Finished(result) => Message::VolumeFinished(result),
                        Completion::Cancelled => Message::Noop,
                    },
                )
            }
            CommandSubmission::Properties => self.show_properties(),
            CommandSubmission::Chmod(mode) => self.run_permission_change(mode),
            CommandSubmission::OpenWith {
                application,
                default,
            } => self.run_open_with(application, default),
            CommandSubmission::Execute(execution) => {
                self.busy = true;
                self.status = execution.status();
                let adapter = self.command_adapter;
                Task::perform(
                    self.operations
                        .run(OperationKind::Command, move |_| Ok(execution.run(&adapter))),
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
        self.busy = false;
        if let Some((summary, detail)) = command_failure_report(&result) {
            self.diagnostics.record(summary, detail);
        }
        let consequences = self.command.complete(result, self.navigation.current());
        self.sync_bottom_bar();
        if let Some(error) = consequences.error {
            self.show_error(error);
            return Task::none();
        }
        if let Some(status) = consequences.status {
            self.status = status;
        }
        if !consequences.refresh {
            return Task::none();
        }
        let tree_refresh = self.invalidate_tree(vec![self.navigation.current().to_path_buf()]);
        if let Some(directory) = consequences.navigate {
            Task::batch([tree_refresh, self.navigate(directory, true, None)])
        } else {
            Task::batch([tree_refresh, self.refresh(None)])
        }
    }

    pub(super) fn sync_bottom_bar(&mut self) {
        let expanded_height = self.desired_expanded_height();
        if let Some(height) = expanded_height {
            self.expanded_bar_height = height;
        }
        self.animation_now = Instant::now();
        self.output_expansion
            .go_mut(expanded_height.is_some(), self.animation_now);
    }

    pub(super) fn desired_expanded_height(&self) -> Option<f32> {
        self.command
            .output()
            .map(|output| expanded_bar_height(&output.detail))
            .or_else(|| {
                self.file_operations
                    .expanded_detail()
                    .map(expanded_bar_height)
            })
            .or_else(|| self.transfers.expanded().then_some(190.0))
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
        self.sync_bottom_bar();
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
        self.file_operations.cancel();
    }

    pub(super) fn show_new_folder(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(|session| session.begin_new_folder());
        widget::operation::focus(Id::new(NEW_FOLDER_ID))
    }

    pub(super) fn show_new_file(
        &mut self,
        template: Option<PathBuf>,
        suggested_name: String,
        label: String,
    ) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(move |session| {
            session.begin_new_file(template, suggested_name, label);
        });
        widget::operation::focus(Id::new(NEW_FOLDER_ID))
    }

    pub(super) fn show_properties(&mut self) -> Task<Message> {
        let Some(path) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
            .map(|entry| entry.path.clone())
        else {
            self.status = "Select one entry to inspect Properties".to_owned();
            return Task::none();
        };
        self.command.close_output();
        self.busy = true;
        self.status = "Reading Properties…".to_owned();
        Task::perform(
            self.operations
                .run(OperationKind::Details, move |_| properties::read(&path)),
            |completion| match completion {
                Completion::Finished(result) => Message::PropertiesFinished(result),
                Completion::Cancelled => {
                    Message::PropertiesFinished(Err("Properties request was replaced".to_owned()))
                }
            },
        )
    }

    pub(super) fn run_permission_change(&mut self, mode: String) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let paths = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.status = "Select at least one entry before :chmod".to_owned();
            return Task::none();
        }
        self.busy = true;
        self.status = "Changing permissions…".to_owned();
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                properties::chmod(paths, &mode)
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
    ) -> Task<Message> {
        let Some(path) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
            .map(|entry| entry.path.clone())
        else {
            self.status = "Select one entry first".to_owned();
            return Task::none();
        };
        self.busy = true;
        self.status = if make_default {
            "Changing the default application…".to_owned()
        } else {
            "Opening with selected application…".to_owned()
        };
        let operation = if make_default {
            OperationKind::Mutation
        } else {
            OperationKind::Background
        };
        Task::perform(
            self.operations.run(operation, move |_| {
                properties::open_with(path, &application, make_default)
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
            || self.busy
            || self.transfers.has_conflict()
        {
            return Task::none();
        }
        let entries = self.selected_trash_entries();
        if entries.is_empty() {
            return Task::none();
        }
        self.busy = true;
        self.status = format!("Restoring {} Trash items…", entries.len());
        self.transfers
            .enqueue_restore(entries)
            .map_or_else(Task::none, |work| self.launch_transfer(work))
    }

    pub(super) fn show_trash_delete_prompt(&mut self, empty: bool) -> Task<Message> {
        if self.navigation.displayed_location() != DisplayedLocation::Trash || self.busy {
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
            self.sync_bottom_bar();
            Task::none()
        }
    }

    pub(super) fn cancel_prompt(&mut self) -> Task<Message> {
        if self.file_operations.cancel() {
            self.sync_bottom_bar();
        }
        Task::none()
    }

    pub(super) fn prompt_blocks_action(&mut self) -> bool {
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
            self.cancel_rename();
        }
        self.command.close_output();
        open(&mut self.file_operations);
        self.sync_bottom_bar();
    }

    pub(super) fn start_file_operation(&mut self, work: FileOperationWork) -> Task<Message> {
        self.busy = true;
        let adapter = self.trash_adapter;
        Task::perform(
            self.operations
                .run(OperationKind::Mutation, move |_| Ok(work.run(&adapter))),
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
        self.busy = false;
        let trash_delete_report = match &completion {
            file_operation::Completion::TrashDelete(report) => Some(report.clone()),
            _ => None,
        };
        let journal_action = match &completion {
            file_operation::Completion::Name {
                kind: file_operation::NameKind::Rename { source },
                result: Ok(destination),
            } => journal::Action::rename(source.clone(), destination.clone()).map(Some),
            file_operation::Completion::Name {
                kind: file_operation::NameKind::NewFolder,
                result: Ok(path),
            } => journal::Action::new_folder(path.clone()).map(Some),
            file_operation::Completion::Name {
                kind:
                    file_operation::NameKind::NewFile {
                        template: Some(template),
                    },
                result: Ok(path),
            } => journal::Action::transfer(
                journal::TransferKind::Copy,
                &[fs::TransferReceipt {
                    source: template.clone(),
                    destination: path.clone(),
                }],
            ),
            file_operation::Completion::Name {
                kind: file_operation::NameKind::NewFile { template: None },
                result: Ok(path),
            } => journal::Action::new_file(path.clone()).map(Some),
            file_operation::Completion::Trash(completion) => {
                journal::Action::trash(&completion.receipts)
            }
            _ => Ok(None),
        };
        let consequences = self.file_operations.complete(completion);
        if let Some(report) = trash_delete_report {
            self.status = format!(
                "Permanently deleted {}  •  {} failed",
                report.deleted,
                report.failures.len()
            );
            if !report.failures.is_empty() {
                self.command.show_settings(
                    report
                        .failures
                        .iter()
                        .map(|(entry, error)| format!("{}: {error}", fs::display_name(&entry.name)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                self.sync_bottom_bar();
            }
        }
        if consequences.renamed {
            self.browser_input.leave_mode();
        }
        self.sync_bottom_bar();
        match journal_action {
            Ok(Some(action)) => {
                if let Err(error) = self.journal.record(action) {
                    self.status_notice = Some(format!(
                        "Operation completed but Undo was not saved: {error}"
                    ));
                }
            }
            Err(error) => {
                self.status_notice = Some(format!(
                    "Operation completed but Undo is unavailable: {error}"
                ));
            }
            Ok(None) => {}
        }
        if consequences.refresh {
            if !self.navigation.folder_displayed() {
                self.refresh_location()
            } else {
                Task::batch([
                    self.invalidate_tree(vec![self.navigation.current().to_path_buf()]),
                    self.refresh(consequences.select),
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
        self.busy = true;
        let mut journal = self.journal.clone();
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                let result = if redo { journal.redo() } else { journal.undo() };
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
        self.busy = false;
        self.journal = journal;
        match result {
            Ok(effect) => {
                self.status = effect.status;
                let tree = self.invalidate_tree(effect.changed_folders);
                let refresh = self.refresh(effect.select);
                Task::batch([tree, refresh])
            }
            Err(error) => {
                self.status = error;
                Task::none()
            }
        }
    }
}
