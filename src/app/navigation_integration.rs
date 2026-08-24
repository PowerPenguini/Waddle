use super::*;

impl App {
    pub(super) fn request_navigation(&mut self, navigation: NavigationRequest) -> Task<Message> {
        let requested = navigation
            .requested()
            .expect("folder navigation request")
            .to_path_buf();
        let options = self.view_preferences.for_directory(&requested);
        self.status = format!("Opening {}…", requested.display());
        Task::perform(
            self.operations.run(OperationKind::Navigation, {
                let path = requested.clone();
                move |_| {
                    fs::open_directory_with(&path, options)
                        .map(|opened| (opened.canonical_path, opened.entries))
                        .map_err(|error| error.to_string())
                }
            }),
            move |completion| match completion {
                Completion::Finished(result) => Message::NavigationFinished {
                    request: navigation,
                    result,
                },
                Completion::Cancelled => Message::NavigationCancelled(navigation),
            },
        )
    }

    pub(super) fn navigate(
        &mut self,
        requested: PathBuf,
        remember: bool,
        select: Option<PathBuf>,
    ) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let navigation = self.navigation.forward(requested, remember, select);
        self.request_navigation(navigation)
    }

    pub(super) fn finish_navigation(
        &mut self,
        request: NavigationRequest,
        completion: NavigationCompletion,
    ) -> Task<Message> {
        match self.navigation.complete(&request, completion) {
            NavigationOutcome::Committed {
                selected,
                refresh,
                location,
            } => {
                self.grid.set_list_mode(
                    self.view_preferences
                        .for_directory(self.navigation.current())
                        .view
                        == fs::ViewMode::List,
                );
                let selected = if location == DisplayedLocation::Folder {
                    let selected_paths = selected
                        .iter()
                        .filter_map(|index| self.navigation.entries().get(*index))
                        .map(|entry| entry.path.clone())
                        .collect::<Vec<_>>();
                    self.navigation
                        .hide_paths(self.transfers.pending_cut_paths());
                    self.navigation
                        .entries()
                        .iter()
                        .enumerate()
                        .filter_map(|(index, entry)| {
                            selected_paths.contains(&entry.path).then_some(index)
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                self.grid
                    .select_indices(&selected, self.navigation.entries().len());
                self.grid.clear_details();
                self.location_input = self.navigation.location_label();
                if !refresh {
                    self.grid.reset_scroll();
                }
                self.sync_location_monitoring();
                self.status = match location {
                    DisplayedLocation::Folder => String::new(),
                    DisplayedLocation::Recent => {
                        format!("{} items  •  Recent", self.navigation.entries().len())
                    }
                    DisplayedLocation::Trash => {
                        format!("{} items  •  Trash", self.navigation.entries().len())
                    }
                };
                match location {
                    DisplayedLocation::Folder => Task::batch([
                        self.load_root_if_needed(),
                        self.schedule_details(),
                        self.load_visible_thumbnails(),
                    ]),
                    DisplayedLocation::Recent => self.load_visible_thumbnails(),
                    DisplayedLocation::Trash => Task::none(),
                }
            }
            NavigationOutcome::Failed { error: _, refresh }
                if refresh && !self.navigation.current().is_dir() =>
            {
                let missing = self.navigation.current().to_path_buf();
                let ancestor = nearest_existing_ancestor(&missing);
                self.status_notice = Some(format!(
                    "{} disappeared; opened {}",
                    missing.display(),
                    ancestor.display()
                ));
                let navigation = self.navigation.forward(ancestor, false, None);
                self.request_navigation(navigation)
            }
            NavigationOutcome::Failed { error, .. } => {
                self.status = error;
                Task::none()
            }
            NavigationOutcome::Ignored => Task::none(),
        }
    }

    pub(super) fn go_parent(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let Some(navigation) = self.navigation.parent() else {
            return Task::none();
        };
        self.request_navigation(navigation)
    }

    pub(super) fn go_back(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let Some(navigation) = self.navigation.back() else {
            return Task::none();
        };
        self.request_navigation(navigation)
    }

    pub(super) fn go_forward(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let Some(navigation) = self.navigation.history_forward() else {
            return Task::none();
        };
        self.request_navigation(navigation)
    }

    pub(super) fn refresh(&mut self, select: Option<PathBuf>) -> Task<Message> {
        let navigation = self.navigation.refresh(select);
        self.request_navigation(navigation)
    }

    pub(super) fn refresh_selected(&mut self, select: Vec<PathBuf>) -> Task<Message> {
        let navigation = self.navigation.refresh_selected(select);
        self.request_navigation(navigation)
    }

    pub(super) fn live_refresh(&mut self) -> Task<Message> {
        if self.navigation.loading() {
            return Task::none();
        }
        if !self.navigation.current().is_dir() {
            let navigation = self.navigation.refresh(None);
            return self.request_navigation(navigation);
        }
        if self.search.is_recursive() {
            return self.update_search(self.search.query().to_owned());
        }
        let selected = self
            .grid
            .selected_items(self.navigation.entries())
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        self.refresh_selected(selected)
    }

    pub(super) fn refresh_location(&mut self) -> Task<Message> {
        match self.navigation.displayed_location() {
            DisplayedLocation::Recent => self.open_recent(),
            DisplayedLocation::Trash => self.open_trash(),
            DisplayedLocation::Folder => self.live_refresh(),
        }
    }

    pub(super) fn open_recent(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() || self.busy || self.navigation.loading() {
            return Task::none();
        }
        self.cancel_search_state();
        let request = self.navigation.recent();
        self.status = "Reading shared Recent history…".to_owned();
        let recent = self.recent.clone();
        Task::perform(
            self.operations
                .run(OperationKind::Navigation, move |_| recent.entries()),
            move |completion| match completion {
                Completion::Finished(result) => Message::RecentLoaded {
                    request,
                    result: Some(result),
                },
                Completion::Cancelled => Message::RecentLoaded {
                    request,
                    result: None,
                },
            },
        )
    }

    pub(super) fn open_trash(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() || self.busy || self.navigation.loading() {
            return Task::none();
        }
        self.cancel_search_state();
        let request = self.navigation.trash();
        self.status = "Reading Trash…".to_owned();
        let trash = self.trash.clone();
        Task::perform(
            self.operations
                .run(OperationKind::Navigation, move |_| trash.entries()),
            move |completion| match completion {
                Completion::Finished(result) => Message::TrashLoaded {
                    request,
                    result: Some(result),
                },
                Completion::Cancelled => Message::TrashLoaded {
                    request,
                    result: None,
                },
            },
        )
    }

    pub(super) fn install_locations(&mut self) {
        let mut entries = self.places.entries();
        entries.extend(self.recent.sidebar_entry());
        entries.push(self.trash.sidebar_entry());
        self.explorer.install_places(entries);
    }

    pub(super) fn change_view_options(
        &mut self,
        change: impl FnOnce(&mut fs::BrowseOptions),
    ) -> Task<Message> {
        let current = self.navigation.current().to_path_buf();
        self.view_preferences.update_directory(&current, change);
        if !self.navigation.folder_displayed() {
            let options = self.view_preferences.for_directory(&current);
            self.grid.set_list_mode(options.view == fs::ViewMode::List);
            return self.load_visible_thumbnails();
        }
        self.live_refresh()
    }

    pub(super) fn load_visible_thumbnails(&mut self) -> Task<Message> {
        if self
            .view_preferences
            .for_directory(self.navigation.current())
            .view
            != fs::ViewMode::Grid
        {
            return Task::none();
        }
        let visible = self
            .grid
            .visible_range(self.navigation.entries().len(), self.status_height());
        let paths = self.navigation.entries()[visible.first_index..visible.last_index]
            .iter()
            .filter(|entry| !entry.is_directory())
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        Task::batch(
            self.thumbnails
                .requests(paths.iter().map(PathBuf::as_path))
                .into_iter()
                .map(|request| Task::perform(thumbnail::load(request), Message::ThumbnailLoaded)),
        )
    }

    pub(super) fn begin_location(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        if self.browser_input.mode() == InputMode::Rename {
            self.cancel_rename();
        }
        self.location_input = self.navigation.current().display().to_string();
        self.browser_input.enter(InputMode::Location);
        widget::operation::focus(Id::new(LOCATION_ID))
    }

    pub(super) fn activate_entry(&mut self, index: usize, double: bool) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        if double && self.browser_input.mode() == InputMode::Rename {
            self.cancel_rename();
        }
        if !double {
            let modified = self.modifiers.control() || self.modifiers.shift();
            self.grid.select_click(
                index,
                self.modifiers.control(),
                self.modifiers.shift(),
                self.navigation.entries().len(),
            );
            if !self.view_preferences.single_click_activation() || modified {
                return self.schedule_details();
            }
        }
        if entry.is_directory() {
            self.navigate(entry.path, true, None)
        } else {
            self.open_entry(entry)
        }
    }

    pub(super) fn activate_selected(&mut self) -> Task<Message> {
        self.grid
            .selected_entry()
            .map_or_else(Task::none, |index| self.open_or_navigate(index))
    }

    pub(super) fn open_or_navigate(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        if entry.is_directory() {
            self.navigate(entry.path, true, None)
        } else {
            self.open_entry(entry)
        }
    }

    pub(super) fn finish_entry_press(&mut self, index: usize) -> Task<Message> {
        self.drag_hover.cancel();
        match self.transfers.release(index) {
            TransferRelease::None => Task::none(),
            TransferRelease::Click(index) => self.activate_entry(index, false),
            TransferRelease::Drop(grabbed_index) => self.finish_drag(grabbed_index),
        }
    }

    pub(super) fn open_entry(&self, entry: FileEntry) -> Task<Message> {
        Task::perform(
            self.operations.run(OperationKind::Background, move |_| {
                let uri = gio::File::for_path(&entry.path).uri();
                gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
                    .map_err(|error| error.to_string())
            }),
            |completion| match completion {
                Completion::Finished(Ok(())) | Completion::Cancelled => Message::Noop,
                Completion::Finished(Err(error)) => Message::OperationError(error),
            },
        )
    }

    pub(super) fn move_selection(&mut self, motion: Motion, count: usize) -> Task<Message> {
        self.grid.move_selection_count(
            motion,
            count,
            self.navigation.entries().len(),
            self.status_height(),
        );
        Task::batch([self.schedule_details(), self.scroll_to_selected()])
    }

    pub(super) fn scroll_to_selected(&self) -> Task<Message> {
        let Some(index) = self.grid.selected_entry() else {
            return Task::none();
        };
        let y = self.grid.scroll_target(index);
        widget::operation::scroll_to(
            Id::new(GRID_SCROLL_ID),
            scrollable::AbsoluteOffset { x: 0.0, y },
        )
    }

    pub(super) fn schedule_details(&mut self) -> Task<Message> {
        self.operations.cancel(OperationKind::Details);
        self.grid.clear_details();
        let Some(entry) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index).cloned())
        else {
            self.refresh_status();
            return Task::none();
        };
        self.refresh_status();
        let path = entry.path.clone();
        Task::perform(
            self.operations.run_after(
                OperationKind::Details,
                Duration::from_millis(50),
                move |_| fs::read_entry_details(&path).map_err(|error| error.to_string()),
            ),
            move |completion| match completion {
                Completion::Finished(result) => Message::DetailsFinished {
                    path: entry.path,
                    result,
                },
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn load_root_if_needed(&mut self) -> Task<Message> {
        let root = {
            let Some(root) = self.explorer.roots.first_mut() else {
                return Task::none();
            };
            if root.loaded || root.loading && !root.children.is_empty() {
                return Task::none();
            }
            root.loading = true;
            (root.id, root.path.clone())
        };
        self.load_tree_node(root.0, root.1)
    }

    pub(super) fn load_tree_node(&self, id: u64, path: PathBuf) -> Task<Message> {
        let worker_path = path.clone();
        let show_hidden = self.view_preferences.for_directory(&path).show_hidden;
        Task::perform(
            self.operations.run(OperationKind::Background, move |_| {
                Ok(fs::read_child_folders_with_hidden(
                    &worker_path,
                    show_hidden,
                ))
            }),
            move |completion| match completion {
                Completion::Finished(result) => {
                    Message::TreeLoaded(id, path, result.unwrap_or_default())
                }
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn activate_tree_row(&mut self, id: u64) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        if find_node(&self.explorer.roots, id)
            .is_some_and(|node| node.kind == state::NodeKind::Recent)
        {
            return self.open_recent();
        }
        if find_node(&self.explorer.roots, id)
            .is_some_and(|node| node.kind == state::NodeKind::Trash)
        {
            return self.open_trash();
        }
        let current = self.navigation.current().to_path_buf();
        let Some(node) = find_node_mut(&mut self.explorer.roots, id) else {
            return Task::none();
        };
        node.expanded = !node.expanded;
        let load = node.expanded && !node.loaded && !node.loading;
        if load {
            node.loading = true;
        }
        let path = node.path.clone();
        let already_current = path == current;
        self.sync_location_monitoring();
        let load_task = if load {
            self.load_tree_node(id, path.clone())
        } else {
            Task::none()
        };
        if already_current {
            load_task
        } else {
            Task::batch([load_task, self.navigate(path, true, None)])
        }
    }

    pub(super) fn move_sidebar(&mut self, motion: Motion, count: usize) -> Task<Message> {
        let rows = flatten_rows(&self.explorer, self.navigation.current());
        let Some(current) = self
            .sidebar_cursor
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .or_else(|| rows.iter().position(|row| row.selected))
            .or((!rows.is_empty()).then_some(0))
        else {
            return Task::none();
        };
        let last = rows.len() - 1;
        let count = count.max(1);
        let target = match motion {
            Motion::Down => current.saturating_add(count).min(last),
            Motion::Up => current.saturating_sub(count),
            Motion::First => 0,
            Motion::Last => last,
            Motion::DisplayIndex(index) => index.min(last),
            Motion::ViewportTop | Motion::HalfPageUp => current.saturating_sub(count),
            Motion::ViewportMiddle => current,
            Motion::ViewportBottom | Motion::HalfPageDown => {
                current.saturating_add(count).min(last)
            }
            Motion::Left => {
                let row = &rows[current];
                if find_node_mut(&mut self.explorer.roots, row.id).is_some_and(|node| node.expanded)
                {
                    if let Some(node) = find_node_mut(&mut self.explorer.roots, row.id) {
                        node.expanded = false;
                    }
                    current
                } else {
                    rows[..current]
                        .iter()
                        .rposition(|candidate| candidate.depth < row.depth)
                        .or_else(|| {
                            (row.depth == 0 && row.kind != state::NodeKind::Computer)
                                .then(|| {
                                    rows[..current].iter().rposition(|candidate| {
                                        candidate.kind == state::NodeKind::Computer
                                    })
                                })
                                .flatten()
                        })
                        .unwrap_or(current)
                }
            }
            Motion::Right => {
                let id = rows[current].id;
                let collapsed =
                    find_node_mut(&mut self.explorer.roots, id).is_some_and(|node| !node.expanded);
                if collapsed {
                    return self.activate_tree_row(id);
                }
                current.saturating_add(1).min(last)
            }
            Motion::RowStart => 0,
            Motion::RowEnd => last,
        };
        self.sidebar_cursor = Some(rows[target].id);
        self.status = format!("Sidebar  •  {}", rows[target].label);
        Task::none()
    }

    pub(super) fn begin_search(&mut self) -> Task<Message> {
        self.browser_input.enter(InputMode::Search);
        self.command.close_output();
        self.sync_bottom_bar();
        self.operations.cancel(OperationKind::Search);
        self.search.begin(&self.grid);
        widget::operation::focus(Id::new(SEARCH_ID))
    }

    pub(super) fn update_search(&mut self, value: String) -> Task<Message> {
        match self
            .search
            .update(&mut self.navigation, &mut self.grid, value)
        {
            SearchUpdate::None => Task::none(),
            SearchUpdate::SelectionChanged => self.schedule_details(),
            SearchUpdate::CancelPending => {
                self.operations.cancel(OperationKind::Search);
                Task::none()
            }
            SearchUpdate::Search { root, query } => self.schedule_recursive_search(root, query),
        }
    }

    pub(super) fn schedule_recursive_search(&self, root: PathBuf, query: String) -> Task<Message> {
        let show_hidden = self.view_preferences.for_directory(&root).show_hidden;
        Task::perform(
            self.operations.run_after(
                OperationKind::Search,
                Duration::from_millis(160),
                move |cancellation| {
                    fs::search_directory_with_hidden(
                        &root,
                        &query,
                        SEARCH_LIMIT,
                        show_hidden,
                        || cancellation.is_cancelled(),
                    )
                    .map_err(|error| error.to_string())
                },
            ),
            |completion| match completion {
                Completion::Finished(result) => Message::SearchFinished(result),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    pub(super) fn submit_search(&mut self) -> Task<Message> {
        self.browser_input.leave_mode();
        self.operations.cancel(OperationKind::Search);
        if let Some(entry) = self.search.submit(&mut self.navigation, &mut self.grid) {
            return if entry.is_directory() {
                self.navigate(entry.path, true, None)
            } else {
                self.open_entry(entry)
            };
        }
        Task::none()
    }

    pub(super) fn cancel_search(&mut self) -> Task<Message> {
        self.operations.cancel(OperationKind::Search);
        self.search.cancel(&mut self.navigation, &mut self.grid);
        self.schedule_details()
    }

    pub(super) fn cancel_search_state(&mut self) {
        if self.browser_input.mode() == InputMode::Rename {
            self.cancel_rename();
        }
        self.operations.cancel(OperationKind::Search);
        self.search.cancel(&mut self.navigation, &mut self.grid);
        self.browser_input.leave_mode();
    }

    pub(super) fn repeat_search(&mut self, reverse: bool) -> Task<Message> {
        if self
            .search
            .repeat(&mut self.navigation, &mut self.grid, reverse)
        {
            self.schedule_details()
        } else {
            Task::none()
        }
    }
}
