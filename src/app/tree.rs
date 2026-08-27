use std::path::{Path, PathBuf};

use gio::prelude::{FileExt, MountExt, VolumeMonitorExt};

use super::{grid::Motion, places};

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

#[derive(Clone, Debug)]
struct FolderNode {
    id: u64,
    path: PathBuf,
    label: String,
    kind: NodeKind,
    expanded: bool,
    loading: bool,
    loaded: bool,
    children: Vec<FolderNode>,
    favorite_index: Option<usize>,
}

impl FolderNode {
    fn root(id: u64) -> Self {
        Self {
            id,
            path: PathBuf::from("/"),
            label: "Computer".to_owned(),
            kind: NodeKind::Computer,
            expanded: true,
            loading: true,
            loaded: false,
            children: Vec::new(),
            favorite_index: None,
        }
    }

    fn drive(id: u64, mount: MountRoot) -> Self {
        Self {
            id,
            path: mount.path,
            label: mount.label,
            kind: NodeKind::Drive,
            expanded: false,
            loading: false,
            loaded: false,
            children: Vec::new(),
            favorite_index: None,
        }
    }

    fn folder(id: u64, path: PathBuf) -> Self {
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
            favorite_index: None,
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
            path,
            label,
            kind,
            expanded: false,
            loading: false,
            loaded: false,
            children: Vec::new(),
            favorite_index,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TreeRow {
    pub(super) id: u64,
    pub(super) path: PathBuf,
    pub(super) label: String,
    pub(super) depth: usize,
    pub(super) loading: bool,
    pub(super) selected: bool,
    pub(super) focused: bool,
    pub(super) kind: NodeKind,
    pub(super) favorite_index: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadRequest {
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
    pub(super) fn new(mounts: Vec<MountRoot>) -> Self {
        Self::with_places(mounts, places::Places::open_default())
    }

    fn with_places(mounts: Vec<MountRoot>, places: places::Places) -> Self {
        let mut tree = Self {
            roots: vec![FolderNode::root(1)],
            next_node_id: 2,
            cursor: None,
            places,
            favorite_drag: None,
        };
        for mount in mounts {
            let id = tree.allocate_node_id();
            tree.roots.push(FolderNode::drive(id, mount));
        }
        tree
    }

    pub(super) fn open_default(additional_locations: Vec<places::Entry>) -> Self {
        let mut tree = Self::new(mounted_roots());
        tree.install_locations(additional_locations);
        tree
    }

    fn allocate_node_id(&mut self) -> u64 {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    pub(super) fn reconcile_mounts(&mut self, mounts: Vec<MountRoot>) -> bool {
        let old_signature = self
            .roots
            .iter()
            .filter(|root| root.kind == NodeKind::Drive)
            .map(|root| (root.path.clone(), root.label.clone()))
            .collect::<Vec<_>>();
        let new_signature = mounts
            .iter()
            .map(|mount| (mount.path.clone(), mount.label.clone()))
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
        self.roots.retain(|root| root.kind != NodeKind::Drive);
        self.roots.extend(reconciled);
        self.retain_valid_cursor();
        true
    }

    pub(super) fn install_locations(&mut self, additional_locations: Vec<places::Entry>) {
        let mut locations = self.places.entries();
        locations.extend(additional_locations);
        self.replace_locations(locations);
    }

    fn replace_locations(&mut self, places: Vec<places::Entry>) {
        self.roots.retain(|node| {
            !matches!(
                node.kind,
                NodeKind::Home
                    | NodeKind::Desktop
                    | NodeKind::Documents
                    | NodeKind::Downloads
                    | NodeKind::Music
                    | NodeKind::Pictures
                    | NodeKind::Videos
                    | NodeKind::Favorite
                    | NodeKind::Recent
                    | NodeKind::Trash
            )
        });
        for (index, place) in (1..).zip(places) {
            let id = self.allocate_node_id();
            self.roots.insert(
                index,
                FolderNode::location(
                    id,
                    place.path,
                    place.label,
                    place.kind,
                    place.favorite_index,
                ),
            );
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

    pub(super) fn refresh_mounts(&mut self) -> bool {
        self.reconcile_mounts(mounted_roots())
    }

    pub(super) fn rows(&self, current: &Path) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        flatten_nodes(&self.roots, self.cursor, 0, current, &mut rows);
        rows
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
        let target = match motion {
            Motion::Down => current.saturating_add(count).min(last),
            Motion::Up => current.saturating_sub(count),
            Motion::First | Motion::RowStart => 0,
            Motion::Last | Motion::RowEnd => last,
            Motion::DisplayIndex(index) => index.min(last),
            Motion::ViewportTop | Motion::HalfPageUp => current.saturating_sub(count),
            Motion::ViewportMiddle => current,
            Motion::ViewportBottom | Motion::HalfPageDown => {
                current.saturating_add(count).min(last)
            }
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
            _ => {
                node.expanded = !node.expanded;
                let should_load = node.expanded && !node.loaded && !node.loading;
                if should_load {
                    node.loading = true;
                }
                let path = node.path.clone();
                Some(Activation::Folder {
                    path: path.clone(),
                    load: should_load.then_some(LoadRequest { id, path }),
                })
            }
        }
    }

    pub(super) fn expand(&mut self, id: u64) -> Option<LoadRequest> {
        let node = find_node_mut(&mut self.roots, id)?;
        if node.expanded {
            return None;
        }
        node.expanded = true;
        let should_load = !node.loaded && !node.loading;
        if should_load {
            node.loading = true;
        }
        should_load.then(|| LoadRequest {
            id,
            path: node.path.clone(),
        })
    }

    pub(super) fn begin_root_load(&mut self) -> Option<LoadRequest> {
        let root = self.roots.first_mut()?;
        if root.loaded || root.loading && !root.children.is_empty() {
            return None;
        }
        root.loading = true;
        Some(LoadRequest {
            id: root.id,
            path: root.path.clone(),
        })
    }

    pub(super) fn install_children(
        &mut self,
        node_id: u64,
        path: &Path,
        folders: Vec<PathBuf>,
    ) -> bool {
        if find_node(&self.roots, node_id).is_none_or(|node| node.path != path) {
            return false;
        }
        let children = folders
            .into_iter()
            .map(|path| {
                let id = self.allocate_node_id();
                FolderNode::folder(id, path)
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
        if find_node(&self.roots, request.id)
            .is_none_or(|node| node.path != request.path || !node.loading)
        {
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
        if node.path != request.path || !node.loading {
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
            .map(|node| node.path.clone())
            .collect()
    }

    pub(super) fn row_path(&self, index: usize, current: &Path) -> Option<PathBuf> {
        self.rows(current).get(index).map(|row| row.path.clone())
    }

    pub(super) fn row_count(&self, current: &Path) -> usize {
        self.rows(current).len()
    }

    pub(super) fn row_target(&self, index: usize, current: &Path) -> Option<(u64, PathBuf)> {
        self.rows(current)
            .get(index)
            .map(|row| (row.id, row.path.clone()))
    }

    fn retain_valid_cursor(&mut self) {
        if self
            .cursor
            .is_some_and(|id| find_node(&self.roots, id).is_none())
        {
            self.cursor = None;
        }
    }

    #[cfg(test)]
    pub(super) fn is_expanded(&self, id: u64) -> bool {
        find_node(&self.roots, id).is_some_and(|node| node.expanded)
    }
}

fn mounted_roots() -> Vec<MountRoot> {
    gio::VolumeMonitor::get()
        .mounts()
        .into_iter()
        .filter(|mount| !mount.is_shadowed())
        .filter_map(|mount| {
            let path = mount.root().path()?;
            (path != Path::new("/")).then(|| MountRoot {
                path,
                label: mount.name().to_string(),
            })
        })
        .collect()
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
            selected: node.path == current,
            focused: cursor == Some(node.id),
            kind: node.kind,
            favorite_index: node.favorite_index,
        });
        if node.expanded {
            flatten_nodes(&node.children, cursor, depth + 1, current, rows);
        }
    }
}

fn collect_expanded_paths(nodes: &[FolderNode], paths: &mut Vec<PathBuf>) {
    for node in nodes {
        if node.expanded && !matches!(node.kind, NodeKind::Recent | NodeKind::Trash) {
            paths.push(node.path.clone());
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
        if changed_folders.iter().any(|path| path == &node.path) {
            node.children.clear();
            node.loaded = false;
            node.loading = node.expanded;
            if node.expanded {
                reloads.push(LoadRequest {
                    id: node.id,
                    path: node.path.clone(),
                });
            }
            continue;
        }
        invalidate_folders(&mut node.children, changed_folders, reloads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_paths_include_only_visible_filesystem_branches() {
        let mut root = FolderNode::root(1);
        let mut expanded = FolderNode::folder(2, PathBuf::from("/expanded"));
        expanded.expanded = true;
        let nested = FolderNode::folder(3, PathBuf::from("/expanded/nested"));
        expanded.children.push(nested);
        let collapsed = FolderNode::folder(4, PathBuf::from("/collapsed"));
        root.children = vec![expanded, collapsed];

        let mut tree = SidebarTree::new(Vec::new());
        tree.roots = vec![root];
        tree.next_node_id = 5;
        tree.cursor = None;
        assert_eq!(
            tree.expanded_paths(),
            [PathBuf::from("/"), PathBuf::from("/expanded")]
        );
    }

    #[test]
    fn cursor_movement_keeps_tree_navigation_rules_local() {
        let mut tree = SidebarTree::new(Vec::new());
        let root = tree.rows(Path::new("/tmp"))[0].id;
        assert!(tree.install_children(root, Path::new("/"), vec![PathBuf::from("/tmp")]));
        tree.focus(root);

        assert_eq!(
            tree.move_cursor(Motion::Down, 1, Path::new("/tmp")),
            MoveOutcome::Focused("tmp".to_owned())
        );
        assert_eq!(tree.focused_id(), Some(tree.rows(Path::new("/tmp"))[1].id));
        assert_eq!(
            tree.move_cursor(Motion::Left, 1, Path::new("/tmp")),
            MoveOutcome::Focused("Computer".to_owned())
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
    fn failed_and_cancelled_loads_stop_loading_and_remain_retryable() {
        let mut tree = SidebarTree::new(Vec::new());
        let root = tree.rows(Path::new("/"))[0].id;
        assert!(tree.install_children(
            root,
            Path::new("/"),
            vec![PathBuf::from("/slow"), PathBuf::from("/cancelled")],
        ));

        let slow = tree
            .rows(Path::new("/"))
            .into_iter()
            .find(|row| row.path == Path::new("/slow"))
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
            .find(|row| row.path == Path::new("/cancelled"))
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
