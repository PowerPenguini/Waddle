use std::path::{Path, PathBuf};

use crate::fs::FileEntry;

use super::trash;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Kind {
    Forward { remember: bool },
    Back { expected: PathBuf },
    HistoryForward { expected: PathBuf },
    Refresh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DisplayedLocation {
    Folder,
    Recent,
    Trash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Target {
    Folder { requested: PathBuf, kind: Kind },
    Recent,
    Trash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Request {
    id: u64,
    target: Target,
    select: Vec<PathBuf>,
}

impl Request {
    pub(super) fn requested(&self) -> Option<&Path> {
        match &self.target {
            Target::Folder { requested, .. } => Some(requested),
            Target::Recent | Target::Trash => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Outcome {
    Ignored,
    Failed {
        error: String,
        refresh: bool,
    },
    Committed {
        selected: Vec<usize>,
        refresh: bool,
        location: DisplayedLocation,
    },
}

pub(super) enum Completion {
    Folder(Result<(PathBuf, Vec<FileEntry>), String>),
    Recent(Result<Vec<FileEntry>, String>),
    Trash(Result<Vec<trash::Entry>, String>),
    Cancelled,
}

#[derive(Clone, Debug)]
struct Display {
    location: DisplayedLocation,
    entries: Vec<FileEntry>,
    trash_entries: Vec<trash::Entry>,
}

#[derive(Clone, Debug)]
pub(super) struct NavigationSession {
    current: PathBuf,
    history: Vec<PathBuf>,
    forward_history: Vec<PathBuf>,
    display: Display,
    pending: Option<Request>,
    next_request_id: u64,
    recursive_origin: Option<Display>,
}

impl NavigationSession {
    pub(super) fn new(current: PathBuf) -> Self {
        Self {
            current,
            history: Vec::new(),
            forward_history: Vec::new(),
            display: Display {
                location: DisplayedLocation::Folder,
                entries: Vec::new(),
                trash_entries: Vec::new(),
            },
            pending: None,
            next_request_id: 1,
            recursive_origin: None,
        }
    }

    pub(super) fn current(&self) -> &Path {
        &self.current
    }

    pub(super) fn entries(&self) -> &[FileEntry] {
        &self.display.entries
    }

    pub(super) fn displayed_location(&self) -> DisplayedLocation {
        self.display.location
    }

    pub(super) fn folder_displayed(&self) -> bool {
        self.display.location == DisplayedLocation::Folder
    }

    pub(super) fn location_label(&self) -> String {
        match self.display.location {
            DisplayedLocation::Folder => self.current.display().to_string(),
            DisplayedLocation::Recent => "Recent".to_owned(),
            DisplayedLocation::Trash => "Trash".to_owned(),
        }
    }

    pub(super) fn trash_entries(&self) -> &[trash::Entry] {
        &self.display.trash_entries
    }

    pub(super) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub(super) fn can_go_forward(&self) -> bool {
        !self.forward_history.is_empty()
    }

    pub(super) fn parent(&mut self) -> Option<Request> {
        if !self.folder_displayed() {
            return Some(self.forward(self.current.clone(), false, None));
        }
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
        self.begin(
            Target::Folder {
                requested,
                kind: Kind::Forward { remember },
            },
            select.into_iter().collect(),
        )
    }

    pub(super) fn back(&mut self) -> Option<Request> {
        if !self.folder_displayed() {
            return Some(self.forward(self.current.clone(), false, None));
        }
        let target = self.history.last()?.clone();
        Some(self.begin(
            Target::Folder {
                requested: target.clone(),
                kind: Kind::Back { expected: target },
            },
            Vec::new(),
        ))
    }

    pub(super) fn history_forward(&mut self) -> Option<Request> {
        if !self.folder_displayed() {
            return Some(self.forward(self.current.clone(), false, None));
        }
        let target = self.forward_history.last()?.clone();
        Some(self.begin(
            Target::Folder {
                requested: target.clone(),
                kind: Kind::HistoryForward { expected: target },
            },
            Vec::new(),
        ))
    }

    pub(super) fn refresh(&mut self, select: Option<PathBuf>) -> Request {
        self.refresh_selected(select.into_iter().collect())
    }

    pub(super) fn refresh_selected(&mut self, select: Vec<PathBuf>) -> Request {
        self.begin(
            Target::Folder {
                requested: self.current.clone(),
                kind: Kind::Refresh,
            },
            select,
        )
    }

    pub(super) fn recent(&mut self) -> Request {
        self.begin(Target::Recent, Vec::new())
    }

    pub(super) fn trash(&mut self) -> Request {
        self.begin(Target::Trash, Vec::new())
    }

    fn begin(&mut self, target: Target, select: Vec<PathBuf>) -> Request {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request = Request { id, target, select };
        self.pending = Some(request.clone());
        request
    }

    pub(super) fn complete(&mut self, request: &Request, completion: Completion) -> Outcome {
        if self.pending.as_ref().map(|pending| pending.id) != Some(request.id) {
            return Outcome::Ignored;
        }
        self.pending = None;
        if matches!(completion, Completion::Cancelled) {
            return Outcome::Ignored;
        }
        match (&request.target, completion) {
            (Target::Folder { kind, .. }, Completion::Folder(result)) => {
                self.complete_folder(kind, &request.select, result)
            }
            (Target::Recent, Completion::Recent(result)) => self.complete_recent(result),
            (Target::Trash, Completion::Trash(result)) => self.complete_trash(result),
            _ => Outcome::Ignored,
        }
    }

    fn complete_folder(
        &mut self,
        kind: &Kind,
        select: &[PathBuf],
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    ) -> Outcome {
        let (canonical, entries) = match result {
            Ok(opened) => opened,
            Err(error) => {
                return Outcome::Failed {
                    error,
                    refresh: matches!(kind, Kind::Refresh),
                };
            }
        };
        let refresh = matches!(kind, Kind::Refresh);
        match kind {
            Kind::Forward { remember } => {
                if canonical != self.current && *remember {
                    self.history.push(self.current.clone());
                    self.forward_history.clear();
                }
                self.current = canonical;
            }
            Kind::Back { expected } => {
                if self.history.last() != Some(expected) {
                    return Outcome::Ignored;
                }
                self.history.pop();
                self.forward_history.push(self.current.clone());
                self.current = canonical;
            }
            Kind::HistoryForward { expected } => {
                if self.forward_history.last() != Some(expected) {
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
            .filter_map(|(index, entry)| select.contains(&entry.path).then_some(index))
            .collect();
        self.display = Display {
            location: DisplayedLocation::Folder,
            entries,
            trash_entries: Vec::new(),
        };
        self.recursive_origin = None;
        Outcome::Committed {
            selected,
            refresh,
            location: DisplayedLocation::Folder,
        }
    }

    fn complete_recent(&mut self, result: Result<Vec<FileEntry>, String>) -> Outcome {
        let entries = match result {
            Ok(entries) => entries,
            Err(error) => {
                return Outcome::Failed {
                    error,
                    refresh: false,
                };
            }
        };
        self.display = Display {
            location: DisplayedLocation::Recent,
            entries,
            trash_entries: Vec::new(),
        };
        self.recursive_origin = None;
        Outcome::Committed {
            selected: Vec::new(),
            refresh: false,
            location: DisplayedLocation::Recent,
        }
    }

    fn complete_trash(&mut self, result: Result<Vec<trash::Entry>, String>) -> Outcome {
        let trash_entries = match result {
            Ok(entries) => entries,
            Err(error) => {
                return Outcome::Failed {
                    error,
                    refresh: false,
                };
            }
        };
        let entries = trash_entries
            .iter()
            .map(|entry| entry.file.clone())
            .collect();
        self.display = Display {
            location: DisplayedLocation::Trash,
            entries,
            trash_entries,
        };
        self.recursive_origin = None;
        Outcome::Committed {
            selected: Vec::new(),
            refresh: false,
            location: DisplayedLocation::Trash,
        }
    }

    pub(super) fn begin_recursive_display(&mut self) {
        if self.recursive_origin.is_none() {
            self.recursive_origin = Some(self.display.clone());
        }
    }

    pub(super) fn install_recursive_entries(&mut self, entries: Vec<FileEntry>) {
        self.display.entries = entries;
    }

    pub(super) fn restore_recursive_display(&mut self) {
        if let Some(display) = self.recursive_origin.take() {
            self.display = display;
        }
    }

    pub(super) fn hide_paths(&mut self, paths: &[PathBuf]) {
        self.display
            .entries
            .retain(|entry| !paths.iter().any(|path| path == &entry.path));
    }

    #[cfg(test)]
    pub(super) fn pending_path(&self) -> Option<&Path> {
        self.pending.as_ref().and_then(Request::requested)
    }

    #[cfg(test)]
    pub(super) fn pending_request(&self) -> Option<Request> {
        self.pending.clone()
    }

    #[cfg(test)]
    pub(super) fn install_folder_entries(&mut self, entries: Vec<FileEntry>) {
        self.pending = None;
        self.display = Display {
            location: DisplayedLocation::Folder,
            entries,
            trash_entries: Vec::new(),
        };
    }

    #[cfg(test)]
    pub(super) fn replace_displayed_entries(&mut self, entries: Vec<FileEntry>) {
        self.install_folder_entries(entries);
    }

    #[cfg(test)]
    pub(super) fn install_trash_entries(&mut self, entries: Vec<trash::Entry>) {
        let request = self.trash();
        let _ = self.complete(&request, Completion::Trash(Ok(entries)));
    }

    #[cfg(test)]
    pub(super) fn settle_for_test(&mut self) {
        self.pending = None;
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
            metadata: Default::default(),
        }
    }

    fn trash_entry(path: &str, original: &str) -> trash::Entry {
        let file = entry(path);
        trash::Entry {
            receipt: crate::journal::TrashReceipt {
                original: PathBuf::from(original),
                trashed: file.path.clone(),
                info: PathBuf::from(format!("{path}.trashinfo")),
            },
            file,
        }
    }

    #[test]
    fn forward_back_and_history_forward_share_one_session() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.forward(PathBuf::from("/next"), true, None);
        assert!(matches!(
            session.complete(
                &request,
                Completion::Folder(Ok((PathBuf::from("/next"), vec![])))
            ),
            Outcome::Committed { .. }
        ));
        assert_eq!(session.current(), Path::new("/next"));
        assert!(session.can_go_back());

        let request = session.back().unwrap();
        assert!(matches!(
            session.complete(
                &request,
                Completion::Folder(Ok((PathBuf::from("/start"), vec![])))
            ),
            Outcome::Committed { .. }
        ));
        assert_eq!(session.current(), Path::new("/start"));
        assert!(session.can_go_forward());

        let request = session.history_forward().unwrap();
        let _ = session.complete(
            &request,
            Completion::Folder(Ok((PathBuf::from("/next"), vec![]))),
        );
        assert_eq!(session.current(), Path::new("/next"));
    }

    #[test]
    fn stale_and_failed_completions_cannot_commit() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let stale = session.forward(PathBuf::from("/stale"), true, None);
        let latest = session.forward(PathBuf::from("/latest"), true, None);

        assert!(matches!(
            session.complete(
                &stale,
                Completion::Folder(Ok((PathBuf::from("/stale"), vec![])))
            ),
            Outcome::Ignored
        ));
        assert_eq!(session.current(), Path::new("/start"));
        assert!(matches!(
            session.complete(
                &latest,
                Completion::Folder(Err("missing".to_owned()))
            ),
            Outcome::Failed { error, .. } if error == "missing"
        ));
        assert_eq!(session.current(), Path::new("/start"));
    }

    #[test]
    fn same_path_refreshes_are_distinguished_by_request_identity() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let stale = session.refresh(None);
        let latest = session.refresh(None);

        assert!(matches!(
            session.complete(
                &stale,
                Completion::Folder(Ok((PathBuf::from("/start"), vec![entry("/start/stale")])))
            ),
            Outcome::Ignored
        ));
        assert!(session.loading());
        assert!(matches!(
            session.complete(
                &latest,
                Completion::Folder(Ok((PathBuf::from("/start"), vec![entry("/start/latest")])))
            ),
            Outcome::Committed { .. }
        ));
        assert_eq!(session.entries()[0].path, PathBuf::from("/start/latest"));
        assert!(!session.loading());
    }

    #[test]
    fn recent_and_trash_are_overlays_on_folder_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        session.install_folder_entries(vec![entry("/start/one")]);
        session.seed_history(vec![PathBuf::from("/back")], Vec::new());

        let recent = session.recent();
        assert!(matches!(
            session.complete(
                &recent,
                Completion::Recent(Ok(vec![entry("/elsewhere/recent")]))
            ),
            Outcome::Committed {
                location: DisplayedLocation::Recent,
                ..
            }
        ));
        let exit = session.back().unwrap();
        assert_eq!(exit.requested(), Some(Path::new("/start")));
        let _ = session.complete(
            &exit,
            Completion::Folder(Ok((PathBuf::from("/start"), vec![entry("/start/one")]))),
        );
        assert!(session.can_go_back());

        let trash = session.trash();
        let _ = session.complete(
            &trash,
            Completion::Trash(Ok(vec![trash_entry("/trash/files/item", "/original/item")])),
        );
        assert_eq!(session.displayed_location(), DisplayedLocation::Trash);
        assert_eq!(session.trash_entries().len(), 1);
    }

    #[test]
    fn failed_overlay_and_recursive_restore_keep_one_coherent_display() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        session.install_trash_entries(vec![trash_entry("/trash/files/item", "/original/item")]);
        let failed = session.recent();
        assert!(matches!(
            session.complete(
                &failed,
                Completion::Recent(Err("history unavailable".to_owned()))
            ),
            Outcome::Failed { .. }
        ));
        assert_eq!(session.displayed_location(), DisplayedLocation::Trash);
        assert_eq!(session.trash_entries().len(), 1);

        session.begin_recursive_display();
        session.install_recursive_entries(vec![entry("/start/search-result")]);
        session.restore_recursive_display();
        assert_eq!(session.displayed_location(), DisplayedLocation::Trash);
        assert_eq!(
            session.entries()[0].path,
            PathBuf::from("/trash/files/item")
        );
        assert_eq!(session.trash_entries().len(), 1);
    }

    #[test]
    fn refresh_restores_requested_selection_without_changing_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let selected = PathBuf::from("/start/two");
        let request = session.refresh(Some(selected.clone()));
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok((
                PathBuf::from("/start"),
                vec![entry("/start/one"), entry("/start/two")],
            ))),
        );

        assert!(matches!(outcome, Outcome::Committed { selected, .. } if selected == [1]));
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
            &request,
            Completion::Folder(Ok((
                PathBuf::from("/start"),
                vec![
                    entry("/start/one"),
                    entry("/start/two"),
                    entry("/start/three"),
                ],
            ))),
        );

        assert!(matches!(outcome, Outcome::Committed { selected, .. } if selected == [0, 2]));
    }

    #[test]
    fn parent_selects_the_folder_that_was_left() {
        let mut session = NavigationSession::new(PathBuf::from("/start/child"));
        let request = session.parent().unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok((
                PathBuf::from("/start"),
                vec![entry("/start/child"), entry("/start/sibling")],
            ))),
        );

        assert!(matches!(outcome, Outcome::Committed { selected, .. } if selected == [0]));
        assert_eq!(session.current(), Path::new("/start"));
    }

    #[test]
    fn opening_the_current_folder_does_not_duplicate_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.forward(PathBuf::from("/start"), true, None);
        let _ = session.complete(
            &request,
            Completion::Folder(Ok((PathBuf::from("/start"), Vec::new()))),
        );

        assert!(!session.can_go_back());
    }
}
