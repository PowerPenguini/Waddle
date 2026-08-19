use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{AppWindow, fs};

use super::Explorer;
use crate::app::{
    executor::TaskExecutor,
    state::{ExplorerState, FolderNode},
    tree::{find_node, find_node_mut, mounted_roots, sync_tree},
};

impl Explorer {
    pub(super) fn tree_row_activated(&self, index: i32) {
        if !self.navigation_allowed() {
            return;
        }
        let index = match usize::try_from(index) {
            Ok(index) => index,
            Err(_) => return,
        };
        let (path, load_children, already_current) = {
            let mut state = self.state.lock().unwrap();
            let Some(id) = state.visible_tree_ids.get(index).copied() else {
                return;
            };
            let current = state.current.clone();
            let Some(node) = find_node_mut(&mut state.roots, id) else {
                return;
            };
            node.expanded = !node.expanded;
            let load = node.expanded && !node.loaded && !node.loading;
            if load {
                node.loading = true;
            }
            let result = (node.path.clone(), load, node.path == current);
            if let Some(ui) = self.ui.upgrade() {
                sync_tree(&ui, &mut state);
            }
            result
        };

        if already_current && !load_children {
            return;
        }
        self.navigate(path, true);
    }

    pub(super) fn cache_tree_navigation_success(
        state: &mut ExplorerState,
        path: &std::path::Path,
        entries: &[fs::FileEntry],
    ) -> bool {
        let Some(node_id) = find_loading_node_id(&state.roots, path) else {
            return false;
        };
        let child_paths: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is_directory())
            .map(|entry| entry.path.clone())
            .collect();
        let children: Vec<_> = child_paths
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

    pub(super) fn cache_tree_navigation_failure(
        state: &mut ExplorerState,
        path: &std::path::Path,
    ) -> bool {
        let Some(node_id) = find_loading_node_id(&state.roots, path) else {
            return false;
        };
        let Some(node) = find_node_mut(&mut state.roots, node_id) else {
            return false;
        };
        node.loading = false;
        node.expanded = false;
        true
    }

    pub(super) fn tree_path_at(&self, index: i32) -> Option<PathBuf> {
        let index = usize::try_from(index).ok()?;
        let state = self.state.lock().unwrap();
        let id = state.visible_tree_ids.get(index).copied()?;
        find_node(&state.roots, id).map(|node| node.path.clone())
    }

    pub(super) fn load_folder_children(
        tasks: TaskExecutor,
        state: Arc<Mutex<ExplorerState>>,
        ui: slint::Weak<AppWindow>,
        node_id: u64,
        path: PathBuf,
    ) {
        tasks.execute(move || {
            let folders = fs::read_child_folders(&path);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = ui.upgrade() else {
                    return;
                };
                let mut state = state.lock().unwrap();
                let Some(existing) = find_node(&state.roots, node_id) else {
                    return;
                };
                if existing.path != path {
                    return;
                }

                let children: Vec<_> = folders
                    .into_iter()
                    .map(|path| {
                        let id = state.allocate_node_id();
                        FolderNode::folder(id, path)
                    })
                    .collect();
                if let Some(node) = find_node_mut(&mut state.roots, node_id) {
                    node.children = children;
                    node.loading = false;
                    node.loaded = true;
                }
                sync_tree(&window, &mut state);
            });
        });
    }

    pub(super) fn refresh_mounts(&self) {
        let mounts = mounted_roots(&self.volume_monitor);
        let mut state = self.state.lock().unwrap();
        if state.reconcile_mounts(mounts)
            && let Some(ui) = self.ui.upgrade()
        {
            sync_tree(&ui, &mut state);
        }
    }
}

fn find_loading_node_id(nodes: &[FolderNode], path: &std::path::Path) -> Option<u64> {
    for node in nodes {
        if node.path == path && node.loading {
            return Some(node.id);
        }
        if let Some(id) = find_loading_node_id(&node.children, path) {
            return Some(id);
        }
    }
    None
}
