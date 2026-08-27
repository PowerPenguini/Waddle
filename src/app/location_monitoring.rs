use std::{collections::HashMap, path::PathBuf};

use iced::Subscription;

use super::{
    directory_watch,
    navigation::{DisplayedLocation, NavigationSession},
    recent::Recent,
    search::SearchSession,
    transfer_session::TransferSession,
    trash::Trash,
    tree::SidebarTree,
};

pub(super) trait Adapter {
    fn watch_many(&self, paths: Vec<PathBuf>) -> bool;
}

impl Adapter for directory_watch::Source {
    fn watch_many(&self, paths: Vec<PathBuf>) -> bool {
        self.watch_many(paths)
    }
}

struct Locations {
    current: PathBuf,
    current_is_displayed: bool,
    pending_cut_paths: Vec<PathBuf>,
    expanded: Vec<PathBuf>,
    displayed_sources: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Change {
    pub(super) refresh_current: bool,
    pub(super) invalidate_tree: Option<PathBuf>,
    pub(super) refresh_displayed: bool,
    pub(super) resync: bool,
    pub(super) notice: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Poll {
    pub(super) refresh_location: bool,
    pub(super) invalidate_tree: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Roles {
    current: bool,
    cut_parent: bool,
    expanded: bool,
    displayed_source: bool,
}

pub(super) struct LocationMonitoring<A> {
    adapter: A,
    roles: HashMap<PathBuf, Roles>,
    fallback: bool,
}

pub(super) type Native = LocationMonitoring<directory_watch::Source>;

impl Native {
    pub(super) fn open() -> Result<Self, String> {
        directory_watch::Source::new().map(Self::new)
    }

    pub(super) fn subscription(&self) -> Subscription<directory_watch::Event> {
        self.adapter.subscription()
    }
}

impl<A: Adapter> LocationMonitoring<A> {
    fn new(adapter: A) -> Self {
        Self {
            adapter,
            roles: HashMap::new(),
            fallback: false,
        }
    }

    pub(super) fn sync(
        &mut self,
        navigation: &NavigationSession,
        transfers: &TransferSession,
        sidebar_tree: &SidebarTree,
        recent: &Recent,
        trash: &Trash,
    ) {
        let displayed_sources = displayed_watch_paths(navigation, sidebar_tree, recent, trash);
        self.sync_locations(Locations {
            current: navigation.current().to_path_buf(),
            current_is_displayed: navigation.folder_displayed(),
            pending_cut_paths: transfers.pending_cut_paths().to_vec(),
            expanded: sidebar_tree.expanded_paths(),
            displayed_sources,
        });
    }

    fn sync_locations(&mut self, locations: Locations) {
        let mut roles = HashMap::<PathBuf, Roles>::new();
        roles.entry(locations.current).or_default().current = locations.current_is_displayed;
        for path in locations
            .pending_cut_paths
            .iter()
            .filter_map(|path| path.parent().map(PathBuf::from))
        {
            roles.entry(path).or_default().cut_parent = true;
        }
        for path in locations.expanded {
            roles.entry(path).or_default().expanded = true;
        }
        for path in locations.displayed_sources {
            roles.entry(path).or_default().displayed_source = true;
        }
        let mut paths = roles.keys().cloned().collect::<Vec<_>>();
        paths.sort();
        self.fallback = self.adapter.watch_many(paths);
        self.roles = roles;
    }

    pub(super) fn handle(
        &mut self,
        event: directory_watch::Event,
        transfers: &mut TransferSession,
    ) -> Change {
        if event.watch_failed {
            self.fallback = true;
            return Change::default();
        }
        let roles = self.roles.get(&event.path).copied().unwrap_or_default();
        let notice = roles
            .cut_parent
            .then(|| transfers.reconcile_pending_cut(&event.removed))
            .flatten();
        Change {
            refresh_current: roles.current,
            invalidate_tree: roles.expanded.then_some(event.path),
            refresh_displayed: roles.displayed_source,
            resync: notice.is_some(),
            notice,
        }
    }

    pub(super) fn poll(&self, search: &SearchSession) -> Poll {
        if !self.fallback {
            return Poll::default();
        }
        let mut invalidate_tree = self
            .roles
            .iter()
            .filter_map(|(path, roles)| roles.expanded.then_some(path.clone()))
            .collect::<Vec<_>>();
        invalidate_tree.sort();
        Poll {
            refresh_location: !search.is_recursive(),
            invalidate_tree,
        }
    }

    #[cfg(test)]
    fn watched_paths(&self) -> std::collections::HashSet<PathBuf> {
        self.roles.keys().cloned().collect()
    }
}

impl super::App {
    pub(super) fn sync_location_monitoring(&mut self) {
        let Some(monitoring) = self.location_monitoring.as_mut() else {
            return;
        };
        monitoring.sync(
            &self.navigation,
            &self.transfers,
            &self.sidebar_tree,
            &self.recent,
            &self.trash,
        );
    }

    #[cfg(test)]
    pub(super) fn displayed_watch_paths(&self) -> Vec<PathBuf> {
        displayed_watch_paths(
            &self.navigation,
            &self.sidebar_tree,
            &self.recent,
            &self.trash,
        )
    }
}

fn displayed_watch_paths(
    navigation: &NavigationSession,
    sidebar_tree: &SidebarTree,
    recent: &Recent,
    trash: &Trash,
) -> Vec<PathBuf> {
    match navigation.displayed_location() {
        DisplayedLocation::Recent => recent.watch_paths(navigation.entries()),
        DisplayedLocation::Trash => {
            let mounts = sidebar_tree.mount_paths();
            trash.watch_paths(navigation.trash_entries(), &mounts)
        }
        DisplayedLocation::Folder => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct MemoryAdapter {
        watched: Arc<Mutex<Vec<PathBuf>>>,
        overflow: bool,
    }

    impl Adapter for MemoryAdapter {
        fn watch_many(&self, paths: Vec<PathBuf>) -> bool {
            *self.watched.lock().unwrap() = paths;
            self.overflow
        }
    }

    #[test]
    fn location_monitoring_classifies_roles_through_one_interface() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("current");
        let cut_parent = temp.path().join("cut");
        let expanded = temp.path().join("expanded");
        let displayed_source = temp.path().join("recent-source");
        for path in [&current, &cut_parent, &expanded, &displayed_source] {
            std::fs::create_dir(path).unwrap();
        }
        let cut_path = cut_parent.join("moved.txt");
        std::fs::write(&cut_path, "x").unwrap();
        let mut transfers = TransferSession::open(temp.path().join("transfers.json"));
        transfers
            .cut(&[crate::fs::FileEntry {
                path: cut_path.clone(),
                name: "moved.txt".into(),
                directory: false,
                metadata: Default::default(),
            }])
            .unwrap();
        let adapter = MemoryAdapter::default();
        let mut monitoring = LocationMonitoring::new(adapter.clone());
        monitoring.sync_locations(Locations {
            current: current.clone(),
            current_is_displayed: true,
            pending_cut_paths: vec![cut_path.clone()],
            expanded: vec![expanded.clone(), current.clone()],
            displayed_sources: vec![displayed_source.clone()],
        });

        assert_eq!(monitoring.watched_paths().len(), 4);
        assert_eq!(adapter.watched.lock().unwrap().len(), 4);
        assert_eq!(
            monitoring.handle(
                directory_watch::Event {
                    path: current.clone(),
                    removed: vec![current.join("gone.txt")],
                    watch_failed: false,
                },
                &mut transfers,
            ),
            Change {
                refresh_current: true,
                invalidate_tree: Some(current),
                ..Change::default()
            }
        );
        std::fs::remove_file(&cut_path).unwrap();
        let cut_change = monitoring.handle(
            directory_watch::Event {
                path: cut_parent,
                removed: vec![cut_path],
                watch_failed: false,
            },
            &mut transfers,
        );
        assert!(cut_change.resync);
        assert!(cut_change.notice.unwrap().contains("Cut completed"));
        assert!(
            monitoring
                .handle(
                    directory_watch::Event {
                        path: displayed_source,
                        removed: Vec::new(),
                        watch_failed: false,
                    },
                    &mut transfers,
                )
                .refresh_displayed
        );
    }

    #[test]
    fn notification_failure_enables_one_polling_fallback_policy() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("current");
        let expanded = temp.path().join("expanded");
        std::fs::create_dir(&current).unwrap();
        std::fs::create_dir(&expanded).unwrap();
        let mut monitoring = LocationMonitoring::new(MemoryAdapter::default());
        monitoring.sync_locations(Locations {
            current: current.clone(),
            current_is_displayed: true,
            pending_cut_paths: Vec::new(),
            expanded: vec![expanded.clone()],
            displayed_sources: Vec::new(),
        });
        let mut transfers = TransferSession::open(temp.path().join("transfers.json"));
        let mut search = SearchSession::default();
        assert_eq!(monitoring.poll(&search), Poll::default());

        monitoring.handle(
            directory_watch::Event {
                path: PathBuf::new(),
                removed: Vec::new(),
                watch_failed: true,
            },
            &mut transfers,
        );
        assert_eq!(
            monitoring.poll(&search),
            Poll {
                refresh_location: true,
                invalidate_tree: vec![expanded.clone()],
            }
        );
        let mut navigation = NavigationSession::new(current);
        let mut grid = crate::app::grid::GridInteraction::default();
        search.begin(&grid);
        let _ = search.update(&mut navigation, &mut grid, "/needle".to_owned());
        assert_eq!(
            monitoring.poll(&search),
            Poll {
                refresh_location: false,
                invalidate_tree: vec![expanded],
            }
        );
    }

    #[test]
    fn location_monitoring_does_not_probe_candidate_paths() {
        let temp = tempfile::tempdir().unwrap();
        let unavailable = temp.path().join("unavailable-automount");
        let adapter = MemoryAdapter::default();
        let mut monitoring = LocationMonitoring::new(adapter.clone());

        monitoring.sync_locations(Locations {
            current: unavailable.clone(),
            current_is_displayed: true,
            pending_cut_paths: Vec::new(),
            expanded: vec![unavailable.clone()],
            displayed_sources: Vec::new(),
        });

        assert_eq!(
            adapter.watched.lock().unwrap().as_slice(),
            [unavailable.as_path()]
        );
        assert!(monitoring.watched_paths().contains(&unavailable));
    }
}
