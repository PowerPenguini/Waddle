use std::path::{Path, PathBuf};

use gio::prelude::{FileExt, MountExt, VolumeMonitorExt};
use slint::{DataTransfer, ModelRc, VecModel};

use crate::{AppWindow, FolderIcon, FolderRow as UiFolderRow};

use super::state::{DraggedEntry, ExplorerState, FolderNode, MountRoot, NodeKind};

pub(super) fn mounted_roots(monitor: &gio::VolumeMonitor) -> Vec<MountRoot> {
    monitor
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

pub(super) fn dragged_path(data: &DataTransfer) -> Option<PathBuf> {
    let user_data = data.user_data()?;
    user_data
        .as_ref()
        .downcast_ref::<DraggedEntry>()
        .map(|entry| entry.path.clone())
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

pub(super) fn sync_tree(ui: &AppWindow, state: &mut ExplorerState) {
    let mut rows = Vec::new();
    let mut ids = Vec::new();
    flatten_nodes(&state.roots, 0, &state.current, &mut rows, &mut ids);

    state.visible_tree_ids = ids;
    ui.set_folder_rows(ModelRc::new(VecModel::from(rows)));
}

fn flatten_nodes(
    nodes: &[FolderNode],
    depth: i32,
    current: &Path,
    rows: &mut Vec<UiFolderRow>,
    ids: &mut Vec<u64>,
) {
    for node in nodes {
        ids.push(node.id);
        rows.push(UiFolderRow {
            label: node.label.clone().into(),
            depth,
            expanded: node.expanded,
            expandable: !node.loaded || !node.children.is_empty(),
            loading: node.loading,
            selected: node.path == current,
            icon: match node.kind {
                NodeKind::Computer => FolderIcon::Computer,
                NodeKind::Drive => FolderIcon::Drive,
                NodeKind::Folder => FolderIcon::Folder,
            },
        });
        if node.expanded {
            flatten_nodes(&node.children, depth + 1, current, rows, ids);
        }
    }
}
