use std::path::{Path, PathBuf};

use crate::{fs, fs::FileEntry, journal};

pub(super) trait TrashAdapter {
    fn trash(&self, path: &Path) -> Result<journal::TrashReceipt, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct GioTrashAdapter;

impl TrashAdapter for GioTrashAdapter {
    fn trash(&self, path: &Path) -> Result<journal::TrashReceipt, String> {
        journal::trash(path)
    }
}

#[derive(Clone, Debug)]
enum NameOperation {
    NewFolder,
    Rename(FileEntry),
}

#[derive(Clone, Debug)]
enum State {
    Idle,
    Rename {
        entry: FileEntry,
        value: String,
        error: String,
    },
    NewFolder {
        value: String,
        error: String,
    },
    Trash {
        entries: Vec<FileEntry>,
        message: String,
    },
    PermanentDelete {
        entries: Vec<FileEntry>,
        message: String,
        detail: String,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) enum View<'a> {
    Idle,
    Rename { value: &'a str, error: &'a str },
    NewFolder { value: &'a str, error: &'a str },
    Trash { message: &'a str },
    PermanentDelete { message: &'a str, detail: &'a str },
    Error { message: &'a str },
}

#[derive(Clone, Debug)]
pub(super) struct Work(WorkKind);

#[derive(Clone, Debug)]
enum WorkKind {
    Name {
        current: PathBuf,
        operation: NameOperation,
        value: String,
    },
    Trash(Vec<FileEntry>),
    PermanentDelete(Vec<FileEntry>),
}

impl Work {
    pub(super) fn run<A: TrashAdapter>(self, trash: &A) -> Completion {
        match self.0 {
            WorkKind::Name {
                current,
                operation,
                value,
            } => {
                let renamed = matches!(operation, NameOperation::Rename(_));
                let source = match &operation {
                    NameOperation::Rename(entry) => Some(entry.path.clone()),
                    NameOperation::NewFolder => None,
                };
                let result = match operation {
                    NameOperation::NewFolder => fs::create_folder(&current, &value),
                    NameOperation::Rename(entry) => fs::rename_entry(&entry.path, &value),
                }
                .map_err(|error| error.to_string());
                Completion::Name {
                    renamed,
                    source,
                    result,
                }
            }
            WorkKind::Trash(entries) => Completion::Trash(run_trash_entries(entries, trash)),
            WorkKind::PermanentDelete(entries) => {
                Completion::PermanentDelete(run_entries(entries, |entry| {
                    fs::delete_permanently(&entry.path).map_err(|error| error.to_string())
                }))
            }
        }
    }
}

fn run_entries(
    entries: Vec<FileEntry>,
    mut operation: impl FnMut(&FileEntry) -> Result<(), String>,
) -> Vec<(FileEntry, String)> {
    entries
        .into_iter()
        .filter_map(|entry| operation(&entry).err().map(|error| (entry, error)))
        .collect()
}

fn run_trash_entries<A: TrashAdapter>(entries: Vec<FileEntry>, trash: &A) -> TrashCompletion {
    let mut failures = Vec::new();
    let mut receipts = Vec::new();
    for entry in entries {
        match trash.trash(&entry.path) {
            Ok(receipt) => receipts.push(receipt),
            Err(error) => failures.push((entry, error)),
        }
    }
    TrashCompletion { failures, receipts }
}

#[derive(Clone, Debug)]
pub(super) enum Completion {
    Name {
        renamed: bool,
        source: Option<PathBuf>,
        result: Result<PathBuf, String>,
    },
    Trash(TrashCompletion),
    PermanentDelete(Vec<(FileEntry, String)>),
}

#[derive(Clone, Debug, Default)]
pub(super) struct TrashCompletion {
    pub(super) failures: Vec<(FileEntry, String)>,
    pub(super) receipts: Vec<journal::TrashReceipt>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Consequences {
    pub(super) refresh: bool,
    pub(super) select: Option<PathBuf>,
    pub(super) renamed: bool,
}

#[derive(Clone, Debug)]
pub(super) struct FileOperationSession {
    state: State,
    busy: bool,
}

impl Default for FileOperationSession {
    fn default() -> Self {
        Self {
            state: State::Idle,
            busy: false,
        }
    }
}

impl FileOperationSession {
    pub(super) fn view(&self) -> View<'_> {
        match &self.state {
            State::Idle => View::Idle,
            State::Rename { value, error, .. } => View::Rename { value, error },
            State::NewFolder { value, error } => View::NewFolder { value, error },
            State::Trash { message, .. } => View::Trash { message },
            State::PermanentDelete {
                message, detail, ..
            } => View::PermanentDelete { message, detail },
            State::Error { message } => View::Error { message },
        }
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(super) fn prompt_active(&self) -> bool {
        matches!(
            self.state,
            State::NewFolder { .. }
                | State::Trash { .. }
                | State::PermanentDelete { .. }
                | State::Error { .. }
        )
    }

    pub(super) fn prompt_accepts_enter(&self) -> bool {
        matches!(
            self.state,
            State::Trash { .. } | State::PermanentDelete { .. } | State::Error { .. }
        )
    }

    pub(super) fn prompt_uses_yes_no(&self) -> bool {
        matches!(
            self.state,
            State::Trash { .. } | State::PermanentDelete { .. }
        )
    }

    pub(super) fn expanded_detail(&self) -> Option<&str> {
        match &self.state {
            State::PermanentDelete { detail, .. } | State::Error { message: detail } => {
                Some(detail)
            }
            _ => None,
        }
    }

    pub(super) fn begin_rename(&mut self, entry: FileEntry) {
        self.busy = false;
        self.state = State::Rename {
            value: fs::display_name(&entry.name),
            entry,
            error: String::new(),
        };
    }

    pub(super) fn begin_new_folder(&mut self) {
        self.busy = false;
        self.state = State::NewFolder {
            value: String::new(),
            error: String::new(),
        };
    }

    pub(super) fn begin_trash(&mut self, entries: Vec<FileEntry>) -> bool {
        if entries.is_empty() {
            return false;
        }
        let message = deletion_confirmation(&entries);
        self.busy = false;
        self.state = State::Trash { entries, message };
        true
    }

    pub(super) fn change_name(&mut self, value: String) {
        if self.busy {
            return;
        }
        match &mut self.state {
            State::Rename {
                value: target,
                error,
                ..
            }
            | State::NewFolder {
                value: target,
                error,
            } => {
                *target = value;
                error.clear();
            }
            _ => {}
        }
    }

    pub(super) fn submit_name(&mut self, current: PathBuf) -> Option<Work> {
        if self.busy {
            return None;
        }
        let (operation, value, error) = match &mut self.state {
            State::Rename {
                entry,
                value,
                error,
            } => (NameOperation::Rename(entry.clone()), value.clone(), error),
            State::NewFolder { value, error } => (NameOperation::NewFolder, value.clone(), error),
            _ => return None,
        };
        if let Err(validation) = fs::validate_name(&value) {
            *error = validation.to_owned();
            return None;
        }
        self.busy = true;
        Some(Work(WorkKind::Name {
            current,
            operation,
            value,
        }))
    }

    pub(super) fn confirm(&mut self, current: PathBuf) -> Option<Work> {
        if self.busy {
            return None;
        }
        match &self.state {
            State::NewFolder { .. } => self.submit_name(current),
            State::Trash { entries, .. } => {
                self.busy = true;
                Some(Work(WorkKind::Trash(entries.clone())))
            }
            State::PermanentDelete { entries, .. } => {
                self.busy = true;
                Some(Work(WorkKind::PermanentDelete(entries.clone())))
            }
            State::Error { .. } => {
                self.state = State::Idle;
                None
            }
            State::Idle | State::Rename { .. } => None,
        }
    }

    pub(super) fn cancel(&mut self) -> bool {
        if self.busy {
            return false;
        }
        self.state = State::Idle;
        true
    }

    pub(super) fn show_error(&mut self, message: String) {
        self.busy = false;
        self.state = State::Error { message };
    }

    pub(super) fn complete(&mut self, completion: Completion) -> Consequences {
        self.busy = false;
        match completion {
            Completion::Name {
                renamed, result, ..
            } => match result {
                Ok(path) => {
                    self.state = State::Idle;
                    Consequences {
                        refresh: true,
                        select: Some(path),
                        renamed,
                    }
                }
                Err(error) => {
                    match &mut self.state {
                        State::Rename { error: target, .. }
                        | State::NewFolder { error: target, .. } => *target = error,
                        _ => self.state = State::Error { message: error },
                    }
                    Consequences::default()
                }
            },
            Completion::Trash(TrashCompletion { failures, .. }) => {
                if failures.is_empty() {
                    self.state = State::Idle;
                    return Consequences {
                        refresh: true,
                        ..Consequences::default()
                    };
                }
                let detail = failure_detail(&failures);
                let entries: Vec<_> = failures.into_iter().map(|(entry, _)| entry).collect();
                self.state = State::PermanentDelete {
                    message: permanent_delete_confirmation(entries.len()),
                    detail: format!("{detail}\n\nThis cannot be undone."),
                    entries,
                };
                Consequences::default()
            }
            Completion::PermanentDelete(failures) => {
                if failures.is_empty() {
                    self.state = State::Idle;
                    Consequences {
                        refresh: true,
                        ..Consequences::default()
                    }
                } else {
                    self.state = State::Error {
                        message: failure_detail(&failures),
                    };
                    Consequences::default()
                }
            }
        }
    }
}

fn deletion_confirmation(entries: &[FileEntry]) -> String {
    if entries.len() == 1 {
        format!("Move “{}” to Trash?", fs::display_name(&entries[0].name))
    } else {
        format!("Move {} selected items to Trash?", entries.len())
    }
}

fn permanent_delete_confirmation(count: usize) -> String {
    if count == 1 {
        "Permanently delete this item instead?".to_owned()
    } else {
        format!("Permanently delete these {count} items instead?")
    }
}

fn failure_detail(failures: &[(FileEntry, String)]) -> String {
    failures
        .iter()
        .map(|(entry, reason)| format!("{}: {reason}", fs::display_name(&entry.name)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Mutex};

    use super::*;

    #[derive(Default)]
    struct MemoryTrashAdapter {
        failures: BTreeSet<PathBuf>,
        calls: Mutex<Vec<PathBuf>>,
    }

    impl TrashAdapter for MemoryTrashAdapter {
        fn trash(&self, path: &Path) -> Result<journal::TrashReceipt, String> {
            self.calls.lock().unwrap().push(path.to_path_buf());
            if self.failures.contains(path) {
                Err("Trash unavailable".to_owned())
            } else {
                Ok(journal::TrashReceipt {
                    original: path.to_path_buf(),
                    trashed: PathBuf::from("/trash").join(path.file_name().unwrap_or_default()),
                    info: PathBuf::from("/trash/info"),
                })
            }
        }
    }

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from("/work").join(name),
            name: name.into(),
            directory: false,
        }
    }

    #[test]
    fn rename_validation_and_failure_stay_inside_the_session() {
        let mut session = FileOperationSession::default();
        session.begin_rename(entry("old.txt"));
        session.change_name("bad/name".to_owned());

        assert!(session.submit_name(PathBuf::from("/work")).is_none());
        assert!(matches!(
            session.view(),
            View::Rename { value, error }
                if value == "bad/name" && error.contains("slash")
        ));

        session.change_name("new.txt".to_owned());
        assert!(session.submit_name(PathBuf::from("/work")).is_some());
        let consequences = session.complete(Completion::Name {
            renamed: true,
            source: None,
            result: Err("collision".to_owned()),
        });
        assert_eq!(consequences, Consequences::default());
        assert!(matches!(
            session.view(),
            View::Rename { value, error } if value == "new.txt" && error == "collision"
        ));
    }

    #[test]
    fn trash_adapter_is_real_and_partial_failure_escalates() {
        let one = entry("one.txt");
        let two = entry("two.txt");
        let adapter = MemoryTrashAdapter {
            failures: BTreeSet::from([two.path.clone()]),
            ..MemoryTrashAdapter::default()
        };
        let mut session = FileOperationSession::default();
        assert!(session.begin_trash(vec![one.clone(), two.clone()]));
        let work = session.confirm(PathBuf::from("/work")).unwrap();
        let completion = work.run(&adapter);

        assert_eq!(
            adapter.calls.lock().unwrap().as_slice(),
            [one.path, two.path]
        );
        assert_eq!(session.complete(completion), Consequences::default());
        assert!(matches!(
            session.view(),
            View::PermanentDelete { message, detail }
                if message.contains("Permanently delete")
                    && detail.contains("Trash unavailable")
                    && detail.contains("cannot be undone")
        ));
        let _: &dyn TrashAdapter = &GioTrashAdapter;
    }

    #[test]
    fn successful_completion_returns_refresh_consequences() {
        let mut session = FileOperationSession::default();
        assert!(session.begin_trash(vec![entry("one.txt")]));
        let _ = session.confirm(PathBuf::from("/work"));

        assert_eq!(
            session.complete(Completion::Trash(TrashCompletion::default())),
            Consequences {
                refresh: true,
                ..Consequences::default()
            }
        );
        assert!(matches!(session.view(), View::Idle));
    }

    #[test]
    fn new_folder_runs_through_the_session_and_can_be_cancelled() {
        let temp = tempfile::tempdir().unwrap();
        let adapter = MemoryTrashAdapter::default();
        let mut session = FileOperationSession::default();
        session.begin_new_folder();
        session.change_name("created".to_owned());
        let completion = session
            .submit_name(temp.path().to_path_buf())
            .unwrap()
            .run(&adapter);
        let consequences = session.complete(completion);

        assert!(temp.path().join("created").is_dir());
        assert!(consequences.refresh);
        assert_eq!(consequences.select, Some(temp.path().join("created")));

        session.begin_new_folder();
        session.change_name("discarded".to_owned());
        assert!(session.cancel());
        assert!(matches!(session.view(), View::Idle));
    }

    #[test]
    fn permanent_delete_failure_and_cancellation_stay_in_the_session() {
        let failed = entry("failed.txt");
        let mut session = FileOperationSession::default();
        assert!(session.begin_trash(vec![failed.clone()]));
        let _ = session.confirm(PathBuf::from("/work"));
        let _ = session.complete(Completion::Trash(TrashCompletion {
            failures: vec![(failed.clone(), "Trash unavailable".to_owned())],
            receipts: Vec::new(),
        }));
        assert!(session.cancel());
        assert!(matches!(session.view(), View::Idle));

        assert!(session.begin_trash(vec![failed.clone()]));
        let _ = session.confirm(PathBuf::from("/work"));
        let _ = session.complete(Completion::Trash(TrashCompletion {
            failures: vec![(failed.clone(), "Trash unavailable".to_owned())],
            receipts: Vec::new(),
        }));
        assert!(session.confirm(PathBuf::from("/work")).is_some());
        let consequences = session.complete(Completion::PermanentDelete(vec![(
            failed,
            "Permission denied".to_owned(),
        )]));

        assert_eq!(consequences, Consequences::default());
        assert!(matches!(
            session.view(),
            View::Error { message } if message.contains("Permission denied")
        ));
    }
}
