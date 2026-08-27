use super::*;

impl App {
    pub(super) fn request_navigation(&mut self, navigation: NavigationRequest) -> Task<Message> {
        let requested = navigation
            .requested()
            .expect("folder navigation request")
            .to_path_buf();
        let options = self.view_preferences.for_directory(&requested);
        self.presentation
            .set_status(format!("Opening {}…", requested.display()));
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

    pub(super) fn transition_navigation(
        &mut self,
        transition: NavigationTransition,
    ) -> Task<Message> {
        self.mouse_back_gesture = None;
        if self.prompt_blocks_action() {
            return Task::none();
        }
        if !transition.preserves_pointer_interaction() {
            self.prepare_navigation_transition();
        }
        self.cancel_search_state();
        self.navigation
            .transition(transition)
            .map_or_else(Task::none, |request| self.request_navigation(request))
    }

    pub(super) fn prepare_navigation_transition(&mut self) {
        self.transfers.cancel_drag();
        self.grid.cancel_drag_hover();
    }

    pub(super) fn finish_navigation(
        &mut self,
        request: NavigationRequest,
        completion: NavigationCompletion,
    ) -> Task<Message> {
        let hidden_paths = self.transfers.pending_cut_paths().to_vec();
        match self
            .navigation
            .complete_with_hidden_paths(&request, completion, &hidden_paths)
        {
            NavigationOutcome::Committed(commit) => {
                let list_mode = self
                    .view_preferences
                    .for_directory(self.navigation.current())
                    .view
                    == fs::ViewMode::List;
                commit.apply_grid(&mut self.grid, self.navigation.entries().len(), list_mode);
                self.location_input = commit.location_input().to_owned();
                self.sync_location_monitoring();
                self.presentation.set_status(commit.status().to_owned());
                match commit.location() {
                    DisplayedLocation::Folder => Task::batch([
                        self.load_root_if_needed(),
                        self.schedule_details(),
                        self.load_visible_thumbnails(),
                    ]),
                    DisplayedLocation::Recent => self.load_visible_thumbnails(),
                    DisplayedLocation::Trash => Task::none(),
                }
            }
            NavigationOutcome::Redirect { request, notice } => {
                self.presentation.set_notice(notice);
                self.request_navigation(request)
            }
            NavigationOutcome::Failed(error) => {
                self.presentation.set_status(error);
                Task::none()
            }
            NavigationOutcome::Ignored => Task::none(),
        }
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
        if self.prompt_blocks_action()
            || self.foreground_operation_active()
            || self.navigation.loading()
        {
            return Task::none();
        }
        self.cancel_search_state();
        let request = self.navigation.recent();
        self.presentation
            .set_status("Reading shared Recent history…".to_owned());
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
        if self.prompt_blocks_action()
            || self.foreground_operation_active()
            || self.navigation.loading()
        {
            return Task::none();
        }
        self.cancel_search_state();
        let request = self.navigation.trash();
        self.presentation.set_status("Reading Trash…".to_owned());
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
        let mut entries = self.recent.sidebar_entry().into_iter().collect::<Vec<_>>();
        entries.push(self.trash.sidebar_entry());
        self.sidebar_tree.install_locations(entries);
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
        let activates_on_single_click = self
            .view_preferences
            .activates_on_single_click(entry.is_directory());
        if double && activates_on_single_click {
            return Task::none();
        }
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
            if !activates_on_single_click || modified {
                return self.schedule_details();
            }
        }
        if entry.is_directory() {
            self.transition_navigation(NavigationTransition::Open {
                requested: entry.path,
                remember: true,
                select: None,
            })
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
            self.transition_navigation(NavigationTransition::Open {
                requested: entry.path,
                remember: true,
                select: None,
            })
        } else {
            self.open_entry(entry)
        }
    }

    pub(super) fn finish_entry_press(&mut self, index: usize) -> Task<Message> {
        self.grid.cancel_drag_hover();
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let destination = self.drop_destination_at(self.grid.cursor(), false);
        match self.transfers.release(index, destination, action) {
            TransferDragRelease::None => Task::none(),
            TransferDragRelease::Click(index) => self.activate_entry(index, false),
            TransferDragRelease::Transfer(request) => self.start_transfer(request),
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
        self.sidebar_tree
            .begin_root_load()
            .map_or_else(Task::none, |request| {
                self.load_tree_node(request.id, request.path)
            })
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
        let current = self.navigation.current().to_path_buf();
        let Some(activation) = self.sidebar_tree.activate(id) else {
            return Task::none();
        };
        match activation {
            TreeActivation::Recent => self.open_recent(),
            TreeActivation::Trash => self.open_trash(),
            TreeActivation::Folder { path, load } => {
                let already_current = path == current;
                self.sync_location_monitoring();
                let load_task = load.map_or_else(Task::none, |request| {
                    self.load_tree_node(request.id, request.path)
                });
                if already_current {
                    load_task
                } else {
                    Task::batch([
                        load_task,
                        self.transition_navigation(NavigationTransition::Open {
                            requested: path,
                            remember: true,
                            select: None,
                        }),
                    ])
                }
            }
        }
    }

    pub(super) fn move_sidebar(&mut self, motion: Motion, count: usize) -> Task<Message> {
        match self
            .sidebar_tree
            .move_cursor(motion, count, self.navigation.current())
        {
            TreeMoveOutcome::None => Task::none(),
            TreeMoveOutcome::Focused(label) => {
                self.presentation.set_status(format!("Sidebar  •  {label}"));
                Task::none()
            }
            TreeMoveOutcome::Activate(id) => self.activate_tree_row(id),
        }
    }

    pub(super) fn begin_search(&mut self) -> Task<Message> {
        self.browser_input.enter(InputMode::Search);
        self.close_command_output();
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
                self.transition_navigation(NavigationTransition::Open {
                    requested: entry.path,
                    remember: true,
                    select: None,
                })
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
