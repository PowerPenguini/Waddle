mod browser_input;
mod command;
mod file_operation;
mod grid;
mod navigation;
mod operations;
mod search;
mod shell;
mod state;
mod tree;

#[cfg(target_os = "linux")]
mod native_clipboard;
#[cfg(target_os = "linux")]
mod native_dnd;
#[cfg(target_os = "linux")]
mod x11_clipboard;

#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    fs, theme,
    transfer::{
        Action as TransferAction, Adapter, ClipboardAdapter, ClipboardImport,
        Event as TransferEvent, NativeUpdate, Outcome as TransferOutcome,
        Preview as TransferPreview, Release as TransferRelease, Request as TransferRequest,
        TransferWorkflow,
    },
};
use browser_input::{
    BrowserInput, Context as InputContext, Intent as InputIntent, Mode as InputMode,
    NamedKey as InputNamedKey, Press as InputPress,
};
use command::{CommandSession, ProcessAdapter, Submission as CommandSubmission};
use file_operation::{
    FileOperationSession, GioTrashAdapter, View as FileOperationView, Work as FileOperationWork,
};
use fs::FileEntry;
use gio::prelude::*;
use grid::{
    CONTENT_GUTTER, DropZone, GridInteraction, LIST_VIEW_TOP_INSET, Motion, SIDEBAR_WIDTH,
    TILE_ROW_HEIGHT, TILE_WIDTH, TOOLBAR_HEIGHT,
};
use iced::time::Instant;
use iced::{
    Alignment, Animation, Background, Border, Color, Element, Fill, Font, Length, Padding, Point,
    Shadow, Size, Subscription, Task, Theme, Vector,
    animation::Easing,
    application, event, gradient, keyboard, mouse, system, time,
    widget::{
        self, Button, Column, Grid, Id, Row, Space, button, column, container, mouse_area, pin,
        row, rule, scrollable, stack, svg, text, text_input,
    },
    window,
};
use navigation::{NavigationSession, Outcome as NavigationOutcome, Request as NavigationRequest};
use operations::{Completion, Kind as OperationKind, Operations};
use search::{SearchSession, Update as SearchUpdate};
use state::ExplorerState;
use tree::{TreeRow, find_node_mut, flatten_rows, mounted_roots};

const STATUS_HEIGHT: f32 = 25.0;
const SEARCH_LIMIT: usize = 1000;
const UI_FONT: Font = Font::with_name("Roboto");
const UI_FONT_SEMIBOLD: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..UI_FONT
};
const MONO_FONT: Font = Font::with_name("JetBrainsMono Nerd Font Mono");
const MONO_FONT_SEMIBOLD: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..MONO_FONT
};

const LOCATION_ID: &str = "location";
const SEARCH_ID: &str = "search";
const COMMAND_ID: &str = "command";
const RENAME_ID: &str = "rename";
const NEW_FOLDER_ID: &str = "new-folder";
const GRID_SCROLL_ID: &str = "grid-scroll";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryIconKind {
    Folder,
    Generic,
    Code,
    Document,
    Pdf,
    Image,
    Audio,
    Video,
    Archive,
    Spreadsheet,
    Presentation,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BrowserFocus {
    Sidebar,
    #[default]
    Entries,
}

#[derive(Clone, Debug)]
enum Message {
    Event(iced::Event, event::Status),
    FindWindow,
    WindowAvailable(Option<window::Id>),
    WindowResized(Size),
    NativeReady(Result<native_clipboard::Attached, String>),
    NativeDndEvent(TransferEvent),
    ExternalDragFinished(Result<TransferOutcome, String>),
    TransferFinished {
        request: TransferRequest,
        report: fs::TransferReport,
    },
    SystemTheme(iced::theme::Mode),
    PollSystem,
    Parent,
    Back,
    Forward,
    LocationChanged(String),
    LocationFocused,
    LocationSubmitted,
    TreeRow(u64),
    TreeLoaded(u64, PathBuf, Vec<PathBuf>),
    EntryPressed(usize),
    EntryReleased(usize),
    EntryHovered(usize),
    EntryUnhovered(usize),
    EntryDoubleClicked(usize),
    EntryContext(usize),
    ContextNewFolder,
    ContextRename,
    ContextTrash,
    CloseContext,
    GridScrolled(f32),
    GridPointerMoved(Point),
    NavigationFinished {
        requested: PathBuf,
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    },
    DetailsFinished {
        path: PathBuf,
        result: Result<String, String>,
    },
    SearchChanged(String),
    SearchSubmitted,
    SearchFinished(Result<fs::SearchResults, String>),
    CommandChanged(String),
    CommandSubmitted,
    CommandFinished(Result<command::Completion, String>),
    AnimationFrame(Instant),
    RenameChanged(String),
    RenameSubmitted,
    PromptInputChanged(String),
    PromptSubmit,
    PromptConfirm,
    PromptCancel,
    FileOperationFinished(file_operation::Completion),
    Copy,
    Paste,
    ClipboardRead(Result<ClipboardImport, String>),
    OperationError(String),
    Noop,
}

pub fn run() -> iced::Result {
    let window = window::Settings {
        size: Size::new(820.0, 560.0),
        min_size: Some(Size::new(660.0, 420.0)),
        transparent: true,
        blur: true,
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    application(App::new, App::update, App::view)
        .title("PolarExp")
        .settings(iced::Settings {
            id: Some("dev.polarexp.PolarExp".to_owned()),
            default_font: UI_FONT,
            antialiasing: true,
            ..iced::Settings::default()
        })
        .window(window)
        .theme(App::iced_theme)
        .style(App::application_style)
        .subscription(App::subscription)
        .run()
}

struct App {
    explorer: ExplorerState,
    navigation: NavigationSession,
    operations: Operations,
    search: SearchSession,
    transfers: TransferWorkflow,
    command: CommandSession,
    command_adapter: ProcessAdapter,
    file_operations: FileOperationSession,
    trash_adapter: GioTrashAdapter,
    grid: GridInteraction,
    browser_input: BrowserInput,
    browser_focus: BrowserFocus,
    sidebar_cursor: Option<u64>,
    location_input: String,
    expanded_bar_height: f32,
    output_expansion: Animation<bool>,
    animation_now: Instant,
    spinner_started: Instant,
    status: String,
    status_notice: Option<String>,
    busy: bool,
    navigation_loading: bool,
    context_menu: Option<(usize, Point)>,
    native_dnd: Option<native_dnd::Source>,
    native_clipboard: Option<native_clipboard::Source>,
    native_dnd_error: Option<String>,
    modifiers: keyboard::Modifiers,
    system_mode: iced::theme::Mode,
    accent: Option<theme::ThemeColors>,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let now = Instant::now();
        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let explorer = ExplorerState::new(mounted_roots());
        let accent = theme::load(theme::interface_settings().as_ref());
        let mut app = Self {
            explorer,
            navigation: NavigationSession::new(current.clone()),
            operations: Operations::default(),
            search: SearchSession::default(),
            transfers: TransferWorkflow::default(),
            command: CommandSession::default(),
            command_adapter: ProcessAdapter,
            file_operations: FileOperationSession::default(),
            trash_adapter: GioTrashAdapter,
            grid: GridInteraction::default(),
            browser_input: BrowserInput::default(),
            browser_focus: BrowserFocus::Entries,
            sidebar_cursor: None,
            location_input: current.display().to_string(),
            expanded_bar_height: STATUS_HEIGHT,
            output_expansion: Animation::new(false)
                .duration(Duration::from_millis(140))
                .easing(Easing::EaseOut),
            animation_now: now,
            spinner_started: now,
            status: String::new(),
            status_notice: None,
            busy: false,
            navigation_loading: false,
            context_menu: None,
            native_dnd: None,
            native_clipboard: None,
            native_dnd_error: None,
            modifiers: keyboard::Modifiers::default(),
            system_mode: iced::theme::Mode::Dark,
            accent,
        };
        let navigation = app.navigation.refresh(None);
        let initial = Task::batch([
            app.request_navigation(navigation),
            system::theme().map(Message::SystemTheme),
            find_window_after_delay(),
        ]);
        (app, initial)
    }

    fn subscription(&self) -> Subscription<Message> {
        let animation =
            if self.output_expansion.is_animating(self.animation_now) || self.spinner_active() {
                time::every(Duration::from_millis(16)).map(Message::AnimationFrame)
            } else {
                Subscription::none()
            };
        let mut subscriptions = vec![
            event::listen_with(|event, status, _| Some(Message::Event(event, status))),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            system::theme_changes().map(Message::SystemTheme),
            time::every(Duration::from_secs(2)).map(|_| Message::PollSystem),
            animation,
        ];
        if let Some(source) = &self.native_clipboard {
            subscriptions.push(source.subscription().map(Message::NativeDndEvent));
        }
        Subscription::batch(subscriptions)
    }

    fn iced_theme(&self) -> Theme {
        let dark = self.system_mode == iced::theme::Mode::Dark;
        let accent = self
            .accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent);
        let palette = if dark {
            iced::theme::Palette {
                background: Color::from_rgb8(28, 28, 28),
                text: Color::from_rgb8(242, 242, 242),
                primary: accent,
                success: Color::from_rgb8(45, 150, 90),
                danger: Color::from_rgb8(196, 43, 28),
                warning: Color::from_rgb8(214, 140, 28),
            }
        } else {
            iced::theme::Palette {
                background: Color::from_rgb8(250, 250, 250),
                text: Color::from_rgb8(28, 28, 28),
                primary: accent,
                success: Color::from_rgb8(26, 128, 72),
                danger: Color::from_rgb8(196, 43, 28),
                warning: Color::from_rgb8(168, 104, 10),
            }
        };
        Theme::custom("PolarExp", palette)
    }

    fn spinner_active(&self) -> bool {
        self.busy
            || self.navigation_loading
            || self.search.is_loading()
            || flatten_rows(&self.explorer, self.navigation.current())
                .iter()
                .any(|row| row.loading)
    }

    fn spinner(&self, size: f32) -> widget::Svg<'static> {
        let angle = self
            .animation_now
            .duration_since(self.spinner_started)
            .as_secs_f32()
            * std::f32::consts::TAU
            / 0.9;
        themed_svg(
            include_bytes!("../ui/icons/spinner.svg"),
            size,
            self.accent_color(),
        )
        .rotation(angle)
    }

    fn application_style(&self, theme: &Theme) -> iced::theme::Style {
        let palette = theme.palette();
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: palette.text,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
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
                Task::none()
            }
            Message::NativeReady(result) => {
                match result {
                    Ok(attached) => {
                        self.native_dnd = attached.wayland_dnd;
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
            Message::ExternalDragFinished(result) => {
                let consequences = self.transfers.finish_outgoing(result);
                self.apply_transfer_consequences(consequences)
            }
            Message::TransferFinished { request, report } => self.finish_transfer(request, report),
            Message::SystemTheme(mode) => {
                self.system_mode = mode;
                Task::none()
            }
            Message::PollSystem => {
                self.accent = theme::load(theme::interface_settings().as_ref());
                let mounts = mounted_roots();
                self.explorer.reconcile_mounts(mounts);
                Task::none()
            }
            Message::Parent => self.go_parent(),
            Message::Back => self.go_back(),
            Message::Forward => self.go_forward(),
            Message::LocationChanged(value) => {
                self.location_input = value;
                Task::none()
            }
            Message::LocationFocused => {
                if self.prompt_blocks_action() {
                    return Task::none();
                }
                if self.browser_input.mode() == InputMode::Rename {
                    self.cancel_rename();
                }
                self.browser_input.enter(InputMode::Location);
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
            Message::TreeLoaded(id, path, folders) => {
                tree::install_children(&mut self.explorer, id, &path, folders);
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
            Message::EntryDoubleClicked(index) => self.activate_entry(index, true),
            Message::EntryContext(index) => {
                if self.prompt_blocks_action() {
                    return Task::none();
                }
                self.browser_focus = BrowserFocus::Entries;
                self.grid
                    .select_only(Some(index), self.navigation.entries().len());
                self.context_menu = Some((index, self.grid.cursor()));
                self.schedule_details()
            }
            Message::ContextNewFolder => {
                self.context_menu = None;
                self.show_new_folder()
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
            Message::CloseContext => {
                self.context_menu = None;
                Task::none()
            }
            Message::GridScrolled(y) => {
                self.grid.set_scroll(y);
                Task::none()
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
            Message::NavigationFinished { requested, result } => {
                self.finish_navigation(requested, result)
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
            Message::SearchChanged(value) => self.update_search(value),
            Message::SearchSubmitted => self.submit_search(),
            Message::SearchFinished(result) => {
                if let Err(error) =
                    self.search
                        .complete(&mut self.navigation, &mut self.grid, result)
                {
                    self.status = error;
                }
                Task::none()
            }
            Message::CommandChanged(value) => {
                self.command.change(value);
                Task::none()
            }
            Message::CommandSubmitted => self.submit_command(),
            Message::CommandFinished(result) => self.finish_command(result),
            Message::AnimationFrame(now) => {
                self.animation_now = now;
                Task::none()
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
            Message::Copy => self.copy_selection(),
            Message::Paste => self.paste(),
            Message::ClipboardRead(result) => match result {
                Ok(payload) => {
                    if self.transfers.import_clipboard(payload) {
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

    fn handle_event(&mut self, event: iced::Event, _status: event::Status) -> Task<Message> {
        match event {
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
                if self
                    .grid
                    .move_cursor(position, self.navigation.entries().len())
                {
                    self.refresh_status();
                }
                if self.transfers.active_drag_index().is_some() && self.grid.cursor_outside_window()
                {
                    tasks.push(self.start_external_drag());
                }
                Task::batch(tasks)
            }
            iced::Event::Mouse(mouse::Event::CursorLeft)
                if self.transfers.active_drag_index().is_some() =>
            {
                self.start_external_drag()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if self.grid.start_marquee(
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
            iced::Event::Window(window::Event::CloseRequested) => self.quit(),
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                modifiers,
                text,
                ..
            }) => {
                self.modifiers = modifiers;
                self.handle_key(key, modified_key, modifiers, text.as_deref())
            }
            _ => Task::none(),
        }
    }

    fn handle_key(
        &mut self,
        key: keyboard::Key,
        modified_key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        produced: Option<&str>,
    ) -> Task<Message> {
        let text = produced
            .map(str::to_owned)
            .or_else(|| match &modified_key {
                keyboard::Key::Character(value) => Some(value.to_string()),
                _ => None,
            })
            .or_else(|| match &key {
                keyboard::Key::Character(value) => Some(value.to_string()),
                _ => None,
            });
        let named = match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => InputNamedKey::Escape,
            keyboard::Key::Named(keyboard::key::Named::Enter) => InputNamedKey::Enter,
            keyboard::Key::Named(keyboard::key::Named::Backspace) => InputNamedKey::Backspace,
            keyboard::Key::Named(keyboard::key::Named::Delete) => InputNamedKey::Delete,
            _ => InputNamedKey::Other,
        };
        let intent = self.browser_input.handle(
            InputPress {
                text,
                named,
                control: modifiers.control(),
                alt: modifiers.alt(),
                logo: modifiers.logo(),
            },
            InputContext {
                prompt_active: self.file_operations.prompt_active(),
                prompt_accepts_enter: self.file_operations.prompt_accepts_enter(),
                prompt_uses_yes_no: self.file_operations.prompt_uses_yes_no(),
                busy: self.busy,
                command_output: self.command.output().is_some(),
                visual_active: self.grid.visual_active(),
                selection_count: self.grid.selection_count(),
                has_selection: self.grid.selected_entry().is_some(),
                pending_cut: !self.transfers.pending_cut_paths().is_empty(),
                file_operators_allowed: self.browser_focus == BrowserFocus::Entries,
            },
        );
        self.apply_input_intent(intent)
    }

    fn apply_input_intent(&mut self, intent: InputIntent) -> Task<Message> {
        match intent {
            InputIntent::None => Task::none(),
            InputIntent::PromptCancel => self.update(Message::PromptCancel),
            InputIntent::PromptConfirm => self.update(Message::PromptConfirm),
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
            InputIntent::Back => self.go_back(),
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
            InputIntent::Move(motion, count) => {
                if self.browser_focus == BrowserFocus::Sidebar {
                    self.move_sidebar(motion, count)
                } else {
                    self.move_selection(motion, count)
                }
            }
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
            InputIntent::Activate if self.browser_focus == BrowserFocus::Sidebar => self
                .sidebar_cursor
                .map_or_else(Task::none, |id| self.activate_tree_row(id)),
            InputIntent::Activate => self.activate_selected(),
            InputIntent::Parent => self.go_parent(),
        }
    }

    fn request_navigation(&mut self, navigation: NavigationRequest) -> Task<Message> {
        let requested = navigation.requested().to_path_buf();
        self.navigation_loading = true;
        self.status = format!("Opening {}…", requested.display());
        Task::perform(
            self.operations.run(OperationKind::Navigation, {
                let path = requested.clone();
                move |_| {
                    fs::open_directory(&path)
                        .map(|opened| (opened.canonical_path, opened.entries))
                        .map_err(|error| error.to_string())
                }
            }),
            move |completion| match completion {
                Completion::Finished(result) => Message::NavigationFinished { requested, result },
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn navigate(
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

    fn finish_navigation(
        &mut self,
        requested: PathBuf,
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    ) -> Task<Message> {
        self.navigation_loading = false;
        match self.navigation.complete(&requested, result) {
            NavigationOutcome::Committed { selected } => {
                let selected_paths = selected
                    .iter()
                    .filter_map(|index| self.navigation.entries().get(*index))
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>();
                self.navigation
                    .hide_paths(self.transfers.pending_cut_paths());
                let selected = self
                    .navigation
                    .entries()
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        selected_paths.contains(&entry.path).then_some(index)
                    })
                    .collect::<Vec<_>>();
                self.grid
                    .select_indices(&selected, self.navigation.entries().len());
                self.grid.clear_details();
                self.location_input = self.navigation.current().display().to_string();
                self.grid.reset_scroll();
                self.status.clear();
                Task::batch([self.load_root_if_needed(), self.schedule_details()])
            }
            NavigationOutcome::Failed(error) => {
                self.status = error;
                Task::none()
            }
            NavigationOutcome::Ignored => Task::none(),
        }
    }

    fn go_parent(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let Some(navigation) = self.navigation.parent() else {
            return Task::none();
        };
        self.request_navigation(navigation)
    }

    fn go_back(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let Some(navigation) = self.navigation.back() else {
            return Task::none();
        };
        self.request_navigation(navigation)
    }

    fn go_forward(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        self.cancel_search_state();
        let Some(navigation) = self.navigation.history_forward() else {
            return Task::none();
        };
        self.request_navigation(navigation)
    }

    fn refresh(&mut self, select: Option<PathBuf>) -> Task<Message> {
        let navigation = self.navigation.refresh(select);
        self.request_navigation(navigation)
    }

    fn refresh_selected(&mut self, select: Vec<PathBuf>) -> Task<Message> {
        let navigation = self.navigation.refresh_selected(select);
        self.request_navigation(navigation)
    }

    fn activate_entry(&mut self, index: usize, double: bool) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        if double && self.browser_input.mode() == InputMode::Rename {
            self.cancel_rename();
        }
        if entry.is_directory() {
            if double {
                return Task::none();
            }
            return self.navigate(entry.path, true, None);
        }
        self.grid
            .select_only(Some(index), self.navigation.entries().len());
        if double {
            return self.open_entry(entry);
        }
        self.schedule_details()
    }

    fn activate_selected(&mut self) -> Task<Message> {
        self.grid
            .selected_entry()
            .map_or_else(Task::none, |index| self.open_or_navigate(index))
    }

    fn open_or_navigate(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        if entry.is_directory() {
            self.navigate(entry.path, true, None)
        } else {
            self.open_entry(entry)
        }
    }

    fn finish_entry_press(&mut self, index: usize) -> Task<Message> {
        match self.transfers.release(index) {
            TransferRelease::None => Task::none(),
            TransferRelease::Click(index) => self.activate_entry(index, false),
            TransferRelease::Drop(grabbed_index) => self.finish_drag(grabbed_index),
        }
    }

    fn open_entry(&self, entry: FileEntry) -> Task<Message> {
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

    fn move_selection(&mut self, motion: Motion, count: usize) -> Task<Message> {
        self.grid.move_selection_count(
            motion,
            count,
            self.navigation.entries().len(),
            self.status_height(),
        );
        Task::batch([self.schedule_details(), self.scroll_to_selected()])
    }

    fn scroll_to_selected(&self) -> Task<Message> {
        let Some(index) = self.grid.selected_entry() else {
            return Task::none();
        };
        let y = self.grid.scroll_target(index);
        widget::operation::scroll_to(
            Id::new(GRID_SCROLL_ID),
            scrollable::AbsoluteOffset { x: 0.0, y },
        )
    }

    fn schedule_details(&mut self) -> Task<Message> {
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

    fn load_root_if_needed(&mut self) -> Task<Message> {
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

    fn load_tree_node(&self, id: u64, path: PathBuf) -> Task<Message> {
        let worker_path = path.clone();
        Task::perform(
            self.operations.run(OperationKind::Background, move |_| {
                Ok(fs::read_child_folders(&worker_path))
            }),
            move |completion| match completion {
                Completion::Finished(result) => {
                    Message::TreeLoaded(id, path, result.unwrap_or_default())
                }
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn activate_tree_row(&mut self, id: u64) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
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

    fn move_sidebar(&mut self, motion: Motion, count: usize) -> Task<Message> {
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

    fn begin_search(&mut self) -> Task<Message> {
        self.browser_input.enter(InputMode::Search);
        self.command.close_output();
        self.sync_bottom_bar();
        self.operations.cancel(OperationKind::Search);
        self.search.begin(&self.grid);
        widget::operation::focus(Id::new(SEARCH_ID))
    }

    fn update_search(&mut self, value: String) -> Task<Message> {
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

    fn schedule_recursive_search(&self, root: PathBuf, query: String) -> Task<Message> {
        Task::perform(
            self.operations.run_after(
                OperationKind::Search,
                Duration::from_millis(160),
                move |cancellation| {
                    fs::search_directory(&root, &query, SEARCH_LIMIT, || {
                        cancellation.is_cancelled()
                    })
                    .map_err(|error| error.to_string())
                },
            ),
            |completion| match completion {
                Completion::Finished(result) => Message::SearchFinished(result),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn submit_search(&mut self) -> Task<Message> {
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

    fn cancel_search(&mut self) -> Task<Message> {
        self.operations.cancel(OperationKind::Search);
        self.search.cancel(&mut self.navigation, &mut self.grid);
        self.schedule_details()
    }

    fn cancel_search_state(&mut self) {
        if self.browser_input.mode() == InputMode::Rename {
            self.cancel_rename();
        }
        self.operations.cancel(OperationKind::Search);
        self.search.cancel(&mut self.navigation, &mut self.grid);
        self.browser_input.leave_mode();
    }

    fn repeat_search(&mut self, reverse: bool) -> Task<Message> {
        if self
            .search
            .repeat(&mut self.navigation, &mut self.grid, reverse)
        {
            self.schedule_details()
        } else {
            Task::none()
        }
    }

    fn begin_command(&mut self, prefix: char) -> Task<Message> {
        self.browser_input.enter(InputMode::Command);
        self.command.begin(prefix);
        self.sync_bottom_bar();
        widget::operation::focus(Id::new(COMMAND_ID))
    }

    fn submit_command(&mut self) -> Task<Message> {
        if self.browser_input.mode() != InputMode::Command {
            return Task::none();
        }
        self.browser_input.leave_mode();
        match self.command.submit(self.navigation.current().to_path_buf()) {
            CommandSubmission::None => Task::none(),
            CommandSubmission::Quit => self.quit(),
            CommandSubmission::Updated => {
                self.sync_bottom_bar();
                Task::none()
            }
            CommandSubmission::Execute(execution) => {
                self.busy = true;
                self.status = execution.status();
                let adapter = self.command_adapter;
                Task::perform(
                    self.operations
                        .run(OperationKind::Command, move |_| Ok(execution.run(&adapter))),
                    |completion| match completion {
                        Completion::Finished(result) => Message::CommandFinished(result),
                        Completion::Cancelled => Message::Noop,
                    },
                )
            }
        }
    }

    fn finish_command(&mut self, result: Result<command::Completion, String>) -> Task<Message> {
        self.busy = false;
        let consequences = self.command.complete(result, self.navigation.current());
        self.sync_bottom_bar();
        if let Some(error) = consequences.error {
            self.show_error(error);
            return Task::none();
        }
        if let Some(status) = consequences.status {
            self.status = status;
        }
        if !consequences.refresh {
            return Task::none();
        }
        let tree_refresh = self.invalidate_tree(vec![self.navigation.current().to_path_buf()]);
        if let Some(directory) = consequences.navigate {
            Task::batch([tree_refresh, self.navigate(directory, true, None)])
        } else {
            Task::batch([tree_refresh, self.refresh(None)])
        }
    }

    fn sync_bottom_bar(&mut self) {
        let expanded_height = self
            .command
            .output()
            .map(|output| expanded_bar_height(&output.detail))
            .or_else(|| {
                self.file_operations
                    .expanded_detail()
                    .map(expanded_bar_height)
            });
        if let Some(height) = expanded_height {
            self.expanded_bar_height = height;
        }
        self.animation_now = Instant::now();
        self.output_expansion
            .go_mut(expanded_height.is_some(), self.animation_now);
    }

    fn show_rename(&mut self, index: usize) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(entry) = self.navigation.entries().get(index).cloned() else {
            return Task::none();
        };
        self.file_operations.begin_rename(entry);
        self.browser_input.enter(InputMode::Rename);
        self.command.close_output();
        self.sync_bottom_bar();
        Task::batch([
            widget::operation::focus(Id::new(RENAME_ID)),
            widget::operation::select_all(Id::new(RENAME_ID)),
        ])
    }

    fn rename_selected(&mut self) -> Task<Message> {
        let Some(index) = self.grid.selected_entry() else {
            return Task::none();
        };
        self.show_rename(index)
    }

    fn cancel_rename(&mut self) {
        self.browser_input.leave_mode();
        self.file_operations.cancel();
    }

    fn show_new_folder(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(|session| session.begin_new_folder());
        widget::operation::focus(Id::new(NEW_FOLDER_ID))
    }

    fn submit_rename(&mut self) -> Task<Message> {
        if self.browser_input.mode() != InputMode::Rename {
            return Task::none();
        }
        self.submit_file_operation_name()
    }

    fn submit_file_operation_name(&mut self) -> Task<Message> {
        let Some(work) = self
            .file_operations
            .submit_name(self.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.start_file_operation(work)
    }

    fn show_trash_prompt(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let entries = self.selected_entries();
        if entries.is_empty() {
            return Task::none();
        }
        self.open_file_operation(move |session| {
            session.begin_trash(entries);
        });
        Task::none()
    }

    fn confirm_prompt(&mut self) -> Task<Message> {
        if let Some(work) = self
            .file_operations
            .confirm(self.navigation.current().to_path_buf())
        {
            self.start_file_operation(work)
        } else {
            self.sync_bottom_bar();
            Task::none()
        }
    }

    fn cancel_prompt(&mut self) -> Task<Message> {
        if self.file_operations.cancel() {
            self.sync_bottom_bar();
        }
        Task::none()
    }

    fn prompt_blocks_action(&mut self) -> bool {
        if !self.file_operations.prompt_active() {
            return false;
        }
        if self.file_operations.is_busy() {
            return true;
        }
        let _ = self.cancel_prompt();
        false
    }

    fn open_file_operation(&mut self, open: impl FnOnce(&mut FileOperationSession)) {
        if self.browser_input.mode() == InputMode::Rename {
            self.cancel_rename();
        }
        self.command.close_output();
        open(&mut self.file_operations);
        self.sync_bottom_bar();
    }

    fn start_file_operation(&mut self, work: FileOperationWork) -> Task<Message> {
        self.busy = true;
        let adapter = self.trash_adapter;
        Task::perform(
            self.operations
                .run(OperationKind::Mutation, move |_| Ok(work.run(&adapter))),
            |completion| match completion {
                Completion::Finished(Ok(completion)) => Message::FileOperationFinished(completion),
                Completion::Finished(Err(error)) => Message::OperationError(error),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn finish_file_operation(&mut self, completion: file_operation::Completion) -> Task<Message> {
        self.busy = false;
        let consequences = self.file_operations.complete(completion);
        if consequences.renamed {
            self.browser_input.leave_mode();
        }
        self.sync_bottom_bar();
        if consequences.refresh {
            Task::batch([
                self.invalidate_tree(vec![self.navigation.current().to_path_buf()]),
                self.refresh(consequences.select),
            ])
        } else {
            Task::none()
        }
    }

    fn copy_selection(&mut self) -> Task<Message> {
        let restore_cut = !self.transfers.pending_cut_paths().is_empty();
        let entries = self.selected_entries();
        if let Some(status) = self.transfers.copy(&entries) {
            self.status = status;
            if let (Some(source), Some(payload)) =
                (&self.native_clipboard, self.transfers.clipboard_payload())
                && let Err(error) = ClipboardAdapter::write_clipboard(source, payload)
            {
                self.status = format!("Copied inside PolarExp; system clipboard failed: {error}");
            }
        }
        if restore_cut {
            self.refresh(None)
        } else {
            Task::none()
        }
    }

    fn cut_selection(&mut self) -> Task<Message> {
        let entries = self.selected_entries();
        let Some(status) = self.transfers.cut(&entries) else {
            return Task::none();
        };
        self.status = status;
        if let (Some(source), Some(payload)) =
            (&self.native_clipboard, self.transfers.clipboard_payload())
            && let Err(error) = ClipboardAdapter::write_clipboard(source, payload)
        {
            self.status = format!("Cut inside PolarExp; system clipboard failed: {error}");
        }
        self.navigation
            .hide_paths(self.transfers.pending_cut_paths());
        self.grid.select_only(None, self.navigation.entries().len());
        Task::none()
    }

    fn cancel_cut(&mut self, status: &str) -> Task<Message> {
        let Some(generation) = self.transfers.cancel_cut() else {
            return Task::none();
        };
        if let Some(source) = self.native_clipboard.as_ref() {
            ClipboardAdapter::clear_clipboard(source, generation);
        }
        self.status = status.to_owned();
        self.refresh(None)
    }

    fn paste(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(source) = self.native_clipboard.as_ref() else {
            return self.paste_current();
        };
        match ClipboardAdapter::read_clipboard(source) {
            Ok(completion) => Task::perform(completion, Message::ClipboardRead),
            Err(error) => {
                self.status = error;
                Task::none()
            }
        }
    }

    fn paste_current(&mut self) -> Task<Message> {
        let Some(request) = self
            .transfers
            .paste(self.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        self.start_transfer(request)
    }

    fn finish_drag(&mut self, grabbed_index: usize) -> Task<Message> {
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let Some(destination) = self.drop_destination_at(self.grid.cursor(), false) else {
            return Task::none();
        };
        let Some(request) = TransferWorkflow::request(
            self.navigation.entries(),
            self.grid.selected_indices(),
            grabbed_index,
            destination,
            action,
        ) else {
            return Task::none();
        };
        self.start_transfer(request)
    }

    fn start_external_drag(&mut self) -> Task<Message> {
        let Some(grabbed_index) = self.transfers.active_drag_index() else {
            return Task::none();
        };
        let Some(source) = self.native_dnd.as_ref() else {
            self.transfers.cancel_drag();
            self.status = self.native_dnd_error.clone().unwrap_or_else(|| {
                "External drag-and-drop is not ready yet; try again in a moment".to_owned()
            });
            return Task::none();
        };
        let entries = TransferWorkflow::entries_for_drag(
            self.navigation.entries(),
            self.grid.selected_indices(),
            grabbed_index,
        );
        let Some(preview) = self.drag_preview(&entries) else {
            self.transfers.cancel_drag();
            return Task::none();
        };
        let copy_only = self.modifiers.control();
        let (count, completion) = match self.transfers.start_outgoing(
            source,
            self.navigation.entries(),
            self.grid.selected_indices(),
            grabbed_index,
            copy_only,
            |_| Some(preview),
        ) {
            Ok(started) => started,
            Err(error) => {
                self.transfers.cancel_drag();
                self.status = format!("Could not start external drag-and-drop: {error}");
                return Task::none();
            }
        };
        self.status = if count == 1 {
            "Dragging 1 item outside PolarExp…".to_owned()
        } else {
            format!("Dragging {count} items outside PolarExp…")
        };
        Task::perform(completion, Message::ExternalDragFinished)
    }

    fn quit(&mut self) -> Task<Message> {
        if let Some(source) = self.native_dnd.take() {
            self.transfers.stop(&source);
        } else {
            self.transfers.cancel_drag();
        }
        iced::exit()
    }

    fn drop_destination_at(&self, point: Point, allow_current: bool) -> Option<PathBuf> {
        let rows = flatten_rows(&self.explorer, self.navigation.current());
        match self.grid.drop_zone(
            point,
            self.navigation.entries().len(),
            rows.len(),
            self.status_height(),
            allow_current,
        )? {
            DropZone::Sidebar(index) => rows.get(index).map(|row| row.path.clone()),
            DropZone::Entry(index) => self
                .navigation
                .entries()
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| entry.path.clone()),
            DropZone::Current => Some(self.navigation.current().to_path_buf()),
        }
    }

    fn drop_highlight_path(&self) -> Option<PathBuf> {
        if let Some(destination) = self.transfers.native_hover_destination() {
            return destination.map(Path::to_path_buf);
        }
        let grabbed_index = self.transfers.active_drag_index()?;
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let destination = self.drop_destination_at(self.grid.cursor(), false)?;
        TransferWorkflow::request(
            self.navigation.entries(),
            self.grid.selected_indices(),
            grabbed_index,
            destination,
            action,
        )
        .map(|request| request.destination)
    }

    fn handle_native_dnd_event(&mut self, event: TransferEvent) -> Task<Message> {
        let resolved_destination = match &event {
            TransferEvent::Hover { position, .. } => self.drop_destination_at(*position, true),
            _ => None,
        };
        let Some(source) = self.native_dnd.as_ref() else {
            self.show_error("Drag-and-drop adapter is unavailable".to_owned());
            return Task::none();
        };
        match self
            .transfers
            .handle_native(source, event, move |_, _| resolved_destination.clone())
        {
            NativeUpdate::None => Task::none(),
            NativeUpdate::Status(status) => {
                self.status_notice = None;
                if status.is_empty() {
                    self.refresh_status();
                } else {
                    self.status = status;
                }
                Task::none()
            }
            NativeUpdate::Notice(message) => {
                self.status = message.clone();
                self.status_notice = Some(message);
                Task::none()
            }
            NativeUpdate::Start(request) => self.start_transfer(request),
            NativeUpdate::Error(error) => {
                self.show_error(error);
                Task::none()
            }
            NativeUpdate::ClipboardLost(true) => {
                self.status = "Cut restored after clipboard ownership changed".to_owned();
                self.refresh(None)
            }
            NativeUpdate::ClipboardLost(false) => Task::none(),
        }
    }

    fn start_transfer(&mut self, request: TransferRequest) -> Task<Message> {
        self.busy = true;
        let operation = request.clone();
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                Ok(fs::transfer_entries(
                    &operation.paths,
                    &operation.destination,
                    operation.action,
                ))
            }),
            move |completion| match completion {
                Completion::Finished(result) => Message::TransferFinished {
                    request,
                    report: result.unwrap_or_else(|error| fs::TransferReport {
                        completed: Vec::new(),
                        failures: vec![fs::TransferFailure {
                            source: PathBuf::new(),
                            error,
                        }],
                    }),
                },
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn finish_transfer(
        &mut self,
        request: TransferRequest,
        report: fs::TransferReport,
    ) -> Task<Message> {
        self.busy = false;
        let adapter = self
            .native_dnd
            .as_ref()
            .map(|source| source as &dyn Adapter);
        let consequences =
            self.transfers
                .finish_transfer(adapter, &request, &report, self.navigation.current());
        self.apply_transfer_consequences(consequences)
    }

    fn apply_transfer_consequences(
        &mut self,
        consequences: crate::transfer::Consequences,
    ) -> Task<Message> {
        if let Some(error) = consequences.error {
            self.show_error(error);
        } else if let Some(status) = consequences.status {
            self.status = status;
        } else {
            self.refresh_status();
        }
        let tree = if consequences.changed_folders.is_empty() {
            Task::none()
        } else {
            self.invalidate_tree(consequences.changed_folders)
        };
        let refresh = if consequences.refresh {
            self.refresh_selected(consequences.select)
        } else {
            Task::none()
        };
        Task::batch([tree, refresh])
    }

    fn selected_entries(&self) -> Vec<FileEntry> {
        self.grid.selected_items(self.navigation.entries())
    }

    fn invalidate_tree(&mut self, changed_folders: Vec<PathBuf>) -> Task<Message> {
        let reloads = tree::invalidate_tree_folders(&mut self.explorer.roots, &changed_folders);
        Task::batch(
            reloads
                .into_iter()
                .map(|(id, path)| self.load_tree_node(id, path)),
        )
    }

    fn mutations_allowed(&self) -> bool {
        !self.busy && !self.navigation_loading && !self.search.is_recursive()
    }

    #[cfg(test)]
    fn delete_operator_pending(&self) -> bool {
        self.browser_input.delete_pending()
    }

    fn show_error(&mut self, message: String) {
        self.busy = false;
        self.open_file_operation(move |session| session.show_error(message));
    }

    fn refresh_status(&mut self) {
        if self.browser_input.pending_sequence().is_some() {
            return;
        }
        if let Some(status) = self.transfers.pending_cut_status() {
            self.status = status;
            return;
        }
        self.status = if self.grid.selection_count() > 1 {
            format!(
                "{} selected  •  {}",
                self.grid.selection_count(),
                self.navigation.current().display()
            )
        } else if let Some(entry) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
        {
            let name = fs::display_name(&entry.name);
            match self.grid.details() {
                Some(details) => format!("{name}  •  {details}"),
                None => format!("{name}  •  Loading details…"),
            }
        } else {
            format!(
                "{} items  •  {}",
                self.navigation.entries().len(),
                self.navigation.current().display()
            )
        };
    }

    fn status_height(&self) -> f32 {
        self.output_expansion.interpolate(
            STATUS_HEIGHT,
            self.expanded_bar_height,
            self.animation_now,
        )
    }

    fn view(&self) -> Element<'_, Message> {
        let base = self.layout();
        let mut layers: Vec<Element<'_, Message>> = vec![base];
        if let Some(preview) = self.drag_preview_view() {
            layers.push(preview);
        }
        if let Some((_, point)) = self.context_menu {
            layers.push(self.context_menu_view(point));
        }
        stack(layers).width(Fill).height(Fill).into()
    }

    fn drag_preview_view(&self) -> Option<Element<'_, Message>> {
        let grabbed_index = self.transfers.active_drag_index()?;
        let entries = TransferWorkflow::entries_for_drag(
            self.navigation.entries(),
            self.grid.selected_indices(),
            grabbed_index,
        );
        let preview = self.drag_preview(&entries)?;
        let preview_bytes = native_dnd::preview_svg(preview).ok()?;
        let preview = svg(svg::Handle::from_memory(preview_bytes))
            .width(native_dnd::ICON_SIZE as f32)
            .height(native_dnd::ICON_SIZE as f32);

        let origin = self.grid.drag_preview_origin();
        Some(
            pin(preview)
                .x(origin.x.max(0.0))
                .y(origin.y.max(0.0))
                .into(),
        )
    }

    fn drag_preview(&self, entries: &[FileEntry]) -> Option<TransferPreview> {
        let first = entries.first()?;
        let icon_kind = entry_icon_kind(first);
        let palette = self.iced_theme().palette();
        let background = palette.background;
        let icon_color = self.entry_icon_color(icon_kind);
        let accent = self.accent_color();
        let badge_text = self.selection_text_color();
        Some(TransferPreview {
            icon: entry_icon_asset(icon_kind),
            count: entries.len(),
            copy: self.modifiers.control(),
            background: rgba(background, 0.92),
            icon_color: rgba(icon_color, 1.0),
            accent: rgba(accent, 1.0),
            badge_text: rgba(badge_text, 1.0),
        })
    }

    fn layout(&self) -> Element<'_, Message> {
        row![self.sidebar(), self.browser()]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut rows = Column::new().spacing(0);
        for tree_row in flatten_rows(&self.explorer, self.navigation.current()) {
            rows = rows.push(self.tree_row(tree_row));
        }
        let content = column![
            container(
                text("Locations")
                    .font(UI_FONT_SEMIBOLD)
                    .size(12)
                    .line_height(iced::Pixels(14.0))
                    .color(with_alpha(self.iced_theme().palette().text, 0.68)),
            )
            .height(30)
            .center_y(30),
            scrollable(container(rows).padding(Padding {
                top: LIST_VIEW_TOP_INSET,
                ..Padding::ZERO
            }))
            .height(Fill),
        ];
        container(content)
            .width(SIDEBAR_WIDTH)
            .height(Fill)
            .padding(Padding::from([8, 12]))
            .style(sidebar_style)
            .into()
    }

    fn tree_row(&self, tree_row: TreeRow) -> Element<'_, Message> {
        let drop_target = self.drop_highlight_path().as_deref() == Some(tree_row.path.as_path());
        let mut line = Row::new()
            .spacing(6)
            .height(Fill)
            .align_y(Alignment::Center)
            .padding(0);
        line = line.push(
            Space::new()
                .width((tree_row.depth as f32 * 16.0) + 5.0)
                .height(1),
        );
        let (icon, icon_color) = match tree_row.kind {
            state::NodeKind::Computer => (
                include_bytes!("../ui/icons/computer.svg").as_slice(),
                self.secondary_text_color(),
            ),
            state::NodeKind::Drive => (
                include_bytes!("../ui/icons/drive.svg").as_slice(),
                self.secondary_text_color(),
            ),
            state::NodeKind::Folder => (
                include_bytes!("../ui/icons/folder.svg").as_slice(),
                self.accent_color(),
            ),
        };
        let selected = tree_row.selected;
        let focused =
            self.browser_focus == BrowserFocus::Sidebar && self.sidebar_cursor == Some(tree_row.id);
        let label_color = if selected || focused {
            self.selection_text_color()
        } else {
            self.iced_theme().palette().text
        };
        if tree_row.loading {
            line = line.push(self.spinner(17.0));
        } else {
            line = line.push(themed_svg(icon, 17.0, icon_color));
        }
        line = line.push(
            text(tree_row.label)
                .size(13)
                .line_height(iced::Pixels(16.0))
                .color(label_color)
                .width(Fill)
                .height(Fill)
                .align_y(Alignment::Center),
        );
        button(line)
            .on_press(Message::TreeRow(tree_row.id))
            .width(Fill)
            .height(32)
            .padding(0)
            .style(move |theme, status| {
                tree_button_style(theme, status, selected || focused, drop_target)
            })
            .into()
    }

    fn browser(&self) -> Element<'_, Message> {
        column![
            self.toolbar(),
            rule::horizontal(1),
            self.grid_body(),
            self.status_bar(),
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let parent = toolbar_button(
            include_bytes!("../ui/icons/up.svg"),
            "Parent folder",
            self.navigation.current().parent().is_some(),
            Message::Parent,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
        );
        let back = toolbar_button(
            include_bytes!("../ui/icons/back.svg"),
            "Back",
            self.navigation.can_go_back(),
            Message::Back,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
        );
        let forward = toolbar_button(
            include_bytes!("../ui/icons/forward.svg"),
            "Forward",
            self.navigation.can_go_forward(),
            Message::Forward,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
        );
        let location_input = text_input("Location", &self.location_input)
            .id(Id::new(LOCATION_ID))
            .on_input(Message::LocationChanged)
            .on_submit(Message::LocationSubmitted)
            .on_paste(Message::LocationChanged)
            .font(UI_FONT)
            .padding(Padding::from([0, 10]))
            .size(14)
            .line_height(iced::Pixels(17.0))
            .style(flat_input_style)
            .width(Fill);
        let location_input = container(location_input)
            .width(Fill)
            .height(33)
            .center_y(33);
        let accent = self.accent_color();
        let location = column![
            location_input,
            container(Space::new().width(Fill).height(1))
                .width(Fill)
                .height(1)
                .style(move |_| solid_background_style(accent)),
        ]
        .spacing(0)
        .width(Fill)
        .height(34);
        let location = mouse_area(location).on_press(Message::LocationFocused);
        container(
            row![parent, back, forward, location]
                .spacing(4)
                .align_y(Alignment::Center),
        )
        .height(TOOLBAR_HEIGHT)
        .padding(Padding::from([6, CONTENT_GUTTER as u16]))
        .style(browser_background_style)
        .into()
    }

    fn grid_body(&self) -> Element<'_, Message> {
        let visible = self
            .grid
            .visible_range(self.navigation.entries().len(), self.status_height());
        let mut grid = Grid::with_capacity(visible.last_index.saturating_sub(visible.first_index))
            .columns(visible.columns)
            .height(widget::grid::aspect_ratio(
                visible.column_width,
                TILE_ROW_HEIGHT,
            ))
            .spacing(0);
        for index in visible.first_index..visible.last_index {
            grid = grid.push(self.file_tile(index));
        }
        let top = Space::new().width(Fill).height(visible.top_space);
        let bottom = Space::new().width(Fill).height(visible.bottom_space);
        let content = column![top, grid, bottom];
        let scroll = scrollable(content)
            .id(Id::new(GRID_SCROLL_ID))
            .on_scroll(|viewport| Message::GridScrolled(viewport.absolute_offset().y))
            .width(Fill)
            .height(Fill);
        let area: Element<'_, Message> = mouse_area(container(scroll).padding(Padding {
            top: CONTENT_GUTTER + LIST_VIEW_TOP_INSET,
            right: CONTENT_GUTTER,
            bottom: CONTENT_GUTTER,
            left: CONTENT_GUTTER,
        }))
        .on_move(Message::GridPointerMoved)
        .into();
        let content: Element<'_, Message> =
            if let Some(bounds) = self.grid.marquee_bounds(self.status_height()) {
                let selection = container(Space::new())
                    .width(bounds.width)
                    .height(bounds.height)
                    .style(move |_| marquee_style(self.accent_color()));
                let overlay = column![
                    Space::new().height(bounds.y),
                    row![
                        Space::new().width(bounds.x),
                        selection,
                        Space::new().width(Fill)
                    ]
                    .height(bounds.height),
                    Space::new().height(Fill),
                ]
                .width(Fill)
                .height(Fill);
                stack![area, overlay].into()
            } else {
                area
            };
        let current_drop_target =
            self.drop_highlight_path().as_deref() == Some(self.navigation.current());
        container(content)
            .width(Fill)
            .height(Fill)
            .clip(true)
            .style(move |theme| grid_background_style(theme, current_drop_target))
            .into()
    }

    fn file_tile(&self, index: usize) -> Element<'_, Message> {
        let entry = &self.navigation.entries()[index];
        let selected = self.grid.is_selected(index);
        let hovered = self.grid.hovered() == Some(index);
        let drop_target = entry.is_directory()
            && self.drop_highlight_path().as_deref() == Some(entry.path.as_path());
        let icon_kind = entry_icon_kind(entry);
        let icon = themed_svg(
            entry_icon_asset(icon_kind),
            48.0,
            self.entry_icon_color(icon_kind),
        );
        let label_color = if selected {
            self.selection_text_color()
        } else {
            self.iced_theme().palette().text
        };
        let content = column![
            container(icon)
                .width(Fill)
                .height(48)
                .center_x(Fill)
                .center_y(48),
            text(fs::display_name(&entry.name))
                .font(UI_FONT)
                .size(13)
                .line_height(iced::Pixels(16.0))
                .color(label_color)
                .width(Fill)
                .height(34)
                .wrapping(iced::advanced::text::Wrapping::WordOrGlyph)
                .align_x(Alignment::Center),
        ]
        .spacing(6)
        .align_x(Alignment::Center);
        let tile = container(content)
            .width(TILE_WIDTH)
            .height(108)
            .padding(Padding {
                top: 10.0,
                right: 7.0,
                bottom: 6.0,
                left: 7.0,
            })
            .clip(true)
            .style(move |theme| tile_style(theme, selected, hovered, drop_target));
        mouse_area(container(tile).width(Fill).center_x(Fill))
            .on_press(Message::EntryPressed(index))
            .on_release(Message::EntryReleased(index))
            .on_double_click(Message::EntryDoubleClicked(index))
            .on_right_press(Message::EntryContext(index))
            .on_enter(Message::EntryHovered(index))
            .on_exit(Message::EntryUnhovered(index))
            .into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let height = self.output_expansion.interpolate(
            STATUS_HEIGHT,
            self.expanded_bar_height,
            self.animation_now,
        );
        let content: Element<'_, Message> = if let Some(output) = self.command.output() {
            let header = row![
                text(&output.summary)
                    .font(MONO_FONT_SEMIBOLD)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .width(Fill),
                text("Esc close")
                    .font(MONO_FONT)
                    .size(11)
                    .color(self.secondary_text_color()),
            ]
            .height(29)
            .align_y(Alignment::Center);
            let output = column![
                header,
                scrollable(
                    text(&output.detail)
                        .font(MONO_FONT)
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .color(with_alpha(self.iced_theme().palette().text, 0.84))
                        .width(Fill)
                        .wrapping(iced::advanced::text::Wrapping::WordOrGlyph),
                )
                .width(Fill)
                .height(Fill),
            ]
            .spacing(1)
            .width(Fill)
            .height(Fill);
            container(output)
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 1.0,
                    right: CONTENT_GUTTER,
                    bottom: 9.0,
                    left: CONTENT_GUTTER,
                })
                .into()
        } else if self.file_operations.prompt_active() {
            self.prompt_bar()
        } else {
            let status: Element<'_, Message> = match self.browser_input.mode() {
                InputMode::Search => {
                    let prefix = if self.search.is_recursive() {
                        "//"
                    } else {
                        "/"
                    };
                    row![
                        text(prefix)
                            .size(12)
                            .line_height(iced::Pixels(15.0))
                            .color(self.accent_color()),
                        text_input("", self.search.query())
                            .id(Id::new(SEARCH_ID))
                            .on_input(Message::SearchChanged)
                            .on_submit(Message::SearchSubmitted)
                            .font(MONO_FONT)
                            .size(12)
                            .line_height(iced::Pixels(15.0))
                            .padding(0)
                            .style(status_input_style)
                            .width(Fill),
                        self.search_count_view(),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
                }
                InputMode::Command => row![
                    text(self.command.prefix().unwrap_or(':').to_string())
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .color(self.accent_color()),
                    text_input("", self.command.text())
                        .id(Id::new(COMMAND_ID))
                        .on_input(Message::CommandChanged)
                        .on_submit(Message::CommandSubmitted)
                        .font(MONO_FONT)
                        .size(12)
                        .line_height(iced::Pixels(15.0))
                        .padding(0)
                        .style(status_input_style)
                        .width(Fill),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
                InputMode::Rename => {
                    if let FileOperationView::Rename { value, error } = self.file_operations.view()
                    {
                        let feedback: Element<'_, Message> = if self.busy {
                            self.spinner(13.0).into()
                        } else if error.is_empty() {
                            text("Enter save  ·  Esc cancel")
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.secondary_text_color())
                                .into()
                        } else {
                            text(error)
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.iced_theme().palette().danger)
                                .into()
                        };
                        row![
                            text("rename")
                                .font(MONO_FONT)
                                .size(11)
                                .line_height(iced::Pixels(13.0))
                                .color(self.accent_color()),
                            text_input("", value)
                                .id(Id::new(RENAME_ID))
                                .on_input_maybe((!self.busy).then_some(Message::RenameChanged))
                                .on_submit_maybe((!self.busy).then_some(Message::RenameSubmitted))
                                .font(MONO_FONT)
                                .size(12)
                                .line_height(iced::Pixels(15.0))
                                .padding(0)
                                .style(status_input_style)
                                .width(Fill),
                            feedback,
                        ]
                        .spacing(7)
                        .align_y(Alignment::Center)
                        .into()
                    } else {
                        Space::new().into()
                    }
                }
                _ => {
                    let indicator: Element<'_, Message> = if self.busy || self.navigation_loading {
                        self.spinner(13.0).into()
                    } else {
                        Space::new().width(0).into()
                    };
                    row![
                        indicator,
                        text(self.status_notice.as_deref().unwrap_or(&self.status))
                            .size(11)
                            .line_height(iced::Pixels(13.0))
                            .color(if self.status_notice.is_some() {
                                self.iced_theme().palette().danger
                            } else {
                                self.secondary_text_color()
                            })
                            .width(Fill),
                    ]
                    .spacing(if self.busy || self.navigation_loading {
                        7
                    } else {
                        0
                    })
                    .align_y(Alignment::Center)
                    .into()
                }
            };
            compact_status_line(status)
        };
        container(content)
            .width(Fill)
            .height(Length::Fixed(height))
            .clip(true)
            .style(status_background_style)
            .into()
    }

    fn prompt_bar(&self) -> Element<'_, Message> {
        match self.file_operations.view() {
            FileOperationView::NewFolder { value, error } => {
                let feedback: Element<'_, Message> = if self.busy {
                    self.spinner(13.0).into()
                } else if error.is_empty() {
                    text("Enter create  ·  Esc cancel")
                        .font(MONO_FONT)
                        .size(11)
                        .color(self.secondary_text_color())
                        .into()
                } else {
                    text(error)
                        .size(11)
                        .line_height(iced::Pixels(13.0))
                        .color(self.iced_theme().palette().danger)
                        .into()
                };
                compact_status_line(
                    row![
                        text("new folder")
                            .font(MONO_FONT)
                            .size(11)
                            .line_height(iced::Pixels(13.0))
                            .color(self.accent_color()),
                        text_input("", value)
                            .id(Id::new(NEW_FOLDER_ID))
                            .on_input_maybe((!self.busy).then_some(Message::PromptInputChanged))
                            .on_submit_maybe((!self.busy).then_some(Message::PromptSubmit))
                            .font(MONO_FONT)
                            .size(12)
                            .line_height(iced::Pixels(15.0))
                            .padding(0)
                            .style(status_input_style)
                            .width(Fill),
                        feedback,
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
            }
            FileOperationView::Trash { message } => compact_status_line(
                row![
                    text("trash")
                        .font(MONO_FONT)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                    text(message)
                        .size(11)
                        .line_height(iced::Pixels(13.0))
                        .width(Fill),
                    text("Y/n")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ),
            FileOperationView::PermanentDelete { message, detail } => {
                let header = row![
                    text("delete permanently")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                    text(message).size(11).width(Fill),
                    text("Y/n")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                ]
                .spacing(8)
                .height(25)
                .align_y(Alignment::Center);
                container(
                    column![
                        header,
                        scrollable(
                            text(detail)
                                .font(MONO_FONT)
                                .size(11)
                                .line_height(iced::Pixels(14.0))
                                .color(self.secondary_text_color())
                                .width(Fill),
                        )
                        .width(Fill)
                        .height(Fill),
                    ]
                    .spacing(2),
                )
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 1.0,
                    right: CONTENT_GUTTER,
                    bottom: 7.0,
                    left: CONTENT_GUTTER,
                })
                .into()
            }
            FileOperationView::Error { message } => {
                let header = row![
                    text("error")
                        .font(MONO_FONT_SEMIBOLD)
                        .size(11)
                        .color(self.iced_theme().palette().danger),
                    Space::new().width(Fill),
                    text("Esc close")
                        .font(MONO_FONT)
                        .size(11)
                        .color(self.secondary_text_color()),
                ]
                .height(25)
                .align_y(Alignment::Center);
                container(
                    column![
                        header,
                        scrollable(
                            text(message)
                                .font(MONO_FONT)
                                .size(11)
                                .line_height(iced::Pixels(14.0))
                                .color(self.secondary_text_color())
                                .width(Fill),
                        )
                        .width(Fill)
                        .height(Fill),
                    ]
                    .spacing(2),
                )
                .width(Fill)
                .height(Fill)
                .padding(Padding {
                    top: 1.0,
                    right: CONTENT_GUTTER,
                    bottom: 7.0,
                    left: CONTENT_GUTTER,
                })
                .into()
            }
            FileOperationView::Idle | FileOperationView::Rename { .. } => Space::new().into(),
        }
    }

    fn search_count_view(&self) -> Element<'_, Message> {
        if !self.search.is_recursive() || self.search.query().is_empty() {
            return Space::new().into();
        }
        if self.search.is_loading() {
            return row![Space::new().width(Fill), self.spinner(13.0)]
                .width(108)
                .align_y(Alignment::Center)
                .into();
        }
        let label = if self.search.is_truncated() {
            "1000+ matches".to_owned()
        } else {
            format!("{} matches", self.navigation.entries().len())
        };
        text(label)
            .size(11)
            .line_height(iced::Pixels(13.0))
            .color(self.secondary_text_color())
            .into()
    }

    fn context_menu_view(&self, point: Point) -> Element<'_, Message> {
        let menu = container(column![
            button(text("New Folder").size(13))
                .on_press(Message::ContextNewFolder)
                .style(button::text)
                .width(Fill),
            button(text("Rename").size(13))
                .on_press(Message::ContextRename)
                .style(button::text)
                .width(Fill),
            button(text("Move to Trash").size(13))
                .on_press(Message::ContextTrash)
                .style(button::text)
                .width(Fill),
        ])
        .width(160)
        .padding(5)
        .style(menu_style);
        let overlay =
            mouse_area(container("").width(Fill).height(Fill)).on_press(Message::CloseContext);
        stack![overlay, pin(menu).x(point.x).y(point.y)].into()
    }

    fn accent_color(&self) -> Color {
        self.accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent)
    }

    fn secondary_text_color(&self) -> Color {
        let mut color = self.iced_theme().palette().text;
        color.a = 0.62;
        color
    }

    fn selection_text_color(&self) -> Color {
        self.accent
            .as_ref()
            .and_then(|colors| colors.selection_foreground)
            .unwrap_or(self.iced_theme().palette().text)
    }

    fn entry_icon_color(&self, kind: EntryIconKind) -> Color {
        let palette = self.iced_theme().palette();
        match kind {
            EntryIconKind::Folder | EntryIconKind::Code => palette.primary,
            EntryIconKind::Image | EntryIconKind::Spreadsheet => palette.success,
            EntryIconKind::Pdf => palette.danger,
            EntryIconKind::Archive | EntryIconKind::Presentation => palette.warning,
            EntryIconKind::Audio | EntryIconKind::Video => Color::from_rgb8(164, 112, 218),
            EntryIconKind::Document | EntryIconKind::Generic => self.secondary_text_color(),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(source) = self.native_dnd.take() {
            self.transfers.stop(&source);
        }
    }
}

fn compact_status_line<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .center_y(Fill)
        .padding(Padding::from([0, CONTENT_GUTTER as u16]))
        .into()
}

fn entry_icon_kind(entry: &FileEntry) -> EntryIconKind {
    if entry.is_directory() {
        return EntryIconKind::Folder;
    }

    let name = entry.name.to_string_lossy().to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "makefile"
            | "dockerfile"
            | "justfile"
            | "cmakelists.txt"
            | "meson.build"
            | "build.gradle"
            | "cargo.lock"
            | "package-lock.json"
    ) {
        return EntryIconKind::Code;
    }
    if matches!(
        name.as_str(),
        "readme" | "license" | "copying" | "changelog" | "authors"
    ) {
        return EntryIconKind::Document;
    }

    let extension = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some(
            "rs" | "go" | "py" | "js" | "jsx" | "ts" | "tsx" | "c" | "h" | "cc" | "cpp" | "hpp"
            | "java" | "kt" | "kts" | "swift" | "rb" | "php" | "sh" | "bash" | "zsh" | "fish"
            | "html" | "htm" | "css" | "scss" | "sass" | "less" | "vue" | "svelte" | "sql" | "json"
            | "jsonc" | "yaml" | "yml" | "toml" | "xml" | "ini" | "conf" | "env" | "lock",
        ) => EntryIconKind::Code,
        Some("pdf") => EntryIconKind::Pdf,
        Some(
            "txt" | "md" | "markdown" | "rtf" | "doc" | "docx" | "odt" | "epub" | "tex" | "log",
        ) => EntryIconKind::Document,
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tif" | "tiff" | "avif"
            | "heic" | "ico",
        ) => EntryIconKind::Image,
        Some("mp3" | "wav" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "wma") => {
            EntryIconKind::Audio
        }
        Some("mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "wmv") => EntryIconKind::Video,
        Some(
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "txz" | "7z" | "rar" | "zst" | "deb"
            | "rpm" | "apk",
        ) => EntryIconKind::Archive,
        Some("csv" | "xls" | "xlsx" | "ods") => EntryIconKind::Spreadsheet,
        Some("ppt" | "pptx" | "odp") => EntryIconKind::Presentation,
        _ => EntryIconKind::Generic,
    }
}

fn entry_icon_asset(kind: EntryIconKind) -> &'static [u8] {
    match kind {
        EntryIconKind::Folder => include_bytes!("../ui/icons/folder.svg"),
        EntryIconKind::Generic => include_bytes!("../ui/icons/file.svg"),
        EntryIconKind::Code => include_bytes!("../ui/icons/file-code.svg"),
        EntryIconKind::Document => include_bytes!("../ui/icons/file-document.svg"),
        EntryIconKind::Pdf => include_bytes!("../ui/icons/file-pdf.svg"),
        EntryIconKind::Image => include_bytes!("../ui/icons/file-image.svg"),
        EntryIconKind::Audio => include_bytes!("../ui/icons/file-audio.svg"),
        EntryIconKind::Video => include_bytes!("../ui/icons/file-video.svg"),
        EntryIconKind::Archive => include_bytes!("../ui/icons/file-archive.svg"),
        EntryIconKind::Spreadsheet => include_bytes!("../ui/icons/file-spreadsheet.svg"),
        EntryIconKind::Presentation => include_bytes!("../ui/icons/file-presentation.svg"),
    }
}

fn find_window_after_delay() -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_millis(100)).await;
        },
        |()| Message::FindWindow,
    )
}

fn clears_status_notice(event: &iced::Event) -> bool {
    matches!(
        event,
        iced::Event::Keyboard(keyboard::Event::KeyPressed { .. })
            | iced::Event::Mouse(mouse::Event::ButtonPressed(_))
    )
}

fn rgba(color: Color, alpha: f32) -> [u8; 4] {
    [
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn expanded_bar_height(detail: &str) -> f32 {
    let lines = detail.lines().count().max(1) as f32;
    (40.0 + lines * 16.0).clamp(56.0, 280.0)
}

fn toolbar_button(
    icon: &'static [u8],
    _accessible_label: &'static str,
    enabled: bool,
    message: Message,
    color: Color,
    background: Color,
) -> Button<'static, Message> {
    let icon = themed_svg(
        icon,
        16.0,
        blend_colors(background, color, if enabled { 0.98 } else { 0.30 }),
    );
    button(icon)
        .on_press_maybe(enabled.then_some(message))
        .width(26)
        .height(30)
        .padding(0)
        .style(toolbar_button_style)
}

fn sidebar_style(theme: &Theme) -> container::Style {
    let background = theme.palette().background;
    let alternate = if background.r > 0.5 {
        Color::from_rgb8(240, 240, 240)
    } else {
        Color::from_rgb8(44, 44, 44)
    };
    // Iced makes differences between transparent stops much more pronounced
    // than Slint. Keep the 112 degree color progression, but use one alpha for
    // the whole surface so the wallpaper remains visible without a hard wedge.
    let gradient = gradient::Linear::new(112.0_f32.to_radians())
        .add_stop(0.0, with_alpha(alternate, 0.84))
        .add_stop(0.58, with_alpha(alternate, 0.84))
        .add_stop(1.0, with_alpha(background, 0.84));
    container::Style {
        background: Some(Background::from(gradient)),
        ..container::Style::default()
    }
}

fn browser_background_style(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().background)
}

fn grid_background_style(theme: &Theme, drop_target: bool) -> container::Style {
    let mut style = browser_background_style(theme);
    if drop_target {
        style.border = Border {
            width: 2.0,
            color: theme.palette().primary,
            radius: 4.0.into(),
        };
    }
    style
}

fn status_background_style(theme: &Theme) -> container::Style {
    container::Style::default().background(lighter(theme.palette().background, 16))
}

fn tree_button_style(
    theme: &Theme,
    status: button::Status,
    selected: bool,
    drop_target: bool,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if drop_target {
            Some(Background::Color(with_alpha(theme.palette().primary, 0.36)))
        } else if selected {
            Some(Background::Color(with_alpha(theme.palette().primary, 0.22)))
        } else if hovered {
            Some(Background::Color(with_alpha(theme.palette().text, 0.06)))
        } else {
            None
        },
        text_color: theme.palette().text,
        border: Border {
            width: if drop_target { 1.0 } else { 0.0 },
            color: theme.palette().primary,
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

fn tile_style(theme: &Theme, selected: bool, hovered: bool, drop_target: bool) -> container::Style {
    let mut style = container::Style::default();
    if drop_target {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.30)));
    } else if selected {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.45)));
    } else if hovered {
        style.background = Some(Background::Color(lighter(theme.palette().background, 16)));
    }
    style.border = Border {
        width: if drop_target { 2.0 } else { 0.0 },
        color: theme.palette().primary,
        radius: 7.0.into(),
    };
    style
}

fn marquee_style(accent: Color) -> container::Style {
    container::Style::default()
        .background(with_alpha(accent, 0.18))
        .border(Border {
            width: 1.0,
            color: accent,
            ..Border::default()
        })
}

fn themed_svg(icon: &'static [u8], size: f32, color: Color) -> widget::Svg<'static> {
    svg(svg::Handle::from_memory(icon))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
}

fn toolbar_button_style(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(Background::Color(lighter(theme.palette().background, 16)))
        } else {
            None
        },
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn solid_background_style(color: Color) -> container::Style {
    container::Style::default().background(color)
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

fn lighter(color: Color, amount: u8) -> Color {
    let amount = amount as f32 / 255.0;
    Color::from_rgba(
        (color.r + amount).min(1.0),
        (color.g + amount).min(1.0),
        (color.b + amount).min(1.0),
        color.a,
    )
}

fn blend_colors(background: Color, foreground: Color, opacity: f32) -> Color {
    Color::from_rgb(
        background.r + (foreground.r - background.r) * opacity,
        background.g + (foreground.g - background.g) * opacity,
        background.b + (foreground.b - background.b) * opacity,
    )
}

fn flat_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    style.background = Background::Color(Color::TRANSPARENT);
    style.border = Border {
        width: 0.0,
        radius: 4.0.into(),
        color: Color::TRANSPARENT,
    };
    style
}

fn status_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = flat_input_style(theme, status);
    style.background = Background::Color(Color::TRANSPARENT);
    style
}

fn menu_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background)),
        border: Border {
            width: 1.0,
            radius: 6.0.into(),
            color: theme.extended_palette().background.strong.color,
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.35),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..container::Style::default()
    }
}
