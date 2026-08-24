use std::{collections::HashMap, path::PathBuf};

use iced::Subscription;

use super::directory_watch;

pub(super) trait Adapter {
    fn watch_many(&self, paths: Vec<PathBuf>) -> bool;
}

impl Adapter for directory_watch::Source {
    fn watch_many(&self, paths: Vec<PathBuf>) -> bool {
        self.watch_many(paths)
    }
}

pub(super) struct Locations {
    pub(super) current: PathBuf,
    pub(super) current_is_displayed: bool,
    pub(super) pending_cut_paths: Vec<PathBuf>,
    pub(super) expanded: Vec<PathBuf>,
    pub(super) displayed_sources: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Change {
    pub(super) removed: Vec<PathBuf>,
    pub(super) cut_parent_changed: bool,
    pub(super) refresh_current: bool,
    pub(super) invalidate_tree: Option<PathBuf>,
    pub(super) refresh_displayed: bool,
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

    pub(super) fn sync(&mut self, locations: Locations) {
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
        roles.retain(|path, _| path.is_dir());
        let mut paths = roles.keys().cloned().collect::<Vec<_>>();
        paths.sort();
        self.fallback = self.adapter.watch_many(paths);
        self.roles = roles;
    }

    pub(super) fn handle(&mut self, event: directory_watch::Event) -> Change {
        if event.watch_failed {
            self.fallback = true;
            return Change::default();
        }
        let roles = self.roles.get(&event.path).copied().unwrap_or_default();
        Change {
            removed: event.removed,
            cut_parent_changed: roles.cut_parent,
            refresh_current: roles.current,
            invalidate_tree: roles.expanded.then_some(event.path),
            refresh_displayed: roles.displayed_source,
        }
    }

    pub(super) fn poll(&self, recursive_search: bool) -> Poll {
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
            refresh_location: !recursive_search,
            invalidate_tree,
        }
    }

    #[cfg(test)]
    fn watched_paths(&self) -> std::collections::HashSet<PathBuf> {
        self.roles.keys().cloned().collect()
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
        let adapter = MemoryAdapter::default();
        let mut monitoring = LocationMonitoring::new(adapter.clone());
        monitoring.sync(Locations {
            current: current.clone(),
            current_is_displayed: true,
            pending_cut_paths: vec![cut_parent.join("moved.txt")],
            expanded: vec![expanded.clone(), current.clone()],
            displayed_sources: vec![displayed_source.clone()],
        });

        assert_eq!(monitoring.watched_paths().len(), 4);
        assert_eq!(adapter.watched.lock().unwrap().len(), 4);
        assert_eq!(
            monitoring.handle(directory_watch::Event {
                path: current.clone(),
                removed: vec![current.join("gone.txt")],
                watch_failed: false,
            }),
            Change {
                removed: vec![current.join("gone.txt")],
                refresh_current: true,
                invalidate_tree: Some(current),
                ..Change::default()
            }
        );
        assert!(
            monitoring
                .handle(directory_watch::Event {
                    path: cut_parent,
                    removed: Vec::new(),
                    watch_failed: false,
                })
                .cut_parent_changed
        );
        assert!(
            monitoring
                .handle(directory_watch::Event {
                    path: displayed_source,
                    removed: Vec::new(),
                    watch_failed: false,
                })
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
        monitoring.sync(Locations {
            current,
            current_is_displayed: true,
            pending_cut_paths: Vec::new(),
            expanded: vec![expanded.clone()],
            displayed_sources: Vec::new(),
        });
        assert_eq!(monitoring.poll(false), Poll::default());

        monitoring.handle(directory_watch::Event {
            path: PathBuf::new(),
            removed: Vec::new(),
            watch_failed: true,
        });
        assert_eq!(
            monitoring.poll(false),
            Poll {
                refresh_location: true,
                invalidate_tree: vec![expanded.clone()],
            }
        );
        assert_eq!(
            monitoring.poll(true),
            Poll {
                refresh_location: false,
                invalidate_tree: vec![expanded],
            }
        );
    }
}
