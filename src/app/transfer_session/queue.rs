use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    fs::{ConflictChoice, TransferBatch, TransferProgress, TransferReport},
    journal,
    transfer::Request,
};

use crate::app::{
    operations::{ForegroundActivity, Operations},
    trash,
};

const MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Debug)]
pub(super) struct Work {
    id: u64,
    operation: Operation,
    batch: Batch,
    cancellation: Arc<AtomicBool>,
    progress: Arc<ProgressTracker>,
    activity: Option<Arc<ForegroundActivity>>,
}

impl Work {
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn run(self) -> WorkOutcome {
        let _activity = self.activity;
        let cancelled = || self.cancellation.load(Ordering::Acquire);
        let progress = |update| self.progress.update(update);
        match self.batch {
            Batch::Filesystem(batch) => {
                let outcome = (*batch).run_with(cancelled, progress);
                let undo = match &outcome {
                    crate::fs::TransferBatchOutcome::Complete(report) => {
                        self.operation.prepare_undo(report)
                    }
                    crate::fs::TransferBatchOutcome::Conflict { .. } => Ok(None),
                };
                WorkOutcome::Filesystem(outcome, undo)
            }
            Batch::Trash(batch) => WorkOutcome::Trash(batch.run(cancelled, progress)),
        }
    }
}

#[derive(Clone, Debug)]
enum Batch {
    Filesystem(Box<TransferBatch>),
    Trash(trash::Batch),
}

#[derive(Clone, Debug)]
pub(in crate::app) enum WorkOutcome {
    Filesystem(crate::fs::TransferBatchOutcome, PreparedUndo),
    Trash(trash::Report),
}

pub(super) type PreparedUndo = Result<Option<journal::Action>, String>;

impl From<crate::fs::TransferBatchOutcome> for WorkOutcome {
    fn from(outcome: crate::fs::TransferBatchOutcome) -> Self {
        Self::Filesystem(outcome, Ok(None))
    }
}

#[derive(Clone, Debug)]
pub(super) enum Operation {
    Transfer(Request),
    Restore(Vec<trash::Entry>),
    Trash(Vec<crate::fs::FileEntry>),
}

impl Operation {
    fn prepare_undo(&self, report: &TransferReport) -> PreparedUndo {
        match self {
            Self::Transfer(request) => journal::Action::transfer(
                match request.action {
                    crate::transfer::Action::Copy => journal::TransferKind::Copy,
                    crate::transfer::Action::Move => journal::TransferKind::Move,
                },
                &report.receipts,
            ),
            Self::Restore(entries) => {
                let receipts = report
                    .receipts
                    .iter()
                    .filter_map(|receipt| {
                        let entry = entries
                            .iter()
                            .find(|entry| entry.receipt.trashed == receipt.source)?;
                        Some(journal::TrashReceipt {
                            original: receipt.destination.clone(),
                            trashed: receipt.source.clone(),
                            info: entry.receipt.info.clone(),
                        })
                    })
                    .collect::<Vec<_>>();
                journal::Action::restore(
                    &receipts,
                    report
                        .receipts
                        .iter()
                        .any(|receipt| receipt.replaced_existing),
                )
            }
            Self::Trash(_) => Ok(None),
        }
        .map_err(|error| error.to_string())
    }

    fn active_action(&self) -> &'static str {
        match self {
            Self::Transfer(request) => match request.action {
                crate::transfer::Action::Copy => "Copying",
                crate::transfer::Action::Move => "Moving",
            },
            Self::Restore(_) => "Restoring",
            Self::Trash(_) => "Moving to Trash",
        }
    }
}

pub(super) struct Finished {
    pub(super) operation: Operation,
    pub(super) next: Option<Work>,
    pub(super) activity: Option<Arc<ForegroundActivity>>,
}

#[derive(Debug, Default)]
pub(super) struct ProgressTracker {
    completed_entries: AtomicU64,
    total_entries: AtomicU64,
    completed_bytes: AtomicU64,
    total_bytes: AtomicU64,
}

impl ProgressTracker {
    pub(super) fn update(&self, progress: TransferProgress) {
        self.completed_entries
            .store(progress.completed_entries, Ordering::Release);
        self.total_entries
            .store(progress.total_entries, Ordering::Release);
        self.completed_bytes
            .store(progress.completed_bytes, Ordering::Release);
        self.total_bytes
            .store(progress.total_bytes, Ordering::Release);
    }

    fn read(&self) -> TransferProgress {
        TransferProgress {
            completed_entries: self.completed_entries.load(Ordering::Acquire),
            total_entries: self.total_entries.load(Ordering::Acquire),
            completed_bytes: self.completed_bytes.load(Ordering::Acquire),
            total_bytes: self.total_bytes.load(Ordering::Acquire),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct Snapshot {
    pub(in crate::app) progress: TransferProgress,
    pub(in crate::app) elapsed: Duration,
    pub(in crate::app) bytes_per_second: u64,
    pub(in crate::app) estimated_remaining: Option<Duration>,
    pub(in crate::app) queued: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::app) struct HistoryEntry {
    recorded_at: u64,
    action: String,
    completed: usize,
    failed: usize,
    retained: usize,
    bytes: u64,
    elapsed_millis: u64,
    cancelled: bool,
    detail: String,
}

#[derive(Clone, Debug)]
struct Active {
    id: u64,
    operation: Operation,
    paused: Option<TransferBatch>,
    cancellation: Arc<AtomicBool>,
    progress: Arc<ProgressTracker>,
    started: Instant,
    activity: Option<Arc<ForegroundActivity>>,
}

struct Pending {
    operation: Operation,
    batch: Batch,
    activity: Option<Arc<ForegroundActivity>>,
}

pub(super) struct Queue {
    path: PathBuf,
    next_id: u64,
    active: Option<Active>,
    pending: VecDeque<Pending>,
    history: Vec<HistoryEntry>,
    last_retry: Option<Retry>,
    expanded: bool,
}

#[derive(Clone, Debug)]
enum Retry {
    Transfer {
        request: Request,
        entries: Vec<(PathBuf, PathBuf)>,
    },
    Trash(Vec<crate::fs::FileEntry>),
    Restore {
        entries: Vec<trash::Entry>,
        transfers: Vec<(PathBuf, PathBuf)>,
    },
}

pub(super) enum Report<'a> {
    Filesystem(&'a TransferReport),
    Trash(&'a trash::Report),
}

impl Queue {
    pub(super) fn open_default() -> Self {
        Self::open(history_path())
    }

    pub(super) fn open(path: PathBuf) -> Self {
        let history = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<HistoryEntry>>(&bytes).ok())
            .unwrap_or_default();
        let mut queue = Self {
            path,
            next_id: 1,
            active: None,
            pending: VecDeque::new(),
            history,
            last_retry: None,
            expanded: false,
        };
        queue.prune();
        queue
    }

    pub(super) fn enqueue_transfer(
        &mut self,
        request: Request,
        batch: TransferBatch,
    ) -> Option<Work> {
        self.enqueue(
            Operation::Transfer(request),
            Batch::Filesystem(Box::new(batch)),
            None,
        )
    }

    pub(super) fn enqueue_restore(
        &mut self,
        entries: Vec<trash::Entry>,
        batch: TransferBatch,
        operations: &Operations,
    ) -> Option<Work> {
        self.enqueue(
            Operation::Restore(entries),
            Batch::Filesystem(Box::new(batch)),
            Some(Arc::new(operations.begin_foreground())),
        )
    }

    pub(super) fn enqueue_trash(
        &mut self,
        entries: Vec<crate::fs::FileEntry>,
        batch: trash::Batch,
    ) -> Option<Work> {
        self.enqueue(Operation::Trash(entries), Batch::Trash(batch), None)
    }

    fn enqueue(
        &mut self,
        operation: Operation,
        batch: Batch,
        activity: Option<Arc<ForegroundActivity>>,
    ) -> Option<Work> {
        if self.active.is_some() {
            self.pending.push_back(Pending {
                operation,
                batch,
                activity,
            });
            None
        } else {
            Some(self.activate(operation, batch, activity))
        }
    }

    pub(super) fn pause_for_conflict(
        &mut self,
        id: u64,
        batch: TransferBatch,
    ) -> Option<Operation> {
        let active = self
            .active
            .as_mut()
            .filter(|active| active.id == id && active.paused.is_none())?;
        active.paused = Some(batch);
        Some(active.operation.clone())
    }

    pub(super) fn resolve_conflict(
        &mut self,
        choice: ConflictChoice,
        remaining: bool,
    ) -> Option<Work> {
        let active = self.active.as_mut()?;
        let batch = active.paused.take()?.resolve(choice, remaining);
        Some(work(active, batch))
    }

    pub(super) fn cancel_conflict(&mut self) -> Option<Work> {
        let active = self.active.as_mut()?;
        let batch = active.paused.take()?;
        active.cancellation.store(true, Ordering::Release);
        Some(work(active, batch))
    }

    pub(super) fn finish(&mut self, id: u64, report: Report<'_>) -> Option<Finished> {
        let operation_matches = self.active.as_ref().is_some_and(|active| {
            matches!(
                (&active.operation, &report),
                (
                    Operation::Transfer(_) | Operation::Restore(_),
                    Report::Filesystem(_)
                ) | (Operation::Trash(_), Report::Trash(_))
            )
        });
        if !operation_matches {
            return None;
        }
        let active = self
            .active
            .take_if(|active| active.id == id && active.paused.is_none())?;
        let snapshot = snapshot(&active, self.pending.len());
        match (&active.operation, report) {
            (Operation::Transfer(request), Report::Filesystem(report)) => {
                self.last_retry = (!report.retry.is_empty()).then(|| {
                    let mut request = request.clone();
                    request.paths = report
                        .retry
                        .iter()
                        .map(|(source, _)| source.clone())
                        .collect();
                    Retry::Transfer {
                        request,
                        entries: report.retry.clone(),
                    }
                });
                self.history
                    .push(transfer_history_entry(request, report, &snapshot));
            }
            (Operation::Trash(entries), Report::Trash(report)) => {
                let retry_paths = report
                    .failures
                    .iter()
                    .map(|(entry, _)| entry.path.as_path())
                    .chain(report.retained.iter().map(|entry| entry.path.as_path()))
                    .collect::<std::collections::BTreeSet<_>>();
                let retry_entries = entries
                    .iter()
                    .filter(|entry| retry_paths.contains(entry.path.as_path()))
                    .cloned()
                    .collect::<Vec<_>>();
                self.last_retry =
                    (!retry_entries.is_empty()).then_some(Retry::Trash(retry_entries));
                self.history.push(trash_history_entry(report, &snapshot));
            }
            (Operation::Restore(entries), Report::Filesystem(report)) => {
                self.last_retry = (!report.retry.is_empty()).then(|| Retry::Restore {
                    entries: entries.clone(),
                    transfers: report.retry.clone(),
                });
            }
            _ => unreachable!("operation and report were checked before finishing"),
        }
        self.prune();
        let _ = self.save();
        let next = self.pending.pop_front().map(
            |Pending {
                 operation,
                 batch,
                 activity,
             }| self.activate(operation, batch, activity),
        );
        Some(Finished {
            operation: active.operation,
            next,
            activity: active.activity,
        })
    }

    pub(super) fn retry(&mut self, operations: &Operations) -> Result<Option<Work>, String> {
        let Some(retry) = self.last_retry.as_ref().cloned() else {
            return Ok(None);
        };
        let (operation, batch) = match retry {
            Retry::Transfer { request, entries } => {
                let batch = TransferBatch::try_new_mapped(entries, request.action)
                    .map_err(|error| error.to_string())?;
                (
                    Operation::Transfer(request),
                    Batch::Filesystem(Box::new(batch)),
                )
            }
            Retry::Trash(entries) => {
                let batch = trash::Batch::new(entries.clone());
                (Operation::Trash(entries), Batch::Trash(batch))
            }
            Retry::Restore { entries, transfers } => {
                let batch = TransferBatch::try_new_mapped(transfers, crate::transfer::Action::Move)
                    .map_err(|error| error.to_string())?;
                (
                    Operation::Restore(entries),
                    Batch::Filesystem(Box::new(batch)),
                )
            }
        };
        self.last_retry = None;
        let activity = matches!(operation, Operation::Restore(_))
            .then(|| Arc::new(operations.begin_foreground()));
        Ok(self.enqueue(operation, batch, activity))
    }

    pub(super) fn cancel(&self) -> bool {
        self.active
            .as_ref()
            .filter(|active| active.paused.is_none())
            .is_some_and(|active| {
                active.cancellation.store(true, Ordering::Release);
                true
            })
    }

    pub(super) fn active(&self) -> bool {
        self.active.is_some()
    }

    #[cfg(test)]
    pub(super) fn restore_active(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| matches!(active.operation, Operation::Restore(_)))
    }

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        self.active
            .as_ref()
            .map(|active| snapshot(active, self.pending.len()))
    }

    pub(super) fn active_action(&self) -> Option<&'static str> {
        self.active
            .as_ref()
            .map(|active| active.operation.active_action())
    }

    pub(super) fn has_retry(&self) -> bool {
        self.last_retry.is_some()
    }

    pub(super) fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }

    pub(super) fn expanded(&self) -> bool {
        self.expanded
    }

    pub(super) fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    pub(super) fn report_text(&self) -> String {
        self.history
            .iter()
            .rev()
            .map(|entry| entry.detail.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn activate(
        &mut self,
        operation: Operation,
        batch: Batch,
        activity: Option<Arc<ForegroundActivity>>,
    ) -> Work {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(ProgressTracker::default());
        self.active = Some(Active {
            id,
            operation: operation.clone(),
            paused: None,
            cancellation: Arc::clone(&cancellation),
            progress: Arc::clone(&progress),
            started: Instant::now(),
            activity: activity.clone(),
        });
        Work {
            id,
            operation,
            batch,
            cancellation,
            progress,
            activity,
        }
    }

    fn prune(&mut self) {
        let oldest = now_seconds().saturating_sub(MAX_AGE_SECONDS);
        self.history.retain(|entry| entry.recorded_at >= oldest);
    }

    fn save(&self) -> Result<(), String> {
        let Some(directory) = self.path.parent() else {
            return Err("transfer history path has no parent".to_owned());
        };
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&self.history).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, &self.path).map_err(|error| error.to_string())
    }
}

fn work(active: &Active, batch: TransferBatch) -> Work {
    Work {
        id: active.id,
        operation: active.operation.clone(),
        batch: Batch::Filesystem(Box::new(batch)),
        cancellation: Arc::clone(&active.cancellation),
        progress: Arc::clone(&active.progress),
        activity: active.activity.clone(),
    }
}

impl HistoryEntry {
    pub(in crate::app) fn summary(&self) -> &str {
        &self.detail
    }
}

fn snapshot(active: &Active, queued: usize) -> Snapshot {
    let progress = active.progress.read();
    let elapsed = active.started.elapsed();
    let bytes_per_second = if elapsed.as_millis() == 0 {
        0
    } else {
        ((u128::from(progress.completed_bytes) * 1_000) / elapsed.as_millis()) as u64
    };
    let estimated_remaining =
        (bytes_per_second > 0 && progress.completed_bytes < progress.total_bytes).then(|| {
            Duration::from_secs(
                (progress.total_bytes - progress.completed_bytes) / bytes_per_second,
            )
        });
    Snapshot {
        progress,
        elapsed,
        bytes_per_second,
        estimated_remaining,
        queued,
    }
}

fn transfer_history_entry(
    request: &Request,
    report: &TransferReport,
    snapshot: &Snapshot,
) -> HistoryEntry {
    let action = request.action.label().to_owned();
    let detail = format!(
        "{action} {} item(s); {} failed; {} retained; {} bytes in {} ms{}",
        report.completed.len(),
        report.failures.len(),
        report.retained.len(),
        snapshot.progress.completed_bytes,
        snapshot.elapsed.as_millis(),
        if report.cancelled { "; cancelled" } else { "" }
    );
    HistoryEntry {
        recorded_at: now_seconds(),
        action,
        completed: report.completed.len(),
        failed: report.failures.len(),
        retained: report.retained.len(),
        bytes: snapshot.progress.completed_bytes,
        elapsed_millis: snapshot.elapsed.as_millis() as u64,
        cancelled: report.cancelled,
        detail,
    }
}

fn trash_history_entry(report: &trash::Report, snapshot: &Snapshot) -> HistoryEntry {
    let action = "Moved to Trash".to_owned();
    let detail = format!(
        "{action} {} item(s); {} failed; {} retained; {} bytes in {} ms{}",
        report.receipts.len(),
        report.failures.len(),
        report.retained.len(),
        snapshot.progress.completed_bytes,
        snapshot.elapsed.as_millis(),
        if report.cancelled { "; cancelled" } else { "" }
    );
    HistoryEntry {
        recorded_at: now_seconds(),
        action,
        completed: report.receipts.len(),
        failed: report.failures.len(),
        retained: report.retained.len(),
        bytes: snapshot.progress.completed_bytes,
        elapsed_millis: snapshot.elapsed.as_millis() as u64,
        cancelled: report.cancelled,
        detail,
    }
}

fn history_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("waddle/transfers.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".waddle-transfers.json"),
        |home| PathBuf::from(home).join(".local/state/waddle/transfers.json"),
    )
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf, thread};

    use crate::{
        fs::{FileEntry, TransferBatchOutcome, TransferFailure},
        transfer::TransferState,
    };

    use super::*;

    #[test]
    fn hunt_failed_restore_retries_itself_instead_of_an_older_copy() {
        let temp = tempfile::tempdir().unwrap();
        let old_source = temp.path().join("old-source");
        let old_destination = temp.path().join("old-destination");
        fs::create_dir(&old_destination).unwrap();
        let mut copy = request(old_destination.to_str().unwrap());
        copy.paths = vec![old_source.clone()];
        let mut queue = Queue::open(temp.path().join("history.json"));
        let work = queue
            .enqueue_transfer(
                copy.clone(),
                TransferBatch::new(copy.paths.clone(), copy.destination.clone(), copy.action),
            )
            .unwrap();
        let id = work.id();
        let WorkOutcome::Filesystem(TransferBatchOutcome::Complete(report), _) = work.run() else {
            panic!("missing copy source must fail");
        };
        assert_eq!(report.failures.len(), 1);
        queue.finish(id, Report::Filesystem(&report)).unwrap();
        assert!(queue.has_retry());

        let source = temp.path().join("Trash/files/item");
        let info = temp.path().join("Trash/info/item.trashinfo");
        let original = temp.path().join("restored/item");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::write(&source, "recover me").unwrap();
        fs::write(&info, "metadata").unwrap();
        let entry = trash::Entry {
            file: FileEntry {
                path: source.clone(),
                name: "item".into(),
                directory: false,
                metadata: Default::default(),
            },
            receipt: journal::TrashReceipt {
                original: original.clone(),
                trashed: source.clone(),
                info: info.clone(),
            },
        };
        let entries = vec![entry];
        let work = queue
            .enqueue_restore(
                entries.clone(),
                trash::restore_batch(&entries),
                &Operations::default(),
            )
            .unwrap();
        let id = work.id();
        let WorkOutcome::Filesystem(TransferBatchOutcome::Complete(report), _) = work.run() else {
            panic!("missing restore parent must fail");
        };
        assert_eq!(report.failures.len(), 1);
        queue.finish(id, Report::Filesystem(&report)).unwrap();
        fs::create_dir(original.parent().unwrap()).unwrap();
        fs::write(&old_source, "older unrelated copy").unwrap();
        let retry = queue
            .retry(&Operations::default())
            .unwrap()
            .expect("failed Restore must be retryable");
        assert!(
            queue.restore_active(),
            "Retry selected the older Copy instead of the failed Restore"
        );
        let id = retry.id();
        let WorkOutcome::Filesystem(TransferBatchOutcome::Complete(report), _) = retry.run() else {
            panic!("repaired Restore must complete");
        };
        assert!(report.failures.is_empty());
        queue.finish(id, Report::Filesystem(&report)).unwrap();
        let restored = trash::finish_restore(report, &entries);
        assert_eq!(restored.restored.len(), 1);
        assert_eq!(fs::read_to_string(original).unwrap(), "recover me");
        assert!(!source.exists());
        assert!(!info.exists());
        assert_eq!(fs::read_dir(old_destination).unwrap().count(), 0);
        assert!(!queue.has_retry());
    }

    fn request(destination: &str) -> Request {
        let mut state = TransferState::default();
        state
            .copy(&[FileEntry {
                path: PathBuf::from("/source/item"),
                name: OsString::from("item"),
                directory: false,
                metadata: Default::default(),
            }])
            .unwrap();
        state.paste(PathBuf::from(destination)).unwrap()
    }

    fn report() -> TransferReport {
        TransferReport {
            retry: Vec::new(),
            completed: vec![PathBuf::from("/target/item")],
            failures: Vec::new(),
            retained: Vec::new(),
            warnings: Vec::new(),
            receipts: Vec::new(),
            cancelled: false,
        }
    }

    fn entry(path: &str) -> FileEntry {
        let path = PathBuf::from(path);
        FileEntry {
            name: path.file_name().unwrap_or_default().to_owned(),
            path,
            directory: false,
            metadata: Default::default(),
        }
    }

    #[test]
    fn retry_preserves_nested_destinations_and_does_not_repeat_successes() {
        use std::os::unix::net::UnixListener;

        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source/folder");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(destination.join("folder")).unwrap();
        fs::write(source.join("a"), "copied once").unwrap();
        let special = UnixListener::bind(source.join("b")).unwrap();
        let mut request = request(destination.to_str().unwrap());
        request.paths = vec![source.clone()];
        let mut queue = Queue::open(temp.path().join("history.json"));
        let work = queue
            .enqueue_transfer(
                request.clone(),
                TransferBatch::new(request.paths.clone(), destination.clone(), request.action),
            )
            .unwrap();
        let id = work.id();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Conflict { batch, .. }, _) =
            work.run()
        else {
            panic!("expected folder conflict");
        };
        queue.pause_for_conflict(id, *batch).unwrap();
        let work = queue
            .resolve_conflict(ConflictChoice::Replace, false)
            .unwrap();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Complete(report), _) =
            work.run()
        else {
            panic!("expected partial completion");
        };
        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            report.retry,
            [(source.join("b"), destination.join("folder/b"))]
        );
        queue.finish(id, Report::Filesystem(&report)).unwrap();

        drop(special);
        fs::remove_file(source.join("b")).unwrap();
        fs::write(source.join("b"), "now readable").unwrap();
        fs::write(destination.join("folder/a"), "edited after Copy").unwrap();
        let retried = queue.retry(&Operations::default()).unwrap().unwrap();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Complete(report), undo) =
            retried.run()
        else {
            panic!("Retry should only copy the failed child");
        };
        assert!(report.failures.is_empty());
        assert!(matches!(undo, Ok(Some(_))));
        assert!(!destination.join("b").exists());
        assert_eq!(
            fs::read_to_string(destination.join("folder/b")).unwrap(),
            "now readable"
        );
        assert_eq!(
            fs::read_to_string(destination.join("folder/a")).unwrap(),
            "edited after Copy"
        );
    }

    #[test]
    fn cancelling_a_conflict_prepares_undo_for_earlier_successes_in_the_worker() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("a"), "first").unwrap();
        fs::write(source.join("b"), "second").unwrap();
        fs::write(destination.join("b"), "existing").unwrap();
        let mut request = request(destination.to_str().unwrap());
        request.paths = vec![source.join("a"), source.join("b")];
        let mut queue = Queue::open(temp.path().join("history.json"));
        let work = queue
            .enqueue_transfer(
                request.clone(),
                TransferBatch::new(request.paths.clone(), destination.clone(), request.action),
            )
            .unwrap();
        let id = work.id();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Conflict { batch, .. }, _) =
            work.run()
        else {
            panic!("expected second item conflict");
        };
        queue.pause_for_conflict(id, *batch).unwrap();
        let work = queue.cancel_conflict().unwrap();
        assert!(queue.active());
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Complete(report), undo) =
            work.run()
        else {
            panic!("expected cancelled completion");
        };
        assert!(report.cancelled);
        assert_eq!(report.completed, [destination.join("a")]);
        assert_eq!(report.retry, [(source.join("b"), destination.join("b"))]);
        let mut journal = journal::Journal::in_memory();
        journal.record(undo.unwrap().unwrap()).unwrap();
        journal.undo().unwrap();
        assert!(!destination.join("a").exists());
        assert_eq!(
            fs::read_to_string(destination.join("b")).unwrap(),
            "existing"
        );
    }

    #[test]
    fn worker_captures_undo_before_returning_a_completed_transfer() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("item");
        let destination = temp.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, "original").unwrap();
        let mut request = request(destination.to_str().unwrap());
        request.paths = vec![source];
        let mut queue = Queue::open(temp.path().join("history.json"));
        let work = queue
            .enqueue_transfer(
                request.clone(),
                TransferBatch::new(request.paths.clone(), destination.clone(), request.action),
            )
            .unwrap();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Complete(_), undo) =
            work.run()
        else {
            panic!("expected completion");
        };
        let action = undo.unwrap().expect("worker must prepare Undo");
        fs::write(
            destination.join("item"),
            "changed before UI handles completion",
        )
        .unwrap();
        let mut journal = journal::Journal::in_memory();
        journal.record(action).unwrap();
        assert!(
            journal.undo().is_err(),
            "Undo must retain the worker's original fingerprint"
        );
        assert!(destination.join("item").exists());
    }

    #[test]
    fn queue_runs_one_transfer_at_a_time_and_persists_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("transfers.json");
        let mut queue = Queue::open(path.clone());
        let first = request("/target");
        let second = request("/another");
        let work = queue
            .enqueue_transfer(
                first.clone(),
                TransferBatch::new(first.paths.clone(), first.destination.clone(), first.action),
            )
            .unwrap();
        assert!(
            queue
                .enqueue_transfer(
                    second.clone(),
                    TransferBatch::new(
                        second.paths.clone(),
                        second.destination.clone(),
                        second.action,
                    ),
                )
                .is_none()
        );
        work.progress.update(TransferProgress {
            completed_entries: 1,
            total_entries: 1,
            completed_bytes: 4096,
            total_bytes: 4096,
        });
        thread::sleep(Duration::from_millis(2));
        let report = report();
        let next = queue
            .finish(work.id, Report::Filesystem(&report))
            .unwrap()
            .next
            .unwrap();
        assert_eq!(queue.active.as_ref().unwrap().id, next.id());
        assert!(matches!(
            &queue.active.as_ref().unwrap().operation,
            Operation::Transfer(Request { destination, .. })
                if destination == std::path::Path::new("/another")
        ));
        assert_eq!(queue.history().len(), 1);
        assert!(queue.report_text().contains("4096 bytes"));

        let reopened = Queue::open(path);
        assert_eq!(reopened.history().len(), 1);
    }

    #[test]
    fn cancellation_retry_and_progress_snapshot_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let mut queue = Queue::open(temp.path().join("transfers.json"));
        let source = temp.path().join("source/item");
        let destination = temp.path().join("target");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(&source, "x").unwrap();
        let mut request = request(destination.to_str().unwrap());
        request.paths = vec![source];
        let work = queue
            .enqueue_transfer(
                request.clone(),
                TransferBatch::new(
                    request.paths.clone(),
                    request.destination.clone(),
                    request.action,
                ),
            )
            .unwrap();
        work.progress.update(TransferProgress {
            completed_entries: 1,
            total_entries: 2,
            completed_bytes: 1024,
            total_bytes: 2048,
        });
        thread::sleep(Duration::from_millis(2));
        assert!(queue.snapshot().unwrap().bytes_per_second > 0);
        assert!(queue.cancel());
        assert!(work.cancellation.load(Ordering::Acquire));

        let failed = TransferReport {
            retry: vec![(request.paths[0].clone(), destination.join("item"))],
            completed: Vec::new(),
            failures: vec![TransferFailure {
                source: request.paths[0].clone(),
                error: "denied".to_owned(),
            }],
            retained: Vec::new(),
            warnings: Vec::new(),
            receipts: Vec::new(),
            cancelled: false,
        };
        assert!(
            queue
                .finish(work.id, Report::Filesystem(&failed))
                .unwrap()
                .next
                .is_none()
        );
        assert!(queue.has_retry());
        fs::remove_dir(&destination).unwrap();
        assert!(queue.retry(&Operations::default()).is_err());
        assert!(queue.has_retry());
        fs::create_dir(&destination).unwrap();
        assert!(queue.retry(&Operations::default()).unwrap().is_some());
    }

    #[test]
    fn trash_uses_progress_cancel_retry_and_history() {
        let temp = tempfile::tempdir().unwrap();
        let mut queue = Queue::open(temp.path().join("transfers.json"));
        let first = entry("/source/one");
        let second = entry("/source/two");
        let work = queue
            .enqueue_trash(
                vec![first.clone(), second.clone()],
                trash::Batch::new(vec![first.clone(), second.clone()]),
            )
            .unwrap();

        assert!(queue.active());
        assert_eq!(queue.active_action(), Some("Moving to Trash"));
        assert!(queue.snapshot().is_some());
        assert!(queue.cancel());
        assert!(work.cancellation.load(Ordering::Acquire));

        let report = trash::Report {
            receipts: Vec::new(),
            failures: vec![(first.clone(), "denied".to_owned())],
            retained: vec![second.clone()],
            cancelled: true,
            undo: Ok(None),
        };
        assert!(
            queue
                .finish(work.id, Report::Trash(&report))
                .unwrap()
                .next
                .is_none()
        );
        assert!(queue.has_retry());
        assert_eq!(queue.history().len(), 1);
        assert!(queue.report_text().contains("Moved to Trash"));

        assert!(queue.retry(&Operations::default()).unwrap().is_some());
        assert!(matches!(
            &queue.active.as_ref().unwrap().operation,
            Operation::Trash(entries)
                if entries.iter().map(|entry| &entry.path).collect::<Vec<_>>()
                    == [&first.path, &second.path]
        ));
    }

    #[test]
    fn queue_keeps_a_paused_batch_until_the_conflict_is_resolved() {
        let temp = tempfile::tempdir().unwrap();
        let source_directory = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source_directory).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_directory.join("item");
        fs::write(&source, "source").unwrap();
        fs::write(destination.join("item"), "existing").unwrap();

        let mut request = request(destination.to_str().unwrap());
        request.paths = vec![source.clone()];
        let mut queue = Queue::open(temp.path().join("transfers.json"));
        let work = queue
            .enqueue_transfer(
                request.clone(),
                TransferBatch::new(
                    request.paths.clone(),
                    request.destination.clone(),
                    request.action,
                ),
            )
            .unwrap();
        let id = work.id();
        let WorkOutcome::Filesystem(TransferBatchOutcome::Conflict { batch, .. }, _) = work.run()
        else {
            panic!("existing destination must pause the Transfer");
        };
        let paused = *batch;

        assert!(
            queue
                .pause_for_conflict(id.wrapping_add(1), paused.clone())
                .is_none()
        );
        assert!(matches!(
            queue.pause_for_conflict(id, paused.clone()),
            Some(Operation::Transfer(_))
        ));
        assert!(queue.pause_for_conflict(id, paused).is_none());
        assert!(queue.finish(id, Report::Filesystem(&report())).is_none());

        let resumed = queue.resolve_conflict(ConflictChoice::Skip, false).unwrap();
        assert_eq!(resumed.id(), id);
        let WorkOutcome::Filesystem(TransferBatchOutcome::Complete(report), _) = resumed.run()
        else {
            panic!("Skip must complete the paused Transfer");
        };
        assert_eq!(report.retained, [source]);
        assert!(queue.finish(id, Report::Filesystem(&report)).is_some());
    }
}

#[cfg(test)]
mod regressions {
    use super::*;
    #[test]
    fn undo_merged_restore_keeps_preexisting_destination_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("trash/files/folder");
        let original = temp.path().join("original/folder");
        let info = temp.path().join("trash/info/folder.trashinfo");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&original).unwrap();
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::write(&info, "metadata").unwrap();
        fs::write(source.join("restored.txt"), "restored").unwrap();
        fs::write(original.join("preexisting.txt"), "must remain").unwrap();
        let entry = trash::Entry {
            file: crate::fs::FileEntry {
                path: source.clone(),
                name: "folder".into(),
                directory: true,
                metadata: Default::default(),
            },
            receipt: journal::TrashReceipt {
                original: original.clone(),
                trashed: source,
                info,
            },
        };
        let mut queue = Queue::open(temp.path().join("history.json"));
        let work = queue
            .enqueue_restore(
                vec![entry.clone()],
                trash::restore_batch(std::slice::from_ref(&entry)),
                &Operations::default(),
            )
            .unwrap();
        let id = work.id();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Conflict { batch, .. }, _) =
            work.run()
        else {
            panic!("expected folder conflict")
        };
        queue.pause_for_conflict(id, *batch).unwrap();
        let resumed = queue
            .resolve_conflict(ConflictChoice::Replace, false)
            .unwrap();
        let WorkOutcome::Filesystem(crate::fs::TransferBatchOutcome::Complete(report), undo) =
            resumed.run()
        else {
            panic!("expected completion")
        };
        assert!(report.receipts[0].replaced_existing);
        let _ = trash::finish_restore(report, &[entry]);
        let mut journal = journal::Journal::in_memory();
        journal
            .record(
                undo.unwrap()
                    .expect("merged Restore is offered as undoable"),
            )
            .unwrap();
        let result = journal.undo();
        assert!(result.unwrap_err().to_string().contains("Refused Undo"));
        assert!(
            original.join("preexisting.txt").exists(),
            "Undo removed preexisting files"
        );
    }
}
