use std::path::{Path, PathBuf};

use iced::{
    Point, Task,
    time::Instant,
    widget::{self, Id, scrollable},
};

use crate::{
    fs::{self, FileEntry},
    transfer::{
        Action as TransferAction, Event as TransferEvent, NativeUpdate, Request as TransferRequest,
    },
};

use super::{
    App, DragHoverEffect, DragHoverTarget, DropZone, GRID_SCROLL_ID, Message, NavigationTransition,
    SIDEBAR_SCROLL_ID, TransferBatchUpdate, TransferCancelUpdate, X11_INBOUND_ID, transfer_session,
    view,
};

pub(super) fn transfer_runtime_message(event: transfer_session::RuntimeEvent) -> Message {
    match event {
        transfer_session::RuntimeEvent::BatchFinished { id, outcome } => {
            Message::TransferBatchFinished { id, outcome }
        }
        transfer_session::RuntimeEvent::Noop => Message::Noop,
    }
}

impl App {
    pub(super) fn copy_selection(&mut self) -> Task<Message> {
        let entries = self.selected_entries();
        let Some(change) = self.transfers.copy(&entries) else {
            return Task::none();
        };
        self.presentation.set_status(change.status);
        self.flash_copy_feedback();
        if change.restore_entries {
            self.sync_location_monitoring();
            self.refresh(None)
        } else {
            Task::none()
        }
    }

    pub(super) fn cut_selection(&mut self) -> Task<Message> {
        let entries = self.selected_entries();
        let Some(change) = self.transfers.cut(&entries) else {
            return Task::none();
        };
        self.presentation.set_status(change.status);
        self.navigation.hide_paths(&change.hide_paths);
        self.sync_location_monitoring();
        self.grid.select_only(None, self.navigation.entries().len());
        Task::none()
    }

    pub(super) fn cancel_cut(&mut self, status: &str) -> Task<Message> {
        if !self.transfers.cancel_cut() {
            return Task::none();
        }
        self.sync_location_monitoring();
        self.presentation.set_status(status.to_owned());
        self.refresh(None)
    }

    pub(super) fn paste(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        if !self.transfers.pending_cut_paths().is_empty() {
            return self.paste_current();
        }
        match self.transfers.clipboard_read() {
            None => self.paste_current(),
            Some(Ok(completion)) => Task::perform(completion, Message::ClipboardRead),
            Some(Err(error)) => {
                self.presentation.set_status(error);
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

    pub(super) fn start_external_drag(&mut self) -> Task<Message> {
        let entries = self.transfers.overview().pointer_drag.entries().to_vec();
        if entries.is_empty() {
            return Task::none();
        }
        let Some(preview) = view::View::transfer_preview(self, &entries) else {
            self.transfers.cancel_drag();
            self.grid.cancel_drag_hover();
            return Task::none();
        };
        let copy_only = self.modifiers.control();
        let (count, completion) = match self
            .transfers
            .start_outgoing_active(copy_only, |_| Some(preview))
        {
            Ok(started) => started,
            Err(error) => {
                self.grid.cancel_drag_hover();
                self.presentation
                    .set_status(format!("Could not start external drag-and-drop: {error}"));
                return Task::none();
            }
        };
        self.grid.cancel_drag_hover();
        self.presentation.set_status(if count == 1 {
            "Dragging 1 item outside Waddle…".to_owned()
        } else {
            format!("Dragging {count} items outside Waddle…")
        });
        Task::perform(completion, Message::ExternalDragFinished)
    }

    pub(super) fn quit(&mut self) -> Task<Message> {
        self.transfers.stop();
        iced::exit()
    }

    pub(super) fn drop_destination_at(&self, point: Point, allow_current: bool) -> Option<PathBuf> {
        let row_count = self.sidebar_tree.row_count(self.navigation.current());
        match self.grid.drop_zone(
            point,
            self.navigation.entries().len(),
            row_count,
            self.status_height(),
            allow_current,
        )? {
            DropZone::Sidebar(index) => {
                self.sidebar_tree.row_path(index, self.navigation.current())
            }
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
        let overview = self.transfers.overview();
        overview.pointer_drag.is_active() || overview.native_hover.is_active()
    }

    pub(super) fn update_drag_hover(&mut self, point: Point) {
        if !self.drag_in_progress() {
            self.grid.cancel_drag_hover();
            return;
        }
        let row_count = self.sidebar_tree.row_count(self.navigation.current());
        let target = match self.grid.drop_zone(
            point,
            self.navigation.entries().len(),
            row_count,
            self.status_height(),
            false,
        ) {
            Some(DropZone::Sidebar(index)) => self
                .sidebar_tree
                .row_target(index, self.navigation.current())
                .map(|(id, path)| DragHoverTarget::Sidebar { id, path }),
            Some(DropZone::Entry(index)) => self
                .navigation
                .entries()
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| DragHoverTarget::Folder(entry.path.clone())),
            _ => None,
        };
        let target = target.filter(|target| {
            if !self.transfers.overview().pointer_drag.is_active() {
                return true;
            }
            let path = match target {
                DragHoverTarget::Sidebar { path, .. } | DragHoverTarget::Folder(path) => path,
            };
            self.transfers.can_drop(
                path,
                if self.modifiers.control() {
                    TransferAction::Copy
                } else {
                    TransferAction::Move
                },
            )
        });
        self.grid.set_drag_hover(target, Instant::now());
    }

    pub(super) fn tick_drag_hover(&mut self, now: Instant) -> Task<Message> {
        if !self.drag_in_progress() {
            self.grid.cancel_drag_hover();
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
        match self.grid.tick(now) {
            Some(DragHoverEffect::Expand(id)) => tasks.push(self.expand_tree_row(id)),
            Some(DragHoverEffect::Enter(path)) => tasks
                .push(self.transition_navigation(NavigationTransition::Hover { requested: path })),
            None => {}
        }
        Task::batch(tasks)
    }

    pub(super) fn expand_tree_row(&mut self, id: u64) -> Task<Message> {
        self.sidebar_tree
            .expand(id)
            .map_or_else(Task::none, |request| self.load_tree_node(request))
    }

    pub(super) fn drop_highlight_path(&self) -> Option<PathBuf> {
        let overview = self.transfers.overview();
        if overview.native_hover.is_active() {
            return overview.native_hover.destination().map(Path::to_path_buf);
        }
        overview.pointer_drag.index()?;
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let destination = self.drop_destination_at(self.grid.cursor(), false)?;
        self.transfers
            .can_drop(&destination, action)
            .then_some(destination)
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
            self.grid.cancel_drag_hover();
        }
        let resolved_destination = match &event {
            TransferEvent::Hover { position, .. } => self.drop_destination_at(*position, true),
            _ => None,
        };
        let update = self
            .transfers
            .handle_native(event, move |_, _| resolved_destination.clone());
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
                self.presentation.clear_notice();
                if status.is_empty() {
                    self.refresh_status();
                } else {
                    self.presentation.set_status(status);
                }
                Task::none()
            }
            NativeUpdate::Notice(message) => {
                self.presentation.set_status(message.clone());
                self.presentation.set_notice(message);
                Task::none()
            }
            NativeUpdate::Start(request) => self.start_transfer(request),
            NativeUpdate::Error(error) => {
                self.show_error(error);
                Task::none()
            }
        }
    }

    pub(super) fn finish_x11_drop(&mut self, generation: u64) -> Task<Message> {
        let Some((paths, action)) = self.transfers.take_x11_drop(generation) else {
            return Task::none();
        };
        let destination = self
            .transfers
            .overview()
            .native_hover
            .destination()
            .map(Path::to_path_buf)
            .or_else(|| self.drop_destination_at(self.grid.cursor(), true));
        let Some(destination) = destination else {
            self.presentation
                .set_notice("This is not a valid drop target".to_owned());
            return self.handle_native_dnd_event(TransferEvent::Leave { id: X11_INBOUND_ID });
        };
        self.handle_native_dnd_event(TransferEvent::Drop {
            id: X11_INBOUND_ID,
            paths,
            destination,
            action,
        })
    }

    pub(super) fn start_transfer(&mut self, request: TransferRequest) -> Task<Message> {
        match self.transfers.start(request, &self.operations) {
            Ok(task) => task.map(transfer_runtime_message),
            Err(error) => {
                self.show_error(error);
                Task::none()
            }
        }
    }

    pub(super) fn finish_transfer_batch(
        &mut self,
        id: u64,
        outcome: fs::TransferBatchOutcome,
    ) -> Task<Message> {
        let current = self.navigation.current().to_path_buf();
        let update = self
            .transfers
            .complete_batch(id, outcome, &current, &self.operations);
        self.apply_transfer_batch_update(update)
    }

    pub(super) fn apply_transfer_batch_update(
        &mut self,
        update: TransferBatchUpdate,
    ) -> Task<Message> {
        self.sync_transient_presentation();
        match update {
            TransferBatchUpdate::Completed { outcome, next } => Task::batch([
                self.apply_transfer_completion(*outcome),
                next.map(transfer_runtime_message),
            ]),
            TransferBatchUpdate::Conflict(prompt) => {
                self.presentation.set_status(prompt);
                Task::none()
            }
            TransferBatchUpdate::Ignored => Task::none(),
        }
    }

    pub(super) fn apply_transfer_completion(
        &mut self,
        completion: transfer_session::CompletionOutcome,
    ) -> Task<Message> {
        if let Some(notice) = completion.notice {
            self.presentation.set_notice(notice);
        }
        if completion.sync_location_monitoring {
            self.sync_location_monitoring();
        }
        match completion.undo {
            transfer_session::UndoOutcome::Record { subject, action } => {
                if let Err(error) = self.journal.record(action) {
                    self.presentation.set_notice(format!(
                        "{subject} completed but Undo was not saved: {error}"
                    ));
                }
            }
            transfer_session::UndoOutcome::Unavailable { subject, error } => {
                self.presentation.set_notice(format!(
                    "{} completed but Undo is unavailable: {error}",
                    subject
                ));
            }
            transfer_session::UndoOutcome::None => {}
        }
        if let Some(detail) = completion.detail {
            self.show_command_detail(detail);
        }
        match completion.presentation {
            transfer_session::CompletionPresentation::Status(status) => {
                self.presentation.set_status(status);
            }
            transfer_session::CompletionPresentation::Error(error) => self.show_error(error),
            transfer_session::CompletionPresentation::Refresh => self.refresh_status(),
        }
        let tree = if completion.changed_folders.is_empty() {
            Task::none()
        } else {
            self.invalidate_tree(completion.changed_folders)
        };
        let refresh = match completion.refresh {
            transfer_session::Refresh::None => Task::none(),
            transfer_session::Refresh::Entries(select) => self.refresh_selected(select),
            transfer_session::Refresh::Trash => self.open_trash(),
        };
        Task::batch([tree, refresh])
    }

    pub(super) fn resolve_transfer_conflict(
        &mut self,
        key: char,
        remaining: bool,
    ) -> Task<Message> {
        let task = self
            .transfers
            .resolve_conflict(key, remaining, &self.operations)
            .map(transfer_runtime_message);
        self.sync_transient_presentation();
        task
    }

    pub(super) fn cancel_transfer_conflict(&mut self) -> Task<Message> {
        let current = self.navigation.current().to_path_buf();
        let update = self.transfers.cancel(&current, &self.operations);
        self.sync_transient_presentation();
        match update {
            TransferCancelUpdate::Conflict(update) => self.apply_transfer_batch_update(update),
            TransferCancelUpdate::Active => {
                self.presentation
                    .set_status("Cancelling active transfer…".to_owned());
                Task::none()
            }
            TransferCancelUpdate::None => Task::none(),
        }
    }

    pub(super) fn selected_entries(&self) -> Vec<FileEntry> {
        self.grid.selected_items(self.navigation.entries())
    }

    pub(super) fn invalidate_tree(&mut self, changed_folders: Vec<PathBuf>) -> Task<Message> {
        let reloads = self.sidebar_tree.invalidate(&changed_folders);
        Task::batch(
            reloads
                .into_iter()
                .map(|request| self.load_tree_node(request)),
        )
    }

    pub(super) fn mutations_allowed(&self) -> bool {
        !self.foreground_operation_active()
            && self.transfers.overview().conflict_prompt.is_none()
            && !self.navigation.loading()
            && !self.search.is_recursive()
            && self.navigation.folder_displayed()
    }
}
