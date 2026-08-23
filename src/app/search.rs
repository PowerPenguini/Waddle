use std::path::PathBuf;

use crate::fs::{FileEntry, SearchResults};

use super::{grid::GridInteraction, navigation::NavigationSession};

#[derive(Clone, Debug)]
struct Recursive {
    directory_entries: Vec<FileEntry>,
    loading: bool,
    truncated: bool,
}

#[derive(Clone, Debug)]
struct Active {
    origin: Option<usize>,
    recursive: Option<Recursive>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Update {
    None,
    SelectionChanged,
    CancelPending,
    Search { root: PathBuf, query: String },
}

#[derive(Clone, Debug, Default)]
pub(super) struct SearchSession {
    active: Option<Active>,
    query: String,
    last_query: String,
}

impl SearchSession {
    pub(super) fn begin(&mut self, grid: &GridInteraction) {
        self.active = Some(Active {
            origin: grid.selected_entry(),
            recursive: None,
        });
        self.query.clear();
    }

    pub(super) fn update(
        &mut self,
        navigation: &mut NavigationSession,
        grid: &mut GridInteraction,
        mut value: String,
    ) -> Update {
        let Some(active) = self.active.as_mut() else {
            return Update::None;
        };
        if active.recursive.is_none() && value.starts_with('/') {
            value.remove(0);
            active.recursive = Some(Recursive {
                directory_entries: navigation.entries().to_vec(),
                loading: false,
                truncated: false,
            });
        }
        self.query = value;

        if let Some(recursive) = active.recursive.as_mut() {
            recursive.loading = !self.query.is_empty();
            recursive.truncated = false;
            if self.query.is_empty() {
                navigation.replace_displayed_entries(Vec::new());
                grid.select_only(None, 0);
                return Update::CancelPending;
            }
            return Update::Search {
                root: navigation.current().to_path_buf(),
                query: self.query.clone(),
            };
        }

        let previous = grid.selected_entry();
        grid.select_only(
            find_match(navigation.entries(), &self.query, active.origin, false),
            navigation.entries().len(),
        );
        if previous == grid.selected_entry() {
            Update::None
        } else {
            Update::SelectionChanged
        }
    }

    pub(super) fn complete(
        &mut self,
        navigation: &mut NavigationSession,
        grid: &mut GridInteraction,
        result: Result<SearchResults, String>,
    ) -> Result<(), String> {
        let Some(recursive) = self
            .active
            .as_mut()
            .and_then(|active| active.recursive.as_mut())
        else {
            return Ok(());
        };
        recursive.loading = false;
        match result {
            Ok(results) => {
                recursive.truncated = results.truncated;
                navigation.replace_displayed_entries(results.entries);
                grid.select_only(
                    (!navigation.entries().is_empty()).then_some(0),
                    navigation.entries().len(),
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn submit(
        &mut self,
        navigation: &mut NavigationSession,
        grid: &mut GridInteraction,
    ) -> Option<FileEntry> {
        let active = self.active.take()?;
        self.last_query = std::mem::take(&mut self.query);
        let selected = grid
            .selected_entry()
            .and_then(|index| navigation.entries().get(index).cloned());
        if let Some(recursive) = active.recursive {
            navigation.replace_displayed_entries(recursive.directory_entries);
            grid.select_only(None, navigation.entries().len());
            selected
        } else {
            None
        }
    }

    pub(super) fn cancel(
        &mut self,
        navigation: &mut NavigationSession,
        grid: &mut GridInteraction,
    ) {
        let Some(active) = self.active.take() else {
            self.query.clear();
            return;
        };
        if let Some(recursive) = active.recursive {
            navigation.replace_displayed_entries(recursive.directory_entries);
        }
        grid.select_only(active.origin, navigation.entries().len());
        self.query.clear();
    }

    pub(super) fn repeat(
        &self,
        navigation: &mut NavigationSession,
        grid: &mut GridInteraction,
        reverse: bool,
    ) -> bool {
        if self.last_query.is_empty() {
            return false;
        }
        let Some(index) = find_match(
            navigation.entries(),
            &self.last_query,
            grid.selected_entry(),
            reverse,
        ) else {
            return false;
        };
        grid.select_only(Some(index), navigation.entries().len());
        true
    }

    #[cfg(test)]
    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn is_recursive(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.recursive.is_some())
    }

    pub(super) fn is_loading(&self) -> bool {
        self.active
            .as_ref()
            .and_then(|active| active.recursive.as_ref())
            .is_some_and(|recursive| recursive.loading)
    }

    pub(super) fn is_truncated(&self) -> bool {
        self.active
            .as_ref()
            .and_then(|active| active.recursive.as_ref())
            .is_some_and(|recursive| recursive.truncated)
    }

    pub(super) fn query(&self) -> &str {
        &self.query
    }
}

fn find_match(
    entries: &[FileEntry],
    query: &str,
    anchor: Option<usize>,
    reverse: bool,
) -> Option<usize> {
    if entries.is_empty() || query.is_empty() {
        return None;
    }
    let query = query.to_lowercase();
    let len = entries.len();
    let first = match (anchor, reverse) {
        (Some(index), false) => (index + 1) % len,
        (Some(index), true) => index.checked_sub(1).unwrap_or(len - 1),
        (None, false) => 0,
        (None, true) => len - 1,
    };
    (0..len)
        .map(|offset| {
            if reverse {
                (first + len - offset) % len
            } else {
                (first + offset) % len
            }
        })
        .find(|index| {
            entries[*index]
                .name
                .to_string_lossy()
                .to_lowercase()
                .contains(&query)
        })
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use super::*;

    fn navigation() -> (NavigationSession, GridInteraction) {
        let mut navigation = NavigationSession::new(PathBuf::from("/start"));
        navigation.replace_displayed_entries(
            ["one", "two", "three"]
                .into_iter()
                .map(|name| FileEntry {
                    path: PathBuf::from("/start").join(name),
                    name: OsString::from(name),
                    directory: false,
                    metadata: Default::default(),
                })
                .collect(),
        );
        let mut grid = GridInteraction::default();
        grid.select_only(Some(0), navigation.entries().len());
        (navigation, grid)
    }

    #[test]
    fn local_search_updates_selection_without_replacing_entries() {
        let (mut navigation, mut grid) = navigation();
        let mut search = SearchSession::default();
        search.begin(&grid);

        assert_eq!(
            search.update(&mut navigation, &mut grid, "tw".to_owned()),
            Update::SelectionChanged
        );
        assert_eq!(grid.selected_entry(), Some(1));
        assert_eq!(navigation.entries().len(), 3);
    }

    #[test]
    fn recursive_cancel_restores_entries_and_origin() {
        let (mut navigation, mut grid) = navigation();
        let mut search = SearchSession::default();
        search.begin(&grid);
        assert!(matches!(
            search.update(&mut navigation, &mut grid, "/needle".to_owned()),
            Update::Search { .. }
        ));
        search
            .complete(
                &mut navigation,
                &mut grid,
                Ok(SearchResults {
                    entries: vec![FileEntry {
                        path: PathBuf::from("/start/nested/needle"),
                        name: OsString::from("needle"),
                        directory: false,
                        metadata: Default::default(),
                    }],
                    truncated: false,
                }),
            )
            .unwrap();
        assert_eq!(navigation.entries().len(), 1);

        search.cancel(&mut navigation, &mut grid);
        assert_eq!(navigation.entries().len(), 3);
        assert_eq!(grid.selected_entry(), Some(0));
        assert!(!search.is_active());
    }

    #[test]
    fn recursive_submit_returns_match_then_restores_directory() {
        let (mut navigation, mut grid) = navigation();
        let mut search = SearchSession::default();
        search.begin(&grid);
        let _ = search.update(&mut navigation, &mut grid, "/needle".to_owned());
        let match_entry = FileEntry {
            path: PathBuf::from("/start/nested/needle"),
            name: OsString::from("needle"),
            directory: false,
            metadata: Default::default(),
        };
        search
            .complete(
                &mut navigation,
                &mut grid,
                Ok(SearchResults {
                    entries: vec![match_entry.clone()],
                    truncated: false,
                }),
            )
            .unwrap();

        assert_eq!(
            search
                .submit(&mut navigation, &mut grid)
                .map(|entry| entry.path),
            Some(match_entry.path)
        );
        assert_eq!(navigation.entries().len(), 3);
        assert_eq!(grid.selected_entry(), None);
    }
}
