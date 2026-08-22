use std::path::PathBuf;

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
    Place,
    Favorite,
    Recent,
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
    pub(super) favorite_index: Option<usize>,
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
            favorite_index: None,
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
            favorite_index: None,
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
            favorite_index: None,
        }
    }

    pub(super) fn location(
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

#[derive(Debug)]
pub(super) struct ExplorerState {
    pub(super) roots: Vec<FolderNode>,
    next_node_id: u64,
}

impl ExplorerState {
    pub(super) fn new(mounts: Vec<MountRoot>) -> Self {
        let mut state = Self {
            roots: vec![FolderNode::root(1)],
            next_node_id: 2,
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

    pub(super) fn reconcile_mounts(&mut self, mounts: Vec<MountRoot>) -> bool {
        let old_signature: Vec<_> = self
            .roots
            .iter()
            .filter(|root| root.kind == NodeKind::Drive)
            .map(|root| (root.path.clone(), root.label.clone()))
            .collect();
        let new_signature: Vec<_> = mounts
            .iter()
            .map(|mount| (mount.path.clone(), mount.label.clone()))
            .collect();
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
        true
    }

    pub(super) fn install_places(&mut self, places: Vec<super::places::Entry>) {
        self.roots.retain(|node| {
            !matches!(
                node.kind,
                NodeKind::Home | NodeKind::Place | NodeKind::Favorite | NodeKind::Recent
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
    }
}
