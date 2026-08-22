use std::{
    collections::BTreeSet,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

use iced::Point;

use crate::fs::{FileEntry, TransferReport};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Preview {
    pub(crate) icon: &'static [u8],
    pub(crate) count: usize,
    pub(crate) copy: bool,
    pub(crate) background: [u8; 4],
    pub(crate) icon_color: [u8; 4],
    pub(crate) accent: [u8; 4],
    pub(crate) badge_text: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Copy,
    Move,
}

impl Action {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::Move => "Moved",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Dropped(Action),
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) enum Event {
    Hover {
        id: u64,
        position: Point,
        action: Action,
    },
    Leave {
        id: u64,
    },
    Drop {
        id: u64,
        paths: Vec<PathBuf>,
        destination: PathBuf,
        action: Action,
    },
    Error(String),
    ClipboardOwnershipLost {
        generation: u64,
    },
}

pub(crate) type AdapterCompletion = Pin<Box<dyn Future<Output = Result<Outcome, String>> + Send>>;
pub(crate) type ClipboardCompletion =
    Pin<Box<dyn Future<Output = Result<ClipboardImport, String>> + Send>>;

pub(crate) trait Adapter {
    fn start(
        &self,
        paths: Vec<PathBuf>,
        preview: Preview,
        copy_only: bool,
    ) -> Result<AdapterCompletion, String>;

    fn set_target(&self, id: u64, destination: Option<PathBuf>);

    fn finish_inbound(&self, id: u64);

    fn shutdown(&self);
}

pub(crate) trait ClipboardAdapter {
    fn write_clipboard(&self, payload: ClipboardPayload) -> Result<(), String>;

    fn read_clipboard(&self) -> Result<ClipboardCompletion, String>;

    fn clear_clipboard(&self, generation: u64);
}

#[derive(Clone, Debug)]
struct Drag {
    index: usize,
    start: Point,
    active: bool,
}

#[derive(Clone, Debug)]
struct Hover {
    id: u64,
    destination: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Initiator {
    InternalDrag,
    NativeDrag,
    Clipboard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClipboardPayload {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) action: Action,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClipboardImport {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) action: Action,
    pub(crate) generation: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Request {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) destination: PathBuf,
    pub(crate) action: Action,
    pub(crate) inbound_id: Option<u64>,
    pub(crate) clipboard_generation: Option<u64>,
    initiator: Initiator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Release {
    None,
    Click(usize),
    Drop(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NativeUpdate {
    None,
    Status(String),
    Notice(String),
    Start(Request),
    Error(String),
    ClipboardLost(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Consequences {
    pub(crate) status: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) changed_folders: Vec<PathBuf>,
    pub(crate) refresh: bool,
    pub(crate) select: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TransferWorkflow {
    drag: Option<Drag>,
    native_active: bool,
    hover: Option<Hover>,
    clipboard: Option<ClipboardPayload>,
    next_clipboard_generation: u64,
}

impl TransferWorkflow {
    pub(crate) fn press(&mut self, index: usize, start: Point, entry_count: usize) {
        if !self.native_active && index < entry_count {
            self.drag = Some(Drag {
                index,
                start,
                active: false,
            });
        }
    }

    pub(crate) fn move_pointer(&mut self, position: Point) -> Option<usize> {
        let drag = self.drag.as_mut()?;
        if !drag.active && distance(drag.start, position) >= 6.0 {
            drag.active = true;
            Some(drag.index)
        } else {
            None
        }
    }

    pub(crate) fn release(&mut self, index: usize) -> Release {
        match self.drag.take() {
            Some(drag) if drag.active => Release::Drop(drag.index),
            Some(_) => Release::Click(index),
            None => Release::None,
        }
    }

    pub(crate) fn active_drag_index(&self) -> Option<usize> {
        self.drag
            .as_ref()
            .filter(|drag| drag.active)
            .map(|drag| drag.index)
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.drag = None;
    }

    pub(crate) fn copy(&mut self, entries: &[FileEntry]) -> Option<String> {
        let first = entries.first()?;
        self.next_clipboard_generation = self.next_clipboard_generation.wrapping_add(1);
        self.clipboard = Some(ClipboardPayload {
            paths: entries.iter().map(|entry| entry.path.clone()).collect(),
            action: Action::Copy,
            generation: self.next_clipboard_generation,
        });
        Some(if entries.len() == 1 {
            format!("Copied {}", first.name.to_string_lossy())
        } else {
            format!("Copied {} items", entries.len())
        })
    }

    pub(crate) fn cut(&mut self, entries: &[FileEntry]) -> Option<String> {
        entries.first()?;
        self.next_clipboard_generation = self.next_clipboard_generation.wrapping_add(1);
        self.clipboard = Some(ClipboardPayload {
            paths: entries.iter().map(|entry| entry.path.clone()).collect(),
            action: Action::Move,
            generation: self.next_clipboard_generation,
        });
        Some(format!(
            "Cut: {} item{}, p paste, Esc cancel",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        ))
    }

    pub(crate) fn paste(&self, destination: PathBuf) -> Option<Request> {
        let clipboard = self.clipboard.as_ref()?;
        if clipboard.action == Action::Move
            && clipboard
                .paths
                .iter()
                .all(|path| path.parent() == Some(destination.as_path()))
        {
            return None;
        }
        Some(Request {
            paths: clipboard.paths.clone(),
            destination,
            action: clipboard.action,
            inbound_id: None,
            clipboard_generation: Some(clipboard.generation),
            initiator: Initiator::Clipboard,
        })
    }

    pub(crate) fn clipboard_payload(&self) -> Option<ClipboardPayload> {
        self.clipboard.clone()
    }

    pub(crate) fn pending_cut_paths(&self) -> &[PathBuf] {
        self.clipboard
            .as_ref()
            .filter(|payload| payload.action == Action::Move)
            .map_or(&[], |payload| payload.paths.as_slice())
    }

    pub(crate) fn pending_cut_status(&self) -> Option<String> {
        let count = self.pending_cut_paths().len();
        (count > 0).then(|| {
            format!(
                "Cut: {count} item{}, p paste, Esc cancel",
                if count == 1 { "" } else { "s" }
            )
        })
    }

    pub(crate) fn cancel_cut(&mut self) -> Option<u64> {
        let generation = self
            .clipboard
            .as_ref()
            .filter(|payload| payload.action == Action::Move)
            .map(|payload| payload.generation)?;
        self.clipboard = None;
        Some(generation)
    }

    pub(crate) fn lose_clipboard(&mut self, generation: u64) -> bool {
        if self.clipboard.as_ref().is_some_and(|payload| {
            payload.action == Action::Move && payload.generation == generation
        }) {
            self.clipboard = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn import_clipboard(&mut self, import: ClipboardImport) -> bool {
        if import.paths.is_empty() {
            return false;
        }
        if let (Some(current), Some(generation)) = (&self.clipboard, import.generation)
            && current.generation == generation
            && current.paths == import.paths
            && current.action == import.action
        {
            return true;
        }
        self.next_clipboard_generation = self.next_clipboard_generation.wrapping_add(1);
        self.clipboard = Some(ClipboardPayload {
            paths: import.paths,
            action: import.action,
            generation: self.next_clipboard_generation,
        });
        true
    }

    pub(crate) fn entries_for_drag(
        entries: &[FileEntry],
        selected: &BTreeSet<usize>,
        grabbed_index: usize,
    ) -> Vec<FileEntry> {
        if selected.contains(&grabbed_index) {
            selected
                .iter()
                .filter_map(|index| entries.get(*index).cloned())
                .collect()
        } else {
            entries.get(grabbed_index).cloned().into_iter().collect()
        }
    }

    pub(crate) fn request(
        entries: &[FileEntry],
        selected: &BTreeSet<usize>,
        grabbed_index: usize,
        destination: PathBuf,
        action: Action,
    ) -> Option<Request> {
        let paths = Self::entries_for_drag(entries, selected, grabbed_index)
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        valid_target(&paths, &destination, action).then_some(Request {
            paths,
            destination,
            action,
            inbound_id: None,
            clipboard_generation: None,
            initiator: Initiator::InternalDrag,
        })
    }

    pub(crate) fn start_outgoing<A, P>(
        &mut self,
        adapter: &A,
        entries: &[FileEntry],
        selected: &BTreeSet<usize>,
        grabbed_index: usize,
        copy_only: bool,
        preview: P,
    ) -> Result<(usize, AdapterCompletion), String>
    where
        A: Adapter,
        P: FnOnce(&[FileEntry]) -> Option<Preview>,
    {
        let entries = Self::entries_for_drag(entries, selected, grabbed_index);
        let preview =
            preview(&entries).ok_or_else(|| "the dragged selection is empty".to_owned())?;
        let paths = entries
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        let count = paths.len();
        let completion = adapter.start(paths, preview, copy_only)?;
        self.drag = None;
        self.native_active = true;
        Ok((count, completion))
    }

    pub(crate) fn finish_outgoing(&mut self, result: Result<Outcome, String>) -> Consequences {
        self.native_active = false;
        match result {
            Ok(Outcome::Dropped(action)) => Consequences {
                status: Some(format!("{} by drag-and-drop", action.label())),
                error: None,
                changed_folders: Vec::new(),
                refresh: true,
                select: Vec::new(),
            },
            Ok(Outcome::Cancelled) => Consequences {
                status: None,
                error: None,
                changed_folders: Vec::new(),
                refresh: false,
                select: Vec::new(),
            },
            Err(error) => Consequences {
                status: Some(format!("External drag-and-drop failed: {error}")),
                error: None,
                changed_folders: Vec::new(),
                refresh: false,
                select: Vec::new(),
            },
        }
    }

    pub(crate) fn handle_native<A, F>(
        &mut self,
        adapter: &A,
        event: Event,
        mut destination_at: F,
    ) -> NativeUpdate
    where
        A: Adapter,
        F: FnMut(Point, bool) -> Option<PathBuf>,
    {
        match event {
            Event::Hover {
                id,
                position,
                action,
            } => {
                let destination = destination_at(position, true);
                adapter.set_target(id, destination.clone());
                self.hover = Some(Hover {
                    id,
                    destination: destination.clone(),
                });
                NativeUpdate::Status(destination.map_or_else(
                    || "This is not a valid drop target".to_owned(),
                    |destination| {
                        format!("{} items into {}", action.label(), destination.display())
                    },
                ))
            }
            Event::Leave { id } => {
                if self.hover.as_ref().is_some_and(|hover| hover.id == id) {
                    self.hover = None;
                    NativeUpdate::Status(String::new())
                } else {
                    NativeUpdate::None
                }
            }
            Event::Drop {
                id,
                paths,
                destination,
                action,
            } => {
                self.hover = None;
                if !valid_target(&paths, &destination, action) {
                    adapter.finish_inbound(id);
                    NativeUpdate::Notice(
                        "The drop target is inside one of the dragged folders".to_owned(),
                    )
                } else {
                    NativeUpdate::Start(Request {
                        paths,
                        destination,
                        action,
                        inbound_id: Some(id),
                        clipboard_generation: None,
                        initiator: Initiator::NativeDrag,
                    })
                }
            }
            Event::Error(error) => {
                self.hover = None;
                NativeUpdate::Error(format!("Drag-and-drop failed: {error}"))
            }
            Event::ClipboardOwnershipLost { generation } => {
                NativeUpdate::ClipboardLost(self.lose_clipboard(generation))
            }
        }
    }

    pub(crate) fn finish_transfer(
        &mut self,
        adapter: Option<&dyn Adapter>,
        request: &Request,
        report: &TransferReport,
        current: &Path,
    ) -> Consequences {
        if let Some(id) = request.inbound_id
            && let Some(adapter) = adapter
        {
            adapter.finish_inbound(id);
        }
        let completed = report.completed.len();
        let error = (!report.failures.is_empty()).then(|| {
            let details = report
                .failures
                .iter()
                .map(|failure| {
                    let name = failure
                        .source
                        .file_name()
                        .map_or_else(|| "item".into(), |name| name.to_string_lossy());
                    format!("{name}: {}", failure.error)
                })
                .collect::<Vec<_>>()
                .join("\n");
            if completed == 0 {
                details
            } else {
                format!(
                    "{} {completed} item(s); some failed:\n{details}",
                    request.action.label()
                )
            }
        });
        let clipboard = request.initiator == Initiator::Clipboard;
        if clipboard
            && request.action == Action::Move
            && request.clipboard_generation
                == self.clipboard.as_ref().map(|payload| payload.generation)
        {
            let failed = report
                .failures
                .iter()
                .map(|failure| failure.source.clone())
                .chain(report.retained.iter().cloned())
                .collect::<Vec<_>>();
            if failed.is_empty() {
                self.clipboard = None;
            } else if let Some(payload) = self.clipboard.as_mut() {
                payload.paths = failed;
            }
        }
        Consequences {
            status: if !report.retained.is_empty() && error.is_none() {
                Some(format!(
                    "Transfer cancelled; {} pending item(s) were left unchanged",
                    report.retained.len()
                ))
            } else {
                (!clipboard && error.is_none())
                    .then(|| format!("{} {completed} item(s)", request.action.label()))
            },
            error,
            changed_folders: if completed > 0 {
                vec![current.to_path_buf(), request.destination.clone()]
            } else {
                Vec::new()
            },
            refresh: completed > 0,
            select: if clipboard {
                report.completed.clone()
            } else {
                Vec::new()
            },
        }
    }

    pub(crate) fn native_hover_destination(&self) -> Option<Option<&Path>> {
        self.hover
            .as_ref()
            .map(|hover| hover.destination.as_deref())
    }

    pub(crate) fn is_native_active(&self) -> bool {
        self.native_active
    }

    pub(crate) fn stop<A: Adapter>(&mut self, adapter: &A) {
        self.drag = None;
        self.native_active = false;
        self.hover = None;
        adapter.shutdown();
    }
}

fn valid_target(paths: &[PathBuf], destination: &Path, action: Action) -> bool {
    !paths.is_empty()
        && paths.iter().all(|source| {
            source != destination
                && !destination.starts_with(source)
                && (action == Action::Copy || source.parent() != Some(destination))
        })
}

fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        sync::{Arc, Mutex},
    };

    use super::*;

    type Targets = Arc<Mutex<Vec<(u64, Option<PathBuf>)>>>;

    #[derive(Clone, Default)]
    struct MemoryAdapter {
        targets: Targets,
        finished: Arc<Mutex<Vec<u64>>>,
        starts: Arc<Mutex<Vec<Vec<PathBuf>>>>,
    }

    impl Adapter for MemoryAdapter {
        fn start(
            &self,
            paths: Vec<PathBuf>,
            _: Preview,
            _: bool,
        ) -> Result<AdapterCompletion, String> {
            self.starts.lock().unwrap().push(paths);
            Ok(Box::pin(async { Ok(Outcome::Cancelled) }))
        }

        fn set_target(&self, id: u64, destination: Option<PathBuf>) {
            self.targets.lock().unwrap().push((id, destination));
        }

        fn finish_inbound(&self, id: u64) {
            self.finished.lock().unwrap().push(id);
        }

        fn shutdown(&self) {}
    }

    fn entry(path: &str, directory: bool) -> FileEntry {
        let path = PathBuf::from(path);
        FileEntry {
            name: path.file_name().unwrap().to_os_string(),
            path,
            directory,
        }
    }

    fn preview(_: &[FileEntry]) -> Option<Preview> {
        Some(Preview {
            icon: b"",
            count: 1,
            copy: false,
            background: [0; 4],
            icon_color: [0; 4],
            accent: [0; 4],
            badge_text: [0; 4],
        })
    }

    #[test]
    fn selected_drag_builds_one_validated_transfer_request() {
        let entries = [entry("/start/one", false), entry("/start/two", false)];
        let selected = [0, 1].into_iter().collect();
        let request = TransferWorkflow::request(
            &entries,
            &selected,
            0,
            PathBuf::from("/target"),
            Action::Move,
        )
        .unwrap();

        assert_eq!(
            request.paths,
            [PathBuf::from("/start/one"), PathBuf::from("/start/two")]
        );
        assert!(
            TransferWorkflow::request(
                &entries,
                &selected,
                0,
                PathBuf::from("/start"),
                Action::Move,
            )
            .is_none()
        );
    }

    #[test]
    fn memory_adapter_observes_hover_and_invalid_drop_completion() {
        let adapter = MemoryAdapter::default();
        let mut workflow = TransferWorkflow::default();
        let target = PathBuf::from("/target");
        let update = workflow.handle_native(
            &adapter,
            Event::Hover {
                id: 7,
                position: Point::ORIGIN,
                action: Action::Copy,
            },
            |_, _| Some(target.clone()),
        );
        assert!(matches!(update, NativeUpdate::Status(_)));
        assert_eq!(
            adapter.targets.lock().unwrap().as_slice(),
            [(7, Some(PathBuf::from("/target")))]
        );

        let update = workflow.handle_native(
            &adapter,
            Event::Drop {
                id: 9,
                paths: vec![PathBuf::from("/start/folder")],
                destination: PathBuf::from("/start/folder/child"),
                action: Action::Move,
            },
            |_, _| None,
        );
        assert!(matches!(update, NativeUpdate::Notice(_)));
        assert_eq!(adapter.finished.lock().unwrap().as_slice(), [9]);
    }

    #[test]
    fn memory_adapter_is_the_second_adapter_at_the_outgoing_seam() {
        let adapter = MemoryAdapter::default();
        let mut workflow = TransferWorkflow::default();
        let entries = vec![FileEntry {
            path: PathBuf::from("/start/item"),
            name: OsString::from("item"),
            directory: false,
        }];

        let (count, _) = workflow
            .start_outgoing(&adapter, &entries, &BTreeSet::new(), 0, false, preview)
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            adapter.starts.lock().unwrap().as_slice(),
            [vec![PathBuf::from("/start/item")]]
        );
        assert!(workflow.is_native_active());
    }

    #[test]
    fn transfer_completion_finishes_the_inbound_adapter_and_returns_refresh_work() {
        let adapter = MemoryAdapter::default();
        let mut workflow = TransferWorkflow::default();
        let request = Request {
            paths: vec![PathBuf::from("/start/item")],
            destination: PathBuf::from("/target"),
            action: Action::Move,
            inbound_id: Some(12),
            clipboard_generation: None,
            initiator: Initiator::NativeDrag,
        };
        let report = TransferReport {
            completed: vec![PathBuf::from("/target/item")],
            failures: Vec::new(),
            retained: Vec::new(),
        };

        let consequences =
            workflow.finish_transfer(Some(&adapter), &request, &report, Path::new("/start"));

        assert_eq!(adapter.finished.lock().unwrap().as_slice(), [12]);
        assert_eq!(consequences.status.as_deref(), Some("Moved 1 item(s)"));
        assert_eq!(
            consequences.changed_folders,
            [PathBuf::from("/start"), PathBuf::from("/target")]
        );
        assert!(consequences.refresh);
        assert!(consequences.select.is_empty());
    }

    #[test]
    fn clipboard_preserves_single_entry_copy_and_selects_the_pasted_result() {
        let entries = [entry("/start/one", false), entry("/start/two", false)];
        let mut workflow = TransferWorkflow::default();

        assert_eq!(workflow.copy(&entries[1..]).as_deref(), Some("Copied two"));
        let request = workflow.paste(PathBuf::from("/target")).unwrap();
        assert_eq!(request.paths, [PathBuf::from("/start/two")]);
        assert_eq!(request.action, Action::Copy);

        let report = TransferReport {
            completed: vec![PathBuf::from("/target/two")],
            failures: Vec::new(),
            retained: Vec::new(),
        };
        let consequences = workflow.finish_transfer(None, &request, &report, Path::new("/target"));

        assert_eq!(consequences.status, None);
        assert_eq!(consequences.select, [PathBuf::from("/target/two")]);
        assert!(workflow.paste(PathBuf::from("/another-target")).is_some());
    }

    #[test]
    fn clipboard_copies_selected_entries_in_display_order() {
        let entries = [
            entry("/start/one", false),
            entry("/start/two", false),
            entry("/start/three", false),
        ];
        let selected = vec![entries[0].clone(), entries[2].clone()];
        let mut workflow = TransferWorkflow::default();

        assert_eq!(workflow.copy(&selected).as_deref(), Some("Copied 2 items"));
        let request = workflow.paste(PathBuf::from("/target")).unwrap();

        assert_eq!(
            request.paths,
            [PathBuf::from("/start/one"), PathBuf::from("/start/three")]
        );
        assert_eq!(request.action, Action::Copy);
    }

    #[test]
    fn clipboard_generation_changes_when_copy_replaces_the_payload() {
        let mut workflow = TransferWorkflow::default();

        workflow.copy(&[entry("/start/one", false)]).unwrap();
        let first = workflow
            .paste(PathBuf::from("/target"))
            .unwrap()
            .clipboard_generation;
        workflow.copy(&[entry("/start/two", false)]).unwrap();
        let second = workflow
            .paste(PathBuf::from("/target"))
            .unwrap()
            .clipboard_generation;

        assert_ne!(first, second);
    }

    #[test]
    fn imported_clipboard_reuses_only_an_identical_current_generation() {
        let mut workflow = TransferWorkflow::default();
        workflow.copy(&[entry("/start/one", false)]).unwrap();
        let current = workflow.clipboard_payload().unwrap();

        assert!(workflow.import_clipboard(ClipboardImport {
            paths: current.paths.clone(),
            action: current.action,
            generation: Some(current.generation),
        }));
        assert_eq!(workflow.clipboard_payload(), Some(current.clone()));

        assert!(workflow.import_clipboard(ClipboardImport {
            paths: vec![PathBuf::from("/external/two")],
            action: Action::Copy,
            generation: Some(current.generation),
        }));
        assert_ne!(
            workflow.clipboard_payload().unwrap().generation,
            current.generation
        );
    }

    #[test]
    fn clipboard_completion_selects_every_pasted_result() {
        let entries = vec![entry("/start/one", false), entry("/start/two", false)];
        let mut workflow = TransferWorkflow::default();
        workflow.copy(&entries).unwrap();
        let request = workflow.paste(PathBuf::from("/target")).unwrap();
        let report = TransferReport {
            completed: vec![PathBuf::from("/target/one"), PathBuf::from("/target/two")],
            failures: Vec::new(),
            retained: Vec::new(),
        };

        let consequences = workflow.finish_transfer(None, &request, &report, Path::new("/target"));

        assert_eq!(consequences.select, report.completed);
        assert!(workflow.paste(PathBuf::from("/another-target")).is_some());
    }

    #[test]
    fn cut_stays_pending_until_cancel_or_the_matching_ownership_is_lost() {
        let entries = [entry("/start/one", false), entry("/start/two", false)];
        let mut workflow = TransferWorkflow::default();

        assert_eq!(
            workflow.cut(&entries).as_deref(),
            Some("Cut: 2 items, p paste, Esc cancel")
        );
        let generation = workflow.clipboard_payload().unwrap().generation;
        assert_eq!(
            workflow.pending_cut_paths(),
            [PathBuf::from("/start/one"), PathBuf::from("/start/two")]
        );
        assert!(!workflow.lose_clipboard(generation + 1));
        assert!(workflow.lose_clipboard(generation));
        assert!(workflow.pending_cut_paths().is_empty());

        workflow.cut(&entries).unwrap();
        assert!(workflow.cancel_cut().is_some());
        assert!(workflow.pending_cut_paths().is_empty());
    }

    #[test]
    fn partial_cut_move_keeps_only_failed_sources_pending() {
        let entries = [entry("/start/one", false), entry("/start/two", false)];
        let mut workflow = TransferWorkflow::default();
        workflow.cut(&entries).unwrap();
        let request = workflow.paste(PathBuf::from("/target")).unwrap();
        let report = TransferReport {
            completed: vec![PathBuf::from("/target/one")],
            failures: vec![crate::fs::TransferFailure {
                source: PathBuf::from("/start/two"),
                error: "denied".to_owned(),
            }],
            retained: Vec::new(),
        };

        workflow.finish_transfer(None, &request, &report, Path::new("/target"));

        assert_eq!(workflow.pending_cut_paths(), [PathBuf::from("/start/two")]);
        assert!(workflow.paste(PathBuf::from("/retry")).is_some());
    }

    #[test]
    fn cut_paste_into_the_source_directory_is_a_no_op() {
        let mut workflow = TransferWorkflow::default();
        workflow.cut(&[entry("/start/one", false)]).unwrap();

        assert!(workflow.paste(PathBuf::from("/start")).is_none());
        assert_eq!(workflow.pending_cut_paths(), [PathBuf::from("/start/one")]);
    }
}
