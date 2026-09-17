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
}

impl History {
    #[cfg(not(test))]
    pub(super) fn open_default() -> Self {
        let path = state_path();
        let records = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut history = Self { path, records };
        history.prune(now());
        history
    }

    #[cfg(test)]
    pub(super) fn open_default() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "waddle-diagnostics-test-{}-{}.json",
                std::process::id(),
                now()
            )),
            records: Vec::new(),
        }
    }

    pub(super) fn record(&mut self, summary: String, detail: String) {
        let timestamp = now();
        self.records.push(Record {
            timestamp,
            summary,
            detail,
        });
        self.prune(timestamp);
        let _ = self.save();
    }

    pub(super) fn report(&self) -> String {
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

    fn prune(&mut self, timestamp: u64) {
        let oldest = timestamp.saturating_sub(RETENTION.as_secs());
        self.records.retain(|record| record.timestamp >= oldest);
        if self.records.len() > MAX_RECORDS {
            self.records.drain(..self.records.len() - MAX_RECORDS);
        }
    }

    fn save(&self) -> Result<(), String> {
        let directory = self.path.parent().ok_or("diagnostic path has no parent")?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.records).map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        temporary
            .write_all(&bytes)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        temporary
            .persist(&self.path)
            .map(|_| ())
            .map_err(|error| error.to_string())
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
        };
        assert_eq!(reopened.records.len(), MAX_RECORDS);
    }
}
