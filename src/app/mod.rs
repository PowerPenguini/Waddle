mod settings;
mod shell;
mod state;
mod tree;

#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use gio::prelude::*;
use iced::time::Instant;
use iced::{
    Alignment, Animation, Background, Border, Color, Element, Fill, Font, Length, Padding, Point,
    Rectangle, Shadow, Size, Subscription, Task, Theme, Vector,
    animation::Easing,
    application, event, gradient, keyboard, mouse, system, time,
    widget::{
        self, Button, Column, Grid, Id, Row, Space, button, column, container, mouse_area, opaque,
        pin, row, rule, scrollable, stack, svg, text, text_input,
    },
    window,
};
use tokio::sync::Semaphore;

use crate::{fs, theme};
use fs::{FileEntry, PreviewData};
use shell::{CommandMode, ShellReport};
use state::{ExplorerState, NavigationKind, PendingName, PendingNavigation, ViewMode};
use tree::{TreeRow, find_node_mut, flatten_rows, mounted_roots};

const SIDEBAR_WIDTH: f32 = 220.0;
const TOOLBAR_HEIGHT: f32 = 46.0;
const STATUS_HEIGHT: f32 = 25.0;
const TILE_WIDTH: f32 = 104.0;
const TILE_PITCH: f32 = 112.0;
const TILE_ROW_HEIGHT: f32 = 116.0;
const CONTENT_GUTTER: f32 = 14.0;
const LIST_VIEW_TOP_INSET: f32 = 6.0;
const TOOLBAR_DIVIDER_HEIGHT: f32 = 1.0;
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
const DIALOG_ID: &str = "dialog-name";
const GRID_SCROLL_ID: &str = "grid-scroll";

#[derive(Clone, Debug)]
enum DialogState {
    None,
    Name {
        title: String,
        value: String,
        error: String,
    },
    Trash {
        message: String,
    },
    PermanentDelete {
        message: String,
        detail: String,
    },
    Error {
        message: String,
    },
}

impl DialogState {
    fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputMode {
    Browser,
    Location,
    Search,
    Command(char),
}

#[derive(Clone, Debug, Default)]
struct PreviewView {
    title: String,
    metadata: String,
    text: String,
    entries: Vec<FileEntry>,
    kind: PreviewKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PreviewKind {
    #[default]
    Empty,
    Directory,
    Text,
    Metadata,
    Error,
}

#[derive(Clone, Debug)]
struct DragState {
    path: PathBuf,
    start: Point,
    active: bool,
}

#[derive(Clone, Debug)]
struct MarqueeState {
    start: Point,
    current: Point,
}

#[derive(Clone, Debug)]
struct OperationLanes {
    navigation: Arc<Semaphore>,
    background: Arc<Semaphore>,
    mutation: Arc<Semaphore>,
}

impl Default for OperationLanes {
    fn default() -> Self {
        Self {
            navigation: Arc::new(Semaphore::new(2)),
            background: Arc::new(Semaphore::new(2)),
            mutation: Arc::new(Semaphore::new(1)),
        }
    }
}

#[derive(Clone, Debug)]
enum Message {
    Event(iced::Event, event::Status),
    WindowResized(Size),
    SystemTheme(iced::theme::Mode),
    PollSystem,
    Parent,
    Back,
    Forward,
    LocationChanged(String),
    LocationFocused,
    LocationSubmitted,
    ViewMode(ViewMode),
    TreeRow(u64),
    TreeLoaded(u64, PathBuf, Vec<PathBuf>),
    EntryPressed(usize),
    EntryReleased(usize),
    EntryHovered(usize),
    EntryUnhovered(usize),
    EntryDoubleClicked(usize),
    RangerPressed(usize),
    RangerReleased,
    RangerActivated(usize),
    RangerParentActivated(usize),
    RangerParentScrolled(f32),
    RangerCurrentScrolled(f32),
    EntryContext(usize),
    ContextNewFolder,
    ContextRename,
    ContextTrash,
    CloseContext,
    GridScrolled(f32),
    GridPointerMoved(Point),
    NavigationFinished {
        id: u64,
        requested: PathBuf,
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    },
    DetailsFinished {
        generation: u64,
        path: PathBuf,
        result: Result<String, String>,
    },
    PreviewFinished {
        generation: u64,
        entry: FileEntry,
        result: Result<PreviewView, String>,
    },
    ParentLoaded(PathBuf, Vec<FileEntry>),
    SearchChanged(String),
    SearchSubmitted,
    SearchFinished {
        generation: u64,
        result: Result<(Vec<FileEntry>, bool), String>,
    },
    CommandChanged(String),
    CommandSubmitted,
    ShellFinished(Result<ShellReport, String>),
    CloseOutput,
    AnimationFrame(Instant),
    DialogInputChanged(String),
    DialogSubmit,
    DialogConfirm,
    DialogCancel,
    NameFinished(Result<PathBuf, String>),
    TrashFinished(Vec<(FileEntry, String)>),
    PermanentDeleteFinished(Vec<(FileEntry, String)>),
    Copy,
    Paste,
    CopyFinished(Result<PathBuf, String>),
    MoveFinished(Result<PathBuf, String>),
    OperationError(String),
    Noop,
}

pub fn run() -> iced::Result {
    let window = window::Settings {
        size: Size::new(820.0, 560.0),
        min_size: Some(Size::new(660.0, 420.0)),
        transparent: true,
        blur: true,
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
    lanes: OperationLanes,
    window_size: Size,
    input_mode: InputMode,
    location_input: String,
    search_text: String,
    command_text: String,
    command_output: Option<(String, String)>,
    command_output_height: f32,
    output_expansion: Animation<bool>,
    animation_now: Instant,
    status: String,
    busy: bool,
    navigation_loading: bool,
    dialog: DialogState,
    preview: PreviewView,
    context_menu: Option<(usize, Point)>,
    cursor: Point,
    drag: Option<DragState>,
    marquee: Option<MarqueeState>,
    hovered_entry: Option<usize>,
    grid_scroll_y: f32,
    ranger_parent_scroll_y: f32,
    ranger_current_scroll_y: f32,
    navigation_id: u64,
    pending_navigation_id: Option<u64>,
    search_generation: u64,
    system_mode: iced::theme::Mode,
    accent: Option<theme::ThemeColors>,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let now = Instant::now();
        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut explorer = ExplorerState::new(current.clone(), mounted_roots());
        explorer.view_mode = settings::load_view_mode();
        let accent = theme::load(theme::interface_settings().as_ref());
        let mut app = Self {
            explorer,
            lanes: OperationLanes::default(),
            window_size: Size::new(820.0, 560.0),
            input_mode: InputMode::Browser,
            location_input: current.display().to_string(),
            search_text: String::new(),
            command_text: String::new(),
            command_output: None,
            command_output_height: STATUS_HEIGHT,
            output_expansion: Animation::new(false)
                .duration(Duration::from_millis(140))
                .easing(Easing::EaseOut),
            animation_now: now,
            status: String::new(),
            busy: false,
            navigation_loading: false,
            dialog: DialogState::None,
            preview: PreviewView::default(),
            context_menu: None,
            cursor: Point::ORIGIN,
            drag: None,
            marquee: None,
            hovered_entry: None,
            grid_scroll_y: 0.0,
            ranger_parent_scroll_y: 0.0,
            ranger_current_scroll_y: 0.0,
            navigation_id: 0,
            pending_navigation_id: None,
            search_generation: 0,
            system_mode: iced::theme::Mode::Dark,
            accent,
        };
        let navigation = PendingNavigation {
            requested: current,
            kind: NavigationKind::Refresh {
                keep_operation_busy: false,
            },
            select: None,
        };
        let initial = Task::batch([
            app.request_navigation(navigation),
            system::theme().map(Message::SystemTheme),
        ]);
        (app, initial)
    }

    fn subscription(&self) -> Subscription<Message> {
        let animation = if self.output_expansion.is_animating(self.animation_now) {
            time::every(Duration::from_millis(16)).map(Message::AnimationFrame)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            event::listen_with(|event, status, _| Some(Message::Event(event, status))),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            system::theme_changes().map(Message::SystemTheme),
            time::every(Duration::from_secs(2)).map(|_| Message::PollSystem),
            animation,
        ])
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

    fn application_style(&self, theme: &Theme) -> iced::theme::Style {
        let palette = theme.palette();
        iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: palette.text,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Event(event, status) => self.handle_event(event, status),
            Message::WindowResized(size) => {
                self.window_size = size;
                Task::none()
            }
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
                self.input_mode = InputMode::Location;
                Task::none()
            }
            Message::LocationSubmitted => {
                self.input_mode = InputMode::Browser;
                let input = PathBuf::from(&self.location_input);
                let requested = if input.is_absolute() {
                    input
                } else {
                    self.explorer.current.join(input)
                };
                self.navigate(requested, true, None)
            }
            Message::ViewMode(mode) => self.set_view_mode(mode),
            Message::TreeRow(id) => self.activate_tree_row(id),
            Message::TreeLoaded(id, path, folders) => {
                tree::install_children(&mut self.explorer, id, &path, folders);
                Task::none()
            }
            Message::EntryPressed(index) => {
                let Some(entry) = self.explorer.entries.get(index) else {
                    return Task::none();
                };
                self.drag = Some(DragState {
                    path: entry.path.clone(),
                    start: self.cursor,
                    active: false,
                });
                Task::none()
            }
            Message::EntryReleased(index) => self.finish_entry_press(index),
            Message::EntryHovered(index) => {
                self.hovered_entry = Some(index);
                Task::none()
            }
            Message::EntryUnhovered(index) => {
                if self.hovered_entry == Some(index) {
                    self.hovered_entry = None;
                }
                Task::none()
            }
            Message::EntryDoubleClicked(index) => self.activate_entry(index, true),
            Message::RangerPressed(index) => {
                let Some(entry) = self.explorer.entries.get(index) else {
                    return Task::none();
                };
                self.drag = Some(DragState {
                    path: entry.path.clone(),
                    start: self.cursor,
                    active: false,
                });
                self.explorer.select_only(Some(index));
                Task::batch([self.schedule_details(), self.schedule_preview()])
            }
            Message::RangerReleased => {
                if self.drag.as_ref().is_some_and(|drag| drag.active) {
                    self.finish_drag()
                } else {
                    self.drag = None;
                    Task::none()
                }
            }
            Message::RangerActivated(index) => self.open_or_navigate(index),
            Message::RangerParentActivated(index) => {
                let Some(path) = self
                    .explorer
                    .parent_entries
                    .get(index)
                    .filter(|entry| entry.is_directory())
                    .map(|entry| entry.path.clone())
                else {
                    return Task::none();
                };
                self.navigate(path, true, None)
            }
            Message::RangerParentScrolled(y) => {
                self.ranger_parent_scroll_y = y;
                Task::none()
            }
            Message::RangerCurrentScrolled(y) => {
                self.ranger_current_scroll_y = y;
                Task::none()
            }
            Message::EntryContext(index) => {
                self.explorer.select_only(Some(index));
                self.context_menu = Some((index, self.cursor));
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
                self.show_trash_dialog()
            }
            Message::CloseContext => {
                self.context_menu = None;
                Task::none()
            }
            Message::GridScrolled(y) => {
                self.grid_scroll_y = y;
                Task::none()
            }
            Message::GridPointerMoved(point) => {
                if let Some(marquee) = &mut self.marquee {
                    marquee.current = Point::new(
                        point.x + SIDEBAR_WIDTH,
                        point.y + TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT,
                    );
                    self.update_marquee_selection();
                }
                Task::none()
            }
            Message::NavigationFinished {
                id,
                requested,
                result,
            } => self.finish_navigation(id, requested, result),
            Message::DetailsFinished {
                generation,
                path,
                result,
            } => {
                if self.explorer.accepts_details(generation, &path) {
                    self.explorer.selected_details = result.ok();
                    self.refresh_status();
                }
                Task::none()
            }
            Message::PreviewFinished {
                generation,
                entry,
                result,
            } => {
                if self.explorer.accepts_preview(generation, &entry.path) {
                    self.preview = result.unwrap_or_else(|error| PreviewView {
                        title: fs::display_name(&entry.name),
                        text: error,
                        kind: PreviewKind::Error,
                        ..PreviewView::default()
                    });
                }
                Task::none()
            }
            Message::ParentLoaded(path, entries) => {
                if self.explorer.current.parent() == Some(path.as_path()) {
                    self.explorer.selected_parent_entry = entries
                        .iter()
                        .position(|entry| entry.path == self.explorer.current);
                    self.explorer.parent_entries = entries;
                }
                Task::none()
            }
            Message::SearchChanged(value) => self.update_search(value),
            Message::SearchSubmitted => self.submit_search(),
            Message::SearchFinished { generation, result } => {
                self.finish_recursive_search(generation, result);
                Task::none()
            }
            Message::CommandChanged(value) => {
                self.command_text = value;
                Task::none()
            }
            Message::CommandSubmitted => self.submit_command(),
            Message::ShellFinished(result) => self.finish_shell(result),
            Message::CloseOutput => {
                self.hide_command_output();
                Task::none()
            }
            Message::AnimationFrame(now) => {
                self.animation_now = now;
                Task::none()
            }
            Message::DialogInputChanged(value) => {
                if let DialogState::Name { value: input, .. } = &mut self.dialog {
                    *input = value;
                }
                Task::none()
            }
            Message::DialogSubmit => self.submit_name(),
            Message::DialogConfirm => self.confirm_dialog(),
            Message::DialogCancel => {
                if !self.busy {
                    self.dialog = DialogState::None;
                    self.explorer.pending_name = None;
                    self.explorer.pending_delete.clear();
                }
                Task::none()
            }
            Message::NameFinished(result) => self.finish_name(result),
            Message::TrashFinished(failures) => self.finish_trash(failures),
            Message::PermanentDeleteFinished(failures) => self.finish_permanent_delete(failures),
            Message::Copy => {
                self.copy_selection();
                Task::none()
            }
            Message::Paste => self.paste(),
            Message::CopyFinished(result) | Message::MoveFinished(result) => {
                self.finish_file_operation(result)
            }
            Message::OperationError(error) => {
                self.show_error(error);
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    fn handle_event(&mut self, event: iced::Event, _status: event::Status) -> Task<Message> {
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                self.cursor = position;
                if let Some(drag) = &mut self.drag
                    && !drag.active
                    && distance(drag.start, position) >= 6.0
                {
                    drag.active = true;
                }
                if let Some(marquee) = &mut self.marquee {
                    marquee.current = position;
                    self.update_marquee_selection();
                }
                Task::none()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if self.grid_selection_start_allowed(self.cursor) =>
            {
                self.marquee = Some(MarqueeState {
                    start: self.cursor,
                    current: self.cursor,
                });
                self.update_marquee_selection();
                Task::none()
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.marquee.take().is_some() =>
            {
                self.schedule_details()
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => self.go_back(),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
                self.go_forward()
            }
            iced::Event::Window(window::Event::Unfocused) if self.marquee.take().is_some() => {
                self.schedule_details()
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                modifiers,
                text,
                ..
            }) => self.handle_key(key, modified_key, modifiers, text.as_deref()),
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
        if self.dialog.is_open() {
            if key == keyboard::Key::Named(keyboard::key::Named::Escape) && !self.busy {
                return self.update(Message::DialogCancel);
            }
            return Task::none();
        }
        if self.input_mode != InputMode::Browser {
            if key != keyboard::Key::Named(keyboard::key::Named::Escape) {
                return Task::none();
            }
            return match self.input_mode {
                InputMode::Search => {
                    self.input_mode = InputMode::Browser;
                    self.search_text.clear();
                    self.cancel_search()
                }
                InputMode::Command(_) => {
                    self.input_mode = InputMode::Browser;
                    self.command_text.clear();
                    Task::none()
                }
                InputMode::Location => {
                    self.input_mode = InputMode::Browser;
                    self.location_input = self.explorer.current.display().to_string();
                    Task::none()
                }
                InputMode::Browser => Task::none(),
            };
        }
        if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
            if self.command_output.is_some() {
                self.hide_command_output();
                return Task::none();
            }
            if self.explorer.visual_selection_anchor.is_some() {
                self.explorer.cancel_visual_selection();
                return self.schedule_details();
            }
            if self.delete_operator_pending() {
                self.status.clear();
                return Task::none();
            }
        }
        if self.busy {
            return Task::none();
        }
        if modifiers.control() && !modifiers.alt() && !modifiers.logo() {
            let lower = produced.unwrap_or_default().to_ascii_lowercase();
            if lower == "c"
                || matches!(key, keyboard::Key::Character(ref value) if value.eq_ignore_ascii_case("c"))
            {
                return self.update(Message::Copy);
            }
            if lower == "v"
                || matches!(key, keyboard::Key::Character(ref value) if value.eq_ignore_ascii_case("v"))
            {
                return self.update(Message::Paste);
            }
            if matches!(key, keyboard::Key::Character(ref value) if value.eq_ignore_ascii_case("o"))
            {
                return self.go_back();
            }
            return Task::none();
        }
        let text = produced.or_else(|| match &modified_key {
            keyboard::Key::Character(value) => Some(value.as_str()),
            _ => None,
        });
        if self.delete_operator_pending() {
            self.status.clear();
            if let Some(motion) =
                text.filter(|value| matches!(*value, "0" | "$" | "h" | "j" | "k" | "l" | "d"))
            {
                self.explorer
                    .select_delete_motion(motion, self.grid_columns() as i32);
                return self.show_trash_dialog();
            }
            return Task::none();
        }
        match text {
            Some("/") => self.begin_search(),
            Some("!") | Some(":") => self.begin_command(text.unwrap().chars().next().unwrap()),
            Some("n") => self.repeat_search(false),
            Some("N") => self.repeat_search(true),
            Some("u") => self.go_back(),
            Some("y") => self.update(Message::Copy),
            Some("p") => self.update(Message::Paste),
            Some("v") => {
                self.explorer.toggle_visual_selection();
                self.schedule_details()
            }
            Some("x") => self.show_trash_dialog(),
            Some("d") => {
                if self.explorer.visual_selection_anchor.is_some()
                    || self.explorer.selected_entries.len() > 1
                {
                    self.show_trash_dialog()
                } else if self.explorer.selected_entry.is_some() {
                    self.status = "d  •  awaiting motion: 0, $, h, j, k, l, or d".to_owned();
                    Task::none()
                } else {
                    Task::none()
                }
            }
            Some("h") => self.move_selection(-1, 0),
            Some("j") => self.move_selection(0, 1),
            Some("k") => self.move_selection(0, -1),
            Some("l") => self.move_selection(1, 0),
            _ if key == keyboard::Key::Named(keyboard::key::Named::Enter) => {
                self.activate_selected()
            }
            _ if key == keyboard::Key::Named(keyboard::key::Named::Backspace) => self.go_parent(),
            _ if key == keyboard::Key::Named(keyboard::key::Named::Delete) => {
                self.show_trash_dialog()
            }
            _ => Task::none(),
        }
    }

    fn request_navigation(&mut self, navigation: PendingNavigation) -> Task<Message> {
        self.navigation_id += 1;
        let id = self.navigation_id;
        let requested = navigation.requested.clone();
        self.pending_navigation_id = Some(id);
        self.explorer.begin_navigation(navigation);
        self.navigation_loading = true;
        self.status = format!("Opening {}…", requested.display());
        let lane = Arc::clone(&self.lanes.navigation);
        Task::perform(
            run_blocking(lane, {
                let path = requested.clone();
                move || {
                    fs::open_directory(&path)
                        .map(|opened| (opened.canonical_path, opened.entries))
                        .map_err(|error| error.to_string())
                }
            }),
            move |result| Message::NavigationFinished {
                id,
                requested,
                result,
            },
        )
    }

    fn navigate(
        &mut self,
        requested: PathBuf,
        remember: bool,
        select: Option<PathBuf>,
    ) -> Task<Message> {
        self.cancel_search_state();
        self.request_navigation(PendingNavigation {
            requested,
            kind: NavigationKind::Forward { remember },
            select,
        })
    }

    fn finish_navigation(
        &mut self,
        id: u64,
        requested: PathBuf,
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    ) -> Task<Message> {
        if self.pending_navigation_id != Some(id) {
            return Task::none();
        }
        self.pending_navigation_id = None;
        self.navigation_loading = false;
        let Some(pending) = self.explorer.take_navigation_for(&requested) else {
            return Task::none();
        };
        match result {
            Ok((canonical, entries)) => {
                if !self.explorer.commit_navigation(pending, canonical, entries) {
                    return Task::none();
                }
                self.location_input = self.explorer.current.display().to_string();
                self.grid_scroll_y = 0.0;
                self.ranger_parent_scroll_y = 0.0;
                self.ranger_current_scroll_y = 0.0;
                self.status.clear();
                let mut tasks = vec![self.load_root_if_needed(), self.schedule_details()];
                if self.explorer.view_mode == ViewMode::Ranger {
                    tasks.push(self.load_parent());
                    tasks.push(self.schedule_preview());
                }
                Task::batch(tasks)
            }
            Err(error) => {
                self.status = error;
                Task::none()
            }
        }
    }

    fn go_parent(&mut self) -> Task<Message> {
        let Some(parent) = self.explorer.current.parent().map(Path::to_path_buf) else {
            return Task::none();
        };
        let current = self.explorer.current.clone();
        self.navigate(parent, true, Some(current))
    }

    fn go_back(&mut self) -> Task<Message> {
        let Some(target) = self.explorer.history.last().cloned() else {
            return Task::none();
        };
        self.cancel_search_state();
        self.request_navigation(PendingNavigation {
            requested: target.clone(),
            kind: NavigationKind::Back { expected: target },
            select: None,
        })
    }

    fn go_forward(&mut self) -> Task<Message> {
        let Some(target) = self.explorer.forward_history.last().cloned() else {
            return Task::none();
        };
        self.cancel_search_state();
        self.request_navigation(PendingNavigation {
            requested: target.clone(),
            kind: NavigationKind::HistoryForward { expected: target },
            select: None,
        })
    }

    fn refresh(&mut self, select: Option<PathBuf>) -> Task<Message> {
        self.request_navigation(PendingNavigation {
            requested: self.explorer.current.clone(),
            kind: NavigationKind::Refresh {
                keep_operation_busy: true,
            },
            select,
        })
    }

    fn set_view_mode(&mut self, mode: ViewMode) -> Task<Message> {
        if self.explorer.view_mode == mode {
            return Task::none();
        }
        self.explorer.view_mode = mode;
        if mode == ViewMode::Ranger
            && self.explorer.selected_entry.is_none()
            && !self.explorer.entries.is_empty()
        {
            self.explorer.select_only(Some(0));
        }
        let save = mode;
        let lane = Arc::clone(&self.lanes.mutation);
        let mut tasks = vec![Task::perform(
            run_blocking(lane, move || {
                settings::save_view_mode(save).map_err(|error| error.to_string())
            }),
            |_| Message::Noop,
        )];
        if mode == ViewMode::Ranger {
            tasks.push(self.load_parent());
            tasks.push(self.schedule_preview());
        } else {
            self.preview = PreviewView::default();
        }
        Task::batch(tasks)
    }

    fn activate_entry(&mut self, index: usize, double: bool) -> Task<Message> {
        let Some(entry) = self.explorer.entries.get(index).cloned() else {
            return Task::none();
        };
        if entry.is_directory() {
            if double {
                return Task::none();
            }
            return self.navigate(entry.path, true, None);
        }
        self.explorer.select_only(Some(index));
        if double {
            return self.open_entry(entry);
        }
        Task::batch([self.schedule_details(), self.schedule_preview()])
    }

    fn activate_selected(&mut self) -> Task<Message> {
        self.explorer
            .selected_entry
            .map_or_else(Task::none, |index| self.open_or_navigate(index))
    }

    fn open_or_navigate(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self.explorer.entries.get(index).cloned() else {
            return Task::none();
        };
        if entry.is_directory() {
            self.navigate(entry.path, true, None)
        } else {
            self.open_entry(entry)
        }
    }

    fn finish_entry_press(&mut self, index: usize) -> Task<Message> {
        if self.drag.is_none() {
            return Task::none();
        }
        if self.drag.as_ref().is_some_and(|drag| drag.active) {
            return self.finish_drag();
        }
        self.drag = None;
        self.activate_entry(index, false)
    }

    fn open_entry(&self, entry: FileEntry) -> Task<Message> {
        let lane = Arc::clone(&self.lanes.background);
        Task::perform(
            run_blocking(lane, move || {
                let uri = gio::File::for_path(&entry.path).uri();
                gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
                    .map_err(|error| error.to_string())
            }),
            |result| match result {
                Ok(()) => Message::Noop,
                Err(error) => Message::OperationError(error),
            },
        )
    }

    fn move_selection(&mut self, horizontal: i32, vertical: i32) -> Task<Message> {
        if self.explorer.view_mode == ViewMode::Ranger {
            self.explorer.move_ranger_selection(vertical + horizontal);
        } else {
            self.explorer
                .move_selection(horizontal, vertical, self.grid_columns() as i32);
        }
        Task::batch([
            self.schedule_details(),
            self.schedule_preview(),
            self.scroll_to_selected(),
        ])
    }

    fn scroll_to_selected(&self) -> Task<Message> {
        let Some(index) = self.explorer.selected_entry else {
            return Task::none();
        };
        if self.explorer.view_mode != ViewMode::Grid {
            return Task::none();
        }
        let y = (index / self.grid_columns()) as f32 * TILE_ROW_HEIGHT;
        widget::operation::scroll_to(
            Id::new(GRID_SCROLL_ID),
            scrollable::AbsoluteOffset { x: 0.0, y },
        )
    }

    fn schedule_details(&mut self) -> Task<Message> {
        let generation = self.explorer.begin_details();
        let Some(entry) = self
            .explorer
            .selected_entry
            .and_then(|index| self.explorer.entries.get(index).cloned())
        else {
            self.refresh_status();
            return Task::none();
        };
        self.refresh_status();
        let path = entry.path.clone();
        let lane = Arc::clone(&self.lanes.background);
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                run_blocking(lane, move || {
                    fs::read_entry_details(&path).map_err(|error| error.to_string())
                })
                .await
            },
            move |result| Message::DetailsFinished {
                generation,
                path: entry.path,
                result,
            },
        )
    }

    fn schedule_preview(&mut self) -> Task<Message> {
        if self.explorer.view_mode != ViewMode::Ranger {
            return Task::none();
        }
        let generation = self.explorer.begin_preview();
        let Some(entry) = self
            .explorer
            .selected_entry
            .and_then(|index| self.explorer.entries.get(index).cloned())
        else {
            self.preview = PreviewView::default();
            return Task::none();
        };
        let worker_entry = entry.clone();
        let lane = Arc::clone(&self.lanes.background);
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(75)).await;
                run_blocking(lane, move || {
                    fs::read_preview(&worker_entry)
                        .map(|data| preview_view(&worker_entry, data))
                        .map_err(|error| error.to_string())
                })
                .await
            },
            move |result| Message::PreviewFinished {
                generation,
                entry,
                result,
            },
        )
    }

    fn load_parent(&self) -> Task<Message> {
        let Some(path) = self.explorer.current.parent().map(Path::to_path_buf) else {
            return Task::done(Message::ParentLoaded(PathBuf::new(), Vec::new()));
        };
        let worker_path = path.clone();
        let lane = Arc::clone(&self.lanes.background);
        Task::perform(
            run_blocking(lane, move || {
                Ok(fs::read_directory(&worker_path).unwrap_or_default())
            }),
            move |result| Message::ParentLoaded(path, result.unwrap_or_default()),
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
        let lane = Arc::clone(&self.lanes.background);
        Task::perform(
            run_blocking(lane, move || Ok(fs::read_child_folders(&worker_path))),
            move |result| Message::TreeLoaded(id, path, result.unwrap_or_default()),
        )
    }

    fn activate_tree_row(&mut self, id: u64) -> Task<Message> {
        let current = self.explorer.current.clone();
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

    fn begin_search(&mut self) -> Task<Message> {
        self.input_mode = InputMode::Search;
        self.search_text.clear();
        self.hide_command_output();
        self.explorer.search_origin = Some(self.explorer.selected_entry);
        widget::operation::focus(Id::new(SEARCH_ID))
    }

    fn update_search(&mut self, value: String) -> Task<Message> {
        self.search_text = value;
        if !self.explorer.recursive_search_active && self.search_text.starts_with('/') {
            self.explorer.recursive_search_active = true;
            self.search_text.remove(0);
        }
        self.explorer.search_draft = self.search_text.clone();
        if self.explorer.recursive_search_active {
            return self.schedule_recursive_search();
        }
        let previous = self.explorer.selected_entry;
        self.explorer.selected_entry = find_match(
            &self.explorer.entries,
            &self.search_text,
            self.explorer.search_origin.flatten(),
            false,
        );
        self.explorer.selected_entries.clear();
        self.explorer
            .selected_entries
            .extend(self.explorer.selected_entry);
        if previous != self.explorer.selected_entry {
            Task::batch([self.schedule_details(), self.schedule_preview()])
        } else {
            Task::none()
        }
    }

    fn schedule_recursive_search(&mut self) -> Task<Message> {
        self.search_generation += 1;
        let generation = self.search_generation;
        let query = self.search_text.clone();
        let root = self.explorer.current.clone();
        self.explorer.recursive_search_loading = !query.is_empty();
        if query.is_empty() {
            self.explorer.entries.clear();
            self.explorer.select_only(None);
            return Task::none();
        }
        let lane = Arc::clone(&self.lanes.background);
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(160)).await;
                run_blocking(lane, move || {
                    fs::search_directory(&root, &query, SEARCH_LIMIT, || false)
                        .map(|results| (results.entries, results.truncated))
                        .map_err(|error| error.to_string())
                })
                .await
            },
            move |result| Message::SearchFinished { generation, result },
        )
    }

    fn finish_recursive_search(
        &mut self,
        generation: u64,
        result: Result<(Vec<FileEntry>, bool), String>,
    ) {
        if generation != self.search_generation || !self.explorer.recursive_search_active {
            return;
        }
        self.explorer.recursive_search_loading = false;
        match result {
            Ok((entries, truncated)) => {
                self.explorer.entries = entries;
                self.explorer.recursive_search_truncated = truncated;
                self.explorer
                    .select_only((!self.explorer.entries.is_empty()).then_some(0));
            }
            Err(error) => self.status = error,
        }
    }

    fn submit_search(&mut self) -> Task<Message> {
        self.input_mode = InputMode::Browser;
        self.explorer.last_search = self.search_text.clone();
        let selected = self
            .explorer
            .selected_entry
            .and_then(|index| self.explorer.entries.get(index).cloned());
        if self.explorer.recursive_search_active {
            self.restore_directory_entries(false);
            if let Some(entry) = selected {
                return if entry.is_directory() {
                    self.navigate(entry.path, true, None)
                } else {
                    self.open_entry(entry)
                };
            }
        }
        Task::none()
    }

    fn cancel_search(&mut self) -> Task<Message> {
        self.restore_directory_entries(true);
        self.schedule_details()
    }

    fn cancel_search_state(&mut self) {
        self.search_generation += 1;
        self.restore_directory_entries(true);
        self.input_mode = InputMode::Browser;
        self.search_text.clear();
    }

    fn restore_directory_entries(&mut self, restore_selection: bool) {
        if self.explorer.recursive_search_active {
            self.explorer.entries = self.explorer.directory_entries.clone();
            self.explorer.recursive_search_active = false;
            self.explorer.recursive_search_loading = false;
            self.explorer.recursive_search_truncated = false;
        }
        if restore_selection && let Some(origin) = self.explorer.search_origin.take() {
            self.explorer.select_only(origin);
        }
        self.explorer.search_draft.clear();
    }

    fn repeat_search(&mut self, reverse: bool) -> Task<Message> {
        let query = self.explorer.last_search.clone();
        if query.is_empty() {
            return Task::none();
        }
        if let Some(index) = find_match(
            &self.explorer.entries,
            &query,
            self.explorer.selected_entry,
            reverse,
        ) {
            self.explorer.select_only(Some(index));
            return Task::batch([self.schedule_details(), self.schedule_preview()]);
        }
        Task::none()
    }

    fn begin_command(&mut self, prefix: char) -> Task<Message> {
        self.input_mode = InputMode::Command(prefix);
        self.command_text.clear();
        self.hide_command_output();
        widget::operation::focus(Id::new(COMMAND_ID))
    }

    fn submit_command(&mut self) -> Task<Message> {
        let InputMode::Command(prefix) = self.input_mode else {
            return Task::none();
        };
        let mode = CommandMode::from_prefix(prefix);
        if shell::is_quit(mode, &self.command_text) {
            return iced::exit();
        }
        if self.command_text.trim().is_empty() {
            self.input_mode = InputMode::Browser;
            return Task::none();
        }
        let command = std::mem::take(&mut self.command_text);
        let current = self.explorer.current.clone();
        self.input_mode = InputMode::Browser;
        self.busy = true;
        self.status = format!("Running {prefix}{command}…");
        let lane = Arc::clone(&self.lanes.mutation);
        Task::perform(
            run_blocking(lane, move || {
                shell::execute(&current, prefix, &command).map_err(|error| error.to_string())
            }),
            Message::ShellFinished,
        )
    }

    fn finish_shell(&mut self, result: Result<ShellReport, String>) -> Task<Message> {
        self.busy = false;
        match result {
            Ok(report) => {
                self.show_command_output(report.summary, report.detail);
                let previous_directory = self.explorer.current.clone();
                let tree_refresh = self.invalidate_tree(vec![previous_directory]);
                if let Some(directory) = report
                    .final_directory
                    .filter(|path| path != &self.explorer.current)
                {
                    return Task::batch([tree_refresh, self.navigate(directory, true, None)]);
                }
                Task::batch([tree_refresh, self.refresh(None)])
            }
            Err(error) if error.contains("interactive terminal") => {
                self.show_command_output(
                    "interactive terminal required".to_owned(),
                    "This command tried to take over the terminal screen, so PolarExp stopped it."
                        .to_owned(),
                );
                Task::none()
            }
            Err(error) => {
                self.show_error(format!("Could not run Bash: {error}"));
                Task::none()
            }
        }
    }

    fn show_command_output(&mut self, summary: String, detail: String) {
        self.command_output_height = command_output_height(&detail);
        self.command_output = Some((summary, detail));
        self.animation_now = Instant::now();
        self.output_expansion.go_mut(true, self.animation_now);
    }

    fn hide_command_output(&mut self) {
        if self.command_output.is_none() && !self.output_expansion.value() {
            return;
        }
        self.command_output = None;
        self.animation_now = Instant::now();
        self.output_expansion.go_mut(false, self.animation_now);
    }

    fn show_rename(&mut self, index: usize) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(entry) = self.explorer.entries.get(index).cloned() else {
            return Task::none();
        };
        self.explorer.pending_name = Some(PendingName::Rename(entry.clone()));
        self.dialog = DialogState::Name {
            title: "Rename".to_owned(),
            value: fs::display_name(&entry.name),
            error: String::new(),
        };
        Task::batch([
            widget::operation::focus(Id::new(DIALOG_ID)),
            widget::operation::select_all(Id::new(DIALOG_ID)),
        ])
    }

    fn show_new_folder(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.explorer.pending_name = Some(PendingName::NewFolder);
        self.dialog = DialogState::Name {
            title: "New Folder".to_owned(),
            value: String::new(),
            error: String::new(),
        };
        widget::operation::focus(Id::new(DIALOG_ID))
    }

    fn submit_name(&mut self) -> Task<Message> {
        let DialogState::Name { value, .. } = &self.dialog else {
            return Task::none();
        };
        if let Err(error) = fs::validate_name(value) {
            if let DialogState::Name { error: target, .. } = &mut self.dialog {
                *target = error.to_owned();
            }
            return Task::none();
        }
        let Some(pending) = self.explorer.pending_name.clone() else {
            return Task::none();
        };
        let value = value.clone();
        let current = self.explorer.current.clone();
        self.busy = true;
        let lane = Arc::clone(&self.lanes.mutation);
        Task::perform(
            run_blocking(lane, move || {
                match pending {
                    PendingName::NewFolder => fs::create_folder(&current, &value),
                    PendingName::Rename(entry) => fs::rename_entry(&entry.path, &value),
                }
                .map_err(|error| error.to_string())
            }),
            Message::NameFinished,
        )
    }

    fn finish_name(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        self.busy = false;
        match result {
            Ok(path) => {
                self.dialog = DialogState::None;
                self.explorer.pending_name = None;
                Task::batch([
                    self.invalidate_tree(vec![self.explorer.current.clone()]),
                    self.refresh(Some(path)),
                ])
            }
            Err(error) => {
                if let DialogState::Name { error: target, .. } = &mut self.dialog {
                    *target = error;
                }
                Task::none()
            }
        }
    }

    fn show_trash_dialog(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let entries = self.selected_entries();
        if entries.is_empty() {
            return Task::none();
        }
        let message = deletion_confirmation(&entries);
        self.explorer.pending_delete = entries;
        self.dialog = DialogState::Trash { message };
        Task::none()
    }

    fn confirm_dialog(&mut self) -> Task<Message> {
        match self.dialog {
            DialogState::Trash { .. } => self.move_pending_to_trash(),
            DialogState::PermanentDelete { .. } => self.delete_pending_permanently(),
            DialogState::Error { .. } => self.update(Message::DialogCancel),
            DialogState::Name { .. } => self.submit_name(),
            DialogState::None => Task::none(),
        }
    }

    fn move_pending_to_trash(&mut self) -> Task<Message> {
        let pending = self.explorer.pending_delete.clone();
        if pending.is_empty() {
            return Task::none();
        }
        self.busy = true;
        let lane = Arc::clone(&self.lanes.mutation);
        Task::perform(
            run_blocking(lane, move || {
                Ok(pending
                    .iter()
                    .filter_map(|entry| {
                        gio::File::for_path(&entry.path)
                            .trash(None::<&gio::Cancellable>)
                            .err()
                            .map(|error| (entry.clone(), error.to_string()))
                    })
                    .collect())
            }),
            |result| Message::TrashFinished(result.unwrap_or_default()),
        )
    }

    fn finish_trash(&mut self, failures: Vec<(FileEntry, String)>) -> Task<Message> {
        self.busy = false;
        if failures.is_empty() {
            self.explorer.pending_delete.clear();
            self.dialog = DialogState::None;
            return Task::batch([
                self.invalidate_tree(vec![self.explorer.current.clone()]),
                self.refresh(None),
            ]);
        }
        let detail = failures
            .iter()
            .map(|(entry, reason)| format!("{}: {reason}", fs::display_name(&entry.name)))
            .collect::<Vec<_>>()
            .join("\n");
        self.explorer.pending_delete = failures.iter().map(|(entry, _)| entry.clone()).collect();
        self.dialog = DialogState::PermanentDelete {
            message: permanent_delete_confirmation(failures.len()),
            detail: format!("{detail}\n\nThis cannot be undone."),
        };
        Task::none()
    }

    fn delete_pending_permanently(&mut self) -> Task<Message> {
        let pending = self.explorer.pending_delete.clone();
        self.busy = true;
        let lane = Arc::clone(&self.lanes.mutation);
        Task::perform(
            run_blocking(lane, move || {
                Ok(pending
                    .iter()
                    .filter_map(|entry| {
                        fs::delete_permanently(&entry.path)
                            .err()
                            .map(|error| (entry.clone(), error.to_string()))
                    })
                    .collect())
            }),
            |result| Message::PermanentDeleteFinished(result.unwrap_or_default()),
        )
    }

    fn finish_permanent_delete(&mut self, failures: Vec<(FileEntry, String)>) -> Task<Message> {
        self.busy = false;
        if failures.is_empty() {
            self.explorer.pending_delete.clear();
            self.dialog = DialogState::None;
            return Task::batch([
                self.invalidate_tree(vec![self.explorer.current.clone()]),
                self.refresh(None),
            ]);
        }
        let detail = failures
            .iter()
            .map(|(entry, reason)| format!("{}: {reason}", fs::display_name(&entry.name)))
            .collect::<Vec<_>>()
            .join("\n");
        self.explorer.pending_delete = failures.into_iter().map(|(entry, _)| entry).collect();
        self.show_error(detail);
        Task::none()
    }

    fn copy_selection(&mut self) {
        if let Some(entry) = self
            .explorer
            .selected_entry
            .and_then(|index| self.explorer.entries.get(index))
        {
            self.explorer.copied_entry = Some(entry.path.clone());
            self.status = format!("Copied {}", fs::display_name(&entry.name));
        }
    }

    fn paste(&mut self) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let Some(source) = self.explorer.copied_entry.clone() else {
            return Task::none();
        };
        let destination = self.explorer.current.clone();
        self.busy = true;
        let lane = Arc::clone(&self.lanes.mutation);
        Task::perform(
            run_blocking(lane, move || {
                fs::copy_entry(&source, &destination).map_err(|error| error.to_string())
            }),
            Message::CopyFinished,
        )
    }

    fn finish_file_operation(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        self.busy = false;
        match result {
            Ok(path) => {
                let mut changed = vec![self.explorer.current.clone()];
                if let Some(parent) = path.parent() {
                    changed.push(parent.to_path_buf());
                }
                Task::batch([self.invalidate_tree(changed), self.refresh(Some(path))])
            }
            Err(error) => {
                self.show_error(error);
                Task::none()
            }
        }
    }

    fn finish_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.drag.take().filter(|drag| drag.active) else {
            return Task::none();
        };
        let Some(destination) = self.drop_destination(&drag.path) else {
            return Task::none();
        };
        self.busy = true;
        let source = drag.path;
        let lane = Arc::clone(&self.lanes.mutation);
        Task::perform(
            run_blocking(lane, move || {
                fs::move_entry(&source, &destination).map_err(|error| error.to_string())
            }),
            Message::MoveFinished,
        )
    }

    fn drop_destination(&self, source: &Path) -> Option<PathBuf> {
        let target = if self.explorer.view_mode == ViewMode::Grid && self.cursor.x < SIDEBAR_WIDTH {
            let rows = flatten_rows(&self.explorer);
            let index = ((self.cursor.y - 42.0) / 32.0).floor() as isize;
            let index = usize::try_from(index).ok()?;
            rows.get(index).map(|row| row.path.clone())
        } else if self.explorer.view_mode == ViewMode::Grid {
            self.grid_index_at(self.cursor)
                .and_then(|index| self.explorer.entries.get(index))
                .filter(|entry| entry.is_directory())
                .map(|entry| entry.path.clone())
        } else if self.cursor.y >= TOOLBAR_HEIGHT && self.cursor.x < self.window_size.width * 0.25 {
            let index = ((self.cursor.y - TOOLBAR_HEIGHT + self.ranger_parent_scroll_y) / 30.0)
                .floor() as usize;
            self.explorer
                .parent_entries
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| entry.path.clone())
        } else if self.cursor.y >= TOOLBAR_HEIGHT && self.cursor.x < self.window_size.width * 0.60 {
            let index = ((self.cursor.y - TOOLBAR_HEIGHT + self.ranger_current_scroll_y) / 30.0)
                .floor() as usize;
            self.explorer
                .entries
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| entry.path.clone())
        } else {
            None
        }?;
        (source != target
            && source.parent() != Some(target.as_path())
            && !target.starts_with(source))
        .then_some(target)
    }

    fn update_marquee_selection(&mut self) {
        let Some(marquee) = &self.marquee else {
            return;
        };
        let columns = self.grid_columns() as i32;
        let (start_row, start_column) = self.grid_selection_cell_at(marquee.start);
        let (end_row, end_column) = self.grid_selection_cell_at(marquee.current);
        self.explorer
            .select_rectangle(start_row, start_column, end_row, end_column, columns);
        self.refresh_status();
    }

    fn selected_entries(&self) -> Vec<FileEntry> {
        if self.explorer.selected_entries.len() > 1 {
            self.explorer
                .selected_entries
                .iter()
                .filter_map(|index| self.explorer.entries.get(*index).cloned())
                .collect()
        } else {
            self.explorer
                .selected_entry
                .and_then(|index| self.explorer.entries.get(index).cloned())
                .into_iter()
                .collect()
        }
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
        !self.busy && !self.navigation_loading && !self.explorer.recursive_search_active
    }

    fn delete_operator_pending(&self) -> bool {
        self.status.starts_with("d  •  awaiting motion")
    }

    fn show_error(&mut self, message: String) {
        self.dialog = DialogState::Error { message };
        self.busy = false;
    }

    fn refresh_status(&mut self) {
        if self.delete_operator_pending() {
            return;
        }
        self.status = if self.explorer.selected_entries.len() > 1 {
            format!(
                "{} selected  •  {}",
                self.explorer.selected_entries.len(),
                self.explorer.current.display()
            )
        } else if let Some(entry) = self
            .explorer
            .selected_entry
            .and_then(|index| self.explorer.entries.get(index))
        {
            let name = fs::display_name(&entry.name);
            match &self.explorer.selected_details {
                Some(details) => format!("{name}  •  {details}"),
                None => format!("{name}  •  Loading details…"),
            }
        } else {
            format!(
                "{} items  •  {}",
                self.explorer.entries.len(),
                self.explorer.current.display()
            )
        };
    }

    fn grid_columns(&self) -> usize {
        let width = (self.window_size.width - SIDEBAR_WIDTH - 2.0 * CONTENT_GUTTER).max(1.0);
        (width / TILE_PITCH).floor().max(1.0) as usize
    }

    fn grid_column_width(&self) -> f32 {
        let width = (self.window_size.width - SIDEBAR_WIDTH - 2.0 * CONTENT_GUTTER).max(1.0);
        width / self.grid_columns() as f32
    }

    fn grid_cell_at(&self, point: Point) -> Option<(i32, i32)> {
        let content_top =
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + CONTENT_GUTTER + LIST_VIEW_TOP_INSET;
        if point.x < SIDEBAR_WIDTH + CONTENT_GUTTER || point.y < content_top {
            return None;
        }
        let width = self.window_size.width - SIDEBAR_WIDTH - 2.0 * CONTENT_GUTTER;
        let column_width = width / self.grid_columns() as f32;
        let column = ((point.x - SIDEBAR_WIDTH - CONTENT_GUTTER) / column_width).floor() as i32;
        let row = ((point.y - content_top + self.grid_scroll_y) / TILE_ROW_HEIGHT).floor() as i32;
        (row >= 0 && column >= 0 && column < self.grid_columns() as i32).then_some((row, column))
    }

    fn grid_selection_cell_at(&self, point: Point) -> (i32, i32) {
        let content_top =
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + CONTENT_GUTTER + LIST_VIEW_TOP_INSET;
        let column =
            ((point.x - SIDEBAR_WIDTH - CONTENT_GUTTER) / self.grid_column_width()).floor() as i32;
        let row = ((point.y - content_top + self.grid_scroll_y) / TILE_ROW_HEIGHT).floor() as i32;
        (row, column)
    }

    fn grid_selection_start_allowed(&self, point: Point) -> bool {
        if self.explorer.view_mode != ViewMode::Grid || !self.mutations_allowed() {
            return false;
        }
        let content_top =
            TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + CONTENT_GUTTER + LIST_VIEW_TOP_INSET;
        let status_height = self.output_expansion.interpolate(
            STATUS_HEIGHT,
            self.command_output_height,
            self.animation_now,
        );
        if point.x < SIDEBAR_WIDTH + CONTENT_GUTTER
            || point.x >= self.window_size.width - CONTENT_GUTTER
            || point.y < content_top
            || point.y >= self.window_size.height - status_height - CONTENT_GUTTER
        {
            return false;
        }
        let Some((row, column)) = self.grid_cell_at(point) else {
            return false;
        };
        let index = row as usize * self.grid_columns() + column as usize;
        if index >= self.explorer.entries.len() {
            return true;
        }
        let column_left = SIDEBAR_WIDTH + CONTENT_GUTTER + column as f32 * self.grid_column_width();
        let tile_left = column_left + (self.grid_column_width() - TILE_WIDTH) / 2.0;
        let row_top = content_top - self.grid_scroll_y + row as f32 * TILE_ROW_HEIGHT;
        let inside_tile = point.x >= tile_left
            && point.x < tile_left + TILE_WIDTH
            && point.y >= row_top
            && point.y < row_top + 108.0;
        !inside_tile
    }

    fn marquee_bounds(&self, marquee: &MarqueeState) -> Rectangle {
        let origin = Point::new(SIDEBAR_WIDTH, TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT);
        let size = Size::new(
            (self.window_size.width - origin.x).max(0.0),
            (self.window_size.height
                - origin.y
                - self.output_expansion.interpolate(
                    STATUS_HEIGHT,
                    self.command_output_height,
                    self.animation_now,
                ))
            .max(0.0),
        );
        let left = (marquee.start.x.min(marquee.current.x) - origin.x).clamp(0.0, size.width);
        let right = (marquee.start.x.max(marquee.current.x) - origin.x).clamp(0.0, size.width);
        let top = (marquee.start.y.min(marquee.current.y) - origin.y).clamp(0.0, size.height);
        let bottom = (marquee.start.y.max(marquee.current.y) - origin.y).clamp(0.0, size.height);
        Rectangle::new(Point::new(left, top), Size::new(right - left, bottom - top))
    }

    fn grid_index_at(&self, point: Point) -> Option<usize> {
        let (row, column) = self.grid_cell_at(point)?;
        let index = row as usize * self.grid_columns() + column as usize;
        (index < self.explorer.entries.len()).then_some(index)
    }

    fn view(&self) -> Element<'_, Message> {
        let base = match self.explorer.view_mode {
            ViewMode::Grid => self.grid_layout(),
            ViewMode::Ranger => self.ranger_layout(),
        };
        let mut layers: Vec<Element<'_, Message>> = vec![base];
        if let Some((_, point)) = self.context_menu {
            layers.push(self.context_menu_view(point));
        }
        if self.dialog.is_open() {
            layers.push(self.dialog_view());
        }
        stack(layers).width(Fill).height(Fill).into()
    }

    fn grid_layout(&self) -> Element<'_, Message> {
        row![self.sidebar(), self.browser(false)]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn ranger_layout(&self) -> Element<'_, Message> {
        self.browser(true)
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut rows = Column::new().spacing(0);
        for tree_row in flatten_rows(&self.explorer) {
            rows = rows.push(self.tree_row(tree_row));
        }
        let content = column![
            container(
                text("Locations")
                    .font(UI_FONT_SEMIBOLD)
                    .size(12)
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
        let label_color = if selected {
            self.selection_text_color()
        } else {
            self.iced_theme().palette().text
        };
        if tree_row.loading {
            line = line.push(
                container(text("◌").size(14).color(icon_color))
                    .width(17)
                    .height(17)
                    .center(Fill),
            );
        } else {
            line = line.push(themed_svg(icon, 17.0, icon_color));
        }
        line = line.push(
            text(tree_row.label)
                .size(13)
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
            .style(move |theme, status| tree_button_style(theme, status, selected))
            .into()
    }

    fn browser(&self, ranger: bool) -> Element<'_, Message> {
        let body = if ranger {
            self.ranger_body()
        } else {
            self.grid_body()
        };
        column![self.toolbar(), rule::horizontal(1), body, self.status_bar(),]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let parent = toolbar_button(
            include_bytes!("../ui/icons/up.svg"),
            "Parent folder",
            self.explorer.current.parent().is_some(),
            Message::Parent,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
        );
        let back = toolbar_button(
            include_bytes!("../ui/icons/back.svg"),
            "Back",
            !self.explorer.history.is_empty(),
            Message::Back,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
        );
        let forward = toolbar_button(
            include_bytes!("../ui/icons/forward.svg"),
            "Forward",
            !self.explorer.forward_history.is_empty(),
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
        let view_switch = row![
            self.view_mode_tab("Grid", ViewMode::Grid),
            self.view_mode_tab("Ranger", ViewMode::Ranger),
        ]
        .spacing(12)
        .width(106)
        .height(34);
        container(
            row![parent, back, forward, location, view_switch]
                .spacing(4)
                .align_y(Alignment::Center),
        )
        .height(TOOLBAR_HEIGHT)
        .padding(Padding::from([6, CONTENT_GUTTER as u16]))
        .style(browser_background_style)
        .into()
    }

    fn view_mode_tab(&self, label: &'static str, mode: ViewMode) -> Element<'_, Message> {
        let selected = self.explorer.view_mode == mode;
        let label_color = if selected {
            self.accent_color()
        } else {
            self.secondary_text_color()
        };
        let underline = if selected {
            self.accent_color()
        } else {
            Color::TRANSPARENT
        };
        let content = column![
            container(text(label).font(UI_FONT).size(12).color(label_color))
                .width(Fill)
                .height(32)
                .center_x(Fill)
                .center_y(32),
            container(Space::new().width(Fill).height(2))
                .width(Fill)
                .height(2)
                .style(move |_| solid_background_style(underline)),
        ]
        .spacing(0)
        .width(Fill)
        .height(34);
        mouse_area(content).on_press(Message::ViewMode(mode)).into()
    }

    fn grid_body(&self) -> Element<'_, Message> {
        let columns = self.grid_columns();
        let total_rows = self.explorer.entries.len().div_ceil(columns);
        let viewport_height = (self.window_size.height
            - TOOLBAR_HEIGHT
            - TOOLBAR_DIVIDER_HEIGHT
            - STATUS_HEIGHT
            - 2.0 * CONTENT_GUTTER
            - LIST_VIEW_TOP_INSET)
            .max(TILE_ROW_HEIGHT);
        let first_row = ((self.grid_scroll_y / TILE_ROW_HEIGHT).floor() as usize).saturating_sub(1);
        let visible_rows = (viewport_height / TILE_ROW_HEIGHT).ceil() as usize + 2;
        let last_row = (first_row + visible_rows).min(total_rows);
        let first_index = first_row * columns;
        let last_index = (last_row * columns).min(self.explorer.entries.len());
        let mut grid = Grid::with_capacity(last_index.saturating_sub(first_index))
            .columns(columns)
            .height(widget::grid::aspect_ratio(
                self.grid_column_width(),
                TILE_ROW_HEIGHT,
            ))
            .spacing(0);
        for index in first_index..last_index {
            grid = grid.push(self.file_tile(index));
        }
        let top = Space::new()
            .width(Fill)
            .height(first_row as f32 * TILE_ROW_HEIGHT);
        let bottom_rows = total_rows.saturating_sub(last_row);
        let bottom = Space::new()
            .width(Fill)
            .height(bottom_rows as f32 * TILE_ROW_HEIGHT);
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
        let content: Element<'_, Message> = if let Some(marquee) = &self.marquee {
            let bounds = self.marquee_bounds(marquee);
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
        container(content)
            .width(Fill)
            .height(Fill)
            .clip(true)
            .style(browser_background_style)
            .into()
    }

    fn file_tile(&self, index: usize) -> Element<'_, Message> {
        let entry = &self.explorer.entries[index];
        let selected = self.explorer.selected_entries.contains(&index);
        let hovered = self.hovered_entry == Some(index);
        let icon_color = if entry.is_directory() {
            self.accent_color()
        } else {
            self.secondary_text_color()
        };
        let icon = themed_svg(
            if entry.is_directory() {
                include_bytes!("../ui/icons/folder.svg")
            } else {
                include_bytes!("../ui/icons/file.svg")
            },
            48.0,
            icon_color,
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
            .style(move |theme| tile_style(theme, selected, hovered));
        mouse_area(container(tile).width(Fill).center_x(Fill))
            .on_press(Message::EntryPressed(index))
            .on_release(Message::EntryReleased(index))
            .on_double_click(Message::EntryDoubleClicked(index))
            .on_right_press(Message::EntryContext(index))
            .on_enter(Message::EntryHovered(index))
            .on_exit(Message::EntryUnhovered(index))
            .into()
    }

    fn ranger_body(&self) -> Element<'_, Message> {
        let parent = self.ranger_column(
            &self.explorer.parent_entries,
            self.explorer.selected_parent_entry,
            true,
        );
        let current =
            self.ranger_column(&self.explorer.entries, self.explorer.selected_entry, false);
        let preview = self.preview_column();
        row![
            parent,
            rule::vertical(1),
            current,
            rule::vertical(1),
            preview
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn ranger_column<'a>(
        &'a self,
        entries: &'a [FileEntry],
        selected: Option<usize>,
        parent: bool,
    ) -> Element<'a, Message> {
        let mut rows = Column::new().spacing(0);
        for (index, entry) in entries.iter().enumerate() {
            let active = if parent {
                selected == Some(index)
            } else {
                self.explorer.selected_entries.contains(&index)
            };
            let hovered = !parent && self.hovered_entry == Some(index);
            let label = fs::display_name(&entry.name);
            let icon = themed_svg(
                if entry.is_directory() {
                    include_bytes!("../ui/icons/folder.svg")
                } else {
                    include_bytes!("../ui/icons/file.svg")
                },
                16.0,
                if entry.is_directory() {
                    self.accent_color()
                } else {
                    self.secondary_text_color()
                },
            );
            let line = row![
                container(icon)
                    .width(16)
                    .height(Fill)
                    .center_x(16)
                    .center_y(Fill),
                text(label)
                    .font(UI_FONT)
                    .size(13)
                    .color(if active {
                        self.selection_text_color()
                    } else {
                        self.iced_theme().palette().text
                    })
                    .width(Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let message = if parent {
                Message::RangerParentActivated(index)
            } else {
                Message::RangerPressed(index)
            };
            let row = container(
                container(line)
                    .height(30)
                    .padding(Padding::from([0, 8]))
                    .clip(true)
                    .style(move |theme| ranger_row_style(theme, active, hovered)),
            )
            .height(32)
            .padding(Padding::from([1, 6]));
            rows = rows.push(
                mouse_area(row)
                    .on_press(message)
                    .on_release(if parent {
                        Message::Noop
                    } else {
                        Message::RangerReleased
                    })
                    .on_double_click(if parent {
                        Message::RangerParentActivated(index)
                    } else {
                        Message::RangerActivated(index)
                    })
                    .on_right_press(if parent {
                        Message::Noop
                    } else {
                        Message::EntryContext(index)
                    })
                    .on_enter(if parent {
                        Message::Noop
                    } else {
                        Message::EntryHovered(index)
                    })
                    .on_exit(if parent {
                        Message::Noop
                    } else {
                        Message::EntryUnhovered(index)
                    }),
            );
        }
        let rows = scrollable(rows)
            .on_scroll(if parent {
                |viewport: scrollable::Viewport| {
                    Message::RangerParentScrolled(viewport.absolute_offset().y)
                }
            } else {
                |viewport: scrollable::Viewport| {
                    Message::RangerCurrentScrolled(viewport.absolute_offset().y)
                }
            })
            .height(Fill);
        container(rows)
            .width(Fill)
            .height(Fill)
            .style(browser_background_style)
            .into()
    }

    fn preview_column(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match self.preview.kind {
            PreviewKind::Directory => self.ranger_column(&self.preview.entries, None, true),
            PreviewKind::Empty => container(text("No preview").size(12)).center(Fill).into(),
            _ => {
                let body = column![
                    text(&self.preview.title).size(13),
                    text(&self.preview.metadata)
                        .size(11)
                        .color(self.secondary_text_color()),
                    text(&self.preview.text).size(12).font(
                        if self.preview.kind == PreviewKind::Text {
                            MONO_FONT
                        } else {
                            UI_FONT
                        }
                    ),
                ]
                .spacing(7);
                scrollable(container(body).padding(10)).height(Fill).into()
            }
        };
        container(content)
            .width(Fill)
            .height(Fill)
            .style(preview_background_style)
            .into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let height = self.output_expansion.interpolate(
            STATUS_HEIGHT,
            self.command_output_height,
            self.animation_now,
        );
        let content: Element<'_, Message> = if let Some((summary, detail)) = &self.command_output {
            let header = row![
                text(summary).font(MONO_FONT_SEMIBOLD).size(11).width(Fill),
                button(text("×").font(UI_FONT).size(17))
                    .on_press(Message::CloseOutput)
                    .width(26)
                    .height(25)
                    .padding(0)
                    .style(output_close_button_style),
            ]
            .height(29)
            .align_y(Alignment::Center);
            let output = column![
                header,
                scrollable(
                    text(detail)
                        .font(MONO_FONT)
                        .size(12)
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
        } else {
            let status: Element<'_, Message> = match self.input_mode {
                InputMode::Search => {
                    let prefix = if self.explorer.recursive_search_active {
                        "//"
                    } else {
                        "/"
                    };
                    row![
                        text(prefix).size(12).color(self.accent_color()),
                        text_input("", &self.search_text)
                            .id(Id::new(SEARCH_ID))
                            .on_input(Message::SearchChanged)
                            .on_submit(Message::SearchSubmitted)
                            .font(MONO_FONT)
                            .size(12)
                            .padding(0)
                            .style(status_input_style)
                            .width(Fill),
                        self.search_count_view(),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
                }
                InputMode::Command(prefix) => row![
                    text(prefix.to_string()).size(12).color(self.accent_color()),
                    text_input("", &self.command_text)
                        .id(Id::new(COMMAND_ID))
                        .on_input(Message::CommandChanged)
                        .on_submit(Message::CommandSubmitted)
                        .font(MONO_FONT)
                        .size(12)
                        .padding(0)
                        .style(status_input_style)
                        .width(Fill),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
                _ => row![
                    text(if self.busy || self.navigation_loading {
                        "◌"
                    } else {
                        ""
                    })
                    .size(12)
                    .color(self.accent_color()),
                    text(&self.status)
                        .size(11)
                        .color(self.secondary_text_color())
                        .width(Fill),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into(),
            };
            container(status)
                .width(Fill)
                .center_y(Fill)
                .padding(Padding::from([0, CONTENT_GUTTER as u16]))
                .into()
        };
        container(content)
            .width(Fill)
            .height(Length::Fixed(height))
            .clip(true)
            .style(status_background_style)
            .into()
    }

    fn search_count_view(&self) -> Element<'_, Message> {
        if !self.explorer.recursive_search_active || self.search_text.is_empty() {
            return Space::new().into();
        }
        let label = if self.explorer.recursive_search_loading {
            "searching…".to_owned()
        } else if self.explorer.recursive_search_truncated {
            "1000+ matches".to_owned()
        } else {
            format!("{} matches", self.explorer.entries.len())
        };
        text(label)
            .size(11)
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

    fn dialog_view(&self) -> Element<'_, Message> {
        let (title, body, confirm): (&str, Element<'_, Message>, &str) = match &self.dialog {
            DialogState::Name {
                title,
                value,
                error,
            } => (
                title,
                column![
                    text_input("Name", value)
                        .id(Id::new(DIALOG_ID))
                        .on_input(Message::DialogInputChanged)
                        .on_submit(Message::DialogSubmit)
                        .padding(8),
                    text(error).size(12).color(Color::from_rgb8(196, 43, 28)),
                ]
                .spacing(6)
                .into(),
                title,
            ),
            DialogState::Trash { message } => (
                "Move to Trash",
                text(message).size(14).into(),
                "Move to Trash",
            ),
            DialogState::PermanentDelete { message, detail } => (
                "Moving to Trash failed",
                column![
                    text(message).size(14),
                    text(detail).size(12).color(self.secondary_text_color())
                ]
                .spacing(7)
                .into(),
                "Delete Permanently",
            ),
            DialogState::Error { message } => ("Error", text(message).size(13).into(), "Close"),
            DialogState::None => return Space::new().into(),
        };
        let cancel: Element<'_, Message> = if matches!(self.dialog, DialogState::Error { .. }) {
            Space::new().into()
        } else {
            button("Cancel")
                .on_press_maybe((!self.busy).then_some(Message::DialogCancel))
                .style(button::secondary)
                .into()
        };
        let actions = row![
            Space::new().width(Fill).height(1),
            cancel,
            button(confirm)
                .on_press_maybe((!self.busy).then_some(Message::DialogConfirm))
                .style(button::primary),
        ]
        .spacing(8);
        let panel = container(
            column![
                text(title).size(18),
                body,
                Space::new().width(1).height(Fill),
                actions
            ]
            .spacing(12),
        )
        .width(410)
        .height(if matches!(self.dialog, DialogState::Name { .. }) {
            210
        } else {
            225
        })
        .padding(20)
        .style(dialog_panel_style);
        opaque(
            container(panel)
                .center(Fill)
                .width(Fill)
                .height(Fill)
                .style(dialog_scrim_style),
        )
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
}

async fn run_blocking<T, F>(lane: Arc<Semaphore>, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let _permit = lane
        .acquire_owned()
        .await
        .map_err(|_| "operation queue closed".to_owned())?;
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("background task failed: {error}"))?
}

fn preview_view(entry: &FileEntry, data: PreviewData) -> PreviewView {
    let title = fs::display_name(&entry.name);
    match data {
        PreviewData::Directory(entries) => PreviewView {
            title,
            entries,
            kind: PreviewKind::Directory,
            ..PreviewView::default()
        },
        PreviewData::Text {
            metadata,
            mut text,
            truncated,
        } => {
            if truncated {
                text.push_str("\n\n… preview truncated");
            }
            PreviewView {
                title,
                metadata,
                text,
                kind: PreviewKind::Text,
                entries: Vec::new(),
            }
        }
        PreviewData::Metadata(metadata) => PreviewView {
            title,
            metadata,
            text: "Binary file".to_owned(),
            kind: PreviewKind::Metadata,
            entries: Vec::new(),
        },
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

fn deletion_confirmation(entries: &[FileEntry]) -> String {
    if let [entry] = entries {
        format!("Move “{}” to Trash?", fs::display_name(&entry.name))
    } else {
        format!("Move {} selected items to Trash?", entries.len())
    }
}

fn permanent_delete_confirmation(count: usize) -> String {
    if count == 1 {
        "Permanently delete this item instead?".to_owned()
    } else {
        format!("Permanently delete these {count} items instead?")
    }
}

fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn command_output_height(detail: &str) -> f32 {
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

fn preview_background_style(theme: &Theme) -> container::Style {
    let mut color = theme.palette().background;
    color = Color::from_rgba(
        (color.r + 0.02).min(1.0),
        (color.g + 0.02).min(1.0),
        (color.b + 0.02).min(1.0),
        1.0,
    );
    container::Style::default().background(color)
}

fn status_background_style(theme: &Theme) -> container::Style {
    container::Style::default().background(lighter(theme.palette().background, 16))
}

fn tree_button_style(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if selected {
            Some(Background::Color(with_alpha(theme.palette().primary, 0.28)))
        } else if hovered {
            Some(Background::Color(with_alpha(theme.palette().text, 0.08)))
        } else {
            None
        },
        text_color: theme.palette().text,
        border: Border {
            radius: 5.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn tile_style(theme: &Theme, selected: bool, hovered: bool) -> container::Style {
    let mut style = container::Style::default();
    if selected {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.45)));
    } else if hovered {
        style.background = Some(Background::Color(lighter(theme.palette().background, 16)));
    }
    style.border = Border {
        radius: 7.0.into(),
        ..Border::default()
    };
    style
}

fn ranger_row_style(theme: &Theme, selected: bool, hovered: bool) -> container::Style {
    if selected {
        container::Style::default()
            .background(with_alpha(theme.palette().primary, 0.45))
            .border(Border {
                radius: 5.0.into(),
                ..Border::default()
            })
    } else if hovered {
        container::Style::default()
            .background(lighter(theme.palette().background, 16))
            .border(Border {
                radius: 5.0.into(),
                ..Border::default()
            })
    } else {
        container::Style::default().border(Border {
            radius: 5.0.into(),
            ..Border::default()
        })
    }
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

fn output_close_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let opacity = match status {
        button::Status::Pressed => 0.45,
        button::Status::Hovered => 1.0,
        _ => 0.62,
    };
    button::Style {
        text_color: with_alpha(theme.palette().text, opacity),
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

fn dialog_panel_style(theme: &Theme) -> container::Style {
    let mut style = menu_style(theme);
    style.border.radius = 8.0.into();
    style
}

fn dialog_scrim_style(_theme: &Theme) -> container::Style {
    container::Style::default().background(Color::from_rgba8(0, 0, 0, 0.44))
}
