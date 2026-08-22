use std::path::{Path, PathBuf};

use gio::prelude::{FileExt, MountExt, VolumeMonitorExt};

use super::state::{ExplorerState, FolderNode, MountRoot, NodeKind};

#[derive(Clone, Debug)]
pub(super) struct TreeRow {
    pub(super) id: u64,
    pub(super) path: PathBuf,
    pub(super) label: String,
    pub(super) depth: usize,
    pub(super) loading: bool,
    pub(super) selected: bool,
    pub(super) kind: NodeKind,
}

pub(super) fn mounted_roots() -> Vec<MountRoot> {
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

pub(super) fn find_node(nodes: &[FolderNode], id: u64) -> Option<&FolderNode> {
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

pub(super) fn find_node_mut(nodes: &mut [FolderNode], id: u64) -> Option<&mut FolderNode> {
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

pub(super) fn flatten_rows(state: &ExplorerState, current: &Path) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    flatten_nodes(&state.roots, 0, current, &mut rows);
    rows
}

fn flatten_nodes(nodes: &[FolderNode], depth: usize, current: &Path, rows: &mut Vec<TreeRow>) {
    for node in nodes {
        rows.push(TreeRow {
            id: node.id,
            path: node.path.clone(),
            label: node.label.clone(),
            depth,
            loading: node.loading,
            selected: node.path == current,
            kind: node.kind,
        });
        if node.expanded {
            flatten_nodes(&node.children, depth + 1, current, rows);
        }
    }
}

pub(super) fn install_children(
    state: &mut ExplorerState,
    node_id: u64,
    path: &Path,
    folders: Vec<PathBuf>,
) -> bool {
    if find_node(&state.roots, node_id).is_none_or(|node| node.path != path) {
        return false;
    }
    let children = folders
        .into_iter()
        .map(|path| {
            let id = state.allocate_node_id();
            FolderNode::folder(id, path)
        })
        .collect();
    let Some(node) = find_node_mut(&mut state.roots, node_id) else {
        return false;
    };
    node.children = children;
    node.loading = false;
    node.loaded = true;
    true
}

pub(super) fn invalidate_tree_folders(
    nodes: &mut [FolderNode],
    changed_folders: &[PathBuf],
) -> Vec<(u64, PathBuf)> {
    let mut reloads = Vec::new();
    invalidate_tree_folders_inner(nodes, changed_folders, &mut reloads);
    reloads
}

fn invalidate_tree_folders_inner(
    nodes: &mut [FolderNode],
    changed_folders: &[PathBuf],
    reloads: &mut Vec<(u64, PathBuf)>,
) {
    for node in nodes {
        if changed_folders.iter().any(|path| path == &node.path) {
            node.children.clear();
            node.loaded = false;
            node.loading = node.expanded;
            if node.expanded {
                reloads.push((node.id, node.path.clone()));
            }
            continue;
        }
        invalidate_tree_folders_inner(&mut node.children, changed_folders, reloads);
    }
}
