use std::{path::PathBuf, time::Duration};

use iced::{Task, event, keyboard, mouse, time::Instant, window};

use crate::{fs, theme, transfer::Event as TransferEvent};

use super::{
    App, BottomInput, BrowserFocus, ContextNavigation, ContextOutcome, ContextTarget,
    DisplayedLocation, InputContext, InputIntent, InputMode, InputNamedKey, InputPress,
    MOUSE_BACK_DOUBLE_CLICK_INTERVAL, Message, Motion, MouseBackGesture, NavigationCompletion,
    NavigationTransition, Scrollbar, X11_INBOUND_ID, clears_status_notice, find_window_after_delay,
    location_monitoring, native_clipboard, transfer_integration, transfer_session,
};

impl App {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Event(event, status) => {
                if clears_status_notice(&event) {
                    self.presentation.clear_notice();
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
                if let Err(error) = self.transfers.install_native(result) {
                    eprintln!("Waddle: external drag-and-drop unavailable: {error}");
                }
                Task::none()
            }
            Message::NativeDndEvent(event) => self.handle_native_dnd_event(event),
            Message::X11DropReady(generation) => self.finish_x11_drop(generation),
            Message::ExternalDragFinished(result) => {
                let completion = self.transfers.finish_outgoing(result);
                self.apply_transfer_completion(completion)
            }
            Message::TransferBatchFinished { id, outcome } => {
                self.finish_transfer_batch(id, *outcome)
            }
            Message::PollTransfer => Task::none(),
            Message::CancelTransfer => {
                self.presentation.set_focus(BrowserFocus::BottomBar);
                self.presentation.reset_bottom_cursor();
                self.cancel_transfer_conflict()
            }
            Message::RetryTransfer => {
                self.presentation.set_focus(BrowserFocus::BottomBar);
                match self.transfers.retry(&self.operations) {
                    Ok(task) => task.map(transfer_integration::transfer_runtime_message),
                    Err(error) => {
                        self.show_error(error);
                        Task::none()
                    }
                }
            }
            Message::ToggleTransferHistory => {
                self.presentation.set_focus(BrowserFocus::BottomBar);
                self.transfers.toggle_expanded();
                self.sync_transient_presentation();
                Task::none()
            }
            Message::CopyTransferReport => {
                self.flash_copy_feedback();
                iced::clipboard::write(self.transfers.report_text())
            }
            Message::CopyCommandReport => {
                let Some(output) = self.command.output() else {
                    return Task::none();
                };
                let report = format!("{}\n\n{}", output.summary, output.detail);
                self.flash_copy_feedback();
                iced::clipboard::write(report)
            }
            Message::SystemTheme(mode) => {
                self.system_mode = mode;
                Task::none()
            }
            Message::PollSystem => {
                let settings = theme::interface_settings();
                self.accent = theme::load(settings.as_ref());
                self.system_accessibility = theme::accessibility(settings.as_ref());
                self.sidebar_tree.refresh_mounts();
                let search = if self.search.is_recursive() {
                    self.live_refresh()
                } else {
                    Task::none()
                };
                let fallback = self
                    .location_monitoring
                    .as_ref()
                    .map(|monitoring| monitoring.poll(&self.search))
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
                let change = match self.location_monitoring.as_mut() {
                    Some(monitoring) => monitoring.handle(event, &mut self.transfers),
                    None => location_monitoring::Change::default(),
                };
                if let Some(notice) = change.notice {
                    self.presentation.set_notice(notice);
                }
                if change.resync {
                    self.sync_location_monitoring();
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
            Message::Parent => self.transition_navigation(NavigationTransition::Parent),
            Message::Back => self.transition_navigation(NavigationTransition::Back),
            Message::Forward => self.transition_navigation(NavigationTransition::HistoryForward),
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
                self.transition_navigation(NavigationTransition::Open {
                    requested,
                    remember: true,
                    select: None,
                })
            }
            Message::TreeRow(id) => {
                self.presentation.set_focus(BrowserFocus::Sidebar);
                self.sidebar_tree.focus(id);
                self.activate_tree_row(id)
            }
            Message::SidebarScrolled(y) => {
                let now = Instant::now();
                self.presentation.set_now(now);
                self.grid.scroll(Scrollbar::Sidebar, y, now);
                self.update_drag_hover(self.grid.cursor());
                Task::none()
            }
            Message::TreeLoaded(id, path, folders) => {
                self.sidebar_tree.install_children(id, &path, folders);
                Task::none()
            }
            Message::FavoritePressed(index) => {
                self.sidebar_tree.press_favorite(index);
                Task::none()
            }
            Message::FavoriteReleased(index) => {
                let mut entries = self.recent.sidebar_entry().into_iter().collect::<Vec<_>>();
                entries.push(self.trash.sidebar_entry());
                match self.sidebar_tree.release_favorite(index, entries) {
                    Ok(true) => self
                        .presentation
                        .set_status("Favorite order saved".to_owned()),
                    Ok(false) => {}
                    Err(error) => self.presentation.set_status(error),
                }
                Task::none()
            }
            Message::EntryPressed(index) => {
                if self.prompt_blocks_action() {
                    return Task::none();
                }
                self.presentation.set_focus(BrowserFocus::Entries);
                self.transfers
                    .press(index, self.grid.cursor(), self.navigation.entries().len());
                Task::none()
            }
            Message::EntryReleased(index) => self.finish_entry_press(index),
            Message::EntryDoubleClicked(index) => self.activate_entry(index, true),
            Message::EntryContext(index) => {
                if self.prompt_blocks_action() {
                    return Task::none();
                }
                self.presentation.set_focus(BrowserFocus::Entries);
                if self
                    .grid
                    .open_entry_context(index, self.navigation.entries().len())
                {
                    self.schedule_details()
                } else {
                    Task::none()
                }
            }
            Message::ContextFocused(index) => {
                if let Some(menu) = self.grid.context_menu() {
                    let item_count = self.context_actions(menu.target).len();
                    self.grid.focus_context(index, item_count);
                }
                Task::none()
            }
            Message::ContextNewFolder => {
                self.grid.close_context();
                self.show_new_folder()
            }
            Message::ContextNewFile => {
                self.grid.close_context();
                self.show_new_file()
            }
            Message::ContextProperties => {
                self.grid.close_context();
                self.show_properties()
            }
            Message::ContextOpenWith => {
                self.grid.close_context();
                self.begin_open_with()
            }
            Message::OpenWithChanged(value) => {
                self.open_with.change_custom(value);
                Task::none()
            }
            Message::OpenWithSubmitted => self.submit_open_with(),
            Message::OpenWithSelected(application) => self.choose_open_with(application),
            Message::ContextRename => {
                let index = self.grid.take_context_entry();
                if let Some(index) = index {
                    self.show_rename(index)
                } else {
                    Task::none()
                }
            }
            Message::ContextTrash => {
                self.grid.close_context();
                self.show_trash_prompt()
            }
            Message::ContextRestore => {
                self.grid.close_context();
                self.restore_selected_trash()
            }
            Message::ContextDeletePermanent => {
                self.grid.close_context();
                self.show_trash_delete_prompt(false)
            }
            Message::ContextEmptyTrash => {
                self.grid.close_context();
                self.show_trash_delete_prompt(true)
            }
            Message::CloseContext => {
                self.grid.close_context();
                Task::none()
            }
            Message::MouseBackTick(now) => self.finish_single_mouse_back_click(now),
            Message::GridScrolled(y) => {
                let now = Instant::now();
                self.presentation.set_now(now);
                self.grid.scroll(Scrollbar::Entries, y, now);
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
                    if !self.transfers.overview().native_active {
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
                    self.presentation.set_status(error);
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
                match result {
                    Ok(status) => {
                        self.presentation.set_status(status);
                        self.sidebar_tree.refresh_mounts();
                    }
                    Err(error) => self.presentation.set_status(error),
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
                match result {
                    Ok(info) => {
                        self.show_command_output(
                            format!("Properties  •  {}", info.name),
                            info.detail,
                        );
                    }
                    Err(error) => self.presentation.set_status(error),
                }
                Task::none()
            }
            Message::MetadataFinished(result) => {
                match result {
                    Ok(status) => self.presentation.set_status(status),
                    Err(error) => {
                        self.show_command_output("File action failed".to_owned(), error);
                    }
                }
                self.schedule_details()
            }
            Message::AnimationFrame(now) => {
                self.presentation.tick(now);
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
                    let destination = self.navigation.current().to_path_buf();
                    if let Some(request) = self.transfers.paste_import(payload, destination) {
                        self.sync_location_monitoring();
                        self.start_transfer(request)
                    } else {
                        self.presentation
                            .set_status("The clipboard does not contain local files".to_owned());
                        Task::none()
                    }
                }
                Err(error) => {
                    self.presentation.set_status(error);
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
        let refocus_bottom_input =
            matches!(event, iced::Event::Mouse(mouse::Event::ButtonPressed(_)));
        let task = match event {
            iced::Event::Window(window::Event::FileHovered(path)) => match self
                .transfers
                .handle_window_file(transfer_session::WindowFileEvent::Hover(path))
            {
                transfer_session::WindowFileUpdate::Hover(action) => {
                    self.handle_native_dnd_event(TransferEvent::Hover {
                        id: X11_INBOUND_ID,
                        position: self.grid.cursor(),
                        action,
                    })
                }
                _ => Task::none(),
            },
            iced::Event::Window(window::Event::FilesHoveredLeft) => match self
                .transfers
                .handle_window_file(transfer_session::WindowFileEvent::Leave)
            {
                transfer_session::WindowFileUpdate::Leave => {
                    self.handle_native_dnd_event(TransferEvent::Leave { id: X11_INBOUND_ID })
                }
                _ => Task::none(),
            },
            iced::Event::Window(window::Event::FileDropped(path)) => match self
                .transfers
                .handle_window_file(transfer_session::WindowFileEvent::Drop(path))
            {
                transfer_session::WindowFileUpdate::Drop(generation) => Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        generation
                    },
                    Message::X11DropReady,
                ),
                _ => Task::none(),
            },
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                self.modifiers = modifiers;
                Task::none()
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let mut tasks = Vec::new();
                if let Some(index) = self.transfers.move_pointer(
                    position,
                    self.navigation.entries(),
                    self.grid.selected_indices(),
                ) && !self.grid.is_selected(index)
                {
                    self.grid
                        .select_only(Some(index), self.navigation.entries().len());
                    tasks.push(self.schedule_details());
                }
                if self
                    .grid
                    .move_cursor(position, self.navigation.entries().len())
                {
                    self.refresh_status();
                }
                if self.transfers.overview().pointer_drag.is_active()
                    && self.grid.cursor_outside_window()
                {
                    tasks.push(self.start_external_drag());
                } else {
                    self.update_drag_hover(position);
                }
                Task::batch(tasks)
            }
            iced::Event::Mouse(mouse::Event::CursorLeft)
                if self.transfers.overview().pointer_drag.is_active() =>
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
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if status == event::Status::Ignored =>
            {
                self.open_background_context()
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.grid.finish_marquee() =>
            {
                self.schedule_details()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => {
                self.press_mouse_back()
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Back)) => {
                self.release_mouse_back()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
                self.transition_navigation(NavigationTransition::HistoryForward)
            }
            iced::Event::Window(window::Event::Unfocused) => {
                self.mouse_back_gesture = None;
                if self.grid.finish_marquee() {
                    self.schedule_details()
                } else {
                    Task::none()
                }
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
        };
        if refocus_bottom_input {
            Task::batch([task, self.refocus_bottom_input()])
        } else {
            task
        }
    }

    fn press_mouse_back(&mut self) -> Task<Message> {
        self.prepare_navigation_transition();
        let now = Instant::now();
        match self.mouse_back_gesture.take() {
            Some(MouseBackGesture::AwaitingSecondClick { first_released_at })
                if now.saturating_duration_since(first_released_at)
                    < MOUSE_BACK_DOUBLE_CLICK_INTERVAL =>
            {
                self.mouse_back_gesture = Some(MouseBackGesture::SecondPressed);
                Task::none()
            }
            Some(MouseBackGesture::AwaitingSecondClick { .. }) => {
                let task = self.transition_navigation(NavigationTransition::Back);
                self.mouse_back_gesture = Some(MouseBackGesture::FirstPressed);
                task
            }
            Some(gesture @ MouseBackGesture::FirstPressed)
            | Some(gesture @ MouseBackGesture::SecondPressed) => {
                self.mouse_back_gesture = Some(gesture);
                Task::none()
            }
            None => {
                self.mouse_back_gesture = Some(MouseBackGesture::FirstPressed);
                Task::none()
            }
        }
    }

    fn release_mouse_back(&mut self) -> Task<Message> {
        match self.mouse_back_gesture.take() {
            Some(MouseBackGesture::FirstPressed) => {
                self.mouse_back_gesture = Some(MouseBackGesture::AwaitingSecondClick {
                    first_released_at: Instant::now(),
                });
                Task::none()
            }
            Some(MouseBackGesture::SecondPressed) => {
                self.transition_navigation(NavigationTransition::Parent)
            }
            Some(gesture @ MouseBackGesture::AwaitingSecondClick { .. }) => {
                self.mouse_back_gesture = Some(gesture);
                Task::none()
            }
            None => Task::none(),
        }
    }

    fn finish_single_mouse_back_click(&mut self, now: Instant) -> Task<Message> {
        let Some(MouseBackGesture::AwaitingSecondClick { first_released_at }) =
            self.mouse_back_gesture
        else {
            return Task::none();
        };
        if now.saturating_duration_since(first_released_at) < MOUSE_BACK_DOUBLE_CLICK_INTERVAL {
            return Task::none();
        }
        self.mouse_back_gesture = None;
        self.transition_navigation(NavigationTransition::Back)
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
        let cancel_bottom_input = self.bottom_input_active()
            && (named == InputNamedKey::Escape
                || named == InputNamedKey::Backspace && self.active_bottom_input_empty());
        if self.grid.context_menu().is_some() && !cancel_bottom_input {
            return self.handle_context_key(named, modifiers.shift());
        }
        if cancel_bottom_input {
            self.grid.close_context();
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
                transfer_conflict: self.transfers.overview().conflict_prompt.is_some(),
                prompt: self.file_operations.prompt_interaction(),
                foreground_operation_active: self.foreground_operation_active(),
                command_output: self.command.output().is_some(),
                visual_active: self.grid.visual_active(),
                selection_count: self.grid.selection_count(),
                has_selection: self.grid.selected_entry().is_some(),
                pending_cut: !self.transfers.pending_cut_paths().is_empty(),
                file_operators_allowed: self.presentation.focus_is(BrowserFocus::Entries)
                    && self.navigation.folder_displayed(),
                bottom_input: BottomInput::new(
                    self.bottom_input_active(),
                    self.active_bottom_input_empty(),
                ),
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
            InputIntent::CancelOpenWith => self.cancel_open_with(),
            InputIntent::CancelLocation => {
                self.location_input = self.navigation.current().display().to_string();
                self.startup
                    .remember_directory(self.navigation.current().to_path_buf());
                Task::none()
            }
            InputIntent::CloseCommandOutput => {
                self.close_command_output();
                Task::none()
            }
            InputIntent::CopyCommandOutput => self.update(Message::CopyCommandReport),
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
                if self.presentation.focus_is(BrowserFocus::Sidebar) {
                    self.sidebar_tree
                        .focus_current_or_first(self.navigation.current());
                }
                self.presentation
                    .set_status(format!("Focus: {}", self.focus_label()));
                Task::none()
            }
            InputIntent::CompleteCommand => {
                if !self.command.complete_setting() {
                    self.presentation
                        .set_status("No unique setting completion".to_owned());
                }
                Task::none()
            }
            InputIntent::SelectAll => {
                self.grid.select_all(self.navigation.entries().len());
                self.schedule_details()
            }
            InputIntent::ToggleActive if !self.presentation.focus_is(BrowserFocus::Entries) => {
                self.activate_focused()
            }
            InputIntent::ToggleActive => {
                self.grid.toggle_active(self.navigation.entries().len());
                self.schedule_details()
            }
            InputIntent::StandardMove { motion, extend } => self.move_focused(motion, 1, extend),
            InputIntent::Back => self.transition_navigation(NavigationTransition::Back),
            InputIntent::Forward => {
                self.transition_navigation(NavigationTransition::HistoryForward)
            }
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
                self.presentation.set_status(status);
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
            InputIntent::Parent => self.transition_navigation(NavigationTransition::Parent),
        }
    }

    pub(super) fn focus_label(&self) -> &'static str {
        self.presentation.focus_label()
    }

    pub(super) fn move_browser_focus(&mut self, reverse: bool) {
        self.presentation
            .move_focus(reverse, self.view_preferences.tree_visible());
    }

    pub(super) fn sync_tree_visibility(&mut self) {
        let visible = self.view_preferences.tree_visible();
        self.grid.set_sidebar_visible(visible);
        if !visible && self.presentation.focus_is(BrowserFocus::Sidebar) {
            self.presentation.set_focus(BrowserFocus::Entries);
        }
        self.update_drag_hover(self.grid.cursor());
    }

    pub(super) fn toggle_tree(&mut self) -> Task<Message> {
        let visible = self.view_preferences.toggle_tree();
        self.sync_tree_visibility();
        self.presentation.set_status(if visible {
            "Tree shown".to_owned()
        } else {
            "Tree hidden".to_owned()
        });
        self.load_visible_thumbnails()
    }

    pub(super) fn move_focused(
        &mut self,
        motion: Motion,
        count: usize,
        extend: bool,
    ) -> Task<Message> {
        match self.presentation.focus() {
            BrowserFocus::Toolbar => {
                let status = self.presentation.move_toolbar_cursor(motion, count);
                self.presentation.set_status(status);
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
                let action_count = self.bottom_actions().len();
                let status = self
                    .presentation
                    .move_bottom_cursor(action_count, motion, count);
                self.presentation.set_status(status);
                Task::none()
            }
        }
    }

    pub(super) fn activate_focused(&mut self) -> Task<Message> {
        match self.presentation.focus() {
            BrowserFocus::Toolbar => {
                let message = match self.presentation.toolbar_cursor().min(4) {
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
                .sidebar_tree
                .focused_id()
                .map_or_else(Task::none, |id| self.activate_tree_row(id)),
            BrowserFocus::Entries => self.activate_selected(),
            BrowserFocus::BottomBar => {
                let actions = self.bottom_actions();
                actions
                    .get(
                        self.presentation
                            .bottom_cursor()
                            .min(actions.len().saturating_sub(1)),
                    )
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
        if self.transfers.overview().active {
            actions.push(Message::CancelTransfer);
        }
        if self.transfers.overview().retry {
            actions.push(Message::RetryTransfer);
        }
        if self.transfers.overview().expanded {
            actions.push(Message::CopyTransferReport);
        }
        if !actions.is_empty() || self.transfers.overview().expanded {
            actions.push(Message::ToggleTransferHistory);
        }
        actions
    }

    pub(super) fn open_background_context(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        let target = ContextTarget::Background;
        if self.context_actions(target).is_empty() {
            return Task::none();
        }
        self.presentation.set_focus(BrowserFocus::Entries);
        if self
            .grid
            .open_background_context(self.navigation.entries().len(), self.status_height())
        {
            self.schedule_details()
        } else {
            Task::none()
        }
    }

    pub(super) fn context_actions(&self, target: ContextTarget) -> Vec<(String, Message)> {
        match (self.navigation.displayed_location(), target) {
            (DisplayedLocation::Trash, ContextTarget::Background) => {
                vec![("Empty Trash".to_owned(), Message::ContextEmptyTrash)]
            }
            (DisplayedLocation::Trash, ContextTarget::Entry(_)) => vec![
                ("Restore".to_owned(), Message::ContextRestore),
                (
                    "Delete permanently".to_owned(),
                    Message::ContextDeletePermanent,
                ),
                ("Empty Trash".to_owned(), Message::ContextEmptyTrash),
                ("Properties".to_owned(), Message::ContextProperties),
                ("Open With…".to_owned(), Message::ContextOpenWith),
            ],
            (DisplayedLocation::Folder, ContextTarget::Background) => vec![
                ("New Folder".to_owned(), Message::ContextNewFolder),
                ("New Empty File".to_owned(), Message::ContextNewFile),
            ],
            (DisplayedLocation::Recent, ContextTarget::Background) => Vec::new(),
            (_, ContextTarget::Entry(_)) => vec![
                ("New Folder".to_owned(), Message::ContextNewFolder),
                ("New Empty File".to_owned(), Message::ContextNewFile),
                ("Properties".to_owned(), Message::ContextProperties),
                ("Open With…".to_owned(), Message::ContextOpenWith),
                ("Rename".to_owned(), Message::ContextRename),
                ("Move to Trash".to_owned(), Message::ContextTrash),
            ],
        }
    }

    pub(super) fn handle_context_key(&mut self, key: InputNamedKey, shift: bool) -> Task<Message> {
        let Some(menu) = self.grid.context_menu() else {
            return Task::none();
        };
        let actions = self.context_actions(menu.target);
        let navigation = match key {
            InputNamedKey::Escape => ContextNavigation::Close,
            InputNamedKey::Tab if shift => ContextNavigation::Previous { wrap: true },
            InputNamedKey::Tab => ContextNavigation::Next { wrap: true },
            InputNamedKey::ArrowUp => ContextNavigation::Previous { wrap: false },
            InputNamedKey::ArrowDown => ContextNavigation::Next { wrap: false },
            InputNamedKey::Home => ContextNavigation::First,
            InputNamedKey::End => ContextNavigation::Last,
            InputNamedKey::Enter | InputNamedKey::Space => ContextNavigation::Activate,
            _ => return Task::none(),
        };
        match self.grid.navigate_context(navigation, actions.len()) {
            ContextOutcome::Activate(index) => actions
                .into_iter()
                .nth(index)
                .map_or_else(Task::none, |(_, message)| self.update(message)),
            ContextOutcome::None | ContextOutcome::Closed => Task::none(),
        }
    }
}
