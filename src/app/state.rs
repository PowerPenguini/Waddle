use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::fs::FileEntry;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ViewMode {
    #[default]
    Grid,
    Ranger,
}

#[derive(Clone, Debug)]
pub(super) struct DraggedEntry {
    pub(super) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MountRoot {
    pub(super) path: PathBuf,
    pub(super) label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NodeKind {
    Computer,
    Drive,
    Folder,
}

#[derive(Clone, Debug)]
pub(super) struct FolderNode {
    pub(super) id: u64,
    pub(super) path: PathBuf,
    pub(super) label: String,
    pub(super) kind: NodeKind,
    pub(super) expanded: bool,
    pub(super) loading: bool,
    pub(super) loaded: bool,
    pub(super) children: Vec<FolderNode>,
}

impl FolderNode {
    pub(super) fn root(id: u64) -> Self {
        Self {
            id,
            path: PathBuf::from("/"),
            label: "Computer".to_owned(),
            kind: NodeKind::Computer,
            expanded: true,
            loading: true,
            loaded: false,
            children: Vec::new(),
        }
    }

    pub(super) fn drive(id: u64, mount: MountRoot) -> Self {
        Self {
            id,
            path: mount.path,
            label: mount.label,
            kind: NodeKind::Drive,
            expanded: false,
            loading: false,
            loaded: false,
            children: Vec::new(),
        }
    }

    pub(super) fn folder(id: u64, path: PathBuf) -> Self {
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Self {
            id,
            path,
            label,
            kind: NodeKind::Folder,
            expanded: false,
            loading: false,
            loaded: false,
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum PendingName {
    NewFolder,
    Rename(FileEntry),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NavigationKind {
    Forward { remember: bool },
    Back { expected: PathBuf },
    HistoryForward { expected: PathBuf },
    Refresh { keep_operation_busy: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingNavigation {
    pub(super) requested: PathBuf,
    pub(super) kind: NavigationKind,
    pub(super) select: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) struct ExplorerState {
    pub(super) current: PathBuf,
    pub(super) history: Vec<PathBuf>,
    pub(super) forward_history: Vec<PathBuf>,
    pub(super) directory_entries: Vec<FileEntry>,
    pub(super) entries: Vec<FileEntry>,
    pub(super) selected_entry: Option<usize>,
    pub(super) selected_entries: BTreeSet<usize>,
    pub(super) visual_selection_anchor: Option<usize>,
    pub(super) selected_details: Option<String>,
    pub(super) details_generation: u64,
    pub(super) view_mode: ViewMode,
    pub(super) parent_entries: Vec<FileEntry>,
    pub(super) selected_parent_entry: Option<usize>,
    pub(super) preview_generation: u64,
    pub(super) roots: Vec<FolderNode>,
    pub(super) visible_tree_ids: Vec<u64>,
    next_node_id: u64,
    pub(super) pending_navigation: Option<PendingNavigation>,
    pub(super) pending_name: Option<PendingName>,
    pub(super) pending_delete: Vec<FileEntry>,
    pub(super) copied_entry: Option<PathBuf>,
    pub(super) search_origin: Option<Option<usize>>,
    pub(super) search_draft: String,
    pub(super) last_search: String,
    pub(super) recursive_search_active: bool,
    pub(super) recursive_search_loading: bool,
    pub(super) recursive_search_truncated: bool,
}

impl ExplorerState {
    pub(super) fn new(current: PathBuf, mounts: Vec<MountRoot>) -> Self {
        let mut state = Self {
            current,
            history: Vec::new(),
            forward_history: Vec::new(),
            directory_entries: Vec::new(),
            entries: Vec::new(),
            selected_entry: None,
            selected_entries: BTreeSet::new(),
            visual_selection_anchor: None,
            selected_details: None,
            details_generation: 0,
            view_mode: ViewMode::Grid,
            parent_entries: Vec::new(),
            selected_parent_entry: None,
            preview_generation: 0,
            roots: vec![FolderNode::root(1)],
            visible_tree_ids: Vec::new(),
            next_node_id: 2,
            pending_navigation: None,
            pending_name: None,
            pending_delete: Vec::new(),
            copied_entry: None,
            search_origin: None,
            search_draft: String::new(),
            last_search: String::new(),
            recursive_search_active: false,
            recursive_search_loading: false,
            recursive_search_truncated: false,
        };
        for mount in mounts {
            let id = state.allocate_node_id();
            state.roots.push(FolderNode::drive(id, mount));
        }
        state
    }

    pub(super) fn allocate_node_id(&mut self) -> u64 {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    pub(super) fn move_selection(
        &mut self,
        horizontal: i32,
        vertical: i32,
        columns: i32,
    ) -> Option<usize> {
        if self.entries.is_empty() {
            self.select_only(None);
            return None;
        }

        let last = self.entries.len() - 1;
        let columns = usize::try_from(columns).unwrap_or(1).max(1);
        let Some(current) = self.selected_entry else {
            let next = if horizontal < 0 || vertical < 0 {
                last
            } else {
                0
            };
            self.selected_entry = Some(next);
            self.update_keyboard_selection();
            return self.selected_entry;
        };

        let column = current % columns;
        let next = if horizontal < 0 {
            if column > 0 { current - 1 } else { current }
        } else if horizontal > 0 {
            if column + 1 < columns && current < last {
                current + 1
            } else {
                current
            }
        } else if vertical < 0 {
            current.checked_sub(columns).unwrap_or(current)
        } else if vertical > 0 {
            if current / columns == last / columns {
                current
            } else {
                current.saturating_add(columns).min(last)
            }
        } else {
            current
        };
        self.selected_entry = Some(next);
        self.update_keyboard_selection();
        self.selected_entry
    }

    pub(super) fn move_ranger_selection(&mut self, delta: i32) -> Option<usize> {
        if self.entries.is_empty() {
            self.select_only(None);
            return None;
        }

        let last = self.entries.len() - 1;
        let Some(current) = self.selected_entry else {
            self.selected_entry = Some(if delta < 0 { last } else { 0 });
            self.update_keyboard_selection();
            return self.selected_entry;
        };
        let next = if delta < 0 {
            current.saturating_sub(1)
        } else if delta > 0 {
            current.saturating_add(1).min(last)
        } else {
            current
        };
        self.selected_entry = Some(next);
        self.update_keyboard_selection();
        self.selected_entry
    }

    pub(super) fn select_only(&mut self, selected: Option<usize>) {
        self.selected_entry = selected.filter(|index| *index < self.entries.len());
        self.selected_entries.clear();
        self.selected_entries.extend(self.selected_entry);
        self.visual_selection_anchor = None;
    }

    pub(super) fn toggle_visual_selection(&mut self) {
        if self.visual_selection_anchor.take().is_some() {
            return;
        }
        if self.entries.is_empty() {
            self.select_only(None);
            return;
        }
        let selected = self.selected_entry.unwrap_or(0).min(self.entries.len() - 1);
        self.selected_entry = Some(selected);
        self.visual_selection_anchor = Some(selected);
        self.update_keyboard_selection();
    }

    pub(super) fn cancel_visual_selection(&mut self) {
        self.select_only(self.selected_entry);
    }

    pub(super) fn select_delete_motion(&mut self, motion: &str, columns: i32) -> bool {
        let Some(current) = self
            .selected_entry
            .filter(|index| *index < self.entries.len())
        else {
            return false;
        };
        let columns = usize::try_from(columns).unwrap_or(1).max(1);
        let last = self.entries.len() - 1;
        let row_start = current / columns * columns;
        let row_end = row_start.saturating_add(columns - 1).min(last);
        let range = match motion {
            "0" => row_start..=current,
            "$" => current..=row_end,
            "d" => row_start..=row_end,
            "h" => current.saturating_sub(usize::from(current > row_start))..=current,
            "l" => current..=current.saturating_add(1).min(row_end),
            "j" => row_start..=row_end.saturating_add(columns).min(last),
            "k" => row_start.saturating_sub(columns)..=row_end,
            _ => return false,
        };
        self.selected_entries.clear();
        self.selected_entries.extend(range);
        self.visual_selection_anchor = None;
        true
    }

    pub(super) fn select_rectangle(
        &mut self,
        start_row: i32,
        start_column: i32,
        end_row: i32,
        end_column: i32,
        columns: i32,
    ) {
        self.visual_selection_anchor = None;
        self.selected_entries.clear();
        if self.entries.is_empty() {
            self.selected_entry = None;
            return;
        }

        let columns = usize::try_from(columns).unwrap_or(1).max(1);
        let last_row = (self.entries.len() - 1) / columns;
        let first_row = usize::try_from(start_row.min(end_row).max(0)).unwrap_or(0);
        let final_row = usize::try_from(start_row.max(end_row).max(0))
            .unwrap_or(usize::MAX)
            .min(last_row);
        let first_column = usize::try_from(start_column.min(end_column).max(0)).unwrap_or(0);
        let final_column = usize::try_from(start_column.max(end_column).max(0))
            .unwrap_or(usize::MAX)
            .min(columns - 1);
        if first_row > final_row || first_column > final_column {
            self.selected_entry = None;
            return;
        }
        for row in first_row..=final_row {
            for column in first_column..=final_column {
                let index = row * columns + column;
                if index < self.entries.len() {
                    self.selected_entries.insert(index);
                }
            }
        }

        let target_row = usize::try_from(end_row.max(0)).unwrap_or(0).min(last_row);
        let target_column = usize::try_from(end_column.max(0))
            .unwrap_or(0)
            .min(columns - 1);
        let target = target_row * columns + target_column;
        self.selected_entry = self
            .selected_entries
            .iter()
            .copied()
            .min_by_key(|index| index.abs_diff(target));
    }

    fn update_keyboard_selection(&mut self) {
        let Some(selected) = self.selected_entry else {
            self.selected_entries.clear();
            return;
        };
        self.selected_entries.clear();
        if let Some(anchor) = self.visual_selection_anchor {
            self.selected_entries
                .extend(anchor.min(selected)..=anchor.max(selected));
        } else {
            self.selected_entries.insert(selected);
        }
    }

    pub(super) fn begin_preview(&mut self) -> u64 {
        self.preview_generation += 1;
        self.preview_generation
    }

    pub(super) fn begin_details(&mut self) -> u64 {
        self.details_generation += 1;
        self.selected_details = None;
        self.details_generation
    }

    pub(super) fn accepts_details(&self, generation: u64, path: &Path) -> bool {
        self.details_generation == generation
            && self
                .selected_entry
                .and_then(|index| self.entries.get(index))
                .is_some_and(|entry| entry.path == path)
    }

    pub(super) fn accepts_preview(&self, generation: u64, path: &Path) -> bool {
        self.preview_generation == generation
            && self
                .selected_entry
                .and_then(|index| self.entries.get(index))
                .is_some_and(|entry| entry.path == path)
    }

    pub(super) fn begin_navigation(&mut self, navigation: PendingNavigation) {
        self.pending_navigation = Some(navigation);
    }

    pub(super) fn cancel_navigation(&mut self) -> bool {
        self.pending_navigation.take().is_some()
    }

    pub(super) fn take_navigation_for(&mut self, path: &Path) -> Option<PendingNavigation> {
        (self.pending_navigation.as_ref()?.requested == path)
            .then(|| self.pending_navigation.take())
            .flatten()
    }

    pub(super) fn commit_navigation(
        &mut self,
        navigation: PendingNavigation,
        canonical_path: PathBuf,
        entries: Vec<FileEntry>,
    ) -> bool {
        match navigation.kind {
            NavigationKind::Forward { remember } => {
                if canonical_path != self.current && remember {
                    self.history.push(self.current.clone());
                    self.forward_history.clear();
                }
                self.current = canonical_path;
            }
            NavigationKind::Back { expected } => {
                if self.history.last() != Some(&expected) {
                    return false;
                }
                self.history.pop();
                self.forward_history.push(self.current.clone());
                self.current = canonical_path;
            }
            NavigationKind::HistoryForward { expected } => {
                if self.forward_history.last() != Some(&expected) {
                    return false;
                }
                self.forward_history.pop();
                self.history.push(self.current.clone());
                self.current = canonical_path;
            }
            NavigationKind::Refresh { .. } => {
                self.current = canonical_path;
            }
        }
        self.selected_entry = navigation
            .select
            .as_deref()
            .and_then(|path| entries.iter().position(|entry| entry.path == path))
            .or_else(|| (self.view_mode == ViewMode::Ranger && !entries.is_empty()).then_some(0));
        self.selected_entries.clear();
        self.selected_entries.extend(self.selected_entry);
        self.visual_selection_anchor = None;
        self.directory_entries = entries.clone();
        self.entries = entries;
        self.search_origin = None;
        self.search_draft.clear();
        self.recursive_search_active = false;
        self.recursive_search_loading = false;
        self.recursive_search_truncated = false;
        self.begin_details();
        true
    }

    pub(super) fn reconcile_mounts(&mut self, mounts: Vec<MountRoot>) -> bool {
        let old_signature: Vec<_> = self
            .roots
            .iter()
            .skip(1)
            .map(|root| (root.path.clone(), root.label.clone()))
            .collect();
        let new_signature: Vec<_> = mounts
            .iter()
            .map(|mount| (mount.path.clone(), mount.label.clone()))
            .collect();
        if old_signature == new_signature {
            return false;
        }

        let mut previous = self.roots.split_off(1);
        let mut reconciled = Vec::with_capacity(mounts.len());
        for mount in mounts {
            if let Some(position) = previous.iter().position(|node| node.path == mount.path) {
                let mut node = previous.remove(position);
                node.label = mount.label;
                reconciled.push(node);
            } else {
                let id = self.allocate_node_id();
                reconciled.push(FolderNode::drive(id, mount));
            }
        }
        self.roots.extend(reconciled);
        true
    }
}
