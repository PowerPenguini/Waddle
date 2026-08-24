use super::*;

impl App {
    pub(super) fn copy_selection(&mut self) -> Task<Message> {
        let restore_cut = !self.transfers.pending_cut_paths().is_empty();
        let entries = self.selected_entries();
        if let Some(status) = self.transfers.copy(&entries) {
            self.status = status;
            if let (Some(source), Some(payload)) =
                (&self.native_clipboard, self.transfers.clipboard_payload())
                && let Err(error) = ClipboardAdapter::write_clipboard(source, payload)
            {
                self.status = format!("Copied inside PolarExp; system clipboard failed: {error}");
            }
        }
        if restore_cut {
            self.sync_location_monitoring();
            self.refresh(None)
        } else {
            Task::none()
        }
    }

    pub(super) fn cut_selection(&mut self) -> Task<Message> {
        let entries = self.selected_entries();
        let Some(status) = self.transfers.cut(&entries) else {
            return Task::none();
        };
        self.status = status;
        if let (Some(source), Some(payload)) =
            (&self.native_clipboard, self.transfers.clipboard_payload())
            && let Err(error) = ClipboardAdapter::write_clipboard(source, payload)
        {
            self.status = format!("Cut inside PolarExp; system clipboard failed: {error}");
        }
        self.navigation
            .hide_paths(self.transfers.pending_cut_paths());
        self.sync_location_monitoring();
        self.grid.select_only(None, self.navigation.entries().len());
        Task::none()
    }

    pub(super) fn cancel_cut(&mut self, status: &str) -> Task<Message> {
        let Some(generation) = self.transfers.cancel_cut() else {
            return Task::none();
        };
        if let Some(source) = self.native_clipboard.as_ref() {
            ClipboardAdapter::clear_clipboard(source, generation);
        }
        self.sync_location_monitoring();
        self.status = status.to_owned();
        self.refresh(None)
    }

    pub(super) fn restore_cut_after_clipboard_loss(&mut self) -> Task<Message> {
        let message = "Cut restored after clipboard ownership changed".to_owned();
        self.status = message.clone();
        self.status_notice = Some(message);
        self.refresh(None)
    }

    pub(super) fn sync_location_monitoring(&mut self) {
        if self.location_monitoring.is_none() {
            return;
        }
        let locations = location_monitoring::Locations {
            current: self.navigation.current().to_path_buf(),
            current_is_displayed: self.navigation.folder_displayed(),
            pending_cut_paths: self.transfers.pending_cut_paths().to_vec(),
            expanded: tree::expanded_paths(&self.explorer.roots),
            displayed_sources: self.displayed_watch_paths(),
        };
        if let Some(monitoring) = self.location_monitoring.as_mut() {
            monitoring.sync(locations);
        }
    }

    pub(super) fn displayed_watch_paths(&self) -> Vec<PathBuf> {
        match self.navigation.displayed_location() {
            DisplayedLocation::Recent => self.recent.watch_paths(self.navigation.entries()),
            DisplayedLocation::Trash => {
                let mounts = self
                    .explorer
                    .roots
                    .iter()
                    .filter(|node| node.kind == state::NodeKind::Drive)
                    .map(|node| node.path.clone())
                    .collect::<Vec<_>>();
                self.trash
                    .watch_paths(self.navigation.trash_entries(), &mounts)
            }
            DisplayedLocation::Folder => Vec::new(),
        }
    }

    pub(super) fn sync_native_cut_clipboard(&mut self, generation: u64) {
        let Some(source) = self.native_clipboard.as_ref() else {
            return;
        };
        if let Some(payload) = self
            .transfers
            .clipboard_payload()
            .filter(|payload| payload.generation == generation)
        {
            if let Err(error) = ClipboardAdapter::write_clipboard(source, payload) {
                self.status_notice = Some(format!(
                    "Cut was updated inside PolarExp; system clipboard failed: {error}"
                ));
            }
        } else {
            ClipboardAdapter::clear_clipboard(source, generation);
        }
    }

    pub(super) fn paste(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(source) = self.native_clipboard.as_ref() else {
            return self.paste_current();
        };
        match ClipboardAdapter::read_clipboard(source) {
            Ok(completion) => Task::perform(completion, Message::ClipboardRead),
            Err(error) => {
                self.status = error;
                Task::none()
            }
        }
    }

    pub(super) fn paste_current(&mut self) -> Task<Message> {
        let Some(request) = self
            .transfers
            .paste(self.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.start_transfer(request)
    }

    pub(super) fn finish_drag(&mut self, grabbed_index: usize) -> Task<Message> {
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let destination = self.drop_destination_at(self.grid.cursor(), false);
        let request =
            destination.and_then(|destination| self.transfers.request_active(destination, action));
        self.transfers.cancel_drag();
        let Some(request) = request else {
            return Task::none();
        };
        let _ = grabbed_index;
        self.start_transfer(request)
    }

    pub(super) fn start_external_drag(&mut self) -> Task<Message> {
        if self.transfers.active_drag_index().is_none() {
            return Task::none();
        }
        let Some(source) = self.native_dnd.as_ref() else {
            self.transfers.cancel_drag();
            self.drag_hover.cancel();
            self.status = self.native_dnd_error.clone().unwrap_or_else(|| {
                "External drag-and-drop is not ready yet; try again in a moment".to_owned()
            });
            return Task::none();
        };
        let entries = self.transfers.active_drag_entries().to_vec();
        let Some(preview) = self.drag_preview(&entries) else {
            self.transfers.cancel_drag();
            self.drag_hover.cancel();
            return Task::none();
        };
        let copy_only = self.modifiers.control();
        let (count, completion) =
            match self
                .transfers
                .start_outgoing_active(source, copy_only, |_| Some(preview))
            {
                Ok(started) => started,
                Err(error) => {
                    self.transfers.cancel_drag();
                    self.drag_hover.cancel();
                    self.status = format!("Could not start external drag-and-drop: {error}");
                    return Task::none();
                }
            };
        self.drag_hover.cancel();
        self.status = if count == 1 {
            "Dragging 1 item outside PolarExp…".to_owned()
        } else {
            format!("Dragging {count} items outside PolarExp…")
        };
        Task::perform(completion, Message::ExternalDragFinished)
    }

    pub(super) fn quit(&mut self) -> Task<Message> {
        if let Some(source) = self.native_dnd.take() {
            self.transfers.stop(&source);
        } else {
            self.transfers.cancel_drag();
        }
        iced::exit()
    }

    pub(super) fn drop_destination_at(&self, point: Point, allow_current: bool) -> Option<PathBuf> {
        let rows = flatten_rows(&self.explorer, self.navigation.current());
        match self.grid.drop_zone(
            point,
            self.navigation.entries().len(),
            rows.len(),
            self.status_height(),
            allow_current,
        )? {
            DropZone::Sidebar(index) => rows.get(index).map(|row| row.path.clone()),
            DropZone::Entry(index) => self
                .navigation
                .entries()
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| entry.path.clone()),
            DropZone::Current => Some(self.navigation.current().to_path_buf()),
        }
    }

    pub(super) fn drag_in_progress(&self) -> bool {
        self.transfers.active_drag_index().is_some()
            || self.transfers.native_hover_destination().is_some()
    }

    pub(super) fn update_drag_hover(&mut self, point: Point) {
        if !self.drag_in_progress() {
            self.drag_hover.cancel();
            return;
        }
        let rows = flatten_rows(&self.explorer, self.navigation.current());
        let target = match self.grid.drop_zone(
            point,
            self.navigation.entries().len(),
            rows.len(),
            self.status_height(),
            false,
        ) {
            Some(DropZone::Sidebar(index)) => {
                rows.get(index).map(|row| drag_hover::Target::Sidebar {
                    id: row.id,
                    path: row.path.clone(),
                })
            }
            Some(DropZone::Entry(index)) => self
                .navigation
                .entries()
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| drag_hover::Target::Folder(entry.path.clone())),
            _ => None,
        };
        let target = target.filter(|target| {
            if self.transfers.active_drag_index().is_none() {
                return true;
            }
            let path = match target {
                drag_hover::Target::Sidebar { path, .. } | drag_hover::Target::Folder(path) => path,
            };
            self.transfers
                .request_active(
                    path.clone(),
                    if self.modifiers.control() {
                        TransferAction::Copy
                    } else {
                        TransferAction::Move
                    },
                )
                .is_some()
        });
        self.drag_hover.set(target, Instant::now());
    }

    pub(super) fn tick_drag_hover(&mut self, now: Instant) -> Task<Message> {
        if !self.drag_in_progress() {
            self.drag_hover.cancel();
            return Task::none();
        }
        let (grid_delta, sidebar_delta) = self.grid.drag_autoscroll(self.status_height());
        let mut tasks = Vec::new();
        if grid_delta.abs() > f32::EPSILON {
            tasks.push(widget::operation::scroll_by(
                Id::new(GRID_SCROLL_ID),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: grid_delta,
                },
            ));
        }
        if sidebar_delta.abs() > f32::EPSILON {
            tasks.push(widget::operation::scroll_by(
                Id::new(SIDEBAR_SCROLL_ID),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: sidebar_delta,
                },
            ));
        }
        match self.drag_hover.tick(now) {
            Some(drag_hover::Effect::Expand(id)) => tasks.push(self.expand_tree_row(id)),
            Some(drag_hover::Effect::Enter(path)) => tasks.push(self.navigate(path, true, None)),
            None => {}
        }
        Task::batch(tasks)
    }

    pub(super) fn expand_tree_row(&mut self, id: u64) -> Task<Message> {
        let Some(node) = find_node_mut(&mut self.explorer.roots, id) else {
            return Task::none();
        };
        if node.expanded {
            return Task::none();
        }
        node.expanded = true;
        let load = !node.loaded && !node.loading;
        if load {
            node.loading = true;
        }
        let path = node.path.clone();
        if load {
            self.load_tree_node(id, path)
        } else {
            Task::none()
        }
    }

    pub(super) fn drop_highlight_path(&self) -> Option<PathBuf> {
        if let Some(destination) = self.transfers.native_hover_destination() {
            return destination.map(Path::to_path_buf);
        }
        self.transfers.active_drag_index()?;
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let destination = self.drop_destination_at(self.grid.cursor(), false)?;
        self.transfers
            .request_active(destination, action)
            .map(|request| request.destination)
    }

    pub(super) fn handle_native_dnd_event(&mut self, event: TransferEvent) -> Task<Message> {
        let hover_position = match &event {
            TransferEvent::Hover { position, .. } => Some(*position),
            _ => None,
        };
        if let Some(position) = hover_position {
            self.grid
                .move_cursor(position, self.navigation.entries().len());
        } else {
            self.drag_hover.cancel();
        }
        let resolved_destination = match &event {
            TransferEvent::Hover { position, .. } => self.drop_destination_at(*position, true),
            _ => None,
        };
        let Some(source) = self.native_dnd.as_ref() else {
            self.show_error("Drag-and-drop adapter is unavailable".to_owned());
            return Task::none();
        };
        let update = self
            .transfers
            .handle_native(source, event, move |_, _| resolved_destination.clone());
        let task = self.apply_native_update(update);
        if let Some(position) = hover_position {
            self.update_drag_hover(position);
        }
        task
    }

    pub(super) fn apply_native_update(&mut self, update: NativeUpdate) -> Task<Message> {
        match update {
            NativeUpdate::None => Task::none(),
            NativeUpdate::Status(status) => {
                self.status_notice = None;
                if status.is_empty() {
                    self.refresh_status();
                } else {
                    self.status = status;
                }
                Task::none()
            }
            NativeUpdate::Notice(message) => {
                self.status = message.clone();
                self.status_notice = Some(message);
                Task::none()
            }
            NativeUpdate::Start(request) => self.start_transfer(request),
            NativeUpdate::Error(error) => {
                self.show_error(error);
                Task::none()
            }
            NativeUpdate::ClipboardLost(true) => self.restore_cut_after_clipboard_loss(),
            NativeUpdate::ClipboardLost(false) => Task::none(),
        }
    }

    pub(super) fn x11_dnd_active(&self) -> bool {
        self.native_dnd
            .as_ref()
            .is_some_and(native_clipboard::DndSource::is_x11)
    }

    pub(super) fn finish_x11_drop(&mut self, generation: u64) -> Task<Message> {
        if generation != self.x11_drop_generation || self.x11_drop_paths.is_empty() {
            return Task::none();
        }
        let paths = std::mem::take(&mut self.x11_drop_paths);
        let destination = self
            .transfers
            .native_hover_destination()
            .flatten()
            .map(Path::to_path_buf)
            .or_else(|| self.drop_destination_at(self.grid.cursor(), true));
        let Some(destination) = destination else {
            self.status_notice = Some("This is not a valid drop target".to_owned());
            return self.handle_native_dnd_event(TransferEvent::Leave { id: X11_INBOUND_ID });
        };
        let action = std::mem::replace(&mut self.x11_drop_action, TransferAction::Copy);
        self.handle_native_dnd_event(TransferEvent::Drop {
            id: X11_INBOUND_ID,
            paths,
            destination,
            action,
        })
    }

    pub(super) fn start_transfer(&mut self, request: TransferRequest) -> Task<Message> {
        match self.transfers.enqueue(request) {
            Ok(Some(work)) => self.launch_transfer(work),
            Ok(None) => Task::none(),
            Err(error) => {
                self.show_error(error);
                Task::none()
            }
        }
    }

    pub(super) fn launch_transfer(&mut self, work: transfer_session::Work) -> Task<Message> {
        if work.restoring() {
            self.busy = true;
            self.status = format!("Restoring {} Trash items…", work.entry_count());
        }
        let id = work.id();
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                Ok::<_, String>(work.run())
            }),
            move |completion| match completion {
                Completion::Finished(Ok(outcome)) => Message::TransferBatchFinished {
                    id,
                    outcome: Box::new(outcome),
                },
                Completion::Finished(Err(error)) => Message::TransferBatchFinished {
                    id,
                    outcome: Box::new(fs::TransferBatchOutcome::Complete(fs::TransferReport {
                        completed: Vec::new(),
                        failures: vec![fs::TransferFailure {
                            source: PathBuf::new(),
                            error,
                        }],
                        retained: Vec::new(),
                        warnings: Vec::new(),
                        receipts: Vec::new(),
                        cancelled: false,
                    })),
                },
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn finish_transfer_batch(
        &mut self,
        id: u64,
        outcome: fs::TransferBatchOutcome,
    ) -> Task<Message> {
        let update = self.transfers.complete_batch(id, outcome);
        self.apply_transfer_batch_update(update)
    }

    pub(super) fn apply_transfer_batch_update(
        &mut self,
        update: TransferBatchUpdate,
    ) -> Task<Message> {
        match update {
            TransferBatchUpdate::Completed(completed) => match *completed {
                TransferCompletedBatch::Transfer(completed) => {
                    let finished = self.finish_transfer(completed.request, completed.report);
                    let next = completed
                        .next
                        .map_or_else(Task::none, |work| self.launch_transfer(work));
                    Task::batch([finished, next])
                }
                TransferCompletedBatch::Restore(completed) => {
                    self.finish_restore_completion(completed)
                }
            },
            TransferBatchUpdate::Conflict => {
                self.status = self
                    .transfers
                    .conflict_prompt()
                    .unwrap_or("Transfer paused for a conflict")
                    .to_owned();
                Task::none()
            }
            TransferBatchUpdate::Ignored => Task::none(),
        }
    }

    pub(super) fn finish_restore_completion(
        &mut self,
        completed: transfer_session::CompletedRestore,
    ) -> Task<Message> {
        self.busy = false;
        match completed.journal_action {
            Ok(Some(action)) => {
                if let Err(error) = self.journal.record(action) {
                    self.status_notice =
                        Some(format!("Restore completed but Undo was not saved: {error}"));
                }
            }
            Err(error) => {
                self.status_notice = Some(format!(
                    "Restore completed but Undo is unavailable: {error}"
                ));
            }
            Ok(None) => {}
        }
        self.status = completed.status;
        if let Some(detail) = completed.detail {
            self.command.show_settings(detail);
            self.sync_bottom_bar();
        }
        let tree = self.invalidate_tree(completed.changed_folders);
        let refresh = self.open_trash();
        let next = completed
            .next
            .map_or_else(Task::none, |work| self.launch_transfer(work));
        Task::batch([tree, refresh, next])
    }

    pub(super) fn resolve_transfer_conflict(
        &mut self,
        key: char,
        remaining: bool,
    ) -> Task<Message> {
        self.transfers
            .resolve_conflict(key, remaining)
            .map_or_else(Task::none, |work| self.launch_transfer(work))
    }

    pub(super) fn cancel_transfer_conflict(&mut self) -> Task<Message> {
        match self.transfers.cancel() {
            TransferCancelUpdate::Conflict(update) => self.apply_transfer_batch_update(update),
            TransferCancelUpdate::Active => {
                self.status = "Cancelling active transfer…".to_owned();
                Task::none()
            }
            TransferCancelUpdate::None => Task::none(),
        }
    }

    pub(super) fn finish_transfer(
        &mut self,
        request: TransferRequest,
        report: fs::TransferReport,
    ) -> Task<Message> {
        let adapter = self
            .native_dnd
            .as_ref()
            .map(|source| source as &dyn Adapter);
        let completion =
            self.transfers
                .finish_transfer(adapter, &request, &report, self.navigation.current());
        if let Some(generation) = completion.clipboard_generation {
            self.sync_native_cut_clipboard(generation);
        }
        self.sync_location_monitoring();
        match completion.journal_action {
            Ok(Some(action)) => {
                if let Err(error) = self.journal.record(action) {
                    self.status_notice = Some(format!(
                        "Transfer completed but Undo was not saved: {error}"
                    ));
                }
            }
            Err(error) => {
                self.status_notice = Some(format!(
                    "Transfer completed but Undo is unavailable: {error}"
                ));
            }
            Ok(None) => {}
        }
        self.apply_transfer_consequences(completion.consequences)
    }

    pub(super) fn apply_transfer_consequences(
        &mut self,
        consequences: crate::transfer::Consequences,
    ) -> Task<Message> {
        if let Some(error) = consequences.error {
            self.show_error(error);
        } else if let Some(status) = consequences.status {
            self.status = status;
        } else {
            self.refresh_status();
        }
        let tree = if consequences.changed_folders.is_empty() {
            Task::none()
        } else {
            self.invalidate_tree(consequences.changed_folders)
        };
        let refresh = if consequences.refresh {
            self.refresh_selected(consequences.select)
        } else {
            Task::none()
        };
        Task::batch([tree, refresh])
    }

    pub(super) fn selected_entries(&self) -> Vec<FileEntry> {
        self.grid.selected_items(self.navigation.entries())
    }

    pub(super) fn invalidate_tree(&mut self, changed_folders: Vec<PathBuf>) -> Task<Message> {
        let reloads = tree::invalidate_tree_folders(&mut self.explorer.roots, &changed_folders);
        Task::batch(
            reloads
                .into_iter()
                .map(|(id, path)| self.load_tree_node(id, path)),
        )
    }

    pub(super) fn mutations_allowed(&self) -> bool {
        !self.busy
            && !self.transfers.has_conflict()
            && !self.navigation.loading()
            && !self.search.is_recursive()
            && self.navigation.folder_displayed()
    }
}
