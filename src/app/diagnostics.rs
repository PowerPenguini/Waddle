use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_RECORDS: usize = 100;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Record {
    timestamp: u64,
    summary: String,
    detail: String,
}

pub(super) struct History {
    path: PathBuf,
    records: Vec<Record>,
    pending_records: Vec<Record>,
}

impl History {
    #[cfg(not(test))]
    pub(super) fn open_default() -> Self {
        let path = state_path();
        let records = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut history = Self {
            path,
            records,
            pending_records: Vec::new(),
        };
        Self::prune(&mut history.records, now());
        history
    }

    #[cfg(test)]
    pub(super) fn open_default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self {
            path: std::env::temp_dir().join(format!(
                "waddle-diagnostics-test-{}-{}-{}.json",
                std::process::id(),
                now(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )),
            records: Vec::new(),
            pending_records: Vec::new(),
        }
    }

    pub(super) fn record(&mut self, summary: String, detail: String) {
        let timestamp = now();
        let record = Record {
            timestamp,
            summary,
            detail,
        };
        self.records.push(record.clone());
        self.pending_records.push(record);
        Self::prune(&mut self.records, timestamp);
        Self::prune(&mut self.pending_records, timestamp);
        let _ = self.save();
    }

    pub(super) fn report(&mut self) -> String {
        let mut records = match self.read_records() {
            Ok(mut records) => {
                records.extend(self.pending_records.iter().cloned());
                records
            }
            Err(_) => self.records.clone(),
        };
        Self::prune(&mut records, now());
        self.records = records;
        if self.records.is_empty() {
            return "No command failures recorded in the last 30 days.".to_owned();
        }
        self.records
            .iter()
            .rev()
            .map(|record| {
                format!(
                    "{}\n{}\n{}",
                    record.timestamp, record.summary, record.detail
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn prune(records: &mut Vec<Record>, timestamp: u64) {
        let oldest = timestamp.saturating_sub(RETENTION.as_secs());
        records.retain(|record| record.timestamp >= oldest);
        records.sort_by_key(|record| record.timestamp);
        if records.len() > MAX_RECORDS {
            records.drain(..records.len() - MAX_RECORDS);
        }
    }

    fn read_records(&self) -> Result<Vec<Record>, String> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&mut self) -> Result<(), String> {
        let directory = self.path.parent().ok_or("diagnostic path has no parent")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))
            .map_err(|error| error.to_string())?;
        lock.lock().map_err(|error| error.to_string())?;
        let mut records = self.read_records()?;
        records.extend(self.pending_records.iter().cloned());
        Self::prune(&mut records, now());
        let bytes = serde_json::to_vec_pretty(&records).map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        temporary
            .write_all(&bytes)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.to_string())?;
        self.records = records;
        self.pending_records.clear();
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(not(test))]
fn state_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("waddle/diagnostics.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".waddle-diagnostics.json"),
        |home| PathBuf::from(home).join(".local/state/waddle/diagnostics.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_failures_from_multiple_windows_remain_in_shared_diagnostics() {
        use crate::app::{App, Message, NavigationSession};
        use iced::{futures::StreamExt, keyboard};

        async fn command(app: &mut App, value: &str) {
            let key = keyboard::Key::Character(":".into());
            drop(app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some(":")));
            drop(app.update(Message::CommandChanged(value.into())));
            let mut pending =
                std::collections::VecDeque::from([app.update(Message::CommandSubmitted)]);
            while let Some(task) = pending.pop_front() {
                if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(message) = action {
                            pending.push_back(app.update(message));
                        }
                    }
                }
            }
        }

        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("diagnostics.json");
                let window = || {
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    app.diagnostics = History {
                        path: path.clone(),
                        records: Vec::new(),
                        pending_records: Vec::new(),
                    };
                    app
                };
                let mut first = window();
                let mut second = window();
                command(&mut first, "printf window_a_failure >&2; false").await;
                command(&mut second, "printf window_b_failure >&2; false").await;
                let stored = fs::read_to_string(&path).unwrap();
                assert!(
                    stored.contains("window_a_failure"),
                    "The second window erased the first failure"
                );
                assert!(stored.contains("window_b_failure"));
                for app in [&mut first, &mut second] {
                    command(app, "diagnostics").await;
                    let detail = &app.command.output().unwrap().detail;
                    assert!(detail.contains("window_a_failure"));
                    assert!(
                        detail.contains("window_b_failure"),
                        "An existing window hid another window's failure"
                    );
                }
                // Block persistence, then let the other window add another record before retry.
                let retained = temp.path().join("retained.json");
                fs::rename(&path, &retained).unwrap();
                fs::create_dir(&path).unwrap();
                command(&mut first, "printf pending_failure >&2; false").await;
                command(&mut first, "diagnostics").await;
                assert!(first.command.output().unwrap().detail.contains("pending_failure"));
                assert!(first.command.output().unwrap().detail.contains("window_b_failure"),
                    "A temporary storage error must not hide shared records already displayed by this window");
                fs::remove_dir(&path).unwrap();
                fs::rename(&retained, &path).unwrap();
                command(&mut second, "printf later_failure >&2; false").await;
                command(&mut first, "printf window_a_failure >&2; false").await;
                let mut reopened = window();
                for app in [&mut first, &mut second, &mut reopened] {
                    command(app, "diagnostics").await;
                    let detail = &app.command.output().unwrap().detail;
                    for (failure, count) in [("window_a_failure", 2), ("window_b_failure", 1),
                        ("pending_failure", 1), ("later_failure", 1)] {
                        assert_eq!(detail.lines().filter(|line| *line == failure).count(), count,
                            "Each failure must be retained exactly once, including retries and repeated commands");
                    }
                }
            });
    }

    #[test]
    fn command_failure_history_preserves_preexisting_temporary_entries() {
        use crate::app::{App, Message, NavigationSession};
        use iced::{Task, futures::StreamExt, keyboard};

        async fn finish(app: &mut App, task: Task<Message>) {
            let mut pending = std::collections::VecDeque::from([task]);
            while let Some(task) = pending.pop_front() {
                if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(message) = action {
                            pending.push_back(app.update(message));
                        }
                    }
                }
            }
        }

        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                for kind in ["symlink", "hardlink", "file", "directory"] {
                    let temp = tempfile::tempdir().unwrap();
                    let path = temp.path().join("diagnostics.json");
                    let collision = path.with_extension("json.tmp");
                    let unrelated = temp.path().join("unrelated.txt");
                    fs::write(&unrelated, b"unrelated contents").unwrap();
                    match kind {
                        "symlink" => std::os::unix::fs::symlink(&unrelated, &collision).unwrap(),
                        "hardlink" => fs::hard_link(&unrelated, &collision).unwrap(),
                        "file" => fs::write(&collision, b"unrelated contents").unwrap(),
                        _ => fs::create_dir(&collision).unwrap(),
                    }
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    app.diagnostics = History {
                        path: path.clone(),
                        records: Vec::new(),
                        pending_records: Vec::new(),
                    };
                    let key = keyboard::Key::Character(":".into());
                    drop(app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some(":")));
                    drop(app.update(Message::CommandChanged(
                        "printf diagnostic_persist_marker >&2; false".into(),
                    )));
                    let task = app.update(Message::CommandSubmitted);
                    finish(&mut app, task).await;
                    assert_eq!(
                        fs::read(&unrelated).unwrap(),
                        b"unrelated contents",
                        "A command failure overwrote the target of a {kind}"
                    );
                    if kind == "directory" {
                        assert!(collision.is_dir());
                    } else {
                        assert_eq!(fs::read(&collision).unwrap(), b"unrelated contents");
                        if kind == "symlink" {
                            assert_eq!(fs::read_link(&collision).unwrap(), unrelated);
                        }
                    }
                    assert!(
                        fs::read_to_string(&path)
                            .unwrap()
                            .contains("diagnostic_persist_marker"),
                        "An occupied temporary name must not prevent retaining the diagnostic"
                    );
                    let key = keyboard::Key::Character(":".into());
                    drop(app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some(":")));
                    drop(app.update(Message::CommandChanged("diagnostics".into())));
                    let task = app.update(Message::CommandSubmitted);
                    finish(&mut app, task).await;
                    assert!(
                        app.command
                            .output()
                            .unwrap()
                            .detail
                            .contains("diagnostic_persist_marker")
                    );
                }
            });
    }

    #[test]
    fn history_is_bounded_persistent_and_reportable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("diagnostics.json");
        let mut history = History {
            path: path.clone(),
            records: Vec::new(),
            pending_records: Vec::new(),
        };
        for index in 0..105 {
            history.record(format!("failure {index}"), "detail".to_owned());
        }
        assert_eq!(history.records.len(), MAX_RECORDS);
        assert!(!history.report().contains("failure 0\n"));
        assert!(history.report().contains("failure 104"));

        let records = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let reopened = History {
            path: temp.path().join("diagnostics.json"),
            records,
            pending_records: Vec::new(),
        };
        assert_eq!(reopened.records.len(), MAX_RECORDS);
    }
}
