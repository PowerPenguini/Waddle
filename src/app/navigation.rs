use std::path::{Path, PathBuf};

use crate::fs::FileEntry;

use super::{grid::GridInteraction, trash};

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
pub(super) enum Transition {
    Open {
        requested: PathBuf,
        remember: bool,
        select: Option<PathBuf>,
    },
    Hover {
        requested: PathBuf,
    },
    Parent,
    Back,
    HistoryForward,
}

impl Transition {
    pub(super) fn preserves_pointer_interaction(&self) -> bool {
        matches!(self, Self::Hover { .. })
    }
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
pub(super) struct Commit {
    selected: Vec<usize>,
    reset_scroll: bool,
    location: DisplayedLocation,
    location_input: String,
    status: String,
}

impl Commit {
    pub(super) fn apply_grid(
        &self,
        grid: &mut GridInteraction,
        entry_count: usize,
        list_mode: bool,
    ) {
        grid.install_navigation(&self.selected, entry_count, list_mode, self.reset_scroll);
    }

    pub(super) fn location(&self) -> DisplayedLocation {
        self.location
    }

    pub(super) fn location_input(&self) -> &str {
        &self.location_input
    }

    pub(super) fn status(&self) -> &str {
        &self.status
    }

    #[cfg(test)]
    fn selected(&self) -> &[usize] {
        &self.selected
    }
}

#[derive(Clone, Debug)]
pub(super) enum Outcome {
    Ignored,
    Failed(String),
    Redirect { request: Request, notice: String },
    Committed(Commit),
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
pub(super) struct SearchDisplay(Display);

#[derive(Clone, Debug)]
pub(super) struct NavigationSession {
    current: PathBuf,
    history: Vec<PathBuf>,
    forward_history: Vec<PathBuf>,
    display: Display,
    pending: Option<Request>,
    next_request_id: u64,
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

    pub(super) fn transition(&mut self, transition: Transition) -> Option<Request> {
        match transition {
            Transition::Open {
                requested,
                remember,
                select,
            } => Some(self.forward(requested, remember, select)),
            Transition::Hover { requested } => Some(self.forward(requested, true, None)),
            Transition::Parent => self.parent(),
            Transition::Back => self.back(),
            Transition::HistoryForward => self.history_forward(),
        }
    }

    fn parent(&mut self) -> Option<Request> {
        if !self.folder_displayed() {
            return Some(self.forward(self.current.clone(), false, None));
        }
        let parent = self.current.parent()?.to_path_buf();
        let current = self.current.clone();
        Some(self.forward(parent, true, Some(current)))
    }

    fn forward(&mut self, requested: PathBuf, remember: bool, select: Option<PathBuf>) -> Request {
        self.begin(
            Target::Folder {
                requested,
                kind: Kind::Forward { remember },
            },
            select.into_iter().collect(),
        )
    }

    fn back(&mut self) -> Option<Request> {
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

    fn history_forward(&mut self) -> Option<Request> {
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

    pub(super) fn complete_with_hidden_paths(
        &mut self,
        request: &Request,
        completion: Completion,
        hidden_paths: &[PathBuf],
    ) -> Outcome {
        if self.pending.as_ref().map(|pending| pending.id) != Some(request.id) {
            return Outcome::Ignored;
        }
        self.pending = None;
        if matches!(completion, Completion::Cancelled) {
            return Outcome::Ignored;
        }
        match (&request.target, completion) {
            (Target::Folder { kind, .. }, Completion::Folder(result)) => {
                self.complete_folder(kind, &request.select, result, hidden_paths)
            }
            (Target::Recent, Completion::Recent(result)) => self.complete_recent(result),
            (Target::Trash, Completion::Trash(result)) => self.complete_trash(result),
            _ => Outcome::Ignored,
        }
    }

    #[cfg(test)]
    pub(super) fn complete(&mut self, request: &Request, completion: Completion) -> Outcome {
        self.complete_with_hidden_paths(request, completion, &[])
    }

    fn complete_folder(
        &mut self,
        kind: &Kind,
        select: &[PathBuf],
        result: Result<(PathBuf, Vec<FileEntry>), String>,
        hidden_paths: &[PathBuf],
    ) -> Outcome {
        let (canonical, mut entries) = match result {
            Ok(opened) => opened,
            Err(error) => {
                if matches!(kind, Kind::Refresh) && !self.current.is_dir() {
                    let missing = self.current.clone();
                    let ancestor = nearest_existing_ancestor(&missing);
                    let notice = format!(
                        "{} disappeared; opened {}",
                        missing.display(),
                        ancestor.display()
                    );
                    let request = self.forward(ancestor, false, None);
                    return Outcome::Redirect { request, notice };
                }
                return Outcome::Failed(error);
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
        entries.retain(|entry| !hidden_paths.iter().any(|path| path == &entry.path));
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
        self.commit(selected, refresh, DisplayedLocation::Folder)
    }

    fn complete_recent(&mut self, result: Result<Vec<FileEntry>, String>) -> Outcome {
        let entries = match result {
            Ok(entries) => entries,
            Err(error) => return Outcome::Failed(error),
        };
        self.display = Display {
            location: DisplayedLocation::Recent,
            entries,
            trash_entries: Vec::new(),
        };
        self.commit(Vec::new(), false, DisplayedLocation::Recent)
    }

    fn complete_trash(&mut self, result: Result<Vec<trash::Entry>, String>) -> Outcome {
        let trash_entries = match result {
            Ok(entries) => entries,
            Err(error) => return Outcome::Failed(error),
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
        self.commit(Vec::new(), false, DisplayedLocation::Trash)
    }

    fn commit(&self, selected: Vec<usize>, refresh: bool, location: DisplayedLocation) -> Outcome {
        let status = match location {
            DisplayedLocation::Folder => String::new(),
            DisplayedLocation::Recent => format!("{} items  •  Recent", self.entries().len()),
            DisplayedLocation::Trash => format!("{} items  •  Trash", self.entries().len()),
        };
        Outcome::Committed(Commit {
            selected,
            reset_scroll: !refresh,
            location,
            location_input: self.location_label(),
            status,
        })
    }

    pub(super) fn capture_search_display(&self) -> SearchDisplay {
        SearchDisplay(self.display.clone())
    }

    pub(super) fn install_search_entries(&mut self, entries: Vec<FileEntry>) {
        self.display.entries = entries;
    }

    pub(super) fn restore_search_display(&mut self, display: SearchDisplay) {
        self.display = display.0;
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

fn nearest_existing_ancestor(path: &Path) -> PathBuf {
    let mut candidate = path.to_path_buf();
    loop {
        if candidate.is_dir() {
            return candidate;
        }
        if !candidate.pop() {
            return PathBuf::from("/");
        }
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
        let request = session
            .transition(Transition::Open {
                requested: PathBuf::from("/next"),
                remember: true,
                select: None,
            })
            .unwrap();
        assert!(matches!(
            session.complete(
                &request,
                Completion::Folder(Ok((PathBuf::from("/next"), vec![])))
            ),
            Outcome::Committed(_)
        ));
        assert_eq!(session.current(), Path::new("/next"));
        assert!(session.can_go_back());

        let request = session.transition(Transition::Back).unwrap();
        assert!(matches!(
            session.complete(
                &request,
                Completion::Folder(Ok((PathBuf::from("/start"), vec![])))
            ),
            Outcome::Committed(_)
        ));
        assert_eq!(session.current(), Path::new("/start"));
        assert!(session.can_go_forward());

        let request = session.transition(Transition::HistoryForward).unwrap();
        let _ = session.complete(
            &request,
            Completion::Folder(Ok((PathBuf::from("/next"), vec![]))),
        );
        assert_eq!(session.current(), Path::new("/next"));
    }

    #[test]
    fn stale_and_failed_completions_cannot_commit() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let stale = session
            .transition(Transition::Open {
                requested: PathBuf::from("/stale"),
                remember: true,
                select: None,
            })
            .unwrap();
        let latest = session
            .transition(Transition::Open {
                requested: PathBuf::from("/latest"),
                remember: true,
                select: None,
            })
            .unwrap();

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
            Outcome::Failed(error) if error == "missing"
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
            Outcome::Committed(_)
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
            Outcome::Committed(commit) if commit.location() == DisplayedLocation::Recent
        ));
        let exit = session.transition(Transition::Back).unwrap();
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
    fn failed_overlay_keeps_one_coherent_display() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        session.install_trash_entries(vec![trash_entry("/trash/files/item", "/original/item")]);
        let failed = session.recent();
        assert!(matches!(
            session.complete(
                &failed,
                Completion::Recent(Err("history unavailable".to_owned()))
            ),
            Outcome::Failed(_)
        ));
        assert_eq!(session.displayed_location(), DisplayedLocation::Trash);
        assert_eq!(session.trash_entries().len(), 1);

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

        assert!(matches!(outcome, Outcome::Committed(commit) if commit.selected() == [1]));
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

        assert!(matches!(outcome, Outcome::Committed(commit) if commit.selected() == [0, 2]));
    }

    #[test]
    fn parent_selects_the_folder_that_was_left() {
        let mut session = NavigationSession::new(PathBuf::from("/start/child"));
        let request = session.transition(Transition::Parent).unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok((
                PathBuf::from("/start"),
                vec![entry("/start/child"), entry("/start/sibling")],
            ))),
        );

        assert!(matches!(outcome, Outcome::Committed(commit) if commit.selected() == [0]));
        assert_eq!(session.current(), Path::new("/start"));
    }

    #[test]
    fn commit_hides_cut_paths_before_restoring_grid_selection() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.refresh_selected(vec![
            PathBuf::from("/start/one"),
            PathBuf::from("/start/two"),
        ]);
        let outcome = session.complete_with_hidden_paths(
            &request,
            Completion::Folder(Ok((
                PathBuf::from("/start"),
                vec![entry("/start/one"), entry("/start/two")],
            ))),
            &[PathBuf::from("/start/one")],
        );
        let Outcome::Committed(commit) = outcome else {
            panic!("navigation did not commit");
        };
        let mut grid = GridInteraction::default();
        commit.apply_grid(&mut grid, session.entries().len(), false);

        assert_eq!(
            session
                .entries()
                .iter()
                .map(|entry| entry.path.as_path())
                .collect::<Vec<_>>(),
            [Path::new("/start/two")]
        );
        assert_eq!(grid.selected_entry(), Some(0));
    }

    #[test]
    fn failed_refresh_redirects_to_the_nearest_existing_ancestor() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("gone/child");
        let mut session = NavigationSession::new(missing.clone());
        let request = session.refresh(None);

        let outcome = session.complete(
            &request,
            Completion::Folder(Err("directory disappeared".to_owned())),
        );
        let Outcome::Redirect { request, notice } = outcome else {
            panic!("missing refresh did not redirect");
        };

        assert_eq!(request.requested(), Some(temp.path()));
        assert!(notice.contains(&missing.display().to_string()));
        assert!(notice.contains(&temp.path().display().to_string()));
    }

    #[test]
    fn opening_the_current_folder_does_not_duplicate_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session
            .transition(Transition::Open {
                requested: PathBuf::from("/start"),
                remember: true,
                select: None,
            })
            .unwrap();
        let _ = session.complete(
            &request,
            Completion::Folder(Ok((PathBuf::from("/start"), Vec::new()))),
        );

        assert!(!session.can_go_back());
    }

    #[test]
    fn hover_is_the_only_transition_that_preserves_pointer_interaction() {
        assert!(
            Transition::Hover {
                requested: PathBuf::from("/hovered")
            }
            .preserves_pointer_interaction()
        );
        assert!(!Transition::Back.preserves_pointer_interaction());
        assert!(!Transition::Parent.preserves_pointer_interaction());
        assert!(!Transition::HistoryForward.preserves_pointer_interaction());
        assert!(
            !Transition::Open {
                requested: PathBuf::from("/opened"),
                remember: true,
                select: None,
            }
            .preserves_pointer_interaction()
        );
    }
}
