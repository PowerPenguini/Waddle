use super::*;

impl App {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Event(event, status) => {
                if clears_status_notice(&event) {
                    self.status_notice = None;
                }
                self.handle_event(event, status)
            }
            Message::FindWindow => window::latest().map(Message::WindowAvailable),
            Message::WindowAvailable(Some(id)) => {
                window::run(id, native_clipboard::Attached::attach).map(Message::NativeReady)
            }
            Message::WindowAvailable(None) => find_window_after_delay(),
            Message::WindowResized(size) => {
                self.grid.resize(size);
                self.startup.remember_size(size);
                self.load_visible_thumbnails()
            }
            Message::NativeReady(result) => {
                match result {
                    Ok(attached) => {
                        self.native_dnd = Some(attached.dnd);
                        self.native_clipboard = Some(attached.clipboard);
                        self.native_dnd_error = None;
                    }
                    Err(error) => {
                        eprintln!("PolarExp: external drag-and-drop unavailable: {error}");
                        self.native_dnd_error = Some(error);
                    }
                }
                Task::none()
            }
            Message::NativeDndEvent(event) => self.handle_native_dnd_event(event),
            Message::X11DropReady(generation) => self.finish_x11_drop(generation),
            Message::ExternalDragFinished(result) => {
                let consequences = self.transfers.finish_outgoing(result);
                self.apply_transfer_consequences(consequences)
            }
            Message::TransferBatchFinished { id, outcome } => {
                self.finish_transfer_batch(id, *outcome)
            }
            Message::PollTransfer => Task::none(),
            Message::CancelTransfer => {
                self.browser_focus = BrowserFocus::BottomBar;
                self.bottom_cursor = 0;
                self.cancel_transfer_conflict()
            }
            Message::RetryTransfer => {
                self.browser_focus = BrowserFocus::BottomBar;
                match self.transfers.retry() {
                    Ok(Some(work)) => self.launch_transfer(work),
                    Ok(None) => Task::none(),
                    Err(error) => {
                        self.show_error(error);
                        Task::none()
                    }
                }
            }
            Message::ToggleTransferHistory => {
                self.browser_focus = BrowserFocus::BottomBar;
                self.transfers.toggle_expanded();
                self.sync_bottom_bar();
                Task::none()
            }
            Message::CopyTransferReport => {
                self.browser_focus = BrowserFocus::BottomBar;
                iced::clipboard::write(self.transfers.report_text())
            }
            Message::CopyCommandReport => {
                self.browser_focus = BrowserFocus::BottomBar;
                self.command.output().map_or_else(Task::none, |output| {
                    iced::clipboard::write(format!("{}\n\n{}", output.summary, output.detail))
                })
            }
            Message::SystemTheme(mode) => {
                self.system_mode = mode;
                Task::none()
            }
            Message::PollSystem => {
                let settings = theme::interface_settings();
                self.accent = theme::load(settings.as_ref());
                self.system_accessibility = theme::accessibility(settings.as_ref());
                let mounts = mounted_roots();
                self.explorer.reconcile_mounts(mounts);
                let search = if self.search.is_recursive() {
                    self.live_refresh()
                } else {
                    Task::none()
                };
                let fallback = self
                    .location_monitoring
                    .as_ref()
                    .map(|monitoring| monitoring.poll(self.search.is_recursive()))
                    .unwrap_or_default();
                let location = if fallback.refresh_location {
                    self.refresh_location()
                } else {
                    Task::none()
                };
                let fallback =
                    Task::batch([self.invalidate_tree(fallback.invalidate_tree), location]);
                Task::batch([search, fallback])
            }
            Message::DirectoryChanged(event) => {
                let change = self
                    .location_monitoring
                    .as_mut()
                    .map(|monitoring| monitoring.handle(event))
                    .unwrap_or_default();
                if change.cut_parent_changed
                    && let Some((generation, removed)) =
                        self.transfers.reconcile_pending_cut(&change.removed)
                {
                    self.sync_native_cut_clipboard(generation);
                    self.sync_location_monitoring();
                    let remaining = self.transfers.pending_cut_paths().len();
                    self.status_notice = Some(if remaining == 0 {
                        format!(
                            "External move or removal confirmed for {removed} item(s); Cut completed"
                        )
                    } else {
                        format!(
                            "External move or removal confirmed for {removed} item(s); {remaining} still pending"
                        )
                    });
                }
                Task::batch([
                    if change.refresh_current {
                        self.live_refresh()
                    } else {
                        Task::none()
                    },
                    change
                        .invalidate_tree
                        .map_or_else(Task::none, |path| self.invalidate_tree(vec![path])),
                    if change.refresh_displayed {
                        self.refresh_location()
                    } else {
                        Task::none()
                    },
                ])
            }
            Message::Refresh => self.refresh_location(),
            Message::ToggleView => self.change_view_options(|options| {
                options.view = match options.view {
                    fs::ViewMode::Grid => fs::ViewMode::List,
                    fs::ViewMode::List => fs::ViewMode::Grid,
                };
            }),
            Message::SortBy(sort) => self.change_view_options(|options| {
                if options.sort == sort {
                    options.descending = !options.descending;
                } else {
                    options.sort = sort;
                    options.descending = false;
                }
            }),
            Message::Parent => self.go_parent(),
            Message::Back => self.go_back(),
            Message::Forward => self.go_forward(),
            Message::LocationChanged(value) => {
                self.location_input = value;
                Task::none()
            }
            Message::LocationSubmitted => {
                self.browser_input.leave_mode();
                let input = PathBuf::from(&self.location_input);
                let requested = if input.is_absolute() {
                    input
                } else {
                    self.navigation.current().join(input)
                };
                self.navigate(requested, true, None)
            }
            Message::TreeRow(id) => {
                self.browser_focus = BrowserFocus::Sidebar;
                self.sidebar_cursor = Some(id);
                self.activate_tree_row(id)
            }
            Message::SidebarScrolled(y) => {
                let now = Instant::now();
                self.animation_now = now;
                self.sidebar_scrollbar.show(now);
                self.grid.set_sidebar_scroll(y);
                self.update_drag_hover(self.grid.cursor());
                Task::none()
            }
            Message::TreeLoaded(id, path, folders) => {
                tree::install_children(&mut self.explorer, id, &path, folders);
                Task::none()
            }
            Message::FavoritePressed(index) => {
                self.favorite_drag = Some(index);
                Task::none()
            }
            Message::FavoriteReleased(index) => {
                let Some(from) = self.favorite_drag.take() else {
                    return Task::none();
                };
                if let Err(error) = self.places.reorder(from, index) {
                    self.status = error;
                } else if from != index {
                    self.install_locations();
                    self.status = "Favorite order saved".to_owned();
                }
                Task::none()
            }
            Message::EntryPressed(index) => {
                if self.prompt_blocks_action() {
                    return Task::none();
                }
                self.browser_focus = BrowserFocus::Entries;
                self.transfers
                    .press(index, self.grid.cursor(), self.navigation.entries().len());
                Task::none()
            }
            Message::EntryReleased(index) => self.finish_entry_press(index),
            Message::EntryHovered(index) => {
                self.grid.enter(index);
                Task::none()
            }
            Message::EntryUnhovered(index) => {
                self.grid.leave(index);
                Task::none()
            }
            Message::EntryDoubleClicked(index) => {
                if self.view_preferences.single_click_activation() {
                    Task::none()
                } else {
                    self.activate_entry(index, true)
                }
            }
            Message::EntryContext(index) => {
                if self.prompt_blocks_action() {
                    return Task::none();
                }
                self.browser_focus = BrowserFocus::Entries;
                self.grid
                    .select_only(Some(index), self.navigation.entries().len());
                self.context_menu = Some((index, self.grid.cursor()));
                self.context_menu_cursor = 0;
                self.schedule_details()
            }
            Message::ContextNewFolder => {
                self.context_menu = None;
                self.show_new_folder()
            }
            Message::ContextNewFile(template, suggested_name, label) => {
                self.context_menu = None;
                self.show_new_file(template, suggested_name, label)
            }
            Message::ContextProperties | Message::ContextOpenWith => {
                self.context_menu = None;
                self.show_properties()
            }
            Message::ContextRename => {
                let index = self.context_menu.take().map(|(index, _)| index);
                if let Some(index) = index {
                    self.show_rename(index)
                } else {
                    Task::none()
                }
            }
            Message::ContextTrash => {
                self.context_menu = None;
                self.show_trash_prompt()
            }
            Message::ContextRestore => {
                self.context_menu = None;
                self.restore_selected_trash()
            }
            Message::ContextDeletePermanent => {
                self.context_menu = None;
                self.show_trash_delete_prompt(false)
            }
            Message::ContextEmptyTrash => {
                self.context_menu = None;
                self.show_trash_delete_prompt(true)
            }
            Message::CloseContext => {
                self.context_menu = None;
                Task::none()
            }
            Message::GridScrolled(y) => {
                let now = Instant::now();
                self.animation_now = now;
                self.entry_scrollbar.show(now);
                self.grid.set_scroll(y);
                self.update_drag_hover(self.grid.cursor());
                self.load_visible_thumbnails()
            }
            Message::GridPointerMoved(point) => {
                if self
                    .grid
                    .move_pointer_in_grid(point, self.navigation.entries().len())
                {
                    self.refresh_status();
                }
                Task::none()
            }
            Message::NavigationFinished { request, result } => {
                self.finish_navigation(request, NavigationCompletion::Folder(result))
            }
            Message::NavigationCancelled(request) => {
                self.finish_navigation(request, NavigationCompletion::Cancelled)
            }
            Message::DetailsFinished { path, result } => {
                if self
                    .grid
                    .selected_entry()
                    .and_then(|index| self.navigation.entries().get(index))
                    .is_some_and(|entry| entry.path == path)
                {
                    self.grid.set_details(result.ok());
                    if !self.transfers.is_native_active() {
                        self.refresh_status();
                    }
                }
                Task::none()
            }
            Message::ThumbnailLoaded(loaded) => {
                self.thumbnails.complete(loaded);
                Task::none()
            }
            Message::SearchChanged(value) => self.update_search(value),
            Message::SearchSubmitted => self.submit_search(),
            Message::SearchFinished(result) => {
                if let Err(error) =
                    self.search
                        .complete(&mut self.navigation, &mut self.grid, result)
                {
                    self.status = error;
                }
                self.load_visible_thumbnails()
            }
            Message::CommandChanged(value) => {
                self.command.change(value);
                Task::none()
            }
            Message::CommandSubmitted => self.submit_command(),
            Message::CommandFinished(result) => self.finish_command(result),
            Message::VolumeFinished(result) => {
                self.busy = false;
                match result {
                    Ok(status) => {
                        self.status = status;
                        self.explorer.reconcile_mounts(mounted_roots());
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::RecentLoaded { request, result } => self.finish_navigation(
                request,
                result.map_or(
                    NavigationCompletion::Cancelled,
                    NavigationCompletion::Recent,
                ),
            ),
            Message::TrashLoaded { request, result } => self.finish_navigation(
                request,
                result.map_or(NavigationCompletion::Cancelled, NavigationCompletion::Trash),
            ),
            Message::PropertiesFinished(result) => {
                self.busy = false;
                match result {
                    Ok(info) => {
                        self.command
                            .show_output(format!("Properties  •  {}", info.name), info.detail);
                        self.sync_bottom_bar();
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::MetadataFinished(result) => {
                self.busy = false;
                match result {
                    Ok(status) => self.status = status,
                    Err(error) => {
                        self.command
                            .show_output("File action failed".to_owned(), error);
                        self.sync_bottom_bar();
                    }
                }
                self.schedule_details()
            }
            Message::AnimationFrame(now) => {
                self.animation_now = now;
                self.sidebar_scrollbar.hide_if_elapsed(now);
                self.entry_scrollbar.hide_if_elapsed(now);
                self.tick_drag_hover(now)
            }
            Message::RenameChanged(value) => {
                if self.browser_input.mode() == InputMode::Rename {
                    self.file_operations.change_name(value);
                }
                Task::none()
            }
            Message::RenameSubmitted => self.submit_rename(),
            Message::PromptInputChanged(value) => {
                self.file_operations.change_name(value);
                Task::none()
            }
            Message::PromptSubmit => self.submit_file_operation_name(),
            Message::PromptConfirm => self.confirm_prompt(),
            Message::PromptCancel => self.cancel_prompt(),
            Message::FileOperationFinished(completion) => self.finish_file_operation(completion),
            Message::JournalFinished { journal, result } => self.finish_journal(*journal, result),
            Message::Copy => self.copy_selection(),
            Message::Paste => self.paste(),
            Message::ClipboardRead(result) => match result {
                Ok(payload) => {
                    if self.transfers.import_clipboard(payload) {
                        self.sync_location_monitoring();
                        self.paste_current()
                    } else {
                        self.status = "The clipboard does not contain local files".to_owned();
                        Task::none()
                    }
                }
                Err(error) => {
                    self.status = error;
                    Task::none()
                }
            },
            Message::OperationError(error) => {
                self.show_error(error);
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    pub(super) fn handle_event(
        &mut self,
        event: iced::Event,
        status: event::Status,
    ) -> Task<Message> {
        match event {
            iced::Event::Window(window::Event::FileHovered(path)) if self.x11_dnd_active() => {
                let first_path = self.x11_drop_paths.is_empty();
                if !self.x11_drop_paths.contains(&path) {
                    self.x11_drop_paths.push(path);
                }
                if first_path {
                    self.x11_drop_action = self.native_dnd.as_ref().map_or(
                        TransferAction::Copy,
                        native_clipboard::DndSource::incoming_action,
                    );
                }
                self.handle_native_dnd_event(TransferEvent::Hover {
                    id: X11_INBOUND_ID,
                    position: self.grid.cursor(),
                    action: self.x11_drop_action,
                })
            }
            iced::Event::Window(window::Event::FilesHoveredLeft) if self.x11_dnd_active() => {
                self.x11_drop_paths.clear();
                self.x11_drop_action = TransferAction::Copy;
                self.handle_native_dnd_event(TransferEvent::Leave { id: X11_INBOUND_ID })
            }
            iced::Event::Window(window::Event::FileDropped(path)) if self.x11_dnd_active() => {
                if !self.x11_drop_paths.contains(&path) {
                    self.x11_drop_paths.push(path);
                }
                self.x11_drop_generation = self.x11_drop_generation.wrapping_add(1);
                let generation = self.x11_drop_generation;
                Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        generation
                    },
                    Message::X11DropReady,
                )
            }
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                self.modifiers = modifiers;
                Task::none()
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let mut tasks = Vec::new();
                if let Some(index) = self.transfers.move_pointer(position)
                    && !self.grid.is_selected(index)
                {
                    self.grid
                        .select_only(Some(index), self.navigation.entries().len());
                    tasks.push(self.schedule_details());
                }
                if self.transfers.active_drag_index().is_some()
                    && self.transfers.active_drag_entries().is_empty()
                {
                    self.transfers.capture_drag_entries(
                        self.navigation.entries(),
                        self.grid.selected_indices(),
                    );
                }
                if self
                    .grid
                    .move_cursor(position, self.navigation.entries().len())
                {
                    self.refresh_status();
                }
                if self.transfers.active_drag_index().is_some() && self.grid.cursor_outside_window()
                {
                    tasks.push(self.start_external_drag());
                } else {
                    self.update_drag_hover(position);
                }
                Task::batch(tasks)
            }
            iced::Event::Mouse(mouse::Event::CursorLeft)
                if self.transfers.active_drag_index().is_some() =>
            {
                self.start_external_drag()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if status == event::Status::Ignored
                    && self.grid.start_marquee(
                        self.grid.cursor(),
                        self.navigation.entries().len(),
                        self.status_height(),
                        self.mutations_allowed() && !self.file_operations.prompt_active(),
                    ) =>
            {
                self.refresh_status();
                Task::none()
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.grid.finish_marquee() =>
            {
                self.schedule_details()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => self.go_back(),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
                self.go_forward()
            }
            iced::Event::Window(window::Event::Unfocused) if self.grid.finish_marquee() => {
                self.schedule_details()
            }
            iced::Event::Window(window::Event::Moved(position)) => {
                self.startup.remember_position(position);
                Task::none()
            }
            iced::Event::Window(window::Event::CloseRequested) => self.quit(),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                modifiers,
                text,
                ..
            }) => {
                self.modifiers = modifiers;
                if status == event::Status::Captured
                    && key == keyboard::Key::Named(keyboard::key::Named::Enter)
                {
                    return Task::none();
                }
                self.handle_key(key, modified_key, modifiers, text.as_deref())
            }
            _ => Task::none(),
        }
    }

    pub(super) fn handle_key(
        &mut self,
        key: keyboard::Key,
        modified_key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        produced: Option<&str>,
    ) -> Task<Message> {
        let modified_text = match &modified_key {
            keyboard::Key::Character(value) => Some(value.to_string()),
            _ => None,
        };
        let key_text = match &key {
            keyboard::Key::Character(value) => Some(value.to_string()),
            _ => None,
        };
        let text = if modifiers.control() || modifiers.alt() || modifiers.logo() {
            modified_text
                .or(key_text)
                .or_else(|| produced.map(str::to_owned))
        } else {
            produced.map(str::to_owned).or(modified_text).or(key_text)
        };
        let named = match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => InputNamedKey::Escape,
            keyboard::Key::Named(keyboard::key::Named::Enter) => InputNamedKey::Enter,
            keyboard::Key::Named(keyboard::key::Named::Backspace) => InputNamedKey::Backspace,
            keyboard::Key::Named(keyboard::key::Named::Delete) => InputNamedKey::Delete,
            keyboard::Key::Named(keyboard::key::Named::F5) => InputNamedKey::Refresh,
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => InputNamedKey::ArrowLeft,
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => InputNamedKey::ArrowRight,
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => InputNamedKey::ArrowUp,
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => InputNamedKey::ArrowDown,
            keyboard::Key::Named(keyboard::key::Named::Home) => InputNamedKey::Home,
            keyboard::Key::Named(keyboard::key::Named::End) => InputNamedKey::End,
            keyboard::Key::Named(keyboard::key::Named::Space) => InputNamedKey::Space,
            keyboard::Key::Named(keyboard::key::Named::Tab) => InputNamedKey::Tab,
            _ => InputNamedKey::Other,
        };
        if self.context_menu.is_some() {
            return self.handle_context_key(named, modifiers.shift());
        }
        let intent = self.browser_input.handle(
            InputPress {
                text,
                named,
                control: modifiers.control(),
                shift: modifiers.shift(),
                alt: modifiers.alt(),
                logo: modifiers.logo(),
            },
            InputContext {
                transfer_conflict: self.transfers.has_conflict(),
                prompt_active: self.file_operations.prompt_active(),
                prompt_accepts_enter: self.file_operations.prompt_accepts_enter(),
                prompt_uses_yes_no: self.file_operations.prompt_uses_yes_no(),
                busy: self.busy,
                command_output: self.command.output().is_some(),
                visual_active: self.grid.visual_active(),
                selection_count: self.grid.selection_count(),
                has_selection: self.grid.selected_entry().is_some(),
                pending_cut: !self.transfers.pending_cut_paths().is_empty(),
                file_operators_allowed: self.browser_focus == BrowserFocus::Entries
                    && self.navigation.folder_displayed(),
            },
        );
        self.apply_input_intent(intent)
    }

    pub(super) fn apply_input_intent(&mut self, intent: InputIntent) -> Task<Message> {
        match intent {
            InputIntent::None => Task::none(),
            InputIntent::PromptCancel => self.update(Message::PromptCancel),
            InputIntent::PromptConfirm => self.update(Message::PromptConfirm),
            InputIntent::ConflictCancel => self.cancel_transfer_conflict(),
            InputIntent::ConflictChoice { key, remaining } => {
                self.resolve_transfer_conflict(key, remaining)
            }
            InputIntent::CancelSearch => self.cancel_search(),
            InputIntent::CancelCommand => {
                self.command.cancel();
                Task::none()
            }
            InputIntent::CancelRename => {
                self.cancel_rename();
                Task::none()
            }
            InputIntent::CancelLocation => {
                self.location_input = self.navigation.current().display().to_string();
                self.startup
                    .remember_directory(self.navigation.current().to_path_buf());
                Task::none()
            }
            InputIntent::CloseCommandOutput => {
                self.command.close_output();
                self.sync_bottom_bar();
                Task::none()
            }
            InputIntent::CancelVisual => {
                self.grid
                    .cancel_visual_selection(self.navigation.entries().len());
                self.schedule_details()
            }
            InputIntent::CancelCut => self.cancel_cut("Cut cancelled"),
            InputIntent::Copy => self.update(Message::Copy),
            InputIntent::Cut => self.cut_selection(),
            InputIntent::Paste => self.update(Message::Paste),
            InputIntent::Undo => self.run_journal(false),
            InputIntent::Redo => self.run_journal(true),
            InputIntent::Refresh => self.refresh_location(),
            InputIntent::ToggleTree => self.toggle_tree(),
            InputIntent::BeginLocation => self.begin_location(),
            InputIntent::MoveFocus { reverse } => {
                self.move_browser_focus(reverse);
                if self.browser_focus == BrowserFocus::Sidebar && self.sidebar_cursor.is_none() {
                    self.sidebar_cursor = flatten_rows(&self.explorer, self.navigation.current())
                        .into_iter()
                        .find(|row| row.selected)
                        .or_else(|| {
                            flatten_rows(&self.explorer, self.navigation.current())
                                .into_iter()
                                .next()
                        })
                        .map(|row| row.id);
                }
                self.status = format!("Focus: {}", self.focus_label());
                Task::none()
            }
            InputIntent::CompleteCommand => {
                if !self.command.complete_setting() {
                    self.status = "No unique setting completion".to_owned();
                }
                Task::none()
            }
            InputIntent::SelectAll => {
                self.grid.select_all(self.navigation.entries().len());
                self.schedule_details()
            }
            InputIntent::ToggleActive if self.browser_focus != BrowserFocus::Entries => {
                self.activate_focused()
            }
            InputIntent::ToggleActive => {
                self.grid.toggle_active(self.navigation.entries().len());
                self.schedule_details()
            }
            InputIntent::StandardMove { motion, extend } => self.move_focused(motion, 1, extend),
            InputIntent::Back => self.go_back(),
            InputIntent::Forward => self.go_forward(),
            InputIntent::BeginSearch => self.begin_search(),
            InputIntent::BeginCommand(prefix) => self.begin_command(prefix),
            InputIntent::RepeatSearch(reverse) => self.repeat_search(reverse),
            InputIntent::Rename => self.rename_selected(),
            InputIntent::ToggleVisual => {
                self.grid
                    .toggle_visual_selection(self.navigation.entries().len());
                self.schedule_details()
            }
            InputIntent::Trash => self.show_trash_prompt(),
            InputIntent::Pending(status) | InputIntent::InvalidSequence(status) => {
                self.status = status;
                Task::none()
            }
            InputIntent::Move(motion, count) => self.move_focused(motion, count, false),
            InputIntent::CutMotion(motion, count) => {
                self.grid.select_delete_motion_count(
                    motion,
                    count,
                    self.navigation.entries().len(),
                    self.status_height(),
                );
                self.cut_selection()
            }
            InputIntent::TrashMotion(motion, count) => {
                self.grid.select_delete_motion_count(
                    motion,
                    count,
                    self.navigation.entries().len(),
                    self.status_height(),
                );
                self.show_trash_prompt()
            }
            InputIntent::Activate => self.activate_focused(),
            InputIntent::Parent => self.go_parent(),
        }
    }

    pub(super) fn focus_label(&self) -> &'static str {
        match self.browser_focus {
            BrowserFocus::Toolbar => "toolbar",
            BrowserFocus::Location => "location",
            BrowserFocus::Sidebar => "sidebar",
            BrowserFocus::Entries => "files",
            BrowserFocus::BottomBar => "bottom bar",
        }
    }

    pub(super) fn move_browser_focus(&mut self, reverse: bool) {
        self.browser_focus = self.browser_focus.moved(reverse);
        if !self.view_preferences.tree_visible() && self.browser_focus == BrowserFocus::Sidebar {
            self.browser_focus = self.browser_focus.moved(reverse);
        }
    }

    pub(super) fn sync_tree_visibility(&mut self) {
        let visible = self.view_preferences.tree_visible();
        self.grid.set_sidebar_visible(visible);
        if !visible && self.browser_focus == BrowserFocus::Sidebar {
            self.browser_focus = BrowserFocus::Entries;
        }
        self.update_drag_hover(self.grid.cursor());
    }

    pub(super) fn toggle_tree(&mut self) -> Task<Message> {
        let visible = self.view_preferences.toggle_tree();
        self.sync_tree_visibility();
        self.status = if visible {
            "Tree shown".to_owned()
        } else {
            "Tree hidden".to_owned()
        };
        self.load_visible_thumbnails()
    }

    pub(super) fn move_focused(
        &mut self,
        motion: Motion,
        count: usize,
        extend: bool,
    ) -> Task<Message> {
        match self.browser_focus {
            BrowserFocus::Toolbar => {
                move_composite_cursor(&mut self.toolbar_cursor, 5, motion, count);
                self.status = format!("Toolbar control {} of 5", self.toolbar_cursor + 1);
                Task::none()
            }
            BrowserFocus::Location => Task::none(),
            BrowserFocus::Sidebar => self.move_sidebar(motion, count),
            BrowserFocus::Entries if extend => {
                self.grid.move_standard(
                    motion,
                    true,
                    self.navigation.entries().len(),
                    self.status_height(),
                );
                Task::batch([self.schedule_details(), self.scroll_to_selected()])
            }
            BrowserFocus::Entries => self.move_selection(motion, count),
            BrowserFocus::BottomBar => {
                let action_count = self.bottom_actions().len().max(1);
                move_composite_cursor(&mut self.bottom_cursor, action_count, motion, count);
                self.status = if action_count == 1 && self.bottom_actions().is_empty() {
                    "Bottom bar has no actions".to_owned()
                } else {
                    format!(
                        "Bottom bar action {} of {action_count}",
                        self.bottom_cursor + 1
                    )
                };
                Task::none()
            }
        }
    }

    pub(super) fn activate_focused(&mut self) -> Task<Message> {
        match self.browser_focus {
            BrowserFocus::Toolbar => {
                let message = match self.toolbar_cursor.min(4) {
                    0 => Message::Parent,
                    1 => Message::Back,
                    2 => Message::Forward,
                    3 => Message::Refresh,
                    _ => Message::ToggleView,
                };
                self.update(message)
            }
            BrowserFocus::Location => self.begin_location(),
            BrowserFocus::Sidebar => self
                .sidebar_cursor
                .map_or_else(Task::none, |id| self.activate_tree_row(id)),
            BrowserFocus::Entries => self.activate_selected(),
            BrowserFocus::BottomBar => {
                let actions = self.bottom_actions();
                actions
                    .get(self.bottom_cursor.min(actions.len().saturating_sub(1)))
                    .cloned()
                    .map_or_else(Task::none, |message| self.update(message))
            }
        }
    }

    pub(super) fn bottom_actions(&self) -> Vec<Message> {
        if self.command.output().is_some() {
            return vec![Message::CopyCommandReport];
        }
        let mut actions = Vec::new();
        if self.transfers.active() {
            actions.push(Message::CancelTransfer);
        }
        if self.transfers.has_retry() {
            actions.push(Message::RetryTransfer);
        }
        if self.transfers.expanded() {
            actions.push(Message::CopyTransferReport);
        }
        if !actions.is_empty() || self.transfers.expanded() {
            actions.push(Message::ToggleTransferHistory);
        }
        actions
    }

    pub(super) fn context_actions(&self) -> Vec<(String, Message)> {
        if self.navigation.displayed_location() == DisplayedLocation::Trash {
            return vec![
                ("Restore".to_owned(), Message::ContextRestore),
                (
                    "Delete permanently".to_owned(),
                    Message::ContextDeletePermanent,
                ),
                ("Empty Trash".to_owned(), Message::ContextEmptyTrash),
                ("Properties".to_owned(), Message::ContextProperties),
                ("Open With…".to_owned(), Message::ContextOpenWith),
            ];
        }
        let mut actions = vec![
            ("New Folder".to_owned(), Message::ContextNewFolder),
            (
                "New Empty File".to_owned(),
                Message::ContextNewFile(None, String::new(), "new file".to_owned()),
            ),
        ];
        actions.extend(self.templates.iter().map(|template| {
            (
                format!("New from {}", template.label),
                Message::ContextNewFile(
                    Some(template.path.clone()),
                    template.suggested_name.clone(),
                    format!("template {}", template.label),
                ),
            )
        }));
        actions.extend([
            ("Properties".to_owned(), Message::ContextProperties),
            ("Open With…".to_owned(), Message::ContextOpenWith),
            ("Rename".to_owned(), Message::ContextRename),
            ("Move to Trash".to_owned(), Message::ContextTrash),
        ]);
        actions
    }

    pub(super) fn handle_context_key(&mut self, key: InputNamedKey, shift: bool) -> Task<Message> {
        let count = self.context_actions().len().max(1);
        match key {
            InputNamedKey::Escape => self.update(Message::CloseContext),
            InputNamedKey::Tab => {
                self.context_menu_cursor = if shift {
                    self.context_menu_cursor.checked_sub(1).unwrap_or(count - 1)
                } else {
                    (self.context_menu_cursor + 1) % count
                };
                Task::none()
            }
            InputNamedKey::ArrowUp => {
                self.context_menu_cursor = self.context_menu_cursor.saturating_sub(1);
                Task::none()
            }
            InputNamedKey::ArrowDown => {
                self.context_menu_cursor = (self.context_menu_cursor + 1).min(count - 1);
                Task::none()
            }
            InputNamedKey::Home => {
                self.context_menu_cursor = 0;
                Task::none()
            }
            InputNamedKey::End => {
                self.context_menu_cursor = count - 1;
                Task::none()
            }
            InputNamedKey::Enter | InputNamedKey::Space => self
                .context_actions()
                .get(self.context_menu_cursor.min(count - 1))
                .map(|(_, message)| message.clone())
                .map_or_else(Task::none, |message| self.update(message)),
            _ => Task::none(),
        }
    }
}
