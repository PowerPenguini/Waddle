use std::path::PathBuf;

use crate::{fs, fs::FileEntry, journal};

#[derive(Clone, Debug)]
enum NameOperation {
    NewFolder,
    NewFile,
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
    NewFile {
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
    TrashDelete {
        entries: Vec<super::trash::Entry>,
        message: String,
        detail: String,
    },
    Error {
        message: String,
    },
    Warning {
        message: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) enum View<'a> {
    Idle,
    Rename { value: &'a str, error: &'a str },
    NewFolder { value: &'a str, error: &'a str },
    NewFile { value: &'a str, error: &'a str },
    Trash { message: &'a str },
    PermanentDelete { message: &'a str, detail: &'a str },
    Error { message: &'a str },
    Warning { message: &'a str },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum PromptInteraction {
    #[default]
    Inactive,
    Input,
    Acknowledgement,
    Confirmation,
}

impl PromptInteraction {
    pub(super) fn is_active(self) -> bool {
        self != Self::Inactive
    }

    pub(super) fn accepts_enter(self) -> bool {
        matches!(self, Self::Acknowledgement | Self::Confirmation)
    }

    pub(super) fn uses_yes_no(self) -> bool {
        self == Self::Confirmation
    }
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
    PermanentDelete(Vec<FileEntry>),
    TrashDelete(Vec<super::trash::Entry>),
}

impl Work {
    pub(super) fn run(self) -> Completion {
        let kind = match self.0 {
            WorkKind::Name {
                current,
                operation,
                value,
            } => {
                let kind = match &operation {
                    NameOperation::Rename(entry) => NameKind::Rename {
                        source: entry.path.clone(),
                    },
                    NameOperation::NewFile => NameKind::NewFile,
                    NameOperation::NewFolder => NameKind::NewFolder,
                };
                let result = match operation {
                    NameOperation::NewFolder => fs::create_folder(&current, &value),
                    NameOperation::NewFile => fs::create_file(&current, &value),
                    NameOperation::Rename(entry) => fs::rename_entry(&entry.path, &value),
                }
                .map_err(|error| error.to_string());
                CompletionKind::Name { kind, result }
            }
            WorkKind::PermanentDelete(entries) => {
                CompletionKind::PermanentDelete(run_entries(entries, |entry| {
                    fs::delete_permanently(&entry.path).map_err(|error| error.to_string())
                }))
            }
            WorkKind::TrashDelete(entries) => {
                CompletionKind::TrashDelete(super::trash::delete(entries))
            }
        };
        Completion::prepare(kind)
    }
}

pub(super) enum Confirmation {
    Work(Work),
    Trash(Vec<FileEntry>),
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

#[derive(Clone, Debug)]
enum NameKind {
    Rename { source: PathBuf },
    NewFolder,
    NewFile,
}

#[derive(Clone, Debug)]
enum CompletionKind {
    Name {
        kind: NameKind,
        result: Result<PathBuf, String>,
    },
    PermanentDelete(Vec<(FileEntry, String)>),
    TrashDelete(super::trash::DeleteReport),
}

#[derive(Clone, Debug)]
pub(super) struct Completion {
    kind: CompletionKind,
    journal_action: Result<Option<journal::Action>, String>,
}

impl Completion {
    fn prepare(kind: CompletionKind) -> Self {
        let journal_action = journal_action(&kind).map_err(|error| error.to_string());
        Self {
            kind,
            journal_action,
        }
    }
}

pub(super) struct CompletionEffects {
    pub(super) status: Option<String>,
    pub(super) detail: Option<String>,
    pub(super) journal_action: Result<Option<journal::Action>, String>,
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
            State::NewFile { value, error } => View::NewFile { value, error },
            State::Trash { message, .. } => View::Trash { message },
            State::PermanentDelete {
                message, detail, ..
            }
            | State::TrashDelete {
                message, detail, ..
            } => View::PermanentDelete { message, detail },
            State::Error { message } => View::Error { message },
            State::Warning { message } => View::Warning { message },
        }
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(super) fn prompt_active(&self) -> bool {
        self.prompt_interaction().is_active()
    }

    pub(super) fn prompt_interaction(&self) -> PromptInteraction {
        match self.state {
            State::NewFolder { .. } | State::NewFile { .. } => PromptInteraction::Input,
            State::Trash { .. } | State::PermanentDelete { .. } | State::TrashDelete { .. } => {
                PromptInteraction::Confirmation
            }
            State::Error { .. } | State::Warning { .. } => PromptInteraction::Acknowledgement,
            State::Idle | State::Rename { .. } => PromptInteraction::Inactive,
        }
    }

    pub(super) fn expanded_detail(&self) -> Option<&str> {
        match &self.state {
            State::PermanentDelete { detail, .. }
            | State::TrashDelete { detail, .. }
            | State::Error { message: detail }
            | State::Warning { message: detail } => Some(detail),
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

    pub(super) fn begin_new_file(&mut self) {
        self.busy = false;
        self.state = State::NewFile {
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

    pub(super) fn begin_trash_delete(
        &mut self,
        entries: Vec<super::trash::Entry>,
        empty: bool,
    ) -> bool {
        if entries.is_empty() {
            return false;
        }
        let message = if empty {
            format!(
                "Empty Trash and permanently delete {} items?",
                entries.len()
            )
        } else if entries.len() == 1 {
            format!(
                "Permanently delete “{}” from Trash?",
                fs::display_name(&entries[0].file.name)
            )
        } else {
            format!("Permanently delete {} selected Trash items?", entries.len())
        };
        self.busy = false;
        self.state = State::TrashDelete {
            entries,
            message,
            detail: "This cannot be undone.".to_owned(),
        };
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
            }
            | State::NewFile {
                value: target,
                error,
                ..
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
            State::NewFile { value, error } => (NameOperation::NewFile, value.clone(), error),
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

    pub(super) fn confirm(&mut self, current: PathBuf) -> Option<Confirmation> {
        if self.busy {
            return None;
        }
        match &self.state {
            State::NewFolder { .. } => self.submit_name(current).map(Confirmation::Work),
            State::NewFile { .. } => self.submit_name(current).map(Confirmation::Work),
            State::Trash { entries, .. } => {
                let entries = entries.clone();
                self.state = State::Idle;
                Some(Confirmation::Trash(entries))
            }
            State::PermanentDelete { entries, .. } => {
                self.busy = true;
                Some(Confirmation::Work(Work(WorkKind::PermanentDelete(
                    entries.clone(),
                ))))
            }
            State::TrashDelete { entries, .. } => {
                self.busy = true;
                Some(Confirmation::Work(Work(WorkKind::TrashDelete(
                    entries.clone(),
                ))))
            }
            State::Error { .. } | State::Warning { .. } => {
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

    pub(super) fn show_warning(&mut self, message: String) {
        self.busy = false;
        self.state = State::Warning { message };
    }

    pub(super) fn finish_trash_transfer(&mut self, failures: Vec<(FileEntry, String)>) {
        self.busy = false;
        if failures.is_empty() {
            self.state = State::Idle;
            return;
        }
        let detail = failure_detail(&failures);
        let entries = failures
            .into_iter()
            .map(|(entry, _)| entry)
            .collect::<Vec<_>>();
        self.state = State::PermanentDelete {
            message: permanent_delete_confirmation(entries.len()),
            detail: format!("{detail}\n\nThis cannot be undone."),
            entries,
        };
    }

    pub(super) fn complete(&mut self, completion: Completion) -> CompletionEffects {
        self.busy = false;
        let Completion {
            kind,
            journal_action,
        } = completion;
        let (status, detail) = completion_feedback(&kind);
        let consequences = match kind {
            CompletionKind::Name { kind, result } => match result {
                Ok(path) => {
                    let renamed = matches!(kind, NameKind::Rename { .. });
                    self.state = State::Idle;
                    StateConsequences {
                        refresh: true,
                        select: Some(path),
                        renamed,
                    }
                }
                Err(error) => {
                    match &mut self.state {
                        State::Rename { error: target, .. }
                        | State::NewFolder { error: target, .. }
                        | State::NewFile { error: target, .. } => *target = error,
                        _ => self.state = State::Error { message: error },
                    }
                    StateConsequences::default()
                }
            },
            CompletionKind::PermanentDelete(failures) => {
                if failures.is_empty() {
                    self.state = State::Idle;
                    StateConsequences {
                        refresh: true,
                        ..StateConsequences::default()
                    }
                } else {
                    self.state = State::Error {
                        message: failure_detail(&failures),
                    };
                    StateConsequences::default()
                }
            }
            CompletionKind::TrashDelete(_) => {
                self.state = State::Idle;
                StateConsequences {
                    refresh: true,
                    ..StateConsequences::default()
                }
            }
        };
        CompletionEffects {
            status,
            detail,
            journal_action,
            refresh: consequences.refresh,
            select: consequences.select,
            renamed: consequences.renamed,
        }
    }
}

#[derive(Default)]
struct StateConsequences {
    refresh: bool,
    select: Option<PathBuf>,
    renamed: bool,
}

fn journal_action(completion: &CompletionKind) -> Result<Option<journal::Action>, journal::Error> {
    match completion {
        CompletionKind::Name {
            kind: NameKind::Rename { source },
            result: Ok(destination),
        } => journal::Action::rename(source.clone(), destination.clone()).map(Some),
        CompletionKind::Name {
            kind: NameKind::NewFolder,
            result: Ok(path),
        } => journal::Action::new_folder(path.clone()).map(Some),
        CompletionKind::Name {
            kind: NameKind::NewFile,
            result: Ok(path),
        } => journal::Action::new_file(path.clone()).map(Some),
        CompletionKind::Name { .. }
        | CompletionKind::PermanentDelete(_)
        | CompletionKind::TrashDelete(_) => Ok(None),
    }
}

fn completion_feedback(completion: &CompletionKind) -> (Option<String>, Option<String>) {
    let CompletionKind::TrashDelete(report) = completion else {
        return (None, None);
    };
    let status = format!(
        "Permanently deleted {}  •  {} failed",
        report.deleted,
        report.failures.len()
    );
    let detail = (!report.failures.is_empty()).then(|| {
        report
            .failures
            .iter()
            .map(|(entry, error)| format!("{}: {error}", fs::display_name(&entry.name)))
            .collect::<Vec<_>>()
            .join("\n")
    });
    (Some(status), detail)
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
    use super::*;

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from("/work").join(name),
            name: name.into(),
            directory: false,
            metadata: Default::default(),
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
        let effects = session.complete(Completion::prepare(CompletionKind::Name {
            kind: NameKind::Rename {
                source: PathBuf::from("/work/old.txt"),
            },
            result: Err("collision".to_owned()),
        }));
        assert!(!effects.refresh);
        assert!(effects.select.is_none());
        assert!(!effects.renamed);
        assert!(matches!(
            session.view(),
            View::Rename { value, error } if value == "new.txt" && error == "collision"
        ));
    }

    #[test]
    fn new_file_uses_the_name_prompt_and_reports_conflicts_inline() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = FileOperationSession::default();
        session.begin_new_file();
        session.change_name("created.md".to_owned());
        let completion = session
            .submit_name(temp.path().to_path_buf())
            .unwrap()
            .run();
        assert!(matches!(
            &completion.kind,
            CompletionKind::Name {
                kind: NameKind::NewFile,
                result: Ok(path),
            } if path == &temp.path().join("created.md")
        ));
        let effects = session.complete(completion);
        assert!(effects.refresh);
        assert!(matches!(effects.journal_action, Ok(Some(_))));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("created.md")).unwrap(),
            ""
        );

        let mut session = FileOperationSession::default();
        session.begin_new_file();
        session.change_name("created.md".to_owned());
        let completion = session
            .submit_name(temp.path().to_path_buf())
            .unwrap()
            .run();
        let effects = session.complete(completion);
        assert!(!effects.refresh);
        assert!(matches!(effects.journal_action, Ok(None)));
        assert!(matches!(
            session.view(),
            View::NewFile { error, .. } if error.contains("exists")
        ));
    }

    #[test]
    fn trash_confirmation_returns_a_transfer_and_partial_failure_escalates() {
        let one = entry("one.txt");
        let two = entry("two.txt");
        let mut session = FileOperationSession::default();
        assert!(session.begin_trash(vec![one.clone(), two.clone()]));
        let Some(Confirmation::Trash(entries)) = session.confirm(PathBuf::from("/work")) else {
            panic!("Trash confirmation must return a Transfer request");
        };
        assert_eq!(entries.len(), 2);
        assert!(matches!(session.view(), View::Idle));

        session.finish_trash_transfer(vec![(two, "Trash unavailable".to_owned())]);
        assert!(matches!(
            session.view(),
            View::PermanentDelete { message, detail }
                if message.contains("Permanently delete")
                    && detail.contains("Trash unavailable")
                    && detail.contains("cannot be undone")
        ));
    }

    #[test]
    fn new_folder_runs_through_the_session_and_can_be_cancelled() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = FileOperationSession::default();
        session.begin_new_folder();
        session.change_name("created".to_owned());
        let completion = session
            .submit_name(temp.path().to_path_buf())
            .unwrap()
            .run();
        let effects = session.complete(completion);

        assert!(temp.path().join("created").is_dir());
        assert!(effects.refresh);
        assert_eq!(effects.select, Some(temp.path().join("created")));

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
        session.finish_trash_transfer(vec![(failed.clone(), "Trash unavailable".to_owned())]);
        assert!(session.cancel());
        assert!(matches!(session.view(), View::Idle));

        assert!(session.begin_trash(vec![failed.clone()]));
        let _ = session.confirm(PathBuf::from("/work"));
        session.finish_trash_transfer(vec![(failed.clone(), "Trash unavailable".to_owned())]);
        assert!(session.confirm(PathBuf::from("/work")).is_some());
        let effects = session.complete(Completion::prepare(CompletionKind::PermanentDelete(vec![
            (failed, "Permission denied".to_owned()),
        ])));

        assert!(!effects.refresh);
        assert!(matches!(
            session.view(),
            View::Error { message } if message.contains("Permission denied")
        ));
    }

    #[test]
    fn trash_delete_feedback_is_interpreted_inside_the_session() {
        let failed = entry("failed.txt");
        let mut session = FileOperationSession::default();
        let effects = session.complete(Completion::prepare(CompletionKind::TrashDelete(
            crate::app::trash::DeleteReport {
                deleted: 2,
                failures: vec![(failed, "Permission denied".to_owned())],
            },
        )));

        assert_eq!(
            effects.status.as_deref(),
            Some("Permanently deleted 2  •  1 failed")
        );
        assert!(
            effects
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("Permission denied"))
        );
        assert!(effects.refresh);
    }
}
