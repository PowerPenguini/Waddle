use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use iced::{Point, Subscription, Task};

use crate::{
    fs::{self, FileEntry, TransferBatchOutcome, TransferReport},
    journal,
    transfer::{
        Action, Adapter, AdapterCompletion, ClipboardAdapter, ClipboardCompletion, ClipboardImport,
        Consequences, Event, NativeUpdate, Outcome, Preview, Release, Request, TransferState,
    },
};

mod queue;

use super::{
    native_clipboard,
    operations::{Completion, Kind as OperationKind, Operations},
    trash,
};
use queue::{
    Finished as QueueFinished, Operation as QueueOperation, PreparedUndo, Queue,
    Report as QueueReport, Work,
};
pub(super) use queue::{HistoryEntry, Snapshot, WorkOutcome};

#[derive(Clone, Debug)]
struct ActiveConflict {
    prompt: String,
}

pub(super) enum Refresh {
    None,
    Entries(Vec<PathBuf>),
    Trash,
}

pub(super) enum BatchUpdate {
    Completed {
        outcome: Box<CompletionOutcome>,
        next: Task<RuntimeEvent>,
    },
    Conflict(String),
    Ignored,
}

#[derive(Debug)]
pub(super) enum RuntimeEvent {
    BatchFinished { id: u64, outcome: Box<WorkOutcome> },
    Noop,
}

pub(super) enum WindowFileEvent {
    Hover(PathBuf),
    Leave,
    Drop(PathBuf),
}

pub(super) enum WindowFileUpdate {
    Ignored,
    Hover(Action),
    Leave,
    Drop(u64),
}

pub(super) enum CancelUpdate {
    Conflict(Task<RuntimeEvent>),
    Active,
    None,
}

pub(super) struct TransferSession {
    state: TransferState,
    queue: Queue,
    conflict: Option<ActiveConflict>,
    native: native_clipboard::Platform,
    local_copy: bool,
}

pub(super) enum CompletionPresentation {
    Status(String),
    Warning(String),
    Error(String),
    Refresh,
}

pub(super) enum UndoOutcome {
    Record {
        subject: &'static str,
        action: journal::Action,
    },
    Unavailable {
        subject: &'static str,
        error: String,
    },
    None,
}

pub(super) struct CompletionOutcome {
    pub(super) presentation: CompletionPresentation,
    pub(super) notice: Option<String>,
    pub(super) detail: Option<String>,
    pub(super) undo: UndoOutcome,
    pub(super) changed_folders: Vec<PathBuf>,
    pub(super) refresh: Refresh,
    pub(super) sync_location_monitoring: bool,
    pub(super) trash_failures: Vec<(FileEntry, String)>,
}

pub(super) struct ClipboardChange {
    pub(super) status: String,
    pub(super) hide_paths: Vec<PathBuf>,
    pub(super) restore_entries: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum PointerDrag<'a> {
    Inactive,
    Active {
        index: usize,
        entries: &'a [FileEntry],
    },
}

impl<'a> PointerDrag<'a> {
    pub(super) fn is_active(self) -> bool {
        matches!(self, Self::Active { .. })
    }

    pub(super) fn index(self) -> Option<usize> {
        match self {
            Self::Inactive => None,
            Self::Active { index, .. } => Some(index),
        }
    }

    pub(super) fn entries(self) -> &'a [FileEntry] {
        match self {
            Self::Inactive => &[],
            Self::Active { entries, .. } => entries,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeHover<'a> {
    Inactive,
    NoDestination,
    Destination(&'a Path),
}

impl<'a> NativeHover<'a> {
    pub(super) fn is_active(self) -> bool {
        !matches!(self, Self::Inactive)
    }

    pub(super) fn destination(self) -> Option<&'a Path> {
        match self {
            Self::Destination(path) => Some(path),
            Self::Inactive | Self::NoDestination => None,
        }
    }
}

pub(super) enum DragRelease {
    None,
    Click(usize),
    Transfer(Request),
}

pub(super) struct Overview<'a> {
    pub(super) conflict_prompt: Option<&'a str>,
    pub(super) active: bool,
    pub(super) retry: bool,
    pub(super) expanded: bool,
    pub(super) snapshot: Option<Snapshot>,
    pub(super) active_action: Option<&'static str>,
    pub(super) history: &'a [HistoryEntry],
    pub(super) pointer_drag: PointerDrag<'a>,
    pub(super) native_hover: NativeHover<'a>,
    pub(super) native_active: bool,
}

impl TransferSession {
    pub(super) fn open_default() -> Self {
        Self {
            state: TransferState::default(),
            queue: Queue::open_default(),
            conflict: None,
            native: native_clipboard::Platform::default(),
            local_copy: false,
        }
    }

    #[cfg(test)]
    pub(super) fn open(path: PathBuf) -> Self {
        Self {
            state: TransferState::default(),
            queue: Queue::open(path),
            conflict: None,
            native: native_clipboard::Platform::default(),
            local_copy: false,
        }
    }

    pub(super) fn install_native(
        &mut self,
        result: Result<native_clipboard::Attached, String>,
    ) -> Result<(), String> {
        self.native.install(result)
    }

    pub(super) fn native_subscription(&self) -> Option<Subscription<Event>> {
        self.native.subscription()
    }

    pub(super) fn clipboard_read(&self) -> Option<Result<ClipboardCompletion, String>> {
        self.clipboard_read_with(
            self.native
                .clipboard()
                .map(|source| source as &dyn ClipboardAdapter),
        )
    }

    fn clipboard_read_with(
        &self,
        adapter: Option<&dyn ClipboardAdapter>,
    ) -> Option<Result<ClipboardCompletion, String>> {
        // Our Copy is already available in the Transfer session, even when
        // Wayland has not supplied an offer back to this data device.
        if self.local_copy {
            return None;
        }
        adapter.map(ClipboardAdapter::read_clipboard)
    }

    pub(super) fn handle_window_file(&mut self, event: WindowFileEvent) -> WindowFileUpdate {
        match event {
            WindowFileEvent::Hover(path) => self
                .native
                .hover_x11_file(path)
                .map_or(WindowFileUpdate::Ignored, WindowFileUpdate::Hover),
            WindowFileEvent::Leave if self.native.leave_x11_files() => WindowFileUpdate::Leave,
            WindowFileEvent::Leave => WindowFileUpdate::Ignored,
            WindowFileEvent::Drop(path) => self
                .native
                .drop_x11_file(path)
                .map_or(WindowFileUpdate::Ignored, WindowFileUpdate::Drop),
        }
    }

    pub(super) fn take_x11_drop(&mut self, generation: u64) -> Option<(Vec<PathBuf>, Action)> {
        self.native.take_x11_drop(generation)
    }

    pub(super) fn start(
        &mut self,
        request: Request,
        operations: &Operations,
    ) -> Result<Task<RuntimeEvent>, String> {
        let batch = fs::TransferBatch::try_new(
            request.paths.clone(),
            request.destination.clone(),
            request.action,
        )
        .map_err(|error| error.to_string())?;
        Ok(self
            .queue
            .enqueue_transfer(request, batch)
            .map_or_else(Task::none, |work| launch(work, operations)))
    }

    pub(super) fn restore(
        &mut self,
        entries: Vec<trash::Entry>,
        operations: &Operations,
    ) -> Task<RuntimeEvent> {
        let batch = trash::restore_batch(&entries);
        self.queue
            .enqueue_restore(entries, batch, operations)
            .map_or_else(Task::none, |work| launch(work, operations))
    }

    pub(super) fn trash(
        &mut self,
        entries: Vec<FileEntry>,
        operations: &Operations,
    ) -> Task<RuntimeEvent> {
        let batch = trash::Batch::new(entries.clone());
        self.queue
            .enqueue_trash(entries, batch)
            .map_or_else(Task::none, |work| launch(work, operations))
    }

    pub(super) fn complete_batch(
        &mut self,
        id: u64,
        outcome: WorkOutcome,
        current: &Path,
        operations: &Operations,
    ) -> BatchUpdate {
        let native = std::mem::take(&mut self.native);
        let update = self.complete_batch_with(
            id,
            outcome,
            native.dnd().map(|source| source as &dyn Adapter),
            native
                .clipboard()
                .map(|source| source as &dyn ClipboardAdapter),
            current,
            operations,
        );
        self.native = native;
        update
    }

    fn complete_batch_with(
        &mut self,
        id: u64,
        outcome: WorkOutcome,
        adapter: Option<&dyn Adapter>,
        clipboard_adapter: Option<&dyn ClipboardAdapter>,
        current: &Path,
        operations: &Operations,
    ) -> BatchUpdate {
        match outcome {
            WorkOutcome::Filesystem(TransferBatchOutcome::Complete(report), undo) => {
                let Some(QueueFinished {
                    operation,
                    next,
                    activity: _activity,
                }) = self.queue.finish(id, QueueReport::Filesystem(&report))
                else {
                    return BatchUpdate::Ignored;
                };
                let completed = match operation {
                    QueueOperation::Transfer(request) => self.finish_transfer(
                        adapter,
                        clipboard_adapter,
                        &request,
                        &report,
                        current,
                        undo,
                    ),
                    QueueOperation::Restore(entries) => restore_completion(report, &entries, undo),
                    QueueOperation::Trash(_) => return BatchUpdate::Ignored,
                };
                BatchUpdate::Completed {
                    outcome: Box::new(completed),
                    next: next.map_or_else(Task::none, |work| launch(work, operations)),
                }
            }
            WorkOutcome::Filesystem(TransferBatchOutcome::Conflict { batch, conflict }, _) => {
                let Some(operation) = self.queue.pause_for_conflict(id, *batch) else {
                    return BatchUpdate::Ignored;
                };
                let source = conflict
                    .source
                    .file_name()
                    .map_or_else(|| "item".into(), |name| name.to_string_lossy());
                let kind = if conflict.directories {
                    "folder conflict; Replace merges"
                } else {
                    "file conflict"
                };
                let prefix = match operation {
                    QueueOperation::Transfer(_) => source.into_owned(),
                    QueueOperation::Restore(_) => format!("Restore {source}"),
                    QueueOperation::Trash(_) => return BatchUpdate::Ignored,
                };
                let prompt = format!(
                    "{prefix}: {kind}  •  r Replace  s Skip  k Keep Both  •  uppercase applies to remaining  •  Esc cancel"
                );
                self.conflict = Some(ActiveConflict {
                    prompt: prompt.clone(),
                });
                BatchUpdate::Conflict(prompt)
            }
            WorkOutcome::Trash(report) => {
                let Some(QueueFinished {
                    operation,
                    next,
                    activity: _activity,
                }) = self.queue.finish(id, QueueReport::Trash(&report))
                else {
                    return BatchUpdate::Ignored;
                };
                let QueueOperation::Trash(entries) = operation else {
                    return BatchUpdate::Ignored;
                };
                BatchUpdate::Completed {
                    outcome: Box::new(trash_completion(report, &entries)),
                    next: next.map_or_else(Task::none, |work| launch(work, operations)),
                }
            }
        }
    }

    pub(super) fn resolve_conflict(
        &mut self,
        key: char,
        remaining: bool,
        operations: &Operations,
    ) -> Task<RuntimeEvent> {
        self.resolve_conflict_work(key, remaining)
            .map_or_else(Task::none, |work| launch(work, operations))
    }

    fn resolve_conflict_work(&mut self, key: char, remaining: bool) -> Option<Work> {
        let active = self.conflict.take()?;
        let choice = match key {
            'r' => fs::ConflictChoice::Replace,
            's' => fs::ConflictChoice::Skip,
            'k' => fs::ConflictChoice::KeepBoth,
            _ => {
                self.conflict = Some(active);
                return None;
            }
        };
        match self.queue.resolve_conflict(choice, remaining) {
            Some(work) => Some(work),
            None => {
                self.conflict = Some(active);
                None
            }
        }
    }

    pub(super) fn cancel(&mut self, operations: &Operations) -> CancelUpdate {
        if self.conflict.is_some() {
            let Some(work) = self.cancel_conflict_work() else {
                return CancelUpdate::None;
            };
            return CancelUpdate::Conflict(launch(work, operations));
        }
        if self.queue.cancel() {
            CancelUpdate::Active
        } else {
            CancelUpdate::None
        }
    }

    fn cancel_conflict_work(&mut self) -> Option<Work> {
        let work = self.queue.cancel_conflict()?;
        self.conflict = None;
        Some(work)
    }

    pub(super) fn retry(&mut self, operations: &Operations) -> Result<Task<RuntimeEvent>, String> {
        let work = self.queue.retry(operations)?;
        Ok(work.map_or_else(Task::none, |work| launch(work, operations)))
    }

    pub(super) fn overview(&self) -> Overview<'_> {
        let pointer_drag = self
            .state
            .active_drag_index()
            .map_or(PointerDrag::Inactive, |index| PointerDrag::Active {
                index,
                entries: self.state.active_drag_entries(),
            });
        let native_hover = match self.state.native_hover_destination() {
            None => NativeHover::Inactive,
            Some(None) => NativeHover::NoDestination,
            Some(Some(destination)) => NativeHover::Destination(destination),
        };
        Overview {
            conflict_prompt: self
                .conflict
                .as_ref()
                .map(|conflict| conflict.prompt.as_str()),
            active: self.queue.active(),
            retry: self.queue.has_retry(),
            expanded: self.queue.expanded(),
            snapshot: self.queue.snapshot(),
            active_action: self.queue.active_action(),
            history: self.queue.history(),
            pointer_drag,
            native_hover,
            native_active: self.state.is_native_active(),
        }
    }

    pub(super) fn toggle_expanded(&mut self) {
        self.queue.toggle_expanded();
    }

    pub(super) fn report_text(&self) -> String {
        self.queue.report_text()
    }

    pub(super) fn press(&mut self, index: usize, start: Point, entry_count: usize) {
        self.state.press(index, start, entry_count);
    }

    pub(super) fn move_pointer(
        &mut self,
        position: Point,
        entries: &[FileEntry],
        selected: &BTreeSet<usize>,
    ) -> Option<usize> {
        let activated = self.state.move_pointer(position);
        if activated.is_some() {
            self.state.capture_drag_entries(entries, selected);
        }
        activated
    }

    pub(super) fn release(
        &mut self,
        index: usize,
        destination: Option<PathBuf>,
        action: Action,
    ) -> DragRelease {
        match self.state.release(index) {
            Release::None => DragRelease::None,
            Release::Click(index) => DragRelease::Click(index),
            Release::Drop(_) => {
                let request = destination
                    .and_then(|destination| self.state.request_active(destination, action));
                self.state.cancel_drag();
                request.map_or(DragRelease::None, DragRelease::Transfer)
            }
        }
    }

    pub(super) fn can_drop(&self, destination: &Path, action: Action) -> bool {
        self.state
            .request_active(destination.to_path_buf(), action)
            .is_some()
    }

    pub(super) fn cancel_drag(&mut self) {
        self.state.cancel_drag();
    }

    pub(super) fn copy(&mut self, entries: &[FileEntry]) -> Option<ClipboardChange> {
        let native = std::mem::take(&mut self.native);
        let change = self.copy_with(
            entries,
            native
                .clipboard()
                .map(|source| source as &dyn ClipboardAdapter),
        );
        self.native = native;
        change
    }

    fn copy_with(
        &mut self,
        entries: &[FileEntry],
        adapter: Option<&dyn ClipboardAdapter>,
    ) -> Option<ClipboardChange> {
        let restore_entries = !self.state.pending_cut_paths().is_empty();
        let status = self.state.copy(entries)?;
        self.local_copy = true;
        let status = self.write_clipboard(adapter).map_or(status, |error| {
            format!("Copied inside Waddle; system clipboard failed: {error}")
        });
        Some(ClipboardChange {
            status,
            hide_paths: Vec::new(),
            restore_entries,
        })
    }

    pub(super) fn cut(&mut self, entries: &[FileEntry]) -> Option<ClipboardChange> {
        let native = std::mem::take(&mut self.native);
        let change = self.cut_with(
            entries,
            native
                .clipboard()
                .map(|source| source as &dyn ClipboardAdapter),
        );
        self.native = native;
        change
    }

    fn cut_with(
        &mut self,
        entries: &[FileEntry],
        adapter: Option<&dyn ClipboardAdapter>,
    ) -> Option<ClipboardChange> {
        let status = self.state.cut(entries)?;
        self.local_copy = false;
        let status = self.write_clipboard(adapter).map_or(status, |error| {
            format!("Cut inside Waddle; system clipboard failed: {error}")
        });
        Some(ClipboardChange {
            status,
            hide_paths: self.state.pending_cut_paths().to_vec(),
            restore_entries: false,
        })
    }

    pub(super) fn paste(&self, destination: PathBuf) -> Option<Request> {
        self.state.paste(destination)
    }

    #[cfg(test)]
    pub(super) fn clipboard_payload(&self) -> Option<crate::transfer::ClipboardPayload> {
        self.state.clipboard_payload()
    }

    pub(super) fn pending_cut_paths(&self) -> &[PathBuf] {
        self.state.pending_cut_paths()
    }

    pub(super) fn pending_cut_status(&self) -> Option<String> {
        self.state.pending_cut_status()
    }

    pub(super) fn reconcile_pending_cut(&mut self, removed_paths: &[PathBuf]) -> Option<String> {
        let native = std::mem::take(&mut self.native);
        let notice = self.reconcile_pending_cut_with(
            removed_paths,
            native
                .clipboard()
                .map(|source| source as &dyn ClipboardAdapter),
        );
        self.native = native;
        notice
    }

    fn reconcile_pending_cut_with(
        &mut self,
        removed_paths: &[PathBuf],
        adapter: Option<&dyn ClipboardAdapter>,
    ) -> Option<String> {
        let (generation, removed) = self.state.reconcile_pending_cut(removed_paths)?;
        let remaining = self.state.pending_cut_paths().len();
        let mut notice = if remaining == 0 {
            format!("External move or removal confirmed for {removed} item(s); Cut completed")
        } else {
            format!(
                "External move or removal confirmed for {removed} item(s); {remaining} still pending"
            )
        };
        if let Some(error) = self.sync_cut_clipboard(adapter, generation) {
            notice.push_str(&format!("; system clipboard failed: {error}"));
        }
        Some(notice)
    }

    pub(super) fn cancel_cut(&mut self) -> bool {
        let native = std::mem::take(&mut self.native);
        let cancelled = self.cancel_cut_with(
            native
                .clipboard()
                .map(|source| source as &dyn ClipboardAdapter),
        );
        self.native = native;
        cancelled
    }

    fn cancel_cut_with(&mut self, adapter: Option<&dyn ClipboardAdapter>) -> bool {
        let Some(generation) = self.state.cancel_cut() else {
            return false;
        };
        if let Some(adapter) = adapter {
            adapter.clear_clipboard(generation);
        }
        true
    }

    pub(super) fn paste_import(
        &mut self,
        import: ClipboardImport,
        destination: PathBuf,
    ) -> Option<Request> {
        if !self.state.import_clipboard(import) {
            return None;
        }
        self.local_copy = false;
        self.state.paste(destination)
    }

    pub(super) fn start_outgoing_active<P>(
        &mut self,
        copy_only: bool,
        preview: P,
    ) -> Result<(usize, AdapterCompletion), String>
    where
        P: FnOnce(&[FileEntry]) -> Option<Preview>,
    {
        let Some(adapter) = self.native.dnd() else {
            self.state.cancel_drag();
            return Err(self
                .native
                .dnd_error()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    "External drag-and-drop is not ready yet; try again in a moment".to_owned()
                }));
        };
        let started = self
            .state
            .start_outgoing_active(adapter, copy_only, preview);
        if started.is_err() {
            self.state.cancel_drag();
        }
        started
    }

    pub(super) fn finish_outgoing(&mut self, result: Result<Outcome, String>) -> CompletionOutcome {
        completion_from_consequences(self.state.finish_outgoing(result), Ok(None), None, false)
    }

    pub(super) fn handle_native<F>(&mut self, event: Event, destination_at: F) -> NativeUpdate
    where
        F: FnMut(Point, bool) -> Option<PathBuf>,
    {
        if matches!(event, Event::ClipboardOwnershipLost) {
            // A newer system selection supersedes our Copy. Keep pending Cut
            // entries intact: they have a separate, explicit cancellation.
            self.local_copy = false;
            return NativeUpdate::None;
        }
        let native = std::mem::take(&mut self.native);
        let Some(adapter) = native.dnd() else {
            self.native = native;
            return NativeUpdate::Error("Drag-and-drop adapter is unavailable".to_owned());
        };
        let update = self.handle_native_with(adapter, event, destination_at);
        self.native = native;
        update
    }

    fn handle_native_with<A, F>(
        &mut self,
        adapter: &A,
        event: Event,
        destination_at: F,
    ) -> NativeUpdate
    where
        A: Adapter,
        F: FnMut(Point, bool) -> Option<PathBuf>,
    {
        self.state.handle_native(adapter, event, destination_at)
    }

    fn finish_transfer(
        &mut self,
        adapter: Option<&dyn Adapter>,
        clipboard_adapter: Option<&dyn ClipboardAdapter>,
        request: &Request,
        report: &TransferReport,
        current: &Path,
        journal_action: PreparedUndo,
    ) -> CompletionOutcome {
        let clipboard_generation = request
            .clipboard_generation
            .filter(|_| request.action == Action::Move);
        let consequences = self
            .state
            .finish_transfer(adapter, request, report, current);
        let notice = clipboard_generation
            .and_then(|generation| self.sync_cut_clipboard(clipboard_adapter, generation))
            .map(|error| {
                format!("Cut was updated inside Waddle; system clipboard failed: {error}")
            });
        completion_from_consequences(consequences, journal_action, notice, true)
    }

    pub(super) fn stop(&mut self) {
        if let Some(adapter) = self.native.take_dnd() {
            self.state.stop(&adapter);
        } else {
            self.state.cancel_drag();
        }
    }

    fn write_clipboard(&self, adapter: Option<&dyn ClipboardAdapter>) -> Option<String> {
        let adapter = adapter?;
        self.state
            .clipboard_payload()
            .and_then(|payload| adapter.write_clipboard(payload).err())
    }

    fn sync_cut_clipboard(
        &self,
        adapter: Option<&dyn ClipboardAdapter>,
        generation: u64,
    ) -> Option<String> {
        let adapter = adapter?;
        if let Some(payload) = self
            .state
            .clipboard_payload()
            .filter(|payload| payload.generation == generation)
        {
            adapter.write_clipboard(payload).err()
        } else {
            adapter.clear_clipboard(generation);
            None
        }
    }
}

fn launch(work: Work, operations: &Operations) -> Task<RuntimeEvent> {
    let id = work.id();
    Task::perform(
        operations.run(OperationKind::Mutation, move |_| {
            Ok::<_, String>(work.run())
        }),
        move |completion| {
            let outcome = match completion {
                Completion::Finished(Ok(outcome)) => outcome,
                Completion::Finished(Err(error)) => WorkOutcome::Filesystem(
                    TransferBatchOutcome::Complete(TransferReport {
                        retry: Vec::new(),
                        completed: Vec::new(),
                        failures: vec![fs::TransferFailure {
                            source: PathBuf::new(),
                            error,
                        }],
                        retained: Vec::new(),
                        warnings: Vec::new(),
                        receipts: Vec::new(),
                        cancelled: false,
                    }),
                    Err("Transfer worker failed before preparing Undo".to_owned()),
                ),
                Completion::Cancelled => return RuntimeEvent::Noop,
            };
            RuntimeEvent::BatchFinished {
                id,
                outcome: Box::new(outcome),
            }
        },
    )
}

fn trash_completion(report: trash::Report, entries: &[FileEntry]) -> CompletionOutcome {
    let completed = report.receipts.len();
    let failed = report.failures.len();
    let retained = report.retained.len();
    let status = if report.cancelled {
        format!("Trash cancelled  •  {completed} moved  •  {failed} failed  •  {retained} retained")
    } else {
        format!("Moved {completed} to Trash  •  {failed} failed")
    };
    let changed_folders = report
        .receipts
        .iter()
        .filter_map(|receipt| receipt.original.parent().map(Path::to_path_buf))
        .collect::<Vec<_>>();
    let refresh = if completed == 0 {
        Refresh::None
    } else {
        Refresh::Entries(Vec::new())
    };
    let failed_paths = report
        .failures
        .iter()
        .map(|(entry, _)| entry.path.as_path())
        .collect::<BTreeSet<_>>();
    let trash_failures = entries
        .iter()
        .filter(|entry| failed_paths.contains(entry.path.as_path()))
        .filter_map(|entry| {
            report
                .failures
                .iter()
                .find(|(failed, _)| failed.path == entry.path)
                .map(|(_, error)| (entry.clone(), error.clone()))
        })
        .collect();
    CompletionOutcome {
        presentation: CompletionPresentation::Status(status),
        notice: None,
        detail: None,
        undo: undo_outcome(report.undo, "Trash"),
        changed_folders,
        refresh,
        sync_location_monitoring: true,
        trash_failures,
    }
}

fn restore_completion(
    report: TransferReport,
    entries: &[trash::Entry],
    journal_action: PreparedUndo,
) -> CompletionOutcome {
    let report = trash::finish_restore(report, entries);
    let mut detail = report
        .failures
        .iter()
        .map(|(path, error)| format!("{}: {error}", path.display()))
        .chain(report.warnings.iter().cloned())
        .collect::<Vec<_>>();
    if report.retained > 0 {
        detail.push(format!("{} items stayed in Trash", report.retained));
    }
    let status = format!(
        "Restored {}  •  {} failed  •  {} kept",
        report.restored.len(),
        report.failures.len(),
        report.retained
    );
    let changed_folders = report
        .restored
        .iter()
        .filter_map(|receipt| receipt.original.parent().map(Path::to_path_buf))
        .collect();
    CompletionOutcome {
        presentation: CompletionPresentation::Status(status),
        notice: None,
        detail: (!detail.is_empty()).then(|| detail.join("\n")),
        undo: undo_outcome(journal_action, "Restore"),
        changed_folders,
        refresh: Refresh::Trash,
        sync_location_monitoring: false,
        trash_failures: Vec::new(),
    }
}

fn completion_from_consequences(
    consequences: Consequences,
    journal_action: PreparedUndo,
    notice: Option<String>,
    sync_location_monitoring: bool,
) -> CompletionOutcome {
    CompletionOutcome {
        presentation: match (
            consequences.error,
            consequences.warning,
            consequences.status,
        ) {
            (Some(error), _, _) => CompletionPresentation::Error(error),
            (None, Some(warning), _) => CompletionPresentation::Warning(warning),
            (None, None, Some(status)) => CompletionPresentation::Status(status),
            (None, None, None) => CompletionPresentation::Refresh,
        },
        notice,
        detail: None,
        undo: undo_outcome(journal_action, "Transfer"),
        changed_folders: consequences.changed_folders,
        refresh: if consequences.refresh {
            Refresh::Entries(consequences.select)
        } else {
            Refresh::None
        },
        sync_location_monitoring,
        trash_failures: Vec::new(),
    }
}

fn undo_outcome<E: ToString>(
    action: Result<Option<journal::Action>, E>,
    subject: &'static str,
) -> UndoOutcome {
    match action {
        Ok(Some(action)) => UndoOutcome::Record { subject, action },
        Err(error) => UndoOutcome::Unavailable {
            subject,
            error: error.to_string(),
        },
        Ok(None) => UndoOutcome::None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        sync::{Arc, Mutex},
    };

    use crate::transfer::{ClipboardCompletion, ClipboardPayload};

    use super::*;

    fn entry(path: PathBuf) -> FileEntry {
        FileEntry {
            name: path
                .file_name()
                .map_or_else(OsString::new, |name| name.to_os_string()),
            path,
            directory: false,
            metadata: Default::default(),
        }
    }

    fn restore_entry(root: &Path, name: &str) -> trash::Entry {
        let trashed = root.join("Trash/files").join(name);
        let info = root.join("Trash/info").join(format!("{name}.trashinfo"));
        let original = root.join("restored").join(name);
        std::fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        std::fs::create_dir_all(info.parent().unwrap()).unwrap();
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::write(&trashed, "content").unwrap();
        std::fs::write(&info, "[Trash Info]").unwrap();
        trash::Entry {
            file: entry(trashed.clone()),
            receipt: journal::TrashReceipt {
                original,
                trashed,
                info,
            },
        }
    }

    #[derive(Clone, Default)]
    struct MemoryClipboard {
        writes: Arc<Mutex<Vec<ClipboardPayload>>>,
        clears: Arc<Mutex<Vec<u64>>>,
        import: Option<ClipboardImport>,
    }

    impl ClipboardAdapter for MemoryClipboard {
        fn write_clipboard(&self, payload: ClipboardPayload) -> Result<(), String> {
            self.writes.lock().unwrap().push(payload);
            Ok(())
        }

        fn read_clipboard(&self) -> Result<ClipboardCompletion, String> {
            let import = self.import.clone().ok_or("unused test read")?;
            Ok(Box::pin(async move { Ok(import) }))
        }

        fn clear_clipboard(&self, generation: u64) {
            self.clears.lock().unwrap().push(generation);
        }
    }

    struct UnavailableClipboard;

    impl ClipboardAdapter for UnavailableClipboard {
        fn write_clipboard(&self, _: ClipboardPayload) -> Result<(), String> {
            Ok(())
        }

        fn read_clipboard(&self) -> Result<ClipboardCompletion, String> {
            Ok(Box::pin(async {
                Err("the Wayland clipboard has no file offer".to_owned())
            }))
        }

        fn clear_clipboard(&self, _: u64) {}
    }

    #[test]
    fn copied_downloads_paste_without_a_wayland_file_offer() {
        runtime().block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let downloads = temp.path().join("Downloads");
            let destination = temp.path().join("pendrive");
            std::fs::create_dir_all(&downloads).unwrap();
            std::fs::create_dir_all(&destination).unwrap();
            let source = downloads.join("notes.txt");
            std::fs::write(&source, "copy me").unwrap();
            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            session
                .copy_with(&[entry(source.clone())], Some(&UnavailableClipboard))
                .unwrap();

            // The same clipboard decision and Transfer path used by App::paste.
            let request = match session.clipboard_read_with(Some(&UnavailableClipboard)) {
                None => session.paste(destination.clone()),
                Some(result) => {
                    let import = result
                        .unwrap()
                        .await
                        .expect("Waddle's own Copy must remain pasteable");
                    session.paste_import(import, destination.clone())
                }
            }
            .unwrap();
            let operations = Operations::default();
            let task = session.start(request, &operations).unwrap();
            assert!(matches!(
                run_task(&mut session, task, &destination, &operations).await,
                BatchUpdate::Completed { .. }
            ));
            assert_eq!(
                std::fs::read_to_string(destination.join("notes.txt")).unwrap(),
                "copy me"
            );
            assert_eq!(std::fs::read_to_string(source).unwrap(), "copy me");
        });
    }

    #[test]
    fn external_clipboard_replaces_waddles_copy_after_ownership_loss() {
        runtime().block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            let external = temp.path().join("external.txt");
            let clipboard = MemoryClipboard {
                import: Some(ClipboardImport {
                    paths: vec![external.clone()],
                    action: Action::Copy,
                    generation: None,
                }),
                ..MemoryClipboard::default()
            };
            session
                .copy_with(&[entry(temp.path().join("old.txt"))], Some(&clipboard))
                .unwrap();
            session.handle_native(Event::ClipboardOwnershipLost, |_, _| None);
            let import = session
                .clipboard_read_with(Some(&clipboard))
                .expect("Paste must consult the new clipboard owner")
                .unwrap()
                .await
                .unwrap();
            let request = session
                .paste_import(import, temp.path().join("pendrive"))
                .unwrap();
            assert_eq!(request.paths, [external]);

            // A later non-file clipboard must report its error, not resurrect
            // either the old local Copy or the previously imported files.
            let result = session
                .clipboard_read_with(Some(&UnavailableClipboard))
                .expect("imported files do not own the clipboard")
                .unwrap()
                .await;
            assert_eq!(
                result.unwrap_err(),
                "the Wayland clipboard has no file offer"
            );
        });
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    async fn run_task(
        session: &mut TransferSession,
        task: Task<RuntimeEvent>,
        current: &Path,
        operations: &Operations,
    ) -> BatchUpdate {
        use iced::futures::StreamExt;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut stream =
                iced_runtime::task::into_stream(task).expect("work should be scheduled");
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(RuntimeEvent::BatchFinished { id, outcome }) =
                    action
                {
                    return session.complete_batch(id, *outcome, current, operations);
                }
            }
            panic!("work should produce a completion");
        })
        .await
        .expect("Transfer should settle")
    }

    #[test]
    fn queued_restores_keep_foreground_activity_until_every_restore_completes() {
        runtime().block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let first = restore_entry(temp.path(), "first");
            let second = restore_entry(temp.path(), "second");
            let operations = Operations::default();
            let mut session = TransferSession::open(temp.path().join("history.json"));
            let task = session.restore(vec![first.clone()], &operations);
            let queued = session.restore(vec![second.clone()], &operations);
            assert!(iced_runtime::task::into_stream(queued).is_none());
            assert!(operations.foreground_active());
            let BatchUpdate::Completed { next, .. } =
                run_task(&mut session, task, temp.path(), &operations).await
            else {
                panic!("first Restore should complete");
            };
            assert!(
                operations.foreground_active(),
                "the second Restore still owns foreground activity"
            );
            assert!(first.receipt.original.exists());
            assert!(!second.receipt.original.exists());
            assert!(matches!(
                run_task(&mut session, next, temp.path(), &operations).await,
                BatchUpdate::Completed { .. }
            ));
            assert!(!operations.foreground_active());
            assert!(second.receipt.original.exists());
            assert!(session.overview().history.is_empty());
        });
    }

    #[test]
    fn restore_work_retains_foreground_activity_after_session_is_dropped() {
        use iced::futures::StreamExt;
        runtime().block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let restore = restore_entry(temp.path(), "item");
            let operations = Operations::default();
            let mut session = TransferSession::open(temp.path().join("history.json"));
            let task = session.restore(vec![restore.clone()], &operations);
            drop(session);
            assert!(
                operations.foreground_active(),
                "submitted work outlives its session"
            );
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut stream = iced_runtime::task::into_stream(task).unwrap();
                let mut completed = false;
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(RuntimeEvent::BatchFinished {
                        outcome,
                        ..
                    }) = action
                    {
                        assert!(matches!(
                            *outcome,
                            WorkOutcome::Filesystem(TransferBatchOutcome::Complete(_), _)
                        ));
                        completed = true;
                    }
                }
                assert!(completed);
            })
            .await
            .expect("submitted Restore should finish");
            assert!(!operations.foreground_active());
            assert!(restore.receipt.original.exists());
            assert!(!restore.receipt.trashed.exists());
        });
    }

    #[test]
    fn drag_activation_captures_sources_and_release_consumes_the_drag() {
        let temp = tempfile::tempdir().unwrap();
        let entries = [
            entry(PathBuf::from("/source/one")),
            entry(PathBuf::from("/source/two")),
        ];
        let selected = BTreeSet::from([0, 1]);
        let mut session = TransferSession::open(temp.path().join("transfers.json"));

        session.press(0, Point::ORIGIN, entries.len());
        assert_eq!(
            session.move_pointer(Point::new(7.0, 0.0), &entries, &selected),
            Some(0)
        );
        assert_eq!(session.overview().pointer_drag.entries().len(), 2);

        let DragRelease::Transfer(request) =
            session.release(0, Some(PathBuf::from("/destination")), Action::Move)
        else {
            panic!("valid release should produce a transfer request");
        };
        assert_eq!(request.paths.len(), 2);
        assert!(!session.overview().pointer_drag.is_active());
        assert!(matches!(
            session.release(0, Some(PathBuf::from("/destination")), Action::Move),
            DragRelease::None
        ));
    }

    #[test]
    fn invalid_drag_release_is_consumed_without_a_request() {
        let temp = tempfile::tempdir().unwrap();
        let entries = [entry(PathBuf::from("/source/folder"))];
        let mut session = TransferSession::open(temp.path().join("transfers.json"));

        session.press(0, Point::ORIGIN, entries.len());
        session.move_pointer(Point::new(7.0, 0.0), &entries, &BTreeSet::new());

        assert!(matches!(
            session.release(0, Some(PathBuf::from("/source/folder/child")), Action::Move,),
            DragRelease::None
        ));
        assert!(!session.overview().pointer_drag.is_active());
    }

    #[test]
    fn transfer_session_owns_cut_reconciliation_and_native_clipboard_updates() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.txt");
        let second = temp.path().join("second.txt");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        let clipboard = MemoryClipboard::default();
        let mut session = TransferSession::open(temp.path().join("transfers.json"));

        let change = session
            .cut_with(
                &[entry(first.clone()), entry(second.clone())],
                Some(&clipboard),
            )
            .unwrap();
        assert_eq!(change.hide_paths, [first.clone(), second.clone()]);
        assert_eq!(clipboard.writes.lock().unwrap().len(), 1);

        std::fs::remove_file(&first).unwrap();
        let notice = session
            .reconcile_pending_cut_with(std::slice::from_ref(&first), Some(&clipboard))
            .unwrap();
        assert!(notice.contains("1 still pending"));
        assert_eq!(
            clipboard
                .writes
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .paths
                .as_slice(),
            std::slice::from_ref(&second)
        );

        std::fs::remove_file(&second).unwrap();
        let notice = session
            .reconcile_pending_cut_with(std::slice::from_ref(&second), Some(&clipboard))
            .unwrap();
        assert!(notice.contains("Cut completed"));
        assert_eq!(clipboard.clears.lock().unwrap().len(), 1);
        assert!(session.pending_cut_paths().is_empty());
    }

    #[test]
    fn transfer_session_resolves_the_conflict_continuation_owned_by_the_queue() {
        runtime().block_on(async {
            let operations = Operations::default();
            let temp = tempfile::tempdir().unwrap();
            let source_directory = temp.path().join("source");
            let destination = temp.path().join("destination");
            std::fs::create_dir_all(&source_directory).unwrap();
            std::fs::create_dir_all(&destination).unwrap();
            let source = source_directory.join("notes.txt");
            std::fs::write(&source, "source").unwrap();
            std::fs::write(destination.join("notes.txt"), "existing").unwrap();

            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            session.copy(&[entry(source)]).unwrap();
            let request = session.paste(destination).unwrap();
            let task = session.start(request, &operations).unwrap();

            assert!(matches!(
                run_task(&mut session, task, temp.path(), &operations).await,
                BatchUpdate::Conflict(_)
            ));
            assert!(session.overview().conflict_prompt.is_some());
            assert!(
                session
                    .overview()
                    .conflict_prompt
                    .unwrap()
                    .contains("r Replace")
            );

            let task = session.resolve_conflict('s', false, &operations);
            assert!(matches!(
                run_task(&mut session, task, temp.path(), &operations).await,
                BatchUpdate::Completed { .. }
            ));
            assert!(session.overview().conflict_prompt.is_none());
            assert!(!session.overview().active);
        });
    }

    #[test]
    fn trash_completion_uses_transfer_feedback_history_and_retry() {
        runtime().block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let first = entry(temp.path().join("missing-one"));
            let second = entry(temp.path().join("missing-two"));
            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            let operations = Operations::default();
            let task = session.trash(vec![first.clone(), second.clone()], &operations);
            let report = trash::Report {
                receipts: vec![journal::TrashReceipt {
                    original: first.path.clone(),
                    trashed: PathBuf::from("/trash/one"),
                    info: PathBuf::from("/trash/info/one.trashinfo"),
                }],
                failures: vec![(second.clone(), "Trash unavailable".to_owned())],
                retained: Vec::new(),
                cancelled: false,
                undo: Ok(None),
            };

            // Inject the mixed desktop result at the same completion input used by production.
            // The real batch only examines missing fixture paths, so it cannot touch user Trash.
            let task = task.map(move |event| match event {
                RuntimeEvent::BatchFinished { id, .. } => RuntimeEvent::BatchFinished {
                    id,
                    outcome: Box::new(WorkOutcome::Trash(report.clone())),
                },
                RuntimeEvent::Noop => RuntimeEvent::Noop,
            });
            let BatchUpdate::Completed { outcome, .. } =
                run_task(&mut session, task, temp.path(), &operations).await
            else {
                panic!("Trash should complete through the Transfer session");
            };
            let completed = *outcome;

            assert!(matches!(
                completed.presentation,
                CompletionPresentation::Status(ref status)
                    if status == "Moved 1 to Trash  •  1 failed"
            ));
            assert_eq!(completed.trash_failures.len(), 1);
            assert_eq!(completed.trash_failures[0].0.path, second.path);
            assert!(matches!(completed.refresh, Refresh::Entries(_)));
            assert!(!session.overview().active);
            assert!(session.overview().retry);
            assert_eq!(session.overview().history.len(), 1);
        });
    }

    #[test]
    fn restore_uses_transfer_conflicts_without_entering_transfer_history() {
        runtime().block_on(async {
            let operations = Operations::default();
            let temp = tempfile::tempdir().unwrap();
            let restore = restore_entry(temp.path(), "notes.txt");
            std::fs::write(&restore.receipt.original, "existing").unwrap();
            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            let task = session.restore(vec![restore.clone()], &operations);

            assert!(matches!(
                run_task(&mut session, task, temp.path(), &operations).await,
                BatchUpdate::Conflict(_)
            ));
            assert!(
                session
                    .overview()
                    .conflict_prompt
                    .unwrap()
                    .starts_with("Restore notes.txt:")
            );
            assert!(session.overview().active);
            assert!(operations.foreground_active());
            assert!(session.overview().history.is_empty());

            let CancelUpdate::Conflict(cancelled) = session.cancel(&operations) else {
                panic!("cancel should resume the paused Restore");
            };
            let BatchUpdate::Completed { outcome, .. } =
                run_task(&mut session, cancelled, temp.path(), &operations).await
            else {
                panic!("restore conflict should complete through Transfer session");
            };
            let completed = *outcome;
            assert!(matches!(
                completed.presentation,
                CompletionPresentation::Status(ref status)
                    if status == "Restored 0  •  0 failed  •  1 kept"
            ));
            assert!(matches!(completed.refresh, Refresh::Trash));
            assert!(restore.receipt.trashed.exists());
            assert!(restore.receipt.info.exists());
            assert!(session.overview().history.is_empty());
            assert!(!operations.foreground_active());
        });
    }

    #[test]
    fn restore_completion_cleans_metadata_and_prepares_undo() {
        runtime().block_on(async {
            let operations = Operations::default();
            let temp = tempfile::tempdir().unwrap();
            let restore = restore_entry(temp.path(), "notes.txt");
            let changed = restore.receipt.original.parent().unwrap().to_path_buf();
            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            let task = session.restore(vec![restore.clone()], &operations);
            let BatchUpdate::Completed { outcome, .. } =
                run_task(&mut session, task, temp.path(), &operations).await
            else {
                panic!("restore should complete");
            };
            let completed = *outcome;

            assert!(matches!(
                completed.presentation,
                CompletionPresentation::Status(ref status)
                    if status == "Restored 1  •  0 failed  •  0 kept"
            ));
            assert_eq!(completed.changed_folders, [changed]);
            assert!(matches!(completed.undo, UndoOutcome::Record { .. }));
            assert!(matches!(completed.refresh, Refresh::Trash));
            assert!(!restore.receipt.trashed.exists());
            assert!(!restore.receipt.info.exists());
            assert!(restore.receipt.original.exists());
            assert!(session.overview().history.is_empty());
            assert!(!session.overview().retry);
        });
    }

    #[test]
    fn restore_retry_stays_foreground_until_completion_and_keeps_undo() {
        use iced::futures::StreamExt;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let restore = restore_entry(temp.path(), "item");
            let parent = restore.receipt.original.parent().unwrap();
            std::fs::remove_dir(parent).unwrap();
            let mut session = TransferSession::open(temp.path().join("history.json"));
            let operations = Operations::default();
            let first = session.restore(vec![restore.clone()], &operations);
            assert!(matches!(
                run_task(&mut session, first, temp.path(), &operations).await,
                BatchUpdate::Completed { .. }
            ));
            assert!(session.overview().retry);
            assert!(!operations.foreground_active());
            assert!(
                session.retry(&operations).is_err(),
                "an unrepaired destination must leave Retry available"
            );
            assert!(session.overview().retry);
            assert!(!operations.foreground_active());
            std::fs::create_dir(parent).unwrap();
            let retry = session.retry(&operations).unwrap();
            assert!(operations.foreground_active());
            let mut stream = iced_runtime::task::into_stream(retry).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(RuntimeEvent::BatchFinished {
                        id,
                        outcome,
                    }) = action
                    {
                        assert!(operations.foreground_active());
                        let BatchUpdate::Completed { outcome, .. } =
                            session.complete_batch(id, *outcome, temp.path(), &operations)
                        else {
                            panic!("repaired Restore must complete");
                        };
                        assert!(matches!(outcome.undo, UndoOutcome::Record { .. }));
                    }
                }
            })
            .await
            .expect("Restore retry must settle");
            assert!(!operations.foreground_active());
            assert!(!session.overview().retry);
            assert_eq!(
                std::fs::read_to_string(&restore.receipt.original).unwrap(),
                "content"
            );
            assert!(!restore.receipt.trashed.exists());
            assert!(!restore.receipt.info.exists());
        });
    }

    #[test]
    fn restore_queues_behind_an_active_transfer_in_submission_order() {
        runtime().block_on(async {
            let operations = Operations::default();
            let temp = tempfile::tempdir().unwrap();
            let source_directory = temp.path().join("source");
            let destination = temp.path().join("destination");
            std::fs::create_dir_all(&source_directory).unwrap();
            std::fs::create_dir_all(&destination).unwrap();
            let source = source_directory.join("copied.txt");
            std::fs::write(&source, "source").unwrap();
            let restore = restore_entry(temp.path(), "restored.txt");

            let mut session = TransferSession::open(temp.path().join("transfers.json"));
            session.copy(&[entry(source)]).unwrap();
            let request = session.paste(destination).unwrap();
            let transfer = session.start(request, &operations).unwrap();
            let queued = session.restore(vec![restore.clone()], &operations);
            assert!(iced_runtime::task::into_stream(queued).is_none());

            let BatchUpdate::Completed { next, .. } =
                run_task(&mut session, transfer, temp.path(), &operations).await
            else {
                panic!("Copy should complete");
            };
            assert_eq!(session.overview().active_action, Some("Restoring"));
            assert_eq!(session.overview().history.len(), 1);
            assert!(operations.foreground_active());
            assert!(matches!(
                run_task(&mut session, next, temp.path(), &operations).await,
                BatchUpdate::Completed { .. }
            ));
            assert!(restore.receipt.original.exists());
            assert!(!operations.foreground_active());
            assert_eq!(session.overview().history.len(), 1);
        });
    }
}
