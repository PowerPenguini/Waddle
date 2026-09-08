use std::path::{Path, PathBuf};

use crate::fs::{FileEntry, OpenedDirectory};

use super::{grid::GridInteraction, trash, tree::LoadRequest};

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
    Reveal {
        requested: PathBuf,
        selected: Vec<PathBuf>,
    },
    Sidebar {
        requested: PathBuf,
        load: Option<LoadRequest>,
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
    tree_load: Option<LoadRequest>,
}

impl Request {
    pub(super) fn tree_load(&self) -> Option<&LoadRequest> {
        self.tree_load.as_ref()
    }

    pub(super) fn location(&self) -> DisplayedLocation {
        match self.target {
            Target::Folder { .. } => DisplayedLocation::Folder,
            Target::Recent => DisplayedLocation::Recent,
            Target::Trash => DisplayedLocation::Trash,
        }
    }

    pub(super) fn requested(&self) -> Option<&Path> {
        match &self.target {
            Target::Folder { requested, .. } => Some(requested),
            Target::Recent | Target::Trash => None,
        }
    }

    pub(super) fn selected_paths(&self) -> &[PathBuf] {
        &self.select
    }
}

/// Effects of a navigation decision; callers execute these without rechecking policy.
#[must_use]
#[derive(Clone, Debug, Default)]
pub(super) struct Start {
    pub(super) request: Option<Request>,
    pub(super) cancelled: Option<Request>,
    pub(super) reuse_tree: Option<LoadRequest>,
    pub(super) reset_pointer: bool,
    pub(super) cancel_search: bool,
}

#[must_use]
pub(super) struct Completed {
    pub(super) outcome: Outcome,
    pub(super) tree_load: Option<LoadRequest>,
    pub(super) refresh: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Commit {
    selected: Vec<usize>,
    reset_scroll: bool,
    reveal_selection: bool,
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

    pub(super) fn reveal_selection(&self) -> bool {
        self.reveal_selection
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
    Redirect { start: Box<Start>, notice: String },
    Committed(Commit),
}

pub(super) enum Completion {
    Folder(Result<OpenedDirectory, String>),
    Recent(Result<Vec<FileEntry>, String>),
    Trash(Result<Vec<trash::Entry>, String>),
    Cancelled,
}

#[derive(Clone, Debug)]
struct Display {
    location: DisplayedLocation,
    entries: Vec<FileEntry>,
    child_folders: Vec<PathBuf>,
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
    deferred_refresh: Option<PathBuf>,
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
                child_folders: Vec::new(),
                trash_entries: Vec::new(),
            },
            pending: None,
            deferred_refresh: None,
            next_request_id: 1,
        }
    }

    pub(super) fn current(&self) -> &Path {
        &self.current
    }

    pub(super) fn entries(&self) -> &[FileEntry] {
        &self.display.entries
    }

    pub(super) fn child_folders(&self) -> &[PathBuf] {
        &self.display.child_folders
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

    pub(super) fn cancel_pending(&mut self) -> Option<Request> {
        self.deferred_refresh = None;
        self.pending.take()
    }

    pub(super) fn can_go_back(&self) -> bool {
        !self.folder_displayed() || !self.history.is_empty()
    }

    pub(super) fn can_go_forward(&self) -> bool {
        !self.forward_history.is_empty()
    }

    pub(super) fn transition(&mut self, transition: Transition) -> Start {
        if self.loading() && matches!(transition, Transition::Back) {
            return Start {
                cancelled: self.cancel_pending(),
                ..Start::default()
            };
        }
        if let Transition::Sidebar { requested, load } = &transition
            && requested == &self.current
            && self.folder_displayed()
            && !self.loading()
        {
            return Start {
                reuse_tree: load.clone(),
                ..Start::default()
            };
        }
        // Even an unavailable destination supersedes the user's previous choice.
        let cancelled = self.cancel_pending();
        let reset_pointer = !transition.preserves_pointer_interaction();
        let mut start = match transition {
            Transition::Open {
                requested,
                remember,
                select,
            } => self.forward(requested, remember, select),
            Transition::Hover { requested } => self.forward(requested, true, None),
            Transition::Reveal {
                requested,
                selected,
            } => self.begin(
                Target::Folder {
                    requested,
                    kind: Kind::Forward { remember: false },
                },
                selected,
            ),
            Transition::Sidebar { requested, load } => {
                let mut start = self.forward(requested, true, None);
                if let Some(request) = start.request.as_mut() {
                    request.tree_load = load;
                    self.pending = Some(request.clone());
                }
                start
            }
            Transition::Parent => self.parent(),
            Transition::Back => self.back(),
            Transition::HistoryForward => self.history_forward(),
        };
        start.cancelled = cancelled;
        start.reset_pointer = reset_pointer;
        start.cancel_search = true;
        start
    }

    /// Coalesce a live refresh while the displayed folder has an in-flight request.
    pub(super) fn defer_refresh(&mut self) -> bool {
        if !self.loading() {
            return false;
        }
        self.deferred_refresh = Some(self.current.clone());
        true
    }

    fn parent(&mut self) -> Start {
        if !self.folder_displayed() {
            return self.forward(self.current.clone(), false, None);
        }
        let Some(parent) = self.current.parent().map(PathBuf::from) else {
            return Start::default();
        };
        let current = self.current.clone();
        self.forward(parent, true, Some(current))
    }

    fn forward(&mut self, requested: PathBuf, remember: bool, select: Option<PathBuf>) -> Start {
        self.begin(
            Target::Folder {
                requested,
                kind: Kind::Forward { remember },
            },
            select.into_iter().collect(),
        )
    }

    fn back(&mut self) -> Start {
        if !self.folder_displayed() {
            return self.forward(self.current.clone(), false, None);
        }
        let Some(target) = self.history.last().cloned() else {
            return Start::default();
        };
        self.begin(
            Target::Folder {
                requested: target.clone(),
                kind: Kind::Back { expected: target },
            },
            Vec::new(),
        )
    }

    fn history_forward(&mut self) -> Start {
        if !self.folder_displayed() {
            return self.forward(self.current.clone(), false, None);
        }
        let Some(target) = self.forward_history.last().cloned() else {
            return Start::default();
        };
        self.begin(
            Target::Folder {
                requested: target.clone(),
                kind: Kind::HistoryForward { expected: target },
            },
            Vec::new(),
        )
    }

    pub(super) fn refresh(&mut self, select: Option<PathBuf>) -> Start {
        self.refresh_selected(select.into_iter().collect())
    }

    pub(super) fn refresh_selected(&mut self, select: Vec<PathBuf>) -> Start {
        self.begin(
            Target::Folder {
                requested: self.current.clone(),
                kind: Kind::Refresh,
            },
            select,
        )
    }

    pub(super) fn recent(&mut self) -> Start {
        let mut start = self.begin(Target::Recent, Vec::new());
        start.cancel_search = true;
        start
    }

    pub(super) fn trash(&mut self) -> Start {
        let mut start = self.begin(Target::Trash, Vec::new());
        start.cancel_search = true;
        start
    }

    fn begin(&mut self, target: Target, select: Vec<PathBuf>) -> Start {
        let cancelled = self.cancel_pending();
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request = Request {
            id,
            target,
            select,
            tree_load: None,
        };
        self.pending = Some(request.clone());
        Start {
            request: Some(request),
            cancelled,
            ..Start::default()
        }
    }

    pub(super) fn complete_with_hidden_paths(
        &mut self,
        request: &Request,
        completion: Completion,
        hidden_paths: &[PathBuf],
    ) -> Completed {
        if self.pending.as_ref().map(|pending| pending.id) != Some(request.id) {
            return Completed {
                outcome: Outcome::Ignored,
                tree_load: None,
                refresh: false,
            };
        }
        let accepted = self.pending.take().expect("matched navigation request");
        let outcome = match (&request.target, completion) {
            (_, Completion::Cancelled) => {
                self.deferred_refresh = None;
                Outcome::Ignored
            }
            (Target::Folder { kind, .. }, Completion::Folder(result)) => {
                self.complete_folder(kind, &request.select, result, hidden_paths)
            }
            (Target::Recent, Completion::Recent(result)) => self.complete_recent(result),
            (Target::Trash, Completion::Trash(result)) => self.complete_trash(result),
            _ => Outcome::Ignored,
        };
        let refresh = !self.loading()
            && self
                .deferred_refresh
                .take()
                .is_some_and(|path| self.folder_displayed() && path == self.current);
        Completed {
            outcome,
            tree_load: accepted.tree_load,
            refresh,
        }
    }

    #[cfg(test)]
    pub(super) fn complete(&mut self, request: &Request, completion: Completion) -> Outcome {
        self.complete_with_hidden_paths(request, completion, &[])
            .outcome
    }

    fn complete_folder(
        &mut self,
        kind: &Kind,
        select: &[PathBuf],
        result: Result<OpenedDirectory, String>,
        hidden_paths: &[PathBuf],
    ) -> Outcome {
        let OpenedDirectory {
            canonical_path: canonical,
            mut entries,
            child_folders,
        } = match result {
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
                    let start = self.forward(ancestor, false, None);
                    return Outcome::Redirect {
                        start: Box::new(start),
                        notice,
                    };
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
            child_folders,
            trash_entries: Vec::new(),
        };
        self.commit(
            selected,
            refresh,
            matches!(kind, Kind::Forward { .. }) && !select.is_empty(),
            DisplayedLocation::Folder,
        )
    }

    fn complete_recent(&mut self, result: Result<Vec<FileEntry>, String>) -> Outcome {
        let entries = match result {
            Ok(entries) => entries,
            Err(error) => return Outcome::Failed(error),
        };
        self.display = Display {
            location: DisplayedLocation::Recent,
            entries,
            child_folders: Vec::new(),
            trash_entries: Vec::new(),
        };
        self.commit(Vec::new(), false, false, DisplayedLocation::Recent)
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
            child_folders: Vec::new(),
            trash_entries,
        };
        self.commit(Vec::new(), false, false, DisplayedLocation::Trash)
    }

    fn commit(
        &self,
        selected: Vec<usize>,
        refresh: bool,
        reveal_selection: bool,
        location: DisplayedLocation,
    ) -> Outcome {
        let status = match location {
            DisplayedLocation::Folder => String::new(),
            DisplayedLocation::Recent => format!("{} items  •  Recent", self.entries().len()),
            DisplayedLocation::Trash => format!("{} items  •  Trash", self.entries().len()),
        };
        Outcome::Committed(Commit {
            selected,
            reset_scroll: !refresh,
            reveal_selection,
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
            child_folders: Vec::new(),
            trash_entries: Vec::new(),
        };
    }

    #[cfg(test)]
    pub(super) fn replace_displayed_entries(&mut self, entries: Vec<FileEntry>) {
        self.install_folder_entries(entries);
    }

    #[cfg(test)]
    pub(super) fn install_trash_entries(&mut self, entries: Vec<trash::Entry>) {
        let request = self.trash().request.unwrap();
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

    #[test]
    fn navigation_session_first_back_cancels_then_second_back_uses_history() {
        let mut session = NavigationSession::new(PathBuf::from("/current"));
        session.seed_history(vec![PathBuf::from("/back")], Vec::new());
        let pending = session
            .transition(Transition::Open {
                requested: PathBuf::from("/slow"),
                remember: true,
                select: None,
            })
            .request
            .unwrap();
        assert!(
            session.transition(Transition::Back).request.is_none(),
            "first Back must cancel the pending request inside the Navigation session"
        );
        assert!(!session.loading());
        assert!(matches!(
            session.complete(
                &pending,
                Completion::Folder(Ok(opened("/slow", Vec::new())))
            ),
            Outcome::Ignored
        ));
        let back = session.transition(Transition::Back).request.unwrap();
        assert_eq!(back.requested(), Some(Path::new("/back")));
    }

    #[test]
    fn unavailable_navigation_still_cancels_a_superseded_request() {
        for transition in [Transition::Parent, Transition::HistoryForward] {
            let mut session = NavigationSession::new(PathBuf::from("/"));
            let pending = session
                .transition(Transition::Open {
                    requested: PathBuf::from("/slow"),
                    remember: true,
                    select: None,
                })
                .request
                .unwrap();
            let start = session.transition(transition);
            assert!(start.request.is_none());
            assert_eq!(
                start.cancelled,
                Some(pending.clone()),
                "a navigation choice with no destination must still cancel the old choice"
            );
            assert!(!session.loading());
            assert!(matches!(
                session.complete(
                    &pending,
                    Completion::Folder(Ok(opened("/slow", Vec::new())))
                ),
                Outcome::Ignored
            ));
            assert_eq!(session.current(), Path::new("/"));
        }
    }

    #[test]
    fn navigation_session_keeps_tree_loads_and_deferred_refresh_with_their_request() {
        let mut session = NavigationSession::new(PathBuf::from("/current"));
        let load = LoadRequest {
            id: 42,
            path: PathBuf::from("/slow"),
        };
        let first = session
            .transition(Transition::Sidebar {
                requested: load.path.clone(),
                load: Some(load.clone()),
            })
            .request
            .unwrap();
        assert!(session.defer_refresh());
        let replacement = session.transition(Transition::Open {
            requested: PathBuf::from("/current"),
            remember: true,
            select: None,
        });
        assert_eq!(replacement.cancelled.unwrap().tree_load(), Some(&load));
        let latest = replacement.request.unwrap();
        let stale = session.complete_with_hidden_paths(
            &first,
            Completion::Folder(Ok(opened("/slow", Vec::new()))),
            &[],
        );
        assert!(matches!(stale.outcome, Outcome::Ignored));
        assert!(stale.tree_load.is_none());
        assert!(!stale.refresh);
        let settled = session.complete_with_hidden_paths(
            &latest,
            Completion::Folder(Ok(opened("/current", Vec::new()))),
            &[],
        );
        assert!(
            !settled.refresh,
            "superseded requests must discard their deferred refresh"
        );

        let refresh = session.refresh(None).request.unwrap();
        assert!(session.defer_refresh());
        assert!(session.defer_refresh());
        let finished = session.complete_with_hidden_paths(
            &refresh,
            Completion::Folder(Ok(opened("/current", Vec::new()))),
            &[],
        );
        assert!(finished.refresh, "changes during a scan require one rescan");
        let duplicate = session.complete_with_hidden_paths(
            &refresh,
            Completion::Folder(Ok(opened("/current", Vec::new()))),
            &[],
        );
        assert!(!duplicate.refresh);
    }

    #[test]
    fn navigation_session_reuses_only_the_displayed_idle_folder() {
        let mut session = NavigationSession::new(PathBuf::from("/current"));
        let load = LoadRequest {
            id: 42,
            path: PathBuf::from("/current"),
        };
        let reuse = session.transition(Transition::Sidebar {
            requested: load.path.clone(),
            load: Some(load.clone()),
        });
        assert!(reuse.request.is_none());
        assert_eq!(reuse.reuse_tree, Some(load.clone()));
        let recent = session.recent().request.unwrap();
        let _ = session.complete(&recent, Completion::Recent(Ok(Vec::new())));
        let open = session.transition(Transition::Sidebar {
            requested: load.path.clone(),
            load: Some(load.clone()),
        });
        assert!(open.reuse_tree.is_none());
        let request = open.request.unwrap();
        let completed = session.complete_with_hidden_paths(
            &request,
            Completion::Folder(Ok(opened("/current", Vec::new()))),
            &[],
        );
        assert_eq!(completed.tree_load, Some(load));
        assert!(session.folder_displayed());
    }

    fn entry(path: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from(path),
            name: OsString::from(path.rsplit('/').next().unwrap()),
            directory: false,
            metadata: Default::default(),
        }
    }

    fn opened(path: &str, entries: Vec<FileEntry>) -> crate::fs::OpenedDirectory {
        crate::fs::OpenedDirectory {
            canonical_path: PathBuf::from(path),
            entries,
            child_folders: Vec::new(),
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
            .request
            .unwrap();
        assert!(matches!(
            session.complete(&request, Completion::Folder(Ok(opened("/next", vec![])))),
            Outcome::Committed(_)
        ));
        assert_eq!(session.current(), Path::new("/next"));
        assert!(session.can_go_back());

        let request = session.transition(Transition::Back).request.unwrap();
        assert!(matches!(
            session.complete(&request, Completion::Folder(Ok(opened("/start", vec![])))),
            Outcome::Committed(_)
        ));
        assert_eq!(session.current(), Path::new("/start"));
        assert!(session.can_go_forward());

        let request = session
            .transition(Transition::HistoryForward)
            .request
            .unwrap();
        let _ = session.complete(&request, Completion::Folder(Ok(opened("/next", vec![]))));
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
            .request
            .unwrap();
        let latest = session
            .transition(Transition::Open {
                requested: PathBuf::from("/latest"),
                remember: true,
                select: None,
            })
            .request
            .unwrap();

        assert!(matches!(
            session.complete(&stale, Completion::Folder(Ok(opened("/stale", vec![])))),
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
    fn cancelling_pending_navigation_preserves_location_and_history() {
        let mut session = NavigationSession::new(PathBuf::from("/current"));
        session.seed_history(vec![PathBuf::from("/back")], Vec::new());
        let pending = session
            .transition(Transition::Open {
                requested: PathBuf::from("/slow"),
                remember: true,
                select: None,
            })
            .request
            .unwrap();

        assert_eq!(session.cancel_pending(), Some(pending.clone()));
        assert!(!session.loading());
        assert_eq!(session.current(), Path::new("/current"));
        assert!(session.can_go_back());
        assert!(matches!(
            session.complete(
                &pending,
                Completion::Folder(Ok(crate::fs::OpenedDirectory {
                    canonical_path: PathBuf::from("/slow"),
                    entries: Vec::new(),
                    child_folders: Vec::new(),
                }))
            ),
            Outcome::Ignored
        ));
        assert_eq!(session.current(), Path::new("/current"));
    }

    #[test]
    fn same_path_refreshes_are_distinguished_by_request_identity() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let stale = session.refresh(None).request.unwrap();
        let latest = session.refresh(None).request.unwrap();

        assert!(matches!(
            session.complete(
                &stale,
                Completion::Folder(Ok(opened("/start", vec![entry("/start/stale")])))
            ),
            Outcome::Ignored
        ));
        assert!(session.loading());
        assert!(matches!(
            session.complete(
                &latest,
                Completion::Folder(Ok(opened("/start", vec![entry("/start/latest")])))
            ),
            Outcome::Committed(_)
        ));
        assert_eq!(session.entries()[0].path, PathBuf::from("/start/latest"));
        assert!(!session.loading());
    }

    #[test]
    fn committed_folder_keeps_child_folders_from_the_same_scan() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session.refresh(None).request.unwrap();
        let mut snapshot = opened("/start", vec![entry("/start/file")]);
        snapshot.child_folders = vec![PathBuf::from("/start/alpha"), PathBuf::from("/start/beta")];

        assert!(matches!(
            session.complete(&request, Completion::Folder(Ok(snapshot))),
            Outcome::Committed(_)
        ));
        assert_eq!(
            session.child_folders(),
            [PathBuf::from("/start/alpha"), PathBuf::from("/start/beta")]
        );
    }

    #[test]
    fn back_is_available_from_recent_and_trash_without_folder_history() {
        for trash in [false, true] {
            let mut session = NavigationSession::new(PathBuf::from("/start"));
            assert!(!session.can_go_back());
            let (request, completion) = if trash {
                (
                    session.trash().request.unwrap(),
                    Completion::Trash(Ok(Vec::new())),
                )
            } else {
                (
                    session.recent().request.unwrap(),
                    Completion::Recent(Ok(Vec::new())),
                )
            };
            assert!(matches!(
                session.complete(&request, completion),
                Outcome::Committed(_)
            ));
            assert!(
                session.can_go_back(),
                "the Back button must let users leave Recent/Trash even on first launch"
            );
            let back = session.transition(Transition::Back).request.unwrap();
            assert_eq!(back.requested(), Some(Path::new("/start")));
            let _ = session.complete(&back, Completion::Folder(Ok(opened("/start", Vec::new()))));
            assert!(session.folder_displayed());
            assert!(
                !session.can_go_back(),
                "returning must not create a history entry"
            );
        }
    }

    #[test]
    fn recent_and_trash_are_overlays_on_folder_history() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        session.install_folder_entries(vec![entry("/start/one")]);
        session.seed_history(vec![PathBuf::from("/back")], Vec::new());

        let recent = session.recent().request.unwrap();
        assert!(matches!(
            session.complete(
                &recent,
                Completion::Recent(Ok(vec![entry("/elsewhere/recent")]))
            ),
            Outcome::Committed(commit) if commit.location() == DisplayedLocation::Recent
        ));
        let exit = session.transition(Transition::Back).request.unwrap();
        assert_eq!(exit.requested(), Some(Path::new("/start")));
        let _ = session.complete(
            &exit,
            Completion::Folder(Ok(opened("/start", vec![entry("/start/one")]))),
        );
        assert!(session.can_go_back());

        let trash = session.trash().request.unwrap();
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
        let failed = session.recent().request.unwrap();
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
        let request = session.refresh(Some(selected.clone())).request.unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok(opened(
                "/start",
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
        let request = session
            .refresh_selected(vec![
                PathBuf::from("/start/three"),
                PathBuf::from("/start/one"),
            ])
            .request
            .unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok(opened(
                "/start",
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
    fn explicit_reveal_scrolls_every_selection_but_refresh_does_not() {
        let first = PathBuf::from("/start/one");
        let second = PathBuf::from("/start/two");
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session
            .transition(Transition::Reveal {
                requested: PathBuf::from("/start"),
                selected: vec![second.clone(), first],
            })
            .request
            .unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok(opened(
                "/start",
                vec![entry("/start/one"), entry("/start/two")],
            ))),
        );

        assert!(
            matches!(outcome, Outcome::Committed(commit) if commit.reveal_selection() && commit.selected() == [0, 1])
        );

        let request = session.refresh(Some(second)).request.unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok(opened(
                "/start",
                vec![entry("/start/one"), entry("/start/two")],
            ))),
        );

        assert!(matches!(outcome, Outcome::Committed(commit) if !commit.reveal_selection()));
    }

    #[test]
    fn parent_selects_the_folder_that_was_left() {
        let mut session = NavigationSession::new(PathBuf::from("/start/child"));
        let request = session.transition(Transition::Parent).request.unwrap();
        let outcome = session.complete(
            &request,
            Completion::Folder(Ok(opened(
                "/start",
                vec![entry("/start/child"), entry("/start/sibling")],
            ))),
        );

        assert!(matches!(outcome, Outcome::Committed(commit) if commit.selected() == [0]));
        assert_eq!(session.current(), Path::new("/start"));
    }

    #[test]
    fn commit_hides_cut_paths_before_restoring_grid_selection() {
        let mut session = NavigationSession::new(PathBuf::from("/start"));
        let request = session
            .refresh_selected(vec![
                PathBuf::from("/start/one"),
                PathBuf::from("/start/two"),
            ])
            .request
            .unwrap();
        let outcome = session.complete_with_hidden_paths(
            &request,
            Completion::Folder(Ok(opened(
                "/start",
                vec![entry("/start/one"), entry("/start/two")],
            ))),
            &[PathBuf::from("/start/one")],
        );
        let Outcome::Committed(commit) = outcome.outcome else {
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
        let request = session.refresh(None).request.unwrap();

        let outcome = session.complete(
            &request,
            Completion::Folder(Err("directory disappeared".to_owned())),
        );
        let Outcome::Redirect { start, notice } = outcome else {
            panic!("missing refresh did not redirect");
        };

        assert_eq!(start.request.unwrap().requested(), Some(temp.path()));
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
            .request
            .unwrap();
        let _ = session.complete(
            &request,
            Completion::Folder(Ok(opened("/start", Vec::new()))),
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
