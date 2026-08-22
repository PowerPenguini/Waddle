use std::{
    fs,
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
                "polarexp-diagnostics-test-{}-{}.json",
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
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&self.records).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, &self.path).map_err(|error| error.to_string())
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
        return PathBuf::from(path).join("polarexp/diagnostics.json");
    }
    std::env::var_os("HOME").map_or_else(
        || PathBuf::from(".polarexp-diagnostics.json"),
        |home| PathBuf::from(home).join(".local/state/polarexp/diagnostics.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
