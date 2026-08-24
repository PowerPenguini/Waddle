use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use iced::Point;

use crate::{
    fs::{self, FileEntry, TransferBatchOutcome, TransferReport},
    journal,
    transfer::{
        Action, Adapter, AdapterCompletion, ClipboardImport, ClipboardPayload, Consequences, Event,
        NativeUpdate, Outcome, Preview, Release, Request, TransferState,
    },
};

pub(super) use super::transfer_queue::{HistoryEntry, Snapshot, Work};
use super::{
    transfer_queue::{Finished as QueueFinished, Operation as QueueOperation, Queue},
    trash,
};

#[derive(Clone, Debug)]
struct ActiveConflict {
    batch: fs::TransferBatch,
    prompt: String,
}

pub(super) struct CompletedTransfer {
    pub(super) request: Request,
    pub(super) report: TransferReport,
    pub(super) next: Option<Work>,
}

pub(super) struct CompletedRestore {
    pub(super) journal_action: Result<Option<journal::Action>, String>,
    pub(super) status: String,
    pub(super) detail: Option<String>,
    pub(super) changed_folders: Vec<PathBuf>,
    pub(super) next: Option<Work>,
}

pub(super) enum CompletedBatch {
    Transfer(CompletedTransfer),
    Restore(CompletedRestore),
}

pub(super) enum BatchUpdate {
    Completed(Box<CompletedBatch>),
    Conflict,
    Ignored,
}

pub(super) enum CancelUpdate {
    Conflict(BatchUpdate),
    Active,
    None,
}

pub(super) struct TransferSession {
    state: TransferState,
    queue: Queue,
    conflict: Option<ActiveConflict>,
}

pub(super) struct TransferCompletion {
    pub(super) consequences: Consequences,
    pub(super) journal_action: Result<Option<journal::Action>, String>,
    pub(super) clipboard_generation: Option<u64>,
}

impl TransferSession {
    pub(super) fn open_default() -> Self {
        Self {
            state: TransferState::default(),
            queue: Queue::open_default(),
            conflict: None,
        }
    }

    #[cfg(test)]
    fn open(path: PathBuf) -> Self {
        Self {
            state: TransferState::default(),
            queue: Queue::open(path),
            conflict: None,
        }
    }

    pub(super) fn enqueue(&mut self, request: Request) -> Result<Option<Work>, String> {
        let batch = fs::TransferBatch::try_new(
            request.paths.clone(),
            request.destination.clone(),
            request.action,
        )
        .map_err(|error| error.to_string())?;
        Ok(self.queue.enqueue_transfer(request, batch))
    }

    pub(super) fn enqueue_restore(&mut self, entries: Vec<trash::Entry>) -> Option<Work> {
        let batch = trash::restore_batch(&entries);
        self.queue.enqueue_restore(entries, batch)
    }

    pub(super) fn complete_batch(&mut self, id: u64, outcome: TransferBatchOutcome) -> BatchUpdate {
        match outcome {
            TransferBatchOutcome::Complete(report) => {
                let Some(QueueFinished { operation, next }) = self.queue.finish(id, &report) else {
                    return BatchUpdate::Ignored;
                };
                let completed = match operation {
                    QueueOperation::Transfer(request) => {
                        CompletedBatch::Transfer(CompletedTransfer {
                            request,
                            report,
                            next,
                        })
                    }
                    QueueOperation::Restore(entries) => {
                        CompletedBatch::Restore(restore_completion(report, &entries, next))
                    }
                };
                BatchUpdate::Completed(Box::new(completed))
            }
            TransferBatchOutcome::Conflict { batch, conflict } => {
                let Some(operation) = self.queue.active_operation(id) else {
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
                };
                let prompt = format!(
                    "{prefix}: {kind}  •  r Replace  s Skip  k Keep Both  •  uppercase applies to remaining  •  Esc cancel"
                );
                self.conflict = Some(ActiveConflict {
                    batch: *batch,
                    prompt,
                });
                BatchUpdate::Conflict
            }
        }
    }

    pub(super) fn resolve_conflict(&mut self, key: char, remaining: bool) -> Option<Work> {
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
        self.queue.resume(active.batch.resolve(choice, remaining))
    }

    pub(super) fn cancel(&mut self) -> CancelUpdate {
        if let Some(active) = self.conflict.take() {
            let Some(id) = self.queue.active_id() else {
                return CancelUpdate::None;
            };
            return CancelUpdate::Conflict(
                self.complete_batch(id, TransferBatchOutcome::Complete(active.batch.cancel())),
            );
        }
        if self.queue.cancel() {
            CancelUpdate::Active
        } else {
            CancelUpdate::None
        }
    }

    pub(super) fn retry(&mut self) -> Result<Option<Work>, String> {
        self.queue.retry()
    }

    pub(super) fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }

    pub(super) fn conflict_prompt(&self) -> Option<&str> {
        self.conflict
            .as_ref()
            .map(|conflict| conflict.prompt.as_str())
    }

    pub(super) fn active(&self) -> bool {
        self.queue.transfer_active()
    }

    pub(super) fn has_retry(&self) -> bool {
        self.queue.has_retry()
    }

    pub(super) fn toggle_expanded(&mut self) {
        self.queue.toggle_expanded();
    }

    pub(super) fn expanded(&self) -> bool {
        self.queue.expanded()
    }

    pub(super) fn report_text(&self) -> String {
        self.queue.report_text()
    }

    pub(super) fn history(&self) -> &[HistoryEntry] {
        self.queue.history()
    }

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        self.queue.snapshot()
    }

    pub(super) fn active_action(&self) -> Option<&'static str> {
        self.queue.active_action()
    }

    pub(super) fn press(&mut self, index: usize, start: Point, entry_count: usize) {
        self.state.press(index, start, entry_count);
    }

    pub(super) fn move_pointer(&mut self, position: Point) -> Option<usize> {
        self.state.move_pointer(position)
    }

    pub(super) fn release(&mut self, index: usize) -> Release {
        self.state.release(index)
    }

    pub(super) fn active_drag_index(&self) -> Option<usize> {
        self.state.active_drag_index()
    }

    pub(super) fn capture_drag_entries(
        &mut self,
        entries: &[FileEntry],
        selected: &BTreeSet<usize>,
    ) {
        self.state.capture_drag_entries(entries, selected);
    }

    pub(super) fn active_drag_entries(&self) -> &[FileEntry] {
        self.state.active_drag_entries()
    }

    pub(super) fn request_active(&self, destination: PathBuf, action: Action) -> Option<Request> {
        self.state.request_active(destination, action)
    }

    pub(super) fn cancel_drag(&mut self) {
        self.state.cancel_drag();
    }

    pub(super) fn copy(&mut self, entries: &[FileEntry]) -> Option<String> {
        self.state.copy(entries)
    }

    pub(super) fn cut(&mut self, entries: &[FileEntry]) -> Option<String> {
        self.state.cut(entries)
    }

    pub(super) fn paste(&self, destination: PathBuf) -> Option<Request> {
        self.state.paste(destination)
    }

    pub(super) fn clipboard_payload(&self) -> Option<ClipboardPayload> {
        self.state.clipboard_payload()
    }

    pub(super) fn pending_cut_paths(&self) -> &[PathBuf] {
        self.state.pending_cut_paths()
    }

    pub(super) fn pending_cut_status(&self) -> Option<String> {
        self.state.pending_cut_status()
    }

    pub(super) fn reconcile_pending_cut(&mut self, removed: &[PathBuf]) -> Option<(u64, usize)> {
        self.state.reconcile_pending_cut(removed)
    }

    pub(super) fn cancel_cut(&mut self) -> Option<u64> {
        self.state.cancel_cut()
    }

    pub(super) fn import_clipboard(&mut self, import: ClipboardImport) -> bool {
        self.state.import_clipboard(import)
    }

    pub(super) fn start_outgoing_active<A, P>(
        &mut self,
        adapter: &A,
        copy_only: bool,
        preview: P,
    ) -> Result<(usize, AdapterCompletion), String>
    where
        A: Adapter,
        P: FnOnce(&[FileEntry]) -> Option<Preview>,
    {
        self.state
            .start_outgoing_active(adapter, copy_only, preview)
    }

    pub(super) fn finish_outgoing(&mut self, result: Result<Outcome, String>) -> Consequences {
        self.state.finish_outgoing(result)
    }

    pub(super) fn handle_native<A, F>(
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

    pub(super) fn finish_transfer(
        &mut self,
        adapter: Option<&dyn Adapter>,
        request: &Request,
        report: &TransferReport,
        current: &Path,
    ) -> TransferCompletion {
        let journal_action = journal::Action::transfer(
            match request.action {
                Action::Copy => journal::TransferKind::Copy,
                Action::Move => journal::TransferKind::Move,
            },
            &report.receipts,
        );
        let clipboard_generation = request
            .clipboard_generation
            .filter(|_| request.action == Action::Move);
        let consequences = self
            .state
            .finish_transfer(adapter, request, report, current);
        TransferCompletion {
            consequences,
            journal_action,
            clipboard_generation,
        }
    }

    pub(super) fn native_hover_destination(&self) -> Option<Option<&Path>> {
        self.state.native_hover_destination()
    }

    pub(super) fn is_native_active(&self) -> bool {
        self.state.is_native_active()
    }

    pub(super) fn stop<A: Adapter>(&mut self, adapter: &A) {
        self.state.stop(adapter);
    }
}

fn restore_completion(
    report: TransferReport,
    entries: &[trash::Entry],
    next: Option<Work>,
) -> CompletedRestore {
    let report = trash::finish_restore(report, entries);
    let journal_action = journal::Action::restore(&report.restored);
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
    CompletedRestore {
        journal_action,
        status,
        detail: (!detail.is_empty()).then(|| detail.join("\n")),
        changed_folders,
        next,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

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

    #[test]
    fn transfer_session_owns_queue_conflict_and_completion() {
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
        let work = session.enqueue(request.clone()).unwrap().unwrap();
        let id = work.id();
        let outcome = work.run();

        assert!(matches!(
            session.complete_batch(id, outcome),
            BatchUpdate::Conflict
        ));
        assert!(session.has_conflict());
        assert!(session.conflict_prompt().unwrap().contains("r Replace"));

        let work = session.resolve_conflict('s', false).unwrap();
        let id = work.id();
        let outcome = work.run();
        assert!(matches!(
            session.complete_batch(id, outcome),
            BatchUpdate::Completed(_)
        ));
        assert!(!session.has_conflict());
        assert!(!session.active());
    }

    #[test]
    fn restore_uses_transfer_conflicts_without_entering_transfer_history() {
        let temp = tempfile::tempdir().unwrap();
        let restore = restore_entry(temp.path(), "notes.txt");
        std::fs::write(&restore.receipt.original, "existing").unwrap();
        let mut session = TransferSession::open(temp.path().join("transfers.json"));
        let work = session.enqueue_restore(vec![restore.clone()]).unwrap();
        assert!(work.restoring());
        let id = work.id();

        assert!(matches!(
            session.complete_batch(id, work.run()),
            BatchUpdate::Conflict
        ));
        assert!(
            session
                .conflict_prompt()
                .unwrap()
                .starts_with("Restore notes.txt:")
        );
        assert!(!session.active());
        assert!(session.history().is_empty());

        let CancelUpdate::Conflict(BatchUpdate::Completed(completed)) = session.cancel() else {
            panic!("restore conflict should complete through Transfer session");
        };
        let CompletedBatch::Restore(completed) = *completed else {
            panic!("expected Restore completion");
        };
        assert_eq!(completed.status, "Restored 0  •  0 failed  •  1 kept");
        assert!(restore.receipt.trashed.exists());
        assert!(restore.receipt.info.exists());
        assert!(session.history().is_empty());
    }

    #[test]
    fn restore_completion_cleans_metadata_and_prepares_undo() {
        let temp = tempfile::tempdir().unwrap();
        let restore = restore_entry(temp.path(), "notes.txt");
        let changed = restore.receipt.original.parent().unwrap().to_path_buf();
        let mut session = TransferSession::open(temp.path().join("transfers.json"));
        let work = session.enqueue_restore(vec![restore.clone()]).unwrap();
        let id = work.id();
        let BatchUpdate::Completed(completed) = session.complete_batch(id, work.run()) else {
            panic!("restore should complete");
        };
        let CompletedBatch::Restore(completed) = *completed else {
            panic!("expected Restore completion");
        };

        assert_eq!(completed.status, "Restored 1  •  0 failed  •  0 kept");
        assert_eq!(completed.changed_folders, [changed]);
        assert!(matches!(completed.journal_action, Ok(Some(_))));
        assert!(!restore.receipt.trashed.exists());
        assert!(!restore.receipt.info.exists());
        assert!(restore.receipt.original.exists());
        assert!(session.history().is_empty());
        assert!(!session.has_retry());
    }

    #[test]
    fn restore_queues_behind_an_active_transfer_in_submission_order() {
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
        let transfer = session.enqueue(request).unwrap().unwrap();
        assert!(session.enqueue_restore(vec![restore]).is_none());

        let id = transfer.id();
        let BatchUpdate::Completed(completed) = session.complete_batch(id, transfer.run()) else {
            panic!("Copy should complete");
        };
        let CompletedBatch::Transfer(completed) = *completed else {
            panic!("expected Copy completion");
        };
        let next = completed.next.unwrap();
        assert!(next.restoring());
        assert_eq!(session.history().len(), 1);
    }
}
