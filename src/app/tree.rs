use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gio::prelude::{FileExt, MountExt, VolumeExt, VolumeMonitorExt};

use crate::fs::StorageUsage;

use super::{grid::Motion, places};

pub(super) const COMPACT_ROW_HEIGHT: f32 = 32.0;
pub(super) const STORAGE_ROW_HEIGHT: f32 = 56.0;
pub(super) const SECTION_SEPARATOR_HEIGHT: f32 = 13.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VolumeRoot {
    pub(super) id: String,
    pub(super) path: Option<PathBuf>,
    pub(super) label: String,
    pub(super) can_unmount: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum NodeKind {
    Computer,
    Drive,
    Folder,
    Home,
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
    Favorite,
    Recent,
    Trash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SidebarSection {
    Places,
    Utilities,
    Devices,
}

pub(super) fn sidebar_section(kind: NodeKind) -> SidebarSection {
    match kind {
        NodeKind::Computer | NodeKind::Drive => SidebarSection::Devices,
        NodeKind::Recent | NodeKind::Trash => SidebarSection::Utilities,
        NodeKind::Folder
        | NodeKind::Home
        | NodeKind::Desktop
        | NodeKind::Documents
        | NodeKind::Downloads
        | NodeKind::Music
        | NodeKind::Pictures
        | NodeKind::Videos
        | NodeKind::Favorite => SidebarSection::Places,
    }
}

#[derive(Clone, Debug)]
struct FolderNode {
    id: u64,
    path: Option<PathBuf>,
    volume_id: Option<String>,
    label: String,
    kind: NodeKind,
    expanded: bool,
    loading: bool,
    loaded: bool,
    load_revision: u64,
    children: Vec<FolderNode>,
    favorite_index: Option<usize>,
    can_unmount: bool,
    storage_usage: Option<StorageUsage>,
    storage_usage_loading: bool,
}

impl FolderNode {
    fn root(id: u64) -> Self {
        Self {
            id,
            path: Some(PathBuf::from("/")),
            volume_id: None,
            label: "Computer".to_owned(),
            kind: NodeKind::Computer,
            expanded: false,
            loading: false,
            loaded: true,
            load_revision: 0,
            children: Vec::new(),
            favorite_index: None,
            can_unmount: false,
            storage_usage: None,
            storage_usage_loading: false,
        }
    }

    fn drive(id: u64, volume: VolumeRoot) -> Self {
        Self {
            id,
            path: volume.path,
            volume_id: Some(volume.id),
            label: volume.label,
            kind: NodeKind::Drive,
            expanded: false,
            loading: false,
            loaded: false,
            load_revision: 0,
            children: Vec::new(),
            favorite_index: None,
            can_unmount: volume.can_unmount,
            storage_usage: None,
            storage_usage_loading: false,
        }
    }

    fn folder(id: u64, path: PathBuf) -> Self {
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Self {
            id,
            path: Some(path),
            volume_id: None,
            label,
            kind: NodeKind::Folder,
            expanded: false,
            loading: false,
            loaded: false,
            load_revision: 0,
            children: Vec::new(),
            favorite_index: None,
            can_unmount: false,
            storage_usage: None,
            storage_usage_loading: false,
        }
    }

    fn location(
        id: u64,
        path: PathBuf,
        label: String,
        kind: NodeKind,
        favorite_index: Option<usize>,
    ) -> Self {
        Self {
            id,
            path: Some(path),
            volume_id: None,
            label,
            kind,
            expanded: false,
            loading: false,
            loaded: false,
            load_revision: 0,
            children: Vec::new(),
            favorite_index,
            can_unmount: false,
            storage_usage: None,
            storage_usage_loading: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TreeRow {
    pub(super) id: u64,
    pub(super) path: Option<PathBuf>,
    pub(super) label: String,
    pub(super) depth: usize,
    pub(super) loading: bool,
    pub(super) selected: bool,
    pub(super) focused: bool,
    pub(super) kind: NodeKind,
    pub(super) favorite_index: Option<usize>,
    pub(super) volume_id: Option<String>,
    pub(super) can_unmount: bool,
    pub(super) storage_usage: Option<StorageUsage>,
}

impl TreeRow {
    pub(super) fn shows_storage_usage(&self) -> bool {
        self.kind == NodeKind::Computer || (self.kind == NodeKind::Drive && self.path.is_some())
    }

    pub(super) fn height(&self) -> f32 {
        if self.shows_storage_usage() {
            STORAGE_ROW_HEIGHT
        } else {
            COMPACT_ROW_HEIGHT
        }
    }
}

fn row_heights_for(rows: &[TreeRow]) -> impl Iterator<Item = f32> + '_ {
    let mut previous_section = None;
    rows.iter().map(move |row| {
        let separator = if row.depth == 0 {
            let section = sidebar_section(row.kind);
            let separated = previous_section.is_some_and(|previous| previous != section);
            previous_section = Some(section);
            if separated {
                SECTION_SEPARATOR_HEIGHT
            } else {
                0.0
            }
        } else {
            0.0
        };
        separator + row.height()
    })
}

fn viewport_rows(rows: &[TreeRow], viewport: std::ops::Range<f32>) -> std::ops::Range<usize> {
    debug_assert!(!rows.is_empty());
    let mut bottom = super::grid::LIST_VIEW_TOP_INSET;
    let height = (viewport.end - viewport.start).max(1.0);
    let maximum = (bottom + row_heights_for(rows).sum::<f32>() - height).max(0.0);
    let top = viewport.start.clamp(0.0, maximum);
    let end = top + height;
    let mut first = None;
    let mut last = 0;
    let mut nearest = None;
    for (index, (row, extent)) in rows.iter().zip(row_heights_for(rows)).enumerate() {
        bottom += extent;
        if bottom > top {
            nearest.get_or_insert(index);
        }
        // Section gaps are not rows. Include a row only when its actual bounds
        // overlap the viewport, including partially visible top/bottom rows.
        if bottom > top && bottom - row.height() < end {
            first.get_or_insert(index);
            last = index;
        }
    }
    let first = first.or(nearest).unwrap_or(rows.len() - 1);
    first..last.max(first) + 1
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadRequest {
    pub(super) id: u64,
    pub(super) path: PathBuf,
    pub(super) revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StorageUsageRequest {
    pub(super) id: u64,
    pub(super) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum LoadOutcome {
    Installed,
    Failed(String),
    Ignored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Activation {
    Recent,
    Trash,
    MountVolume {
        id: String,
        label: String,
    },
    Folder {
        path: PathBuf,
        load: Option<LoadRequest>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum MoveOutcome {
    None,
    Focused(String),
    Activate(u64),
}

#[derive(Debug)]
pub(super) struct SidebarTree {
    roots: Vec<FolderNode>,
    next_node_id: u64,
    cursor: Option<u64>,
    places: places::Places,
    favorite_drag: Option<usize>,
}

impl SidebarTree {
    pub(super) fn new(volumes: Vec<VolumeRoot>) -> Self {
        Self::with_places(volumes, places::Places::open_default())
    }

    fn with_places(volumes: Vec<VolumeRoot>, places: places::Places) -> Self {
        let mut tree = Self {
            roots: vec![FolderNode::root(1)],
            next_node_id: 2,
            cursor: None,
            places,
            favorite_drag: None,
        };
        for volume in volumes {
            let id = tree.allocate_node_id();
            tree.roots.push(FolderNode::drive(id, volume));
        }
        tree
    }

    pub(super) fn open_default(additional_locations: Vec<places::Entry>) -> Self {
        let mut tree = Self::new(volume_roots());
        tree.install_locations(additional_locations);
        tree
    }

    fn allocate_node_id(&mut self) -> u64 {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    pub(super) fn reconcile_volumes(&mut self, volumes: Vec<VolumeRoot>) -> bool {
        let old_signature = self
            .roots
            .iter()
            .filter(|root| root.kind == NodeKind::Drive)
            .map(|root| {
                (
                    root.volume_id.clone(),
                    root.path.clone(),
                    root.label.clone(),
                    root.can_unmount,
                )
            })
            .collect::<Vec<_>>();
        let new_signature = volumes
            .iter()
            .map(|volume| {
                (
                    Some(volume.id.clone()),
                    volume.path.clone(),
                    volume.label.clone(),
                    volume.can_unmount,
                )
            })
            .collect::<Vec<_>>();
        if old_signature == new_signature {
            return false;
        }

        let mut previous = self
            .roots
            .iter()
            .filter(|root| root.kind == NodeKind::Drive)
            .cloned()
            .collect::<Vec<_>>();
        let mut reconciled = Vec::with_capacity(volumes.len());
        for volume in volumes {
            if let Some(position) = previous
                .iter()
                .position(|node| node.volume_id.as_deref() == Some(volume.id.as_str()))
            {
                let mut node = previous.remove(position);
                if node.path != volume.path {
                    node.expanded = false;
                    node.loading = false;
                    node.loaded = false;
                    node.children.clear();
                    node.storage_usage = None;
                    node.storage_usage_loading = false;
                }
                node.path = volume.path;
                node.label = volume.label;
                node.can_unmount = volume.can_unmount;
                reconciled.push(node);
            } else {
                let id = self.allocate_node_id();
                reconciled.push(FolderNode::drive(id, volume));
            }
        }
        self.roots.retain(|root| root.kind != NodeKind::Drive);
        self.roots.splice(1..1, reconciled);
        self.retain_valid_cursor();
        true
    }

    pub(super) fn install_locations(&mut self, additional_locations: Vec<places::Entry>) {
        let mut locations = self.places.entries();
        locations.extend(additional_locations);
        self.replace_locations(locations);
    }

    fn replace_locations(&mut self, places: Vec<places::Entry>) {
        let mut previous = HashMap::new();
        let roots = std::mem::take(&mut self.roots);
        for node in roots {
            if matches!(
                node.kind,
                NodeKind::Computer | NodeKind::Drive | NodeKind::Folder
            ) {
                self.roots.push(node);
            } else {
                previous.insert((node.kind, node.path.clone()), node);
            }
        }
        for place in places {
            let key = (place.kind, Some(place.path.clone()));
            let node = if let Some(mut node) = previous.remove(&key) {
                node.label = place.label;
                node.favorite_index = place.favorite_index;
                node
            } else {
                let id = self.allocate_node_id();
                FolderNode::location(
                    id,
                    place.path,
                    place.label,
                    place.kind,
                    place.favorite_index,
                )
            };
            self.roots.push(node);
        }
        self.retain_valid_cursor();
    }

    #[cfg(test)]
    pub(super) fn install_places(&mut self, places: Vec<places::Entry>) {
        self.replace_locations(places);
    }

    pub(super) fn favorite_command(
        &mut self,
        current: &Path,
        arguments: &str,
        additional_locations: Vec<places::Entry>,
    ) -> Result<String, String> {
        let status = self.places.command(current, arguments)?;
        self.install_locations(additional_locations);
        Ok(status)
    }

    pub(super) fn press_favorite(&mut self, index: usize) {
        self.favorite_drag = Some(index);
    }

    pub(super) fn release_favorite(
        &mut self,
        index: usize,
        additional_locations: Vec<places::Entry>,
    ) -> Result<bool, String> {
        let Some(from) = self.favorite_drag.take() else {
            return Ok(false);
        };
        if from == index {
            return Ok(false);
        }
        self.places.reorder(from, index)?;
        self.install_locations(additional_locations);
        Ok(true)
    }

    pub(super) fn refresh_volumes(&mut self) -> bool {
        self.reconcile_volumes(volume_roots())
    }

    pub(super) fn rows(&self, current: &Path) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        flatten_nodes(&self.roots, self.cursor, 0, current, &mut rows);
        rows
    }

    pub(super) fn row_heights(&self, current: &Path) -> Vec<f32> {
        let rows = self.rows(current);
        row_heights_for(&rows).collect()
    }

    pub(super) fn focused_row_bounds(&self, current: &Path) -> Option<(std::ops::Range<f32>, f32)> {
        let rows = self.rows(current);
        let mut bottom = 0.0;
        let mut focused = None;
        for (row, height) in rows.iter().zip(row_heights_for(&rows)) {
            bottom += height;
            if row.focused {
                focused = Some(bottom - row.height()..bottom);
            }
        }
        focused.map(|bounds| (bounds, bottom))
    }

    pub(super) fn begin_storage_usage_refresh(&mut self) -> Vec<StorageUsageRequest> {
        self.roots
            .iter_mut()
            .filter(|node| {
                matches!(node.kind, NodeKind::Computer | NodeKind::Drive)
                    && node.path.is_some()
                    && !node.storage_usage_loading
            })
            .filter_map(|node| {
                node.storage_usage_loading = true;
                Some(StorageUsageRequest {
                    id: node.id,
                    path: node.path.clone()?,
                })
            })
            .collect()
    }

    pub(super) fn complete_storage_usage(
        &mut self,
        request: &StorageUsageRequest,
        result: Result<StorageUsage, String>,
    ) -> bool {
        let Some(node) = find_node_mut(&mut self.roots, request.id) else {
            return false;
        };
        if node.path.as_deref() != Some(request.path.as_path()) || !node.storage_usage_loading {
            return false;
        }
        node.storage_usage_loading = false;
        if let Ok(usage) = result {
            node.storage_usage = Some(usage);
        }
        true
    }

    pub(super) fn has_loading(&self) -> bool {
        has_visible_loading(&self.roots)
    }

    pub(super) fn focused_id(&self) -> Option<u64> {
        self.cursor
    }

    pub(super) fn focus(&mut self, id: u64) -> bool {
        if find_node(&self.roots, id).is_none() {
            return false;
        }
        self.cursor = Some(id);
        true
    }

    pub(super) fn focus_current_or_first(&mut self, current: &Path) {
        self.retain_valid_cursor();
        if self.cursor.is_some() {
            return;
        }
        self.cursor = self
            .rows(current)
            .into_iter()
            .find(|row| row.selected)
            .or_else(|| self.rows(current).into_iter().next())
            .map(|row| row.id);
    }

    pub(super) fn move_cursor(
        &mut self,
        motion: Motion,
        count: usize,
        current_path: &Path,
        viewport: std::ops::Range<f32>,
    ) -> MoveOutcome {
        let rows = self.rows(current_path);
        let Some(current) = self
            .cursor
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .or_else(|| rows.iter().position(|row| row.selected))
            .or((!rows.is_empty()).then_some(0))
        else {
            return MoveOutcome::None;
        };
        let last = rows.len() - 1;
        let count = count.max(1);
        let visible = if matches!(
            motion,
            Motion::ViewportTop
                | Motion::ViewportMiddle
                | Motion::ViewportBottom
                | Motion::HalfPageUp
                | Motion::HalfPageDown
        ) {
            viewport_rows(&rows, viewport)
        } else {
            0..rows.len()
        };
        let target = match motion {
            Motion::Down => current.saturating_add(count).min(last),
            Motion::Up => current.saturating_sub(count),
            Motion::First | Motion::RowStart => 0,
            Motion::Last | Motion::RowEnd => last,
            Motion::DisplayIndex(index) => index.min(last),
            Motion::ViewportTop => visible.start.saturating_add(count - 1).min(visible.end - 1),
            Motion::ViewportMiddle => visible.start + (visible.len() - 1) / 2,
            Motion::ViewportBottom => visible.end.saturating_sub(count).max(visible.start),
            Motion::HalfPageUp => {
                current.saturating_sub(count.saturating_mul(visible.len().div_ceil(2)))
            }
            Motion::HalfPageDown => current
                .saturating_add(count.saturating_mul(visible.len().div_ceil(2)))
                .min(last),
            Motion::Left => {
                let row = &rows[current];
                if find_node(&self.roots, row.id).is_some_and(|node| node.expanded) {
                    if let Some(node) = find_node_mut(&mut self.roots, row.id) {
                        node.expanded = false;
                    }
                    current
                } else {
                    rows[..current]
                        .iter()
                        .rposition(|candidate| candidate.depth < row.depth)
                        .or_else(|| {
                            (row.depth == 0 && row.kind != NodeKind::Computer)
                                .then(|| {
                                    rows[..current]
                                        .iter()
                                        .rposition(|candidate| candidate.kind == NodeKind::Computer)
                                })
                                .flatten()
                        })
                        .unwrap_or(current)
                }
            }
            Motion::Right => {
                let id = rows[current].id;
                if find_node(&self.roots, id).is_some_and(|node| !node.expanded) {
                    return MoveOutcome::Activate(id);
                }
                current.saturating_add(1).min(last)
            }
        };
        self.cursor = Some(rows[target].id);
        MoveOutcome::Focused(rows[target].label.clone())
    }

    pub(super) fn activate(&mut self, id: u64) -> Option<Activation> {
        let node = find_node_mut(&mut self.roots, id)?;
        match node.kind {
            NodeKind::Recent => Some(Activation::Recent),
            NodeKind::Trash => Some(Activation::Trash),
            NodeKind::Computer => Some(Activation::Folder {
                path: node.path.clone()?,
                load: None,
            }),
            NodeKind::Drive if node.path.is_none() => {
                if node.loading {
                    return None;
                }
                node.loading = true;
                Some(Activation::MountVolume {
                    id: node.volume_id.clone()?,
                    label: node.label.clone(),
                })
            }
            _ => {
                let path = node.path.clone()?;
                node.expanded = !node.expanded;
                let should_load = node.expanded && !node.loaded && !node.loading;
                if should_load {
                    node.loading = true;
                    node.load_revision = node.load_revision.wrapping_add(1);
                }
                Some(Activation::Folder {
                    path: path.clone(),
                    load: should_load.then_some(LoadRequest {
                        id,
                        path,
                        revision: node.load_revision,
                    }),
                })
            }
        }
    }

    pub(super) fn expand(&mut self, id: u64) -> Option<LoadRequest> {
        let node = find_node_mut(&mut self.roots, id)?;
        if node.kind == NodeKind::Computer {
            return None;
        }
        let path = node.path.clone()?;
        if node.expanded {
            return None;
        }
        node.expanded = true;
        let should_load = !node.loaded && !node.loading;
        if should_load {
            node.loading = true;
            node.load_revision = node.load_revision.wrapping_add(1);
        }
        should_load.then_some(LoadRequest {
            id,
            path,
            revision: node.load_revision,
        })
    }

    pub(super) fn install_children(
        &mut self,
        node_id: u64,
        path: &Path,
        folders: Vec<PathBuf>,
    ) -> bool {
        if find_node(&self.roots, node_id).is_none_or(|node| node.path.as_deref() != Some(path)) {
            return false;
        }
        let previous = {
            let Some(node) = find_node_mut(&mut self.roots, node_id) else {
                return false;
            };
            std::mem::take(&mut node.children)
        };
        let mut previous = previous
            .into_iter()
            .filter_map(|node| node.path.clone().map(|path| (path, node)))
            .collect::<HashMap<_, _>>();
        let children = folders
            .into_iter()
            .map(|path| {
                previous.remove(&path).unwrap_or_else(|| {
                    let id = self.allocate_node_id();
                    FolderNode::folder(id, path)
                })
            })
            .collect();
        let Some(node) = find_node_mut(&mut self.roots, node_id) else {
            return false;
        };
        node.children = children;
        node.loading = false;
        node.loaded = true;
        self.retain_valid_cursor();
        true
    }

    pub(super) fn complete_load(
        &mut self,
        request: &LoadRequest,
        result: Result<Vec<PathBuf>, String>,
    ) -> LoadOutcome {
        if find_node(&self.roots, request.id).is_none_or(|node| {
            node.path.as_deref() != Some(&request.path)
                || !node.loading
                || node.load_revision != request.revision
        }) {
            return LoadOutcome::Ignored;
        }
        match result {
            Ok(folders) => {
                if self.install_children(request.id, &request.path, folders) {
                    LoadOutcome::Installed
                } else {
                    LoadOutcome::Ignored
                }
            }
            Err(error) => {
                let Some(node) = find_node_mut(&mut self.roots, request.id) else {
                    return LoadOutcome::Ignored;
                };
                node.loading = false;
                node.loaded = false;
                node.expanded = false;
                self.retain_valid_cursor();
                LoadOutcome::Failed(error)
            }
        }
    }

    pub(super) fn cancel_load(&mut self, request: &LoadRequest) -> bool {
        let Some(node) = find_node_mut(&mut self.roots, request.id) else {
            return false;
        };
        if node.path.as_deref() != Some(&request.path)
            || !node.loading
            || node.load_revision != request.revision
        {
            return false;
        }
        node.loading = false;
        node.loaded = false;
        node.expanded = false;
        self.retain_valid_cursor();
        true
    }

    pub(super) fn invalidate(&mut self, changed_folders: &[PathBuf]) -> Vec<LoadRequest> {
        let mut reloads = Vec::new();
        invalidate_folders(&mut self.roots, changed_folders, &mut reloads);
        self.retain_valid_cursor();
        reloads
    }

    pub(super) fn expanded_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        collect_expanded_paths(&self.roots, &mut paths);
        paths
    }

    pub(super) fn mount_paths(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .filter(|node| node.kind == NodeKind::Drive)
            .filter_map(|node| node.path.clone())
            .collect()
    }

    pub(super) fn finish_volume_mount(&mut self, id: &str, path: Option<PathBuf>) -> bool {
        let Some(node) = self
            .roots
            .iter_mut()
            .find(|node| node.kind == NodeKind::Drive && node.volume_id.as_deref() == Some(id))
        else {
            return false;
        };
        node.loading = false;
        if let Some(path) = path {
            node.path = Some(path);
            node.can_unmount = true;
            node.expanded = false;
            node.loaded = false;
            node.children.clear();
            node.storage_usage = None;
            node.storage_usage_loading = false;
        }
        true
    }

    pub(super) fn begin_volume_unmount(&mut self, id: &str) -> Option<(String, PathBuf)> {
        let node = self
            .roots
            .iter_mut()
            .find(|node| node.kind == NodeKind::Drive && node.volume_id.as_deref() == Some(id))?;
        if node.loading || !node.can_unmount {
            return None;
        }
        let path = node.path.clone()?;
        node.loading = true;
        Some((node.label.clone(), path))
    }

    pub(super) fn finish_volume_unmount(&mut self, id: &str) -> bool {
        let Some(node) = self
            .roots
            .iter_mut()
            .find(|node| node.kind == NodeKind::Drive && node.volume_id.as_deref() == Some(id))
        else {
            return false;
        };
        node.loading = false;
        true
    }

    pub(super) fn volume_path(&self, id: &str) -> Option<PathBuf> {
        self.roots
            .iter()
            .find(|node| node.kind == NodeKind::Drive && node.volume_id.as_deref() == Some(id))
            .and_then(|node| node.path.clone())
    }

    pub(super) fn row_path(&self, index: usize, current: &Path) -> Option<PathBuf> {
        self.rows(current)
            .get(index)
            .and_then(|row| row.path.clone())
    }

    pub(super) fn row_target(&self, index: usize, current: &Path) -> Option<(u64, PathBuf)> {
        self.rows(current)
            .get(index)
            .and_then(|row| row.path.clone().map(|path| (row.id, path)))
    }

    fn retain_valid_cursor(&mut self) {
        if self
            .cursor
            .is_some_and(|id| find_visible_node(&self.roots, id).is_none())
        {
            self.cursor = None;
        }
    }

    #[cfg(test)]
    pub(super) fn is_expanded(&self, id: u64) -> bool {
        find_node(&self.roots, id).is_some_and(|node| node.expanded)
    }
}

fn volume_roots() -> Vec<VolumeRoot> {
    super::desktop_volumes::query(volume_roots_from_monitor).unwrap_or_default()
}

fn volume_roots_from_monitor(monitor: &gio::VolumeMonitor) -> Vec<VolumeRoot> {
    let mut roots = monitor
        .volumes()
        .into_iter()
        .filter_map(|volume| {
            let mount = volume.get_mount();
            if mount.as_ref().is_some_and(MountExt::is_shadowed) {
                return None;
            }
            if mount.is_none() && !volume.can_mount() {
                return None;
            }
            let (path, can_unmount) = match mount {
                Some(mount) => {
                    let path = mount.root().path()?;
                    if path == Path::new("/") {
                        return None;
                    }
                    (Some(path), mount.can_unmount() || mount.can_eject())
                }
                None => (None, false),
            };
            Some(VolumeRoot {
                id: places::volume_id(&volume),
                path,
                label: volume.name().to_string(),
                can_unmount,
            })
        })
        .collect::<Vec<_>>();

    for mount in monitor
        .mounts()
        .into_iter()
        .filter(|mount| !mount.is_shadowed())
    {
        let Some(path) = mount.root().path().filter(|path| path != Path::new("/")) else {
            continue;
        };
        if roots
            .iter()
            .any(|volume| volume.path.as_deref() == Some(path.as_path()))
        {
            continue;
        }
        roots.push(VolumeRoot {
            id: places::mount_id(&mount),
            path: Some(path),
            label: mount.name().to_string(),
            can_unmount: mount.can_unmount() || mount.can_eject(),
        });
    }
    roots
}

fn find_node(nodes: &[FolderNode], id: u64) -> Option<&FolderNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_visible_node(nodes: &[FolderNode], id: u64) -> Option<&FolderNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if node.expanded
            && let Some(found) = find_visible_node(&node.children, id)
        {
            return Some(found);
        }
    }
    None
}

fn find_node_mut(nodes: &mut [FolderNode], id: u64) -> Option<&mut FolderNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

fn flatten_nodes(
    nodes: &[FolderNode],
    cursor: Option<u64>,
    depth: usize,
    current: &Path,
    rows: &mut Vec<TreeRow>,
) {
    for node in nodes {
        rows.push(TreeRow {
            id: node.id,
            path: node.path.clone(),
            label: node.label.clone(),
            depth,
            loading: node.loading,
            selected: node.path.as_deref() == Some(current),
            focused: cursor == Some(node.id),
            kind: node.kind,
            favorite_index: node.favorite_index,
            volume_id: node.volume_id.clone(),
            can_unmount: node.can_unmount,
            storage_usage: node.storage_usage,
        });
        if node.expanded {
            flatten_nodes(&node.children, cursor, depth + 1, current, rows);
        }
    }
}

fn has_visible_loading(nodes: &[FolderNode]) -> bool {
    nodes
        .iter()
        .any(|node| node.loading || (node.expanded && has_visible_loading(&node.children)))
}

fn collect_expanded_paths(nodes: &[FolderNode], paths: &mut Vec<PathBuf>) {
    for node in nodes {
        if node.expanded && !matches!(node.kind, NodeKind::Recent | NodeKind::Trash) {
            if let Some(path) = &node.path {
                paths.push(path.clone());
            }
            collect_expanded_paths(&node.children, paths);
        }
    }
}

fn invalidate_folders(
    nodes: &mut [FolderNode],
    changed_folders: &[PathBuf],
    reloads: &mut Vec<LoadRequest>,
) {
    for node in nodes {
        if node
            .path
            .as_ref()
            .is_some_and(|node_path| changed_folders.iter().any(|path| path == node_path))
        {
            node.loaded = false;
            node.load_revision = node.load_revision.wrapping_add(1);
            node.loading = node.expanded;
            if node.expanded {
                reloads.push(LoadRequest {
                    id: node.id,
                    path: node.path.clone().expect("expanded nodes have paths"),
                    revision: node.load_revision,
                });
            }
        }
        invalidate_folders(&mut node.children, changed_folders, reloads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshing_a_parent_preserves_expanded_children_and_focus() {
        for outcome in ["success", "error", "cancel"] {
            let temp = tempfile::tempdir().unwrap();
            let branch = temp.path().join("branch");
            let leaf = branch.join("leaf");
            let removed = temp.path().join("removed");
            let added = temp.path().join("added");
            std::fs::create_dir_all(&leaf).unwrap();
            std::fs::create_dir(&removed).unwrap();
            let mut tree = SidebarTree::new(vec![VolumeRoot {
                id: "test-volume".to_owned(),
                path: Some(temp.path().to_path_buf()),
                label: "Data".to_owned(),
                can_unmount: true,
            }]);
            let drive = tree
                .rows(temp.path())
                .into_iter()
                .find(|row| row.kind == NodeKind::Drive)
                .unwrap();
            let load = tree.expand(drive.id).unwrap();
            assert!(matches!(
                tree.complete_load(
                    &load,
                    Ok(crate::fs::read_child_folders(temp.path()).unwrap())
                ),
                LoadOutcome::Installed
            ));
            let branch_id = tree
                .rows(temp.path())
                .into_iter()
                .find(|row| row.path.as_ref() == Some(&branch))
                .unwrap()
                .id;
            let load = tree.expand(branch_id).unwrap();
            assert!(matches!(
                tree.complete_load(&load, Ok(crate::fs::read_child_folders(&branch).unwrap())),
                LoadOutcome::Installed
            ));
            let leaf_id = tree
                .rows(temp.path())
                .into_iter()
                .find(|row| row.path.as_ref() == Some(&leaf))
                .unwrap()
                .id;
            assert!(tree.focus(leaf_id));
            std::fs::remove_dir(&removed).unwrap();
            std::fs::create_dir(&added).unwrap();
            let refresh = tree.invalidate(&[temp.path().to_path_buf()]).pop().unwrap();
            assert_eq!(
                tree.focused_id(),
                Some(leaf_id),
                "parent refresh lost focus while loading"
            );
            assert!(
                tree.rows(temp.path()).iter().any(|row| row.id == leaf_id),
                "parent refresh hid the expanded branch while loading"
            );
            let refresh = if outcome == "success" {
                refresh
            } else {
                if outcome == "error" {
                    assert!(matches!(
                        tree.complete_load(&refresh, Err("refresh failed".to_owned())),
                        LoadOutcome::Failed(_)
                    ));
                } else {
                    assert!(tree.cancel_load(&refresh));
                }
                assert_eq!(
                    tree.focused_id(),
                    None,
                    "collapsed cached children retained hidden focus"
                );
                assert!(tree.focus(leaf_id));
                tree.focus_current_or_first(temp.path());
                assert_eq!(
                    tree.focused_id(),
                    Some(drive.id),
                    "focus entry retained an invisible cached cursor"
                );
                tree.expand(drive.id)
                    .expect("failed or cancelled scans must be retryable")
            };
            assert!(matches!(
                tree.complete_load(
                    &refresh,
                    Ok(crate::fs::read_child_folders(temp.path()).unwrap())
                ),
                LoadOutcome::Installed
            ));
            assert!(
                tree.is_expanded(branch_id),
                "unchanged child branch collapsed after parent refresh"
            );
            let rows = tree.rows(temp.path());
            assert!(rows.iter().any(|row| row.id == leaf_id));
            assert!(rows.iter().any(|row| row.path.as_ref() == Some(&added)));
            assert!(!rows.iter().any(|row| row.path.as_ref() == Some(&removed)));
            if outcome == "success" {
                assert_eq!(tree.focused_id(), Some(leaf_id));
                let new_leaf = branch.join("new-leaf");
                std::fs::create_dir(&new_leaf).unwrap();
                for child_first in [false, true] {
                    let mut refreshes =
                        tree.invalidate(&[temp.path().to_path_buf(), branch.clone()]);
                    assert_eq!(
                        refreshes.len(),
                        2,
                        "parent invalidation skipped a requested child refresh"
                    );
                    if child_first {
                        refreshes.reverse();
                    }
                    for request in refreshes {
                        let folders = crate::fs::read_child_folders(&request.path).unwrap();
                        assert!(matches!(
                            tree.complete_load(&request, Ok(folders)),
                            LoadOutcome::Installed
                        ));
                    }
                    assert_eq!(tree.focused_id(), Some(leaf_id));
                    assert!(
                        tree.rows(temp.path())
                            .iter()
                            .any(|row| row.path.as_ref() == Some(&new_leaf))
                    );
                }
            }
        }
    }

    #[test]
    fn overlapping_tree_loads_ignore_older_results_and_cancellations() {
        for stale in ["success", "error", "cancel"] {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("one")).unwrap();
            let mut tree = SidebarTree::new(vec![VolumeRoot {
                id: "test-volume".to_owned(),
                path: Some(temp.path().to_path_buf()),
                label: "Data".to_owned(),
                can_unmount: true,
            }]);
            let drive = tree
                .rows(temp.path())
                .into_iter()
                .find(|row| row.kind == NodeKind::Drive)
                .unwrap();
            let old = tree.expand(drive.id).unwrap();
            let old_folders = crate::fs::read_child_folders(temp.path()).unwrap();
            std::fs::create_dir(temp.path().join("two")).unwrap();
            let newer = tree.invalidate(&[temp.path().to_path_buf()]).pop().unwrap();
            let newer_folders = crate::fs::read_child_folders(temp.path()).unwrap();
            if stale == "cancel" {
                assert!(
                    !tree.cancel_load(&old),
                    "an old cancellation cancelled the newer load"
                );
            } else {
                let result = if stale == "success" {
                    Ok(old_folders)
                } else {
                    Err("old failure".to_owned())
                };
                assert!(
                    matches!(tree.complete_load(&old, result), LoadOutcome::Ignored),
                    "an old {stale} completion superseded the newer load"
                );
            }
            assert!(
                tree.rows(temp.path())
                    .iter()
                    .find(|row| row.id == drive.id)
                    .unwrap()
                    .loading
            );
            assert!(matches!(
                tree.complete_load(&newer, Ok(newer_folders)),
                LoadOutcome::Installed
            ));
            let children: Vec<_> = tree
                .rows(temp.path())
                .into_iter()
                .filter(|row| row.depth == 1)
                .map(|row| row.label)
                .collect();
            assert_eq!(children, ["one", "two"]);
        }
    }

    #[test]
    fn native_volume_queries_share_one_thread_and_the_default_context() {
        const TRACE: &str = "WADDLE_VOLUME_MONITOR_TRACE";
        let Some(trace) = std::env::var_os(TRACE) else {
            let fixture = tempfile::tempdir().unwrap();
            let source = fixture.path().join("volume_trace.c");
            let library = fixture.path().join("volume_trace.so");
            let trace = fixture.path().join("calls.log");
            std::fs::write(
                &source,
                r#"#define _GNU_SOURCE
#include <gio/gio.h>
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
GVolumeMonitor *g_volume_monitor_get(void) {
    GVolumeMonitor *(*original)(void) = dlsym(RTLD_NEXT, "g_volume_monitor_get");
    FILE *trace = fopen(getenv("WADDLE_VOLUME_MONITOR_TRACE"), "a");
    if (trace) {
        fprintf(trace, "%lu %d\n", (unsigned long)pthread_self(),
                g_main_context_get_thread_default() != NULL);
        fclose(trace);
    }
    return original();
}
"#,
            )
            .unwrap();
            let flags = std::process::Command::new("pkg-config")
                .args(["--cflags", "--libs", "gio-2.0"])
                .output()
                .unwrap();
            assert!(flags.status.success());
            let compiled = std::process::Command::new("cc")
                .args(["-shared", "-fPIC", "-o"])
                .arg(&library)
                .arg(&source)
                .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
                .args(["-ldl", "-pthread"])
                .output()
                .unwrap();
            assert!(
                compiled.status.success(),
                "{}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "app::tree::tests::native_volume_queries_share_one_thread_and_the_default_context",
                    "--nocapture",
                ])
                .env(TRACE, &trace)
                .env("LD_PRELOAD", library)
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
            let calls = std::fs::read_to_string(trace).unwrap();
            let mut owners = std::collections::HashSet::new();
            for call in calls.lines() {
                let (thread, has_thread_context) = call.split_once(' ').unwrap();
                owners.insert(thread);
                assert_eq!(
                    has_thread_context, "0",
                    "volume monitoring used a private thread context: {calls}"
                );
            }
            assert!(
                !owners.is_empty(),
                "the fixture did not observe native volume discovery"
            );
            assert_eq!(
                owners.len(),
                1,
                "native volume discovery ran on different threads: {calls}"
            );
            return;
        };
        // Keep callers alive together, but invoke each path in turn so the
        // regression observes thread ownership without relying on a crash race.
        let turn = std::sync::atomic::AtomicUsize::new(0);
        let finished = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for index in 0..4 {
                let turn = &turn;
                let finished = &finished;
                let missing = PathBuf::from(&trace).join("nonexistent-waddle-volume");
                scope.spawn(move || {
                    while turn.load(std::sync::atomic::Ordering::Acquire) != index {
                        std::thread::yield_now();
                    }
                    match index {
                        0 => {
                            let _ = volume_roots();
                        }
                        1 => {
                            assert!(places::mount_volume(missing.to_str().unwrap()).is_err());
                        }
                        2 => {
                            assert!(places::unmount_volume(missing.to_str().unwrap()).is_err());
                        }
                        3 => {
                            assert!(
                                places::run_volume_command(&format!("mount {}", missing.display()))
                                    .is_err()
                            );
                        }
                        _ => unreachable!(),
                    }
                    turn.fetch_add(1, std::sync::atomic::Ordering::Release);
                    finished.wait();
                });
            }
        });
    }

    #[test]
    fn unmounted_volume_is_visible_and_activates_mount_before_navigation() {
        let volume = VolumeRoot {
            id: "uuid:1234".to_owned(),
            path: None,
            label: "USB Stick".to_owned(),
            can_unmount: false,
        };
        let mut tree = SidebarTree::new(vec![volume.clone()]);
        let row = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.label == volume.label)
            .expect("unmounted volumes belong in the sidebar");

        assert_eq!(row.kind, NodeKind::Drive);
        assert_eq!(row.path, None);
        assert_eq!(tree.row_path(1, Path::new("/")), None);
        assert_eq!(
            tree.activate(row.id),
            Some(Activation::MountVolume {
                id: volume.id.clone(),
                label: volume.label.clone(),
            })
        );
        assert!(
            tree.rows(Path::new("/"))
                .into_iter()
                .find(|candidate| candidate.id == row.id)
                .unwrap()
                .loading
        );
        assert_eq!(tree.activate(row.id), None);
        assert!(tree.finish_volume_mount(&volume.id, None));
        assert!(matches!(
            tree.activate(row.id),
            Some(Activation::MountVolume { .. })
        ));

        let original_id = row.id;
        let mounted_path = PathBuf::from("/run/media/user/USB Stick");
        assert!(tree.finish_volume_mount(&volume.id, Some(mounted_path.clone())));
        let mounted = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.label == "USB Stick")
            .unwrap();
        assert_eq!(mounted.id, original_id);
        assert_eq!(mounted.path, Some(mounted_path.clone()));
        assert_eq!(mounted.volume_id.as_deref(), Some(volume.id.as_str()));
        assert!(mounted.can_unmount);
        assert_eq!(
            tree.begin_volume_unmount(&volume.id),
            Some((volume.label.clone(), mounted_path.clone()))
        );
        assert!(tree.begin_volume_unmount(&volume.id).is_none());
        assert!(
            tree.rows(Path::new("/"))
                .into_iter()
                .find(|candidate| candidate.id == mounted.id)
                .unwrap()
                .loading
        );
        assert!(tree.finish_volume_unmount(&volume.id));
        assert!(
            !tree
                .rows(Path::new("/"))
                .into_iter()
                .find(|candidate| candidate.id == mounted.id)
                .unwrap()
                .loading
        );
        assert!(matches!(
            tree.activate(mounted.id),
            Some(Activation::Folder { path, .. }) if path == mounted_path
        ));
    }

    #[test]
    fn computer_is_a_single_shortcut_to_the_filesystem_root() {
        let mut tree = SidebarTree::new(Vec::new());
        let computer = tree.rows(Path::new("/elsewhere"))[0].clone();

        assert_eq!(computer.kind, NodeKind::Computer);
        assert!(!computer.loading);
        assert_eq!(tree.rows(Path::new("/elsewhere")).len(), 1);
        assert_eq!(
            tree.activate(computer.id),
            Some(Activation::Folder {
                path: PathBuf::from("/"),
                load: None,
            })
        );
        assert!(!tree.is_expanded(computer.id));
        assert!(tree.expand(computer.id).is_none());
        assert_eq!(tree.rows(Path::new("/elsewhere")).len(), 1);
    }

    #[test]
    fn storage_usage_refreshes_computer_and_only_mounted_drives() {
        let mounted = VolumeRoot {
            id: "uuid:mounted".to_owned(),
            path: Some(PathBuf::from("/media/mounted")),
            label: "Mounted".to_owned(),
            can_unmount: true,
        };
        let unmounted = VolumeRoot {
            id: "uuid:unmounted".to_owned(),
            path: None,
            label: "Unmounted".to_owned(),
            can_unmount: false,
        };
        let mut tree = SidebarTree::new(vec![mounted, unmounted]);

        let requests = tree.begin_storage_usage_refresh();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.path.as_path())
                .collect::<Vec<_>>(),
            [Path::new("/"), Path::new("/media/mounted")]
        );
        assert!(tree.begin_storage_usage_refresh().is_empty());

        let root = requests[0].clone();
        let usage = StorageUsage {
            used_bytes: 60,
            available_bytes: 40,
        };
        assert!(tree.complete_storage_usage(&root, Ok(usage)));
        assert_eq!(
            tree.rows(Path::new("/elsewhere"))[0].storage_usage,
            Some(usage)
        );
    }

    #[test]
    fn storage_rows_reserve_space_and_keep_section_gaps_in_pointer_geometry() {
        let mounted = VolumeRoot {
            id: "uuid:mounted".to_owned(),
            path: Some(PathBuf::from("/media/mounted")),
            label: "Mounted".to_owned(),
            can_unmount: true,
        };
        let unmounted = VolumeRoot {
            id: "uuid:unmounted".to_owned(),
            path: None,
            label: "Unmounted".to_owned(),
            can_unmount: false,
        };
        let mut tree = SidebarTree::new(vec![mounted.clone(), unmounted.clone()]);
        let places = vec![places::Entry {
            path: PathBuf::from("/home/example"),
            label: "Home".to_owned(),
            kind: NodeKind::Home,
            favorite_index: None,
        }];
        tree.install_places(places);

        assert_eq!(
            tree.row_heights(Path::new("/elsewhere")),
            [
                STORAGE_ROW_HEIGHT,
                STORAGE_ROW_HEIGHT,
                COMPACT_ROW_HEIGHT,
                SECTION_SEPARATOR_HEIGHT + COMPACT_ROW_HEIGHT,
            ]
        );
        let mut mounted = mounted;
        mounted.label = "Renamed disk".to_owned();
        assert!(tree.reconcile_volumes(vec![mounted, unmounted]));
        assert_eq!(
            tree.rows(Path::new("/elsewhere"))
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ["Computer", "Renamed disk", "Unmounted", "Home"],
        );
    }

    #[test]
    fn loading_state_is_available_without_building_sidebar_rows() {
        let volume = VolumeRoot {
            id: "uuid:data".to_owned(),
            path: Some(PathBuf::from("/data")),
            label: "Data".to_owned(),
            can_unmount: true,
        };
        let mut tree = SidebarTree::new(vec![volume]);
        let drive = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.kind == NodeKind::Drive)
            .unwrap();
        let Activation::Folder {
            load: Some(request),
            ..
        } = tree.activate(drive.id).unwrap()
        else {
            panic!("an unopened drive should request its children");
        };
        assert!(tree.has_loading());

        assert_eq!(
            tree.complete_load(&request, Ok(Vec::new())),
            LoadOutcome::Installed
        );

        assert!(!tree.has_loading());
    }

    #[test]
    fn expanded_paths_include_only_visible_filesystem_branches() {
        let mut root = FolderNode::folder(1, PathBuf::from("/root"));
        root.expanded = true;
        let mut expanded = FolderNode::folder(2, PathBuf::from("/root/expanded"));
        expanded.expanded = true;
        let nested = FolderNode::folder(3, PathBuf::from("/root/expanded/nested"));
        expanded.children.push(nested);
        let collapsed = FolderNode::folder(4, PathBuf::from("/root/collapsed"));
        root.children = vec![expanded, collapsed];

        let mut tree = SidebarTree::new(Vec::new());
        tree.roots = vec![root];
        tree.next_node_id = 5;
        tree.cursor = None;
        assert_eq!(
            tree.expanded_paths(),
            [PathBuf::from("/root"), PathBuf::from("/root/expanded")]
        );
    }

    #[test]
    fn cursor_movement_keeps_tree_navigation_rules_local() {
        let volume = VolumeRoot {
            id: "uuid:data".to_owned(),
            path: Some(PathBuf::from("/data")),
            label: "Data".to_owned(),
            can_unmount: true,
        };
        let mut tree = SidebarTree::new(vec![volume]);
        let drive = tree
            .rows(Path::new("/tmp"))
            .into_iter()
            .find(|row| row.kind == NodeKind::Drive)
            .unwrap();
        let Activation::Folder {
            load: Some(request),
            ..
        } = tree.activate(drive.id).unwrap()
        else {
            panic!("an unopened drive should request its children");
        };
        assert_eq!(
            tree.complete_load(&request, Ok(vec![PathBuf::from("/data/tmp")])),
            LoadOutcome::Installed
        );
        tree.focus(drive.id);

        assert_eq!(
            tree.move_cursor(Motion::Down, 1, Path::new("/data/tmp"), 0.0..500.0),
            MoveOutcome::Focused("tmp".to_owned())
        );
        assert_eq!(
            tree.focused_id(),
            Some(tree.rows(Path::new("/data/tmp"))[2].id)
        );
        assert_eq!(
            tree.move_cursor(Motion::Left, 1, Path::new("/data/tmp"), 0.0..500.0),
            MoveOutcome::Focused("Data".to_owned())
        );
    }

    #[test]
    fn favorite_drag_reorders_places_through_the_tree_interface() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let mut places = places::Places::empty_at(temp.path().join("favorites.json"));
        places.command(&first, "add First").unwrap();
        places.command(&second, "add Second").unwrap();
        let mut tree = SidebarTree::with_places(Vec::new(), places);
        tree.install_locations(Vec::new());

        tree.press_favorite(0);
        assert!(tree.release_favorite(1, Vec::new()).unwrap());

        let labels = tree
            .rows(Path::new("/elsewhere"))
            .into_iter()
            .filter(|row| row.kind == NodeKind::Favorite)
            .map(|row| row.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, ["Second", "First"]);
    }

    #[test]
    fn favorite_reordering_preserves_expanded_descendants_and_pending_loads() {
        for pending in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let first = temp.path().join("first");
            let nested = first.join("nested");
            let leaf = nested.join("leaf");
            let second = temp.path().join("second");
            std::fs::create_dir_all(&leaf).unwrap();
            std::fs::create_dir(&second).unwrap();
            let mut places = places::Places::empty_at(temp.path().join("favorites.json"));
            places.command(&first, "add First").unwrap();
            places.command(&second, "add Second").unwrap();
            let mut tree = SidebarTree::with_places(Vec::new(), places);
            tree.install_locations(Vec::new());
            let first_id = tree
                .rows(&first)
                .iter()
                .find(|row| row.kind == NodeKind::Favorite && row.path.as_ref() == Some(&first))
                .unwrap()
                .id;
            let request = tree.expand(first_id).unwrap();
            assert_eq!(
                tree.complete_load(&request, Ok(crate::fs::read_child_folders(&first).unwrap())),
                LoadOutcome::Installed
            );
            let nested_id = tree
                .rows(&first)
                .iter()
                .find(|row| row.path.as_ref() == Some(&nested))
                .unwrap()
                .id;
            let request = tree.expand(nested_id).unwrap();
            if !pending {
                assert_eq!(
                    tree.complete_load(
                        &request,
                        Ok(crate::fs::read_child_folders(&nested).unwrap())
                    ),
                    LoadOutcome::Installed
                );
            }
            let focused = if pending {
                nested_id
            } else {
                tree.rows(&first)
                    .iter()
                    .find(|row| row.path.as_ref() == Some(&leaf))
                    .unwrap()
                    .id
            };
            assert!(tree.focus(focused));
            tree.press_favorite(0);
            assert!(tree.release_favorite(1, Vec::new()).unwrap());
            assert_eq!(
                tree.focused_id(),
                Some(focused),
                "Favorite reorder discarded expanded focus"
            );
            assert!(tree.rows(&first).iter().any(|row| row.id == focused));
            let first_node = find_node(&tree.roots, first_id).unwrap();
            assert!(first_node.expanded);
            assert_eq!(first_node.favorite_index, Some(1));
            if pending {
                assert_eq!(
                    tree.complete_load(
                        &request,
                        Ok(crate::fs::read_child_folders(&nested).unwrap())
                    ),
                    LoadOutcome::Installed
                );
                assert!(
                    tree.rows(&first)
                        .iter()
                        .any(|row| row.path.as_ref() == Some(&leaf))
                );
            }
        }
    }

    #[test]
    fn locations_sharing_a_path_preserve_separate_state_when_reordered() {
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let mut tree = SidebarTree::with_places(
            Vec::new(),
            places::Places::empty_at(temp.path().join("favorites.json")),
        );
        let entry = |kind, label: &str| places::Entry {
            path: temp.path().to_path_buf(),
            label: label.to_owned(),
            kind,
            favorite_index: (kind == NodeKind::Favorite).then_some(0),
        };
        tree.install_places(vec![
            entry(NodeKind::Home, "Home"),
            entry(NodeKind::Favorite, "Favorite"),
        ]);
        let home_id = tree
            .roots
            .iter()
            .find(|node| node.kind == NodeKind::Home)
            .unwrap()
            .id;
        let favorite_id = tree
            .roots
            .iter()
            .find(|node| node.kind == NodeKind::Favorite)
            .unwrap()
            .id;
        let home_load = tree.expand(home_id).unwrap();
        assert_eq!(
            tree.complete_load(
                &home_load,
                Ok(crate::fs::read_child_folders(temp.path()).unwrap())
            ),
            LoadOutcome::Installed
        );
        let home_child = find_node(&tree.roots, home_id).unwrap().children[0].id;
        let favorite_load = tree.expand(favorite_id).unwrap();
        assert!(tree.focus(home_child));
        tree.install_places(vec![
            entry(NodeKind::Favorite, "Renamed"),
            entry(NodeKind::Home, "Home"),
        ]);
        assert_eq!(tree.focused_id(), Some(home_child));
        assert_eq!(
            find_node(&tree.roots, favorite_id).unwrap().label,
            "Renamed"
        );
        assert_eq!(
            tree.complete_load(
                &favorite_load,
                Ok(crate::fs::read_child_folders(temp.path()).unwrap())
            ),
            LoadOutcome::Installed
        );
        let favorite_child = find_node(&tree.roots, favorite_id).unwrap().children[0].id;
        assert_ne!(home_child, favorite_child);
        tree.install_places(vec![entry(NodeKind::Favorite, "Renamed")]);
        assert_eq!(tree.focused_id(), None);
        assert!(find_node(&tree.roots, home_id).is_none());
        assert_eq!(
            find_node(&tree.roots, favorite_id).unwrap().children[0].id,
            favorite_child
        );
    }

    #[test]
    fn failed_and_cancelled_loads_stop_loading_and_remain_retryable() {
        let volume = VolumeRoot {
            id: "uuid:data".to_owned(),
            path: Some(PathBuf::from("/data")),
            label: "Data".to_owned(),
            can_unmount: true,
        };
        let mut tree = SidebarTree::new(vec![volume]);
        let drive = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.kind == NodeKind::Drive)
            .unwrap();
        let Activation::Folder {
            load: Some(request),
            ..
        } = tree.activate(drive.id).unwrap()
        else {
            panic!("an unopened drive should request its children");
        };
        assert_eq!(
            tree.complete_load(
                &request,
                Ok(vec![
                    PathBuf::from("/data/slow"),
                    PathBuf::from("/data/cancelled"),
                ]),
            ),
            LoadOutcome::Installed
        );

        let slow = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.path.as_deref() == Some(Path::new("/data/slow")))
            .unwrap();
        let Activation::Folder {
            load: Some(slow_request),
            ..
        } = tree.activate(slow.id).unwrap()
        else {
            panic!("unloaded folder should request a load");
        };
        assert_eq!(
            tree.complete_load(&slow_request, Err("mount timed out".to_owned())),
            LoadOutcome::Failed("mount timed out".to_owned())
        );
        assert!(!tree.is_expanded(slow.id));
        assert!(matches!(
            tree.activate(slow.id),
            Some(Activation::Folder { load: Some(_), .. })
        ));

        let cancelled = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.path.as_deref() == Some(Path::new("/data/cancelled")))
            .unwrap();
        let Activation::Folder {
            load: Some(cancelled_request),
            ..
        } = tree.activate(cancelled.id).unwrap()
        else {
            panic!("unloaded folder should request a load");
        };
        assert!(tree.cancel_load(&cancelled_request));
        assert!(!tree.is_expanded(cancelled.id));
        assert_eq!(
            tree.complete_load(&cancelled_request, Ok(Vec::new())),
            LoadOutcome::Ignored
        );
    }
}
