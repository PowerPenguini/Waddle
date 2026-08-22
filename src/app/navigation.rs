use std::path::{Path, PathBuf};

use crate::fs::FileEntry;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Kind {
    Forward { remember: bool },
    Back { expected: PathBuf },
    HistoryForward { expected: PathBuf },
    Refresh,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Request {
    requested: PathBuf,
    kind: Kind,
    select: Vec<PathBuf>,
}

impl Request {
    pub(super) fn requested(&self) -> &Path {
        &self.requested
    }
}

#[derive(Clone, Debug)]
pub(super) enum Outcome {
    Ignored,
    Failed(String),
    Committed { selected: Vec<usize> },
}

#[derive(Clone, Debug)]
pub(super) struct NavigationSession {
    current: PathBuf,
    history: Vec<PathBuf>,
    forward_history: Vec<PathBuf>,
    entries: Vec<FileEntry>,
    pending: Option<Request>,
}

impl NavigationSession {
    pub(super) fn new(current: PathBuf) -> Self {
        Self {
            current,
            history: Vec::new(),
            forward_history: Vec::new(),
            entries: Vec::new(),
            pending: None,
        }
    }

    pub(super) fn current(&self) -> &Path {
        &self.current
    }

    pub(super) fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    pub(super) fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub(super) fn can_go_forward(&self) -> bool {
        !self.forward_history.is_empty()
    }

    pub(super) fn parent(&mut self) -> Option<Request> {
        let parent = self.current.parent()?.to_path_buf();
        let current = self.current.clone();
        Some(self.forward(parent, true, Some(current)))
    }

    pub(super) fn forward(
        &mut self,
        requested: PathBuf,
        remember: bool,
        select: Option<PathBuf>,
    ) -> Request {
        self.begin(Request {
            requested,
            kind: Kind::Forward { remember },
            select: select.into_iter().collect(),
        })
    }

    pub(super) fn back(&mut self) -> Option<Request> {
        let target = self.history.last()?.clone();
        Some(self.begin(Request {
            requested: target.clone(),
            kind: Kind::Back { expected: target },
            select: Vec::new(),
        }))
    }

    pub(super) fn history_forward(&mut self) -> Option<Request> {
        let target = self.forward_history.last()?.clone();
        Some(self.begin(Request {
            requested: target.clone(),
            kind: Kind::HistoryForward { expected: target },
            select: Vec::new(),
        }))
    }

    pub(super) fn refresh(&mut self, select: Option<PathBuf>) -> Request {
        self.refresh_selected(select.into_iter().collect())
    }

    pub(super) fn refresh_selected(&mut self, select: Vec<PathBuf>) -> Request {
        self.begin(Request {
            requested: self.current.clone(),
            kind: Kind::Refresh,
            select,
        })
    }

    fn begin(&mut self, request: Request) -> Request {
        self.pending = Some(request.clone());
        request
    }

    pub(super) fn complete(
        &mut self,
        requested: &Path,
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    ) -> Outcome {
        let Some(request) = self
            .pending
            .take_if(|pending| pending.requested == requested)
        else {
            return Outcome::Ignored;
        };
        let (canonical, entries) = match result {
            Ok(opened) => opened,
            Err(error) => return Outcome::Failed(error),
        };
        match request.kind {
            Kind::Forward { remember } => {
                if canonical != self.current && remember {
                    self.history.push(self.current.clone());
                    self.forward_history.clear();
                }
                self.current = canonical;
            }
            Kind::Back { expected } => {
                if self.history.last() != Some(&expected) {
                    return Outcome::Ignored;
                }
                self.history.pop();
                self.forward_history.push(self.current.clone());
                self.current = canonical;
            }
            Kind::HistoryForward { expected } => {
                if self.forward_history.last() != Some(&expected) {
                    return Outcome::Ignored;
                }
                self.forward_history.pop();
                self.history.push(self.current.clone());
                self.current = canonical;
            }
            Kind::Refresh => self.current = canonical,
        }
        let selected = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| request.select.contains(&entry.path).then_some(index))
            .collect();
        self.entries = entries;
        Outcome::Committed { selected }
    }

    pub(super) fn replace_displayed_entries(&mut self, entries: Vec<FileEntry>) {
        self.entries = entries;
    }

    #[cfg(test)]
    pub(super) fn pending_path(&self) -> Option<&Path> {
        self.pending.as_ref().map(Request::requested)
    }

    #[cfg(test)]
    pub(super) fn seed_history(&mut self, back: Vec<PathBuf>, forward: Vec<PathBuf>) {
        self.history = back;
        self.forward_history = forward;
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn entry(path: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from(path),
            name: OsString::from(path.rsplit('/').next().unwrap()),
            directory: false,
        }
    }

    #[test]
    fn forward_back_and_history_forward_share_one_session() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.forward(PathBuf::from("/next"), true, None);
        assert!(matches!(
            session.complete(request.requested(), Ok((PathBuf::from("/next"), vec![]))),
            Outcome::Committed { .. }
        ));
        assert_eq!(session.current(), Path::new("/next"));
        assert!(session.can_go_back());

        let request = session.back().unwrap();
        assert!(matches!(
            session.complete(request.requested(), Ok((PathBuf::from("/start"), vec![]))),
            Outcome::Committed { .. }
        ));
        assert_eq!(session.current(), Path::new("/start"));
        assert!(session.can_go_forward());

        let request = session.history_forward().unwrap();
        let _ = session.complete(request.requested(), Ok((PathBuf::from("/next"), vec![])));
        assert_eq!(session.current(), Path::new("/next"));
    }

    #[test]
    fn stale_and_failed_completions_cannot_commit() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let stale = session.forward(PathBuf::from("/stale"), true, None);
        let latest = session.forward(PathBuf::from("/latest"), true, None);

        assert!(matches!(
            session.complete(stale.requested(), Ok((PathBuf::from("/stale"), vec![]))),
            Outcome::Ignored
        ));
        assert_eq!(session.current(), Path::new("/start"));
        assert!(matches!(
            session.complete(latest.requested(), Err("missing".to_owned())),
            Outcome::Failed(error) if error == "missing"
        ));
        assert_eq!(session.current(), Path::new("/start"));
    }

    #[test]
    fn refresh_restores_requested_selection_without_changing_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let selected = PathBuf::from("/start/two");
        let request = session.refresh(Some(selected.clone()));
        let outcome = session.complete(
            request.requested(),
            Ok((
                PathBuf::from("/start"),
                vec![entry("/start/one"), entry("/start/two")],
            )),
        );

        assert!(matches!(outcome, Outcome::Committed { selected } if selected == [1]));
        assert!(!session.can_go_back());
        assert_eq!(session.entries()[1].path, selected);
    }

    #[test]
    fn refresh_restores_every_requested_selection_in_display_order() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.refresh_selected(vec![
            PathBuf::from("/start/three"),
            PathBuf::from("/start/one"),
        ]);
        let outcome = session.complete(
            request.requested(),
            Ok((
                PathBuf::from("/start"),
                vec![
                    entry("/start/one"),
                    entry("/start/two"),
                    entry("/start/three"),
                ],
            )),
        );

        assert!(matches!(outcome, Outcome::Committed { selected } if selected == [0, 2]));
    }

    #[test]
    fn parent_selects_the_folder_that_was_left() {
        let mut session = NavigationSession::new(PathBuf::from("/start/child"));
        let request = session.parent().unwrap();
        let outcome = session.complete(
            request.requested(),
            Ok((
                PathBuf::from("/start"),
                vec![entry("/start/child"), entry("/start/sibling")],
            )),
        );

        assert!(matches!(outcome, Outcome::Committed { selected } if selected == [0]));
        assert_eq!(session.current(), Path::new("/start"));
    }

    #[test]
    fn opening_the_current_folder_does_not_duplicate_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.forward(PathBuf::from("/start"), true, None);
        let _ = session.complete(
            request.requested(),
            Ok((PathBuf::from("/start"), Vec::new())),
        );

        assert!(!session.can_go_back());
    }
}
