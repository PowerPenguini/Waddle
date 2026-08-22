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
    fs::{TransferBatch, TransferProgress, TransferReport},
    transfer::Request,
};

const MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Debug)]
pub(super) struct Work {
    pub(super) id: u64,
    pub(super) request: Request,
    pub(super) batch: TransferBatch,
    pub(super) cancellation: Arc<AtomicBool>,
    pub(super) progress: Arc<ProgressTracker>,
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
pub(super) struct Snapshot {
    pub(super) progress: TransferProgress,
    pub(super) elapsed: Duration,
    pub(super) bytes_per_second: u64,
    pub(super) estimated_remaining: Option<Duration>,
    pub(super) queued: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct HistoryEntry {
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
    request: Request,
    cancellation: Arc<AtomicBool>,
    progress: Arc<ProgressTracker>,
    started: Instant,
}

pub(super) struct Queue {
    path: PathBuf,
    next_id: u64,
    active: Option<Active>,
    pending: VecDeque<(Request, TransferBatch)>,
    history: Vec<HistoryEntry>,
    last_retry: Option<Request>,
    expanded: bool,
}

impl Queue {
    pub(super) fn open_default() -> Self {
        Self::open(history_path())
    }

    fn open(path: PathBuf) -> Self {
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

    pub(super) fn enqueue(&mut self, request: Request, batch: TransferBatch) -> Option<Work> {
        if self.active.is_some() {
            self.pending.push_back((request, batch));
            None
        } else {
            Some(self.activate(request, batch))
        }
    }

    pub(super) fn resume(&self, batch: TransferBatch) -> Option<Work> {
        let active = self.active.as_ref()?;
        Some(Work {
            id: active.id,
            request: active.request.clone(),
            batch,
            cancellation: Arc::clone(&active.cancellation),
            progress: Arc::clone(&active.progress),
        })
    }

    pub(super) fn finish(&mut self, id: u64, report: &TransferReport) -> Option<Work> {
        let active = self.active.take_if(|active| active.id == id)?;
        let snapshot = snapshot(&active, self.pending.len());
        let retry_paths = report
            .failures
            .iter()
            .map(|failure| failure.source.clone())
            .chain(report.retained.iter().cloned())
            .collect::<Vec<_>>();
        self.last_retry = (!retry_paths.is_empty()).then(|| {
            let mut request = active.request.clone();
            request.paths = retry_paths;
            request
        });
        self.history.push(history_entry(&active, report, &snapshot));
        self.prune();
        let _ = self.save();
        self.pending
            .pop_front()
            .map(|(request, batch)| self.activate(request, batch))
    }

    pub(super) fn retry(&mut self) -> Option<Work> {
        let request = self.last_retry.take()?;
        let batch = TransferBatch::new(
            request.paths.clone(),
            request.destination.clone(),
            request.action,
        );
        self.enqueue(request, batch)
    }

    pub(super) fn cancel(&self) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.cancellation.store(true, Ordering::Release);
            true
        })
    }

    pub(super) fn active_id(&self) -> Option<u64> {
        self.active.as_ref().map(|active| active.id)
    }

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        self.active
            .as_ref()
            .map(|active| snapshot(active, self.pending.len()))
    }

    pub(super) fn active_action(&self) -> Option<&'static str> {
        self.active
            .as_ref()
            .map(|active| match active.request.action {
                crate::transfer::Action::Copy => "Copying",
                crate::transfer::Action::Move => "Moving",
            })
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

    fn activate(&mut self, request: Request, batch: TransferBatch) -> Work {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(ProgressTracker::default());
        self.active = Some(Active {
            id,
            request: request.clone(),
            cancellation: Arc::clone(&cancellation),
            progress: Arc::clone(&progress),
            started: Instant::now(),
        });
        Work {
            id,
            request,
            batch,
            cancellation,
            progress,
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

impl HistoryEntry {
    pub(super) fn summary(&self) -> &str {
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

fn history_entry(active: &Active, report: &TransferReport, snapshot: &Snapshot) -> HistoryEntry {
    let action = active.request.action.label().to_owned();
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

fn history_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("polarexp/transfers.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".polarexp-transfers.json"),
        |home| PathBuf::from(home).join(".local/state/polarexp/transfers.json"),
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
        fs::{FileEntry, TransferFailure},
        transfer::TransferWorkflow,
    };

    use super::*;

    fn request(destination: &str) -> Request {
        let mut workflow = TransferWorkflow::default();
        workflow
            .copy(&[FileEntry {
                path: PathBuf::from("/source/item"),
                name: OsString::from("item"),
                directory: false,
            }])
            .unwrap();
        workflow.paste(PathBuf::from(destination)).unwrap()
    }

    fn report() -> TransferReport {
        TransferReport {
            completed: vec![PathBuf::from("/target/item")],
            failures: Vec::new(),
            retained: Vec::new(),
            warnings: Vec::new(),
            receipts: Vec::new(),
            cancelled: false,
        }
    }

    #[test]
    fn queue_runs_one_transfer_at_a_time_and_persists_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("transfers.json");
        let mut queue = Queue::open(path.clone());
        let first = request("/target");
        let second = request("/another");
        let work = queue
            .enqueue(
                first.clone(),
                TransferBatch::new(first.paths.clone(), first.destination.clone(), first.action),
            )
            .unwrap();
        assert!(
            queue
                .enqueue(
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
        let next = queue.finish(work.id, &report()).unwrap();
        assert_eq!(next.request.destination, PathBuf::from("/another"));
        assert_eq!(queue.history().len(), 1);
        assert!(queue.report_text().contains("4096 bytes"));

        let reopened = Queue::open(path);
        assert_eq!(reopened.history().len(), 1);
    }

    #[test]
    fn cancellation_retry_and_progress_snapshot_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let mut queue = Queue::open(temp.path().join("transfers.json"));
        let request = request("/target");
        let work = queue
            .enqueue(
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
        assert!(queue.finish(work.id, &failed).is_none());
        assert!(queue.has_retry());
        assert!(queue.retry().is_some());
    }
}
