mod browser_input;
mod command;
mod diagnostics;
mod directory_watch;
mod drag_hover;
mod file_operation;
mod grid;
mod navigation;
mod operations;
mod places;
mod properties;
mod recent;
mod search;
mod shell;
mod startup;
mod state;
mod templates;
mod thumbnail;
mod transfer_queue;
mod trash;
mod tree;
mod view_preferences;

#[cfg(target_os = "linux")]
mod native_clipboard;
#[cfg(target_os = "linux")]
mod native_dnd;
#[cfg(target_os = "linux")]
mod x11_clipboard;
mod x11_dnd;

#[cfg(test)]
mod tests;

use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use crate::{
    fs, journal, theme,
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
    CONTENT_GUTTER, DropZone, GridInteraction, LIST_ROW_HEIGHT, LIST_VIEW_TOP_INSET, Motion,
    SIDEBAR_WIDTH, TILE_ROW_HEIGHT, TILE_WIDTH, TOOLBAR_HEIGHT,
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
use tree::{TreeRow, find_node, find_node_mut, flatten_rows, mounted_roots};

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
const SIDEBAR_SCROLL_ID: &str = "sidebar-scroll";
const X11_INBOUND_ID: u64 = u64::MAX;

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
    Toolbar,
    Location,
    Sidebar,
    #[default]
    Entries,
    BottomBar,
}

impl BrowserFocus {
    const ORDER: [Self; 5] = [
        Self::Toolbar,
        Self::Location,
        Self::Sidebar,
        Self::Entries,
        Self::BottomBar,
    ];

    fn moved(self, reverse: bool) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        let next = if reverse {
            index.checked_sub(1).unwrap_or(Self::ORDER.len() - 1)
        } else {
            (index + 1) % Self::ORDER.len()
        };
        Self::ORDER[next]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VirtualLocation {
    Recent,
    Trash,
}

#[derive(Clone, Debug)]
enum Message {
    Event(iced::Event, event::Status),
    FindWindow,
    WindowAvailable(Option<window::Id>),
    WindowResized(Size),
    NativeReady(Result<native_clipboard::Attached, String>),
    NativeDndEvent(TransferEvent),
    X11DropReady(u64),
    ExternalDragFinished(Result<TransferOutcome, String>),
    TransferBatchFinished {
        id: u64,
        request: TransferRequest,
        outcome: Box<fs::TransferBatchOutcome>,
    },
    PollTransfer,
    CancelTransfer,
    RetryTransfer,
    ToggleTransferHistory,
    CopyTransferReport,
    CopyCommandReport,
    SystemTheme(iced::theme::Mode),
    PollSystem,
    DirectoryChanged(directory_watch::Event),
    Refresh,
    ToggleView,
    CycleSort,
    ToggleSortDirection,
    ToggleFoldersFirst,
    ToggleHidden,
    ToggleClickActivation,
    Parent,
    Back,
    Forward,
    LocationChanged(String),
    LocationSubmitted,
    Breadcrumb(PathBuf),
    TreeRow(u64),
    SidebarScrolled(f32),
    TreeLoaded(u64, PathBuf, Vec<PathBuf>),
    FavoritePressed(usize),
    FavoriteReleased(usize),
    EntryPressed(usize),
    EntryReleased(usize),
    EntryHovered(usize),
    EntryUnhovered(usize),
    EntryDoubleClicked(usize),
    EntryContext(usize),
    ContextNewFolder,
    ContextNewFile(Option<PathBuf>, String, String),
    ContextProperties,
    ContextOpenWith,
    ContextRename,
    ContextTrash,
    ContextRestore,
    ContextDeletePermanent,
    ContextEmptyTrash,
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
    ThumbnailLoaded(thumbnail::Loaded),
    SearchChanged(String),
    SearchSubmitted,
    SearchFinished(Result<fs::SearchResults, String>),
    CommandChanged(String),
    CommandSubmitted,
    CommandFinished(Result<command::Completion, String>),
    VolumeFinished(Result<String, String>),
    RecentLoaded(Option<Result<Vec<FileEntry>, String>>),
    TrashLoaded(Option<Result<Vec<trash::Entry>, String>>),
    RestoreFinished {
        entries: Vec<trash::Entry>,
        outcome: Box<fs::TransferBatchOutcome>,
    },
    PropertiesFinished(Result<properties::Info, String>),
    MetadataFinished(Result<String, String>),
    AnimationFrame(Instant),
    RenameChanged(String),
    RenameSubmitted,
    PromptInputChanged(String),
    PromptSubmit,
    PromptConfirm,
    PromptCancel,
    FileOperationFinished(file_operation::Completion),
    JournalFinished {
        journal: Box<journal::Journal>,
        result: Result<journal::Effect, String>,
    },
    Copy,
    Paste,
    ClipboardRead(Result<ClipboardImport, String>),
    OperationError(String),
    Noop,
}

pub fn run() -> iced::Result {
    let window = startup::State::open_default().window_settings();
    application(App::new, App::update, App::view)
        .title("PolarExp")
        .settings(iced::Settings {
            id: Some("io.github.powerpenguini.PolarExp".to_owned()),
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
    startup: startup::State,
    places: places::Places,
    recent: recent::Recent,
    trash: trash::Trash,
    trash_entries: Vec<trash::Entry>,
    templates: Vec<templates::Template>,
    virtual_location: Option<VirtualLocation>,
    operations: Operations,
    search: SearchSession,
    transfers: TransferWorkflow,
    transfer_conflict: Option<ActiveTransferConflict>,
    restore_conflict: Option<ActiveRestoreConflict>,
    transfer_queue: transfer_queue::Queue,
    command: CommandSession,
    command_adapter: ProcessAdapter,
    diagnostics: diagnostics::History,
    file_operations: FileOperationSession,
    journal: journal::Journal,
    directory_watch: Option<directory_watch::Source>,
    watch_poll_fallback: bool,
    view_preferences: view_preferences::Preferences,
    thumbnails: thumbnail::Cache,
    trash_adapter: GioTrashAdapter,
    grid: GridInteraction,
    drag_hover: drag_hover::State,
    browser_input: BrowserInput,
    browser_focus: BrowserFocus,
    toolbar_cursor: usize,
    breadcrumb_cursor: usize,
    bottom_cursor: usize,
    sidebar_cursor: Option<u64>,
    favorite_drag: Option<usize>,
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
    context_menu_cursor: usize,
    native_dnd: Option<native_clipboard::DndSource>,
    native_clipboard: Option<native_clipboard::Source>,
    native_dnd_error: Option<String>,
    x11_drop_paths: Vec<PathBuf>,
    x11_drop_generation: u64,
    x11_drop_action: TransferAction,
    modifiers: keyboard::Modifiers,
    system_mode: iced::theme::Mode,
    system_accessibility: theme::AccessibilityPreferences,
    accent: Option<theme::ThemeColors>,
}

#[derive(Clone, Debug)]
struct ActiveTransferConflict {
    request: TransferRequest,
    batch: fs::TransferBatch,
}

#[derive(Clone, Debug)]
struct ActiveRestoreConflict {
    batch: fs::TransferBatch,
    entries: Vec<trash::Entry>,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let now = Instant::now();
        let startup = startup::State::open_default();
        let view_preferences = view_preferences::Preferences::open_default();
        let current =
            startup.initial_directory(view_preferences.remember_last_directory_on_startup());
        let places = places::Places::open_default();
        let recent = recent::Recent::open_default();
        let trash = trash::Trash::open_default();
        let mut explorer = ExplorerState::new(mounted_roots());
        let mut locations = places.entries();
        locations.extend(recent.sidebar_entry());
        locations.push(trash.sidebar_entry());
        explorer.install_places(locations);
        let interface_settings = theme::interface_settings();
        let accent = theme::load(interface_settings.as_ref());
        let system_accessibility = theme::accessibility(interface_settings.as_ref());
        let (journal, journal_error) = match journal::Journal::open_default() {
            Ok(journal) => (journal, None),
            Err(error) => (journal::Journal::empty_default(), Some(error)),
        };
        let (directory_watch, watch_error) = match directory_watch::Source::new() {
            Ok(source) => (Some(source), None),
            Err(error) => (None, Some(error)),
        };
        let startup_error = startup
            .error()
            .map(str::to_owned)
            .or(journal_error)
            .or(watch_error);
        let mut app = Self {
            explorer,
            navigation: NavigationSession::new(current.clone()),
            startup,
            places,
            recent,
            trash,
            trash_entries: Vec::new(),
            templates: templates::discover(),
            virtual_location: None,
            operations: Operations::default(),
            search: SearchSession::default(),
            transfers: TransferWorkflow::default(),
            transfer_conflict: None,
            restore_conflict: None,
            transfer_queue: transfer_queue::Queue::open_default(),
            command: CommandSession::default(),
            command_adapter: ProcessAdapter,
            diagnostics: diagnostics::History::open_default(),
            file_operations: FileOperationSession::default(),
            journal,
            directory_watch,
            watch_poll_fallback: false,
            view_preferences,
            thumbnails: thumbnail::Cache::default(),
            trash_adapter: GioTrashAdapter,
            grid: GridInteraction::default(),
            drag_hover: drag_hover::State::default(),
            browser_input: BrowserInput::default(),
            browser_focus: BrowserFocus::Entries,
            toolbar_cursor: 0,
            breadcrumb_cursor: 0,
            bottom_cursor: 0,
            sidebar_cursor: None,
            favorite_drag: None,
            location_input: current.display().to_string(),
            expanded_bar_height: STATUS_HEIGHT,
            output_expansion: Animation::new(false)
                .duration(Duration::from_millis(140))
                .easing(Easing::EaseOut),
            animation_now: now,
            spinner_started: now,
            status: startup_error.unwrap_or_default(),
            status_notice: None,
            busy: false,
            navigation_loading: false,
            context_menu: None,
            context_menu_cursor: 0,
            native_dnd: None,
            native_clipboard: None,
            native_dnd_error: None,
            x11_drop_paths: Vec::new(),
            x11_drop_generation: 0,
            x11_drop_action: TransferAction::Copy,
            modifiers: keyboard::Modifiers::default(),
            system_mode: iced::theme::Mode::Dark,
            system_accessibility,
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
        let animation = if !self.reduced_motion()
            && (self.output_expansion.is_animating(self.animation_now)
                || self.spinner_active()
                || self.drag_in_progress())
        {
            time::every(Duration::from_millis(16)).map(Message::AnimationFrame)
        } else if self.reduced_motion() && self.drag_in_progress() {
            time::every(Duration::from_millis(100)).map(Message::AnimationFrame)
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
        if let Some(source) = &self.directory_watch {
            subscriptions.push(source.subscription().map(Message::DirectoryChanged));
        }
        if self.transfer_queue.active_id().is_some() {
            subscriptions
                .push(time::every(Duration::from_millis(100)).map(|_| Message::PollTransfer));
        }
        Subscription::batch(subscriptions)
    }

    fn iced_theme(&self) -> Theme {
        let dark = self.system_mode == iced::theme::Mode::Dark;
        let accent = self
            .accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent);
        let palette = if self.high_contrast() {
            iced::theme::Palette {
                background: Color::BLACK,
                text: Color::WHITE,
                primary: Color::from_rgb8(0, 220, 255),
                success: Color::from_rgb8(80, 255, 120),
                danger: Color::from_rgb8(255, 80, 80),
                warning: Color::from_rgb8(255, 230, 0),
            }
        } else if dark {
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
            || self.transfer_queue.active_id().is_some()
            || self.navigation_loading
            || self.search.is_loading()
            || flatten_rows(&self.explorer, self.navigation.current())
                .iter()
                .any(|row| row.loading)
    }

    fn spinner(&self, size: f32) -> widget::Svg<'static> {
        let angle = if self.reduced_motion() {
            0.0
        } else {
            self.animation_now
                .duration_since(self.spinner_started)
                .as_secs_f32()
                * std::f32::consts::TAU
                / 0.9
        };
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

    fn high_contrast(&self) -> bool {
        self.view_preferences
            .high_contrast()
            .resolve(self.system_accessibility.high_contrast)
    }

    fn reduced_motion(&self) -> bool {
        self.view_preferences
            .reduced_motion()
            .resolve(self.system_accessibility.reduced_motion)
    }

    fn reduced_transparency(&self) -> bool {
        self.view_preferences
            .reduced_transparency()
            .resolve(self.system_accessibility.reduced_transparency)
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
            Message::TransferBatchFinished {
                id,
                request,
                outcome,
            } => self.finish_transfer_batch(id, request, *outcome),
            Message::PollTransfer => Task::none(),
            Message::CancelTransfer => {
                self.browser_focus = BrowserFocus::BottomBar;
                self.bottom_cursor = 0;
                if self.transfer_conflict.is_some() {
                    return self.cancel_transfer_conflict();
                }
                if self.transfer_queue.cancel() {
                    self.status = "Cancelling active transfer…".to_owned();
                }
                Task::none()
            }
            Message::RetryTransfer => {
                self.browser_focus = BrowserFocus::BottomBar;
                match self.transfer_queue.retry() {
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
                self.transfer_queue.toggle_expanded();
                self.sync_bottom_bar();
                Task::none()
            }
            Message::CopyTransferReport => {
                self.browser_focus = BrowserFocus::BottomBar;
                iced::clipboard::write(self.transfer_queue.report_text())
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
                let fallback = if self.watch_poll_fallback {
                    let expanded = tree::expanded_paths(&self.explorer.roots);
                    let location = if self.search.is_recursive() {
                        Task::none()
                    } else {
                        self.refresh_location()
                    };
                    Task::batch([self.invalidate_tree(expanded), location])
                } else {
                    Task::none()
                };
                Task::batch([search, fallback])
            }
            Message::DirectoryChanged(event) => {
                let cut_parent_changed = self
                    .transfers
                    .pending_cut_paths()
                    .iter()
                    .filter_map(|path| path.parent())
                    .any(|parent| parent == event.path);
                if cut_parent_changed
                    && let Some((generation, removed)) =
                        self.transfers.reconcile_pending_cut(&event.moved_out)
                {
                    self.sync_native_cut_clipboard(generation);
                    self.sync_directory_watches();
                    let remaining = self.transfers.pending_cut_paths().len();
                    self.status_notice = Some(if remaining == 0 {
                        format!("External move confirmed for {removed} item(s); Cut completed")
                    } else {
                        format!(
                            "External move confirmed for {removed} item(s); {remaining} still pending"
                        )
                    });
                }
                let current =
                    self.virtual_location.is_none() && event.path == self.navigation.current();
                let expanded = tree::expanded_paths(&self.explorer.roots).contains(&event.path);
                let virtual_location = self.virtual_watch_paths().contains(&event.path);
                Task::batch([
                    if current {
                        self.live_refresh()
                    } else {
                        Task::none()
                    },
                    if expanded {
                        self.invalidate_tree(vec![event.path])
                    } else {
                        Task::none()
                    },
                    if virtual_location {
                        self.refresh_location()
                    } else {
                        Task::none()
                    },
                ])
            }
            Message::Refresh => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 3;
                self.refresh_location()
            }
            Message::ToggleView => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 4;
                self.change_view_options(|options| {
                    options.view = match options.view {
                        fs::ViewMode::Grid => fs::ViewMode::List,
                        fs::ViewMode::List => fs::ViewMode::Grid,
                    };
                })
            }
            Message::CycleSort => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 5;
                self.change_view_options(|options| {
                    options.sort = match options.sort {
                        fs::SortKey::Name => fs::SortKey::Modified,
                        fs::SortKey::Modified => fs::SortKey::Size,
                        fs::SortKey::Size => fs::SortKey::Type,
                        fs::SortKey::Type => fs::SortKey::Name,
                    };
                })
            }
            Message::ToggleSortDirection => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 6;
                self.change_view_options(|options| options.descending = !options.descending)
            }
            Message::ToggleFoldersFirst => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 7;
                self.change_view_options(|options| options.folders_first = !options.folders_first)
            }
            Message::ToggleHidden => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 8;
                self.change_view_options(|options| options.show_hidden = !options.show_hidden)
            }
            Message::ToggleClickActivation => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 9;
                self.view_preferences.toggle_single_click_activation();
                self.status = format!(
                    "{}-click activation enabled",
                    if self.view_preferences.single_click_activation() {
                        "Single"
                    } else {
                        "Double"
                    }
                );
                Task::none()
            }
            Message::Parent => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 0;
                self.go_parent()
            }
            Message::Back => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 1;
                self.go_back()
            }
            Message::Forward => {
                self.browser_focus = BrowserFocus::Toolbar;
                self.toolbar_cursor = 2;
                self.go_forward()
            }
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
            Message::Breadcrumb(path) => {
                self.browser_focus = BrowserFocus::Location;
                self.breadcrumb_cursor = breadcrumb_segments(self.navigation.current())
                    .iter()
                    .position(|(_, candidate)| candidate == &path)
                    .unwrap_or_default();
                self.navigate(path, true, None)
            }
            Message::TreeRow(id) => {
                self.browser_focus = BrowserFocus::Sidebar;
                self.sidebar_cursor = Some(id);
                self.activate_tree_row(id)
            }
            Message::SidebarScrolled(y) => {
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
            Message::RecentLoaded(result) => {
                self.busy = false;
                let Some(result) = result else {
                    return Task::none();
                };
                match result {
                    Ok(entries) => {
                        self.virtual_location = Some(VirtualLocation::Recent);
                        self.sync_directory_watches();
                        self.navigation.replace_displayed_entries(entries);
                        self.grid.select_only(None, self.navigation.entries().len());
                        self.grid.clear_details();
                        self.grid.reset_scroll();
                        self.location_input = "Recent".to_owned();
                        self.status =
                            format!("{} items  •  Recent", self.navigation.entries().len());
                        self.load_visible_thumbnails()
                    }
                    Err(error) => {
                        self.status = error;
                        Task::none()
                    }
                }
            }
            Message::TrashLoaded(result) => {
                self.busy = false;
                let Some(result) = result else {
                    return Task::none();
                };
                match result {
                    Ok(entries) => {
                        self.virtual_location = Some(VirtualLocation::Trash);
                        self.sync_directory_watches();
                        self.navigation.replace_displayed_entries(
                            entries.iter().map(|entry| entry.file.clone()).collect(),
                        );
                        self.trash_entries = entries;
                        self.grid.select_only(None, self.navigation.entries().len());
                        self.grid.clear_details();
                        self.grid.reset_scroll();
                        self.location_input = "Trash".to_owned();
                        self.status = format!("{} items  •  Trash", self.trash_entries.len());
                        Task::none()
                    }
                    Err(error) => {
                        self.status = error;
                        Task::none()
                    }
                }
            }
            Message::RestoreFinished { entries, outcome } => self.finish_restore(entries, *outcome),
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
                        self.sync_directory_watches();
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
                transfer_conflict: self.transfer_conflict.is_some()
                    || self.restore_conflict.is_some(),
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
                    && self.virtual_location.is_none(),
            },
        );
        self.apply_input_intent(intent)
    }

    fn apply_input_intent(&mut self, intent: InputIntent) -> Task<Message> {
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
            InputIntent::ToggleHidden => {
                self.change_view_options(|options| options.show_hidden = !options.show_hidden)
            }
            InputIntent::BeginLocation => self.begin_location(),
            InputIntent::MoveFocus { reverse } => {
                self.browser_focus = self.browser_focus.moved(reverse);
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

    fn focus_label(&self) -> &'static str {
        match self.browser_focus {
            BrowserFocus::Toolbar => "toolbar",
            BrowserFocus::Location => "location",
            BrowserFocus::Sidebar => "sidebar",
            BrowserFocus::Entries => "files",
            BrowserFocus::BottomBar => "bottom bar",
        }
    }

    fn move_focused(&mut self, motion: Motion, count: usize, extend: bool) -> Task<Message> {
        match self.browser_focus {
            BrowserFocus::Toolbar => {
                move_composite_cursor(&mut self.toolbar_cursor, 10, motion, count);
                self.status = format!("Toolbar control {} of 10", self.toolbar_cursor + 1);
                Task::none()
            }
            BrowserFocus::Location => {
                let count = breadcrumb_segments(self.navigation.current()).len().max(1);
                move_composite_cursor(&mut self.breadcrumb_cursor, count, motion, 1);
                self.status = format!("Location segment {} of {count}", self.breadcrumb_cursor + 1);
                Task::none()
            }
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

    fn activate_focused(&mut self) -> Task<Message> {
        match self.browser_focus {
            BrowserFocus::Toolbar => {
                let message = match self.toolbar_cursor.min(9) {
                    0 => Message::Parent,
                    1 => Message::Back,
                    2 => Message::Forward,
                    3 => Message::Refresh,
                    4 => Message::ToggleView,
                    5 => Message::CycleSort,
                    6 => Message::ToggleSortDirection,
                    7 => Message::ToggleFoldersFirst,
                    8 => Message::ToggleHidden,
                    _ => Message::ToggleClickActivation,
                };
                self.update(message)
            }
            BrowserFocus::Location => {
                if self.virtual_location.is_some() {
                    self.status = "This virtual location has no parent breadcrumb".to_owned();
                    Task::none()
                } else {
                    let segments = breadcrumb_segments(self.navigation.current());
                    segments
                        .get(self.breadcrumb_cursor.min(segments.len().saturating_sub(1)))
                        .map(|(_, path)| path.clone())
                        .map_or_else(Task::none, |path| self.navigate(path, true, None))
                }
            }
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

    fn bottom_actions(&self) -> Vec<Message> {
        if self.command.output().is_some() {
            return vec![Message::CopyCommandReport];
        }
        let mut actions = Vec::new();
        if self.transfer_queue.active_id().is_some() {
            actions.push(Message::CancelTransfer);
        }
        if self.transfer_queue.has_retry() {
            actions.push(Message::RetryTransfer);
        }
        if self.transfer_queue.expanded() {
            actions.push(Message::CopyTransferReport);
        }
        if !actions.is_empty() || self.transfer_queue.expanded() {
            actions.push(Message::ToggleTransferHistory);
        }
        actions
    }

    fn context_actions(&self) -> Vec<(String, Message)> {
        if self.virtual_location == Some(VirtualLocation::Trash) {
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

    fn handle_context_key(&mut self, key: InputNamedKey, shift: bool) -> Task<Message> {
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

    fn request_navigation(&mut self, navigation: NavigationRequest) -> Task<Message> {
        let requested = navigation.requested().to_path_buf();
        let options = self.view_preferences.for_directory(&requested);
        self.navigation_loading = true;
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
        self.virtual_location = None;
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
            NavigationOutcome::Committed { selected, refresh } => {
                self.grid.set_list_mode(
                    self.view_preferences
                        .for_directory(self.navigation.current())
                        .view
                        == fs::ViewMode::List,
                );
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
                if !refresh {
                    self.grid.reset_scroll();
                }
                self.sync_directory_watches();
                self.status.clear();
                Task::batch([
                    self.load_root_if_needed(),
                    self.schedule_details(),
                    self.load_visible_thumbnails(),
                ])
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

    fn go_parent(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() {
            return Task::none();
        }
        if self.virtual_location.is_some() {
            let current = self.navigation.current().to_path_buf();
            return self.navigate(current, false, None);
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
        if self.virtual_location.is_some() {
            let current = self.navigation.current().to_path_buf();
            return self.navigate(current, false, None);
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
        if self.virtual_location.is_some() {
            let current = self.navigation.current().to_path_buf();
            return self.navigate(current, false, None);
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

    fn live_refresh(&mut self) -> Task<Message> {
        if self.navigation_loading {
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

    fn refresh_location(&mut self) -> Task<Message> {
        match self.virtual_location {
            Some(VirtualLocation::Recent) => self.open_recent(),
            Some(VirtualLocation::Trash) => self.open_trash(),
            None => self.live_refresh(),
        }
    }

    fn open_recent(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() || self.busy || self.navigation_loading {
            return Task::none();
        }
        self.cancel_search_state();
        self.busy = true;
        self.status = "Reading shared Recent history…".to_owned();
        let recent = self.recent.clone();
        Task::perform(
            self.operations
                .run(OperationKind::Navigation, move |_| recent.entries()),
            |completion| match completion {
                Completion::Finished(result) => Message::RecentLoaded(Some(result)),
                Completion::Cancelled => Message::RecentLoaded(None),
            },
        )
    }

    fn open_trash(&mut self) -> Task<Message> {
        if self.prompt_blocks_action() || self.busy || self.navigation_loading {
            return Task::none();
        }
        self.cancel_search_state();
        self.busy = true;
        self.status = "Reading Trash…".to_owned();
        let trash = self.trash.clone();
        Task::perform(
            self.operations
                .run(OperationKind::Navigation, move |_| trash.entries()),
            |completion| match completion {
                Completion::Finished(result) => Message::TrashLoaded(Some(result)),
                Completion::Cancelled => Message::TrashLoaded(None),
            },
        )
    }

    fn install_locations(&mut self) {
        let mut entries = self.places.entries();
        entries.extend(self.recent.sidebar_entry());
        entries.push(self.trash.sidebar_entry());
        self.explorer.install_places(entries);
    }

    fn change_view_options(
        &mut self,
        change: impl FnOnce(&mut fs::BrowseOptions),
    ) -> Task<Message> {
        let current = self.navigation.current().to_path_buf();
        let mut options = self.view_preferences.for_directory(&current);
        change(&mut options);
        self.view_preferences.set_directory(current, options);
        if self.virtual_location.is_some() {
            self.grid.set_list_mode(options.view == fs::ViewMode::List);
            return self.load_visible_thumbnails();
        }
        self.live_refresh()
    }

    fn load_visible_thumbnails(&mut self) -> Task<Message> {
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

    fn begin_location(&mut self) -> Task<Message> {
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
        self.drag_hover.cancel();
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

    fn activate_tree_row(&mut self, id: u64) -> Task<Message> {
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
        self.sync_directory_watches();
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
            CommandSubmission::Refresh => self.live_refresh(),
            CommandSubmission::Diagnostics => {
                self.command.show_diagnostics(self.diagnostics.report());
                self.sync_bottom_bar();
                Task::none()
            }
            CommandSubmission::Settings { local, arguments } => {
                match self.view_preferences.apply_command(
                    self.navigation.current(),
                    local,
                    &arguments,
                ) {
                    Ok(status) => {
                        if arguments.is_empty() || arguments == "all" {
                            self.command.show_settings(status);
                            self.sync_bottom_bar();
                            return Task::none();
                        }
                        self.status = status;
                        self.live_refresh()
                    }
                    Err(error) => {
                        self.status = error;
                        Task::none()
                    }
                }
            }
            CommandSubmission::Favorite(arguments) => {
                match self.places.command(self.navigation.current(), &arguments) {
                    Ok(status) => {
                        self.install_locations();
                        if arguments.is_empty() || arguments == "list" {
                            self.command.show_settings(status);
                            self.sync_bottom_bar();
                        } else {
                            self.status = status;
                        }
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            CommandSubmission::Recent(arguments) => {
                match self.recent.command(&arguments) {
                    Ok((effect, status)) => {
                        self.status = status;
                        self.install_locations();
                        match effect {
                            recent::Effect::Open => return self.open_recent(),
                            recent::Effect::Reload
                                if self.virtual_location == Some(VirtualLocation::Recent) =>
                            {
                                return self.open_recent();
                            }
                            recent::Effect::Disabled
                                if self.virtual_location == Some(VirtualLocation::Recent) =>
                            {
                                let current = self.navigation.current().to_path_buf();
                                return self.navigate(current, false, None);
                            }
                            recent::Effect::Reload
                            | recent::Effect::Disabled
                            | recent::Effect::Enabled => {}
                        }
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            CommandSubmission::Volume(arguments) => {
                self.busy = true;
                self.status = "Waiting for desktop volume authorization…".to_owned();
                Task::perform(
                    self.operations.run(OperationKind::Background, move |_| {
                        places::run_volume_command(&arguments)
                    }),
                    |completion| match completion {
                        Completion::Finished(result) => Message::VolumeFinished(result),
                        Completion::Cancelled => Message::Noop,
                    },
                )
            }
            CommandSubmission::Properties => self.show_properties(),
            CommandSubmission::Chmod(mode) => self.run_permission_change(mode),
            CommandSubmission::OpenWith {
                application,
                default,
            } => self.run_open_with(application, default),
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
        if let Some((summary, detail)) = command_failure_report(&result) {
            self.diagnostics.record(summary, detail);
        }
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
        let expanded_height = self.desired_expanded_height();
        if let Some(height) = expanded_height {
            self.expanded_bar_height = height;
        }
        self.animation_now = Instant::now();
        self.output_expansion
            .go_mut(expanded_height.is_some(), self.animation_now);
    }

    fn desired_expanded_height(&self) -> Option<f32> {
        self.command
            .output()
            .map(|output| expanded_bar_height(&output.detail))
            .or_else(|| {
                self.file_operations
                    .expanded_detail()
                    .map(expanded_bar_height)
            })
            .or_else(|| self.transfer_queue.expanded().then_some(190.0))
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

    fn show_new_file(
        &mut self,
        template: Option<PathBuf>,
        suggested_name: String,
        label: String,
    ) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.open_file_operation(move |session| {
            session.begin_new_file(template, suggested_name, label);
        });
        widget::operation::focus(Id::new(NEW_FOLDER_ID))
    }

    fn show_properties(&mut self) -> Task<Message> {
        let Some(path) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
            .map(|entry| entry.path.clone())
        else {
            self.status = "Select one entry to inspect Properties".to_owned();
            return Task::none();
        };
        self.command.close_output();
        self.busy = true;
        self.status = "Reading Properties…".to_owned();
        Task::perform(
            self.operations
                .run(OperationKind::Details, move |_| properties::read(&path)),
            |completion| match completion {
                Completion::Finished(result) => Message::PropertiesFinished(result),
                Completion::Cancelled => {
                    Message::PropertiesFinished(Err("Properties request was replaced".to_owned()))
                }
            },
        )
    }

    fn run_permission_change(&mut self, mode: String) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        let paths = self
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        if paths.is_empty() {
            self.status = "Select at least one entry before :chmod".to_owned();
            return Task::none();
        }
        self.busy = true;
        self.status = "Changing permissions…".to_owned();
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                properties::chmod(paths, &mode)
            }),
            |completion| match completion {
                Completion::Finished(result) => Message::MetadataFinished(result),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn run_open_with(&mut self, application: String, make_default: bool) -> Task<Message> {
        let Some(path) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
            .map(|entry| entry.path.clone())
        else {
            self.status = "Select one entry first".to_owned();
            return Task::none();
        };
        self.busy = true;
        self.status = if make_default {
            "Changing the default application…".to_owned()
        } else {
            "Opening with selected application…".to_owned()
        };
        let operation = if make_default {
            OperationKind::Mutation
        } else {
            OperationKind::Background
        };
        Task::perform(
            self.operations.run(operation, move |_| {
                properties::open_with(path, &application, make_default)
            }),
            |completion| match completion {
                Completion::Finished(result) => Message::MetadataFinished(result),
                Completion::Cancelled => Message::Noop,
            },
        )
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

    fn selected_trash_entries(&self) -> Vec<trash::Entry> {
        let selected = self
            .grid
            .selected_items(self.navigation.entries())
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        self.trash_entries
            .iter()
            .filter(|entry| selected.contains(&entry.file.path))
            .cloned()
            .collect()
    }

    fn restore_selected_trash(&mut self) -> Task<Message> {
        if self.virtual_location != Some(VirtualLocation::Trash)
            || self.busy
            || self.restore_conflict.is_some()
        {
            return Task::none();
        }
        let entries = self.selected_trash_entries();
        if entries.is_empty() {
            return Task::none();
        }
        self.launch_restore(trash::restore_batch(&entries), entries)
    }

    fn launch_restore(
        &mut self,
        batch: fs::TransferBatch,
        entries: Vec<trash::Entry>,
    ) -> Task<Message> {
        self.busy = true;
        self.status = format!("Restoring {} Trash items…", entries.len());
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |cancel| {
                Ok(batch.run_with(|| cancel.is_cancelled(), |_| {}))
            }),
            move |completion| match completion {
                Completion::Finished(Ok(outcome)) => Message::RestoreFinished {
                    entries,
                    outcome: Box::new(outcome),
                },
                Completion::Finished(Err(error)) => Message::OperationError(error),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn finish_restore(
        &mut self,
        entries: Vec<trash::Entry>,
        outcome: fs::TransferBatchOutcome,
    ) -> Task<Message> {
        self.busy = false;
        match outcome {
            fs::TransferBatchOutcome::Conflict { batch, conflict } => {
                let name = conflict
                    .destination
                    .file_name()
                    .map_or_else(|| "item".into(), |name| name.to_string_lossy());
                let kind = if conflict.directories {
                    "folder conflict; Replace merges"
                } else {
                    "file conflict"
                };
                self.status = format!(
                    "Restore {name}: {kind}  •  r Replace  s Skip  k Keep Both  •  uppercase applies to remaining  •  Esc cancel"
                );
                self.restore_conflict = Some(ActiveRestoreConflict {
                    batch: *batch,
                    entries,
                });
                Task::none()
            }
            fs::TransferBatchOutcome::Complete(report) => {
                let report = trash::finish_restore(report, &entries);
                match journal::Action::restore(&report.restored) {
                    Ok(Some(action)) => {
                        if let Err(error) = self.journal.record(action) {
                            self.status_notice =
                                Some(format!("Restore completed but Undo was not saved: {error}"));
                        }
                    }
                    Err(error) => {
                        self.status_notice = Some(format!(
                            "Restore completed but Undo is unavailable: {error}"
                        ));
                    }
                    Ok(None) => {}
                }
                let mut detail = report
                    .failures
                    .iter()
                    .map(|(path, error)| format!("{}: {error}", path.display()))
                    .chain(report.warnings.iter().cloned())
                    .collect::<Vec<_>>();
                if report.retained > 0 {
                    detail.push(format!("{} items stayed in Trash", report.retained));
                }
                self.status = format!(
                    "Restored {}  •  {} failed  •  {} kept",
                    report.restored.len(),
                    report.failures.len(),
                    report.retained
                );
                if !detail.is_empty() {
                    self.command.show_settings(detail.join("\n"));
                    self.sync_bottom_bar();
                }
                let changed = report
                    .restored
                    .iter()
                    .filter_map(|receipt| receipt.original.parent().map(Path::to_path_buf))
                    .collect::<Vec<_>>();
                let tree = self.invalidate_tree(changed);
                Task::batch([tree, self.open_trash()])
            }
        }
    }

    fn show_trash_delete_prompt(&mut self, empty: bool) -> Task<Message> {
        if self.virtual_location != Some(VirtualLocation::Trash) || self.busy {
            return Task::none();
        }
        let entries = if empty {
            self.trash_entries.clone()
        } else {
            self.selected_trash_entries()
        };
        self.open_file_operation(move |session| {
            session.begin_trash_delete(entries, empty);
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
        let trash_delete_report = match &completion {
            file_operation::Completion::TrashDelete(report) => Some(report.clone()),
            _ => None,
        };
        let journal_action = match &completion {
            file_operation::Completion::Name {
                kind: file_operation::NameKind::Rename { source },
                result: Ok(destination),
            } => journal::Action::rename(source.clone(), destination.clone()).map(Some),
            file_operation::Completion::Name {
                kind: file_operation::NameKind::NewFolder,
                result: Ok(path),
            } => journal::Action::new_folder(path.clone()).map(Some),
            file_operation::Completion::Name {
                kind:
                    file_operation::NameKind::NewFile {
                        template: Some(template),
                    },
                result: Ok(path),
            } => journal::Action::transfer(
                journal::TransferKind::Copy,
                &[fs::TransferReceipt {
                    source: template.clone(),
                    destination: path.clone(),
                }],
            ),
            file_operation::Completion::Name {
                kind: file_operation::NameKind::NewFile { template: None },
                result: Ok(path),
            } => journal::Action::new_file(path.clone()).map(Some),
            file_operation::Completion::Trash(completion) => {
                journal::Action::trash(&completion.receipts)
            }
            _ => Ok(None),
        };
        let consequences = self.file_operations.complete(completion);
        if let Some(report) = trash_delete_report {
            self.status = format!(
                "Permanently deleted {}  •  {} failed",
                report.deleted,
                report.failures.len()
            );
            if !report.failures.is_empty() {
                self.command.show_settings(
                    report
                        .failures
                        .iter()
                        .map(|(entry, error)| format!("{}: {error}", fs::display_name(&entry.name)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                self.sync_bottom_bar();
            }
        }
        if consequences.renamed {
            self.browser_input.leave_mode();
        }
        self.sync_bottom_bar();
        match journal_action {
            Ok(Some(action)) => {
                if let Err(error) = self.journal.record(action) {
                    self.status_notice = Some(format!(
                        "Operation completed but Undo was not saved: {error}"
                    ));
                }
            }
            Err(error) => {
                self.status_notice = Some(format!(
                    "Operation completed but Undo is unavailable: {error}"
                ));
            }
            Ok(None) => {}
        }
        if consequences.refresh {
            if self.virtual_location.is_some() {
                self.refresh_location()
            } else {
                Task::batch([
                    self.invalidate_tree(vec![self.navigation.current().to_path_buf()]),
                    self.refresh(consequences.select),
                ])
            }
        } else {
            Task::none()
        }
    }

    fn run_journal(&mut self, redo: bool) -> Task<Message> {
        if !self.mutations_allowed() {
            return Task::none();
        }
        self.busy = true;
        let mut journal = self.journal.clone();
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                let result = if redo { journal.redo() } else { journal.undo() };
                Ok((journal, result))
            }),
            |completion| match completion {
                Completion::Finished(Ok((journal, result))) => Message::JournalFinished {
                    journal: Box::new(journal),
                    result,
                },
                Completion::Finished(Err(error)) => Message::OperationError(error),
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn finish_journal(
        &mut self,
        journal: journal::Journal,
        result: Result<journal::Effect, String>,
    ) -> Task<Message> {
        self.busy = false;
        self.journal = journal;
        match result {
            Ok(effect) => {
                self.status = effect.status;
                let tree = self.invalidate_tree(effect.changed_folders);
                let refresh = self.refresh(effect.select);
                Task::batch([tree, refresh])
            }
            Err(error) => {
                self.status = error;
                Task::none()
            }
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
            self.sync_directory_watches();
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
        self.sync_directory_watches();
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
        self.sync_directory_watches();
        self.status = status.to_owned();
        self.refresh(None)
    }

    fn sync_directory_watches(&mut self) {
        let Some(source) = &self.directory_watch else {
            return;
        };
        let mut paths = vec![self.navigation.current().to_path_buf()];
        paths.extend(
            self.transfers
                .pending_cut_paths()
                .iter()
                .filter_map(|path| path.parent().map(Path::to_path_buf)),
        );
        paths.extend(tree::expanded_paths(&self.explorer.roots));
        paths.extend(self.virtual_watch_paths());
        paths.retain(|path| path.is_dir());
        let mut seen = HashSet::new();
        paths.retain(|path| seen.insert(path.clone()));
        self.watch_poll_fallback = source.watch_many(paths);
    }

    fn virtual_watch_paths(&self) -> Vec<PathBuf> {
        match self.virtual_location {
            Some(VirtualLocation::Recent) => self.recent.watch_paths(self.navigation.entries()),
            Some(VirtualLocation::Trash) => {
                let mounts = self
                    .explorer
                    .roots
                    .iter()
                    .filter(|node| node.kind == state::NodeKind::Drive)
                    .map(|node| node.path.clone())
                    .collect::<Vec<_>>();
                self.trash.watch_paths(&self.trash_entries, &mounts)
            }
            None => Vec::new(),
        }
    }

    fn sync_native_cut_clipboard(&mut self, generation: u64) {
        let Some(source) = self.native_clipboard.as_ref() else {
            return;
        };
        if let Some(payload) = self
            .transfers
            .clipboard_payload()
            .filter(|payload| payload.generation == generation)
        {
            if let Err(error) = ClipboardAdapter::write_clipboard(source, payload) {
                self.status_notice = Some(format!(
                    "Cut was updated inside PolarExp; system clipboard failed: {error}"
                ));
            }
        } else {
            ClipboardAdapter::clear_clipboard(source, generation);
        }
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
        let destination = self.drop_destination_at(self.grid.cursor(), false);
        let request =
            destination.and_then(|destination| self.transfers.request_active(destination, action));
        self.transfers.cancel_drag();
        let Some(request) = request else {
            return Task::none();
        };
        let _ = grabbed_index;
        self.start_transfer(request)
    }

    fn start_external_drag(&mut self) -> Task<Message> {
        if self.transfers.active_drag_index().is_none() {
            return Task::none();
        }
        let Some(source) = self.native_dnd.as_ref() else {
            self.transfers.cancel_drag();
            self.drag_hover.cancel();
            self.status = self.native_dnd_error.clone().unwrap_or_else(|| {
                "External drag-and-drop is not ready yet; try again in a moment".to_owned()
            });
            return Task::none();
        };
        let entries = self.transfers.active_drag_entries().to_vec();
        let Some(preview) = self.drag_preview(&entries) else {
            self.transfers.cancel_drag();
            self.drag_hover.cancel();
            return Task::none();
        };
        let copy_only = self.modifiers.control();
        let (count, completion) =
            match self
                .transfers
                .start_outgoing_active(source, copy_only, |_| Some(preview))
            {
                Ok(started) => started,
                Err(error) => {
                    self.transfers.cancel_drag();
                    self.drag_hover.cancel();
                    self.status = format!("Could not start external drag-and-drop: {error}");
                    return Task::none();
                }
            };
        self.drag_hover.cancel();
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

    fn drag_in_progress(&self) -> bool {
        self.transfers.active_drag_index().is_some()
            || self.transfers.native_hover_destination().is_some()
    }

    fn update_drag_hover(&mut self, point: Point) {
        if !self.drag_in_progress() {
            self.drag_hover.cancel();
            return;
        }
        let rows = flatten_rows(&self.explorer, self.navigation.current());
        let target = match self.grid.drop_zone(
            point,
            self.navigation.entries().len(),
            rows.len(),
            self.status_height(),
            false,
        ) {
            Some(DropZone::Sidebar(index)) => {
                rows.get(index).map(|row| drag_hover::Target::Sidebar {
                    id: row.id,
                    path: row.path.clone(),
                })
            }
            Some(DropZone::Entry(index)) => self
                .navigation
                .entries()
                .get(index)
                .filter(|entry| entry.is_directory())
                .map(|entry| drag_hover::Target::Folder(entry.path.clone())),
            _ => None,
        };
        let target = target.filter(|target| {
            if self.transfers.active_drag_index().is_none() {
                return true;
            }
            let path = match target {
                drag_hover::Target::Sidebar { path, .. } | drag_hover::Target::Folder(path) => path,
            };
            self.transfers
                .request_active(
                    path.clone(),
                    if self.modifiers.control() {
                        TransferAction::Copy
                    } else {
                        TransferAction::Move
                    },
                )
                .is_some()
        });
        self.drag_hover.set(target, Instant::now());
    }

    fn tick_drag_hover(&mut self, now: Instant) -> Task<Message> {
        if !self.drag_in_progress() {
            self.drag_hover.cancel();
            return Task::none();
        }
        let (grid_delta, sidebar_delta) = self.grid.drag_autoscroll(self.status_height());
        let mut tasks = Vec::new();
        if grid_delta.abs() > f32::EPSILON {
            tasks.push(widget::operation::scroll_by(
                Id::new(GRID_SCROLL_ID),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: grid_delta,
                },
            ));
        }
        if sidebar_delta.abs() > f32::EPSILON {
            tasks.push(widget::operation::scroll_by(
                Id::new(SIDEBAR_SCROLL_ID),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: sidebar_delta,
                },
            ));
        }
        match self.drag_hover.tick(now) {
            Some(drag_hover::Effect::Expand(id)) => tasks.push(self.expand_tree_row(id)),
            Some(drag_hover::Effect::Enter(path)) => tasks.push(self.navigate(path, true, None)),
            None => {}
        }
        Task::batch(tasks)
    }

    fn expand_tree_row(&mut self, id: u64) -> Task<Message> {
        let Some(node) = find_node_mut(&mut self.explorer.roots, id) else {
            return Task::none();
        };
        if node.expanded {
            return Task::none();
        }
        node.expanded = true;
        let load = !node.loaded && !node.loading;
        if load {
            node.loading = true;
        }
        let path = node.path.clone();
        if load {
            self.load_tree_node(id, path)
        } else {
            Task::none()
        }
    }

    fn drop_highlight_path(&self) -> Option<PathBuf> {
        if let Some(destination) = self.transfers.native_hover_destination() {
            return destination.map(Path::to_path_buf);
        }
        self.transfers.active_drag_index()?;
        let action = if self.modifiers.control() {
            TransferAction::Copy
        } else {
            TransferAction::Move
        };
        let destination = self.drop_destination_at(self.grid.cursor(), false)?;
        self.transfers
            .request_active(destination, action)
            .map(|request| request.destination)
    }

    fn handle_native_dnd_event(&mut self, event: TransferEvent) -> Task<Message> {
        let hover_position = match &event {
            TransferEvent::Hover { position, .. } => Some(*position),
            _ => None,
        };
        if let Some(position) = hover_position {
            self.grid
                .move_cursor(position, self.navigation.entries().len());
        } else {
            self.drag_hover.cancel();
        }
        let resolved_destination = match &event {
            TransferEvent::Hover { position, .. } => self.drop_destination_at(*position, true),
            _ => None,
        };
        let Some(source) = self.native_dnd.as_ref() else {
            self.show_error("Drag-and-drop adapter is unavailable".to_owned());
            return Task::none();
        };
        let task = match self
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
        };
        if let Some(position) = hover_position {
            self.update_drag_hover(position);
        }
        task
    }

    fn x11_dnd_active(&self) -> bool {
        self.native_dnd
            .as_ref()
            .is_some_and(native_clipboard::DndSource::is_x11)
    }

    fn finish_x11_drop(&mut self, generation: u64) -> Task<Message> {
        if generation != self.x11_drop_generation || self.x11_drop_paths.is_empty() {
            return Task::none();
        }
        let paths = std::mem::take(&mut self.x11_drop_paths);
        let destination = self
            .transfers
            .native_hover_destination()
            .flatten()
            .map(Path::to_path_buf)
            .or_else(|| self.drop_destination_at(self.grid.cursor(), true));
        let Some(destination) = destination else {
            self.status_notice = Some("This is not a valid drop target".to_owned());
            return self.handle_native_dnd_event(TransferEvent::Leave { id: X11_INBOUND_ID });
        };
        let action = std::mem::replace(&mut self.x11_drop_action, TransferAction::Copy);
        self.handle_native_dnd_event(TransferEvent::Drop {
            id: X11_INBOUND_ID,
            paths,
            destination,
            action,
        })
    }

    fn start_transfer(&mut self, request: TransferRequest) -> Task<Message> {
        let batch = match fs::TransferBatch::try_new(
            request.paths.clone(),
            request.destination.clone(),
            request.action,
        ) {
            Ok(batch) => batch,
            Err(error) => {
                self.show_error(error.to_string());
                return Task::none();
            }
        };
        self.transfer_queue
            .enqueue(request, batch)
            .map_or_else(Task::none, |work| self.launch_transfer(work))
    }

    fn launch_transfer(&mut self, work: transfer_queue::Work) -> Task<Message> {
        let transfer_queue::Work {
            id,
            request,
            batch,
            cancellation,
            progress,
        } = work;
        Task::perform(
            self.operations.run(OperationKind::Mutation, move |_| {
                Ok(batch.run_with(
                    || cancellation.load(std::sync::atomic::Ordering::Acquire),
                    |update| progress.update(update),
                ))
            }),
            move |completion| match completion {
                Completion::Finished(Ok(outcome)) => Message::TransferBatchFinished {
                    id,
                    request,
                    outcome: Box::new(outcome),
                },
                Completion::Finished(Err(error)) => Message::TransferBatchFinished {
                    id,
                    request,
                    outcome: Box::new(fs::TransferBatchOutcome::Complete(fs::TransferReport {
                        completed: Vec::new(),
                        failures: vec![fs::TransferFailure {
                            source: PathBuf::new(),
                            error,
                        }],
                        retained: Vec::new(),
                        warnings: Vec::new(),
                        receipts: Vec::new(),
                        cancelled: false,
                    })),
                },
                Completion::Cancelled => Message::Noop,
            },
        )
    }

    fn finish_transfer_batch(
        &mut self,
        id: u64,
        request: TransferRequest,
        outcome: fs::TransferBatchOutcome,
    ) -> Task<Message> {
        match outcome {
            fs::TransferBatchOutcome::Complete(report) => {
                let next = self.transfer_queue.finish(id, &report);
                let finished = self.finish_transfer(request, report);
                let next = next.map_or_else(Task::none, |work| self.launch_transfer(work));
                Task::batch([finished, next])
            }
            fs::TransferBatchOutcome::Conflict { batch, conflict } => {
                let source = conflict
                    .source
                    .file_name()
                    .map_or_else(|| "item".into(), |name| name.to_string_lossy());
                let kind = if conflict.directories {
                    "folder conflict; Replace merges"
                } else {
                    "file conflict"
                };
                self.status = format!(
                    "{source}: {kind}  •  r Replace  s Skip  k Keep Both  •  uppercase applies to remaining  •  Esc cancel"
                );
                self.transfer_conflict = Some(ActiveTransferConflict {
                    request,
                    batch: *batch,
                });
                Task::none()
            }
        }
    }

    fn resolve_transfer_conflict(&mut self, key: char, remaining: bool) -> Task<Message> {
        if self.restore_conflict.is_some() {
            return self.resolve_restore_conflict(key, remaining);
        }
        let Some(active) = self.transfer_conflict.take() else {
            return Task::none();
        };
        let choice = match key {
            'r' => fs::ConflictChoice::Replace,
            's' => fs::ConflictChoice::Skip,
            'k' => fs::ConflictChoice::KeepBoth,
            _ => {
                self.transfer_conflict = Some(active);
                return Task::none();
            }
        };
        let batch = active.batch.resolve(choice, remaining);
        self.transfer_queue
            .resume(batch)
            .map_or_else(Task::none, |work| self.launch_transfer(work))
    }

    fn cancel_transfer_conflict(&mut self) -> Task<Message> {
        if self.restore_conflict.is_some() {
            return self.cancel_restore_conflict();
        }
        let Some(active) = self.transfer_conflict.take() else {
            return Task::none();
        };
        let Some(id) = self.transfer_queue.active_id() else {
            return Task::none();
        };
        self.finish_transfer_batch(
            id,
            active.request,
            fs::TransferBatchOutcome::Complete(active.batch.cancel()),
        )
    }

    fn resolve_restore_conflict(&mut self, key: char, remaining: bool) -> Task<Message> {
        let Some(active) = self.restore_conflict.take() else {
            return Task::none();
        };
        let choice = match key {
            'r' => fs::ConflictChoice::Replace,
            's' => fs::ConflictChoice::Skip,
            'k' => fs::ConflictChoice::KeepBoth,
            _ => {
                self.restore_conflict = Some(active);
                return Task::none();
            }
        };
        self.launch_restore(active.batch.resolve(choice, remaining), active.entries)
    }

    fn cancel_restore_conflict(&mut self) -> Task<Message> {
        let Some(active) = self.restore_conflict.take() else {
            return Task::none();
        };
        self.finish_restore(
            active.entries,
            fs::TransferBatchOutcome::Complete(active.batch.cancel()),
        )
    }

    fn finish_transfer(
        &mut self,
        request: TransferRequest,
        report: fs::TransferReport,
    ) -> Task<Message> {
        let journal_action = journal::Action::transfer(
            match request.action {
                TransferAction::Copy => journal::TransferKind::Copy,
                TransferAction::Move => journal::TransferKind::Move,
            },
            &report.receipts,
        );
        let adapter = self
            .native_dnd
            .as_ref()
            .map(|source| source as &dyn Adapter);
        let consequences =
            self.transfers
                .finish_transfer(adapter, &request, &report, self.navigation.current());
        if request.action == TransferAction::Move
            && let Some(generation) = request.clipboard_generation
        {
            self.sync_native_cut_clipboard(generation);
        }
        self.sync_directory_watches();
        match journal_action {
            Ok(Some(action)) => {
                if let Err(error) = self.journal.record(action) {
                    self.status_notice = Some(format!(
                        "Transfer completed but Undo was not saved: {error}"
                    ));
                }
            }
            Err(error) => {
                self.status_notice = Some(format!(
                    "Transfer completed but Undo is unavailable: {error}"
                ));
            }
            Ok(None) => {}
        }
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
        !self.busy
            && self.transfer_conflict.is_none()
            && self.restore_conflict.is_none()
            && !self.navigation_loading
            && !self.search.is_recursive()
            && self.virtual_location.is_none()
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
        let location = self.virtual_location.map_or_else(
            || self.navigation.current().display().to_string(),
            |location| match location {
                VirtualLocation::Recent => "Recent".to_owned(),
                VirtualLocation::Trash => "Trash".to_owned(),
            },
        );
        self.status = if self.grid.selection_count() > 1 {
            format!("{} selected  •  {}", self.grid.selection_count(), location)
        } else if let Some(entry) = self
            .grid
            .selected_entry()
            .and_then(|index| self.navigation.entries().get(index))
        {
            let name = fs::display_name(&entry.name);
            if self.virtual_location == Some(VirtualLocation::Trash) {
                self.trash_entries
                    .iter()
                    .find(|trashed| trashed.file.path == entry.path)
                    .map_or_else(
                        || format!("{name}  •  Trash"),
                        |trashed| {
                            format!(
                                "{name}  •  originally {}",
                                trashed.receipt.original.display()
                            )
                        },
                    )
            } else {
                match self.grid.details() {
                    Some(details) => format!("{name}  •  {details}"),
                    None => format!("{name}  •  Loading details…"),
                }
            }
        } else {
            format!("{} items  •  {}", self.navigation.entries().len(), location)
        };
    }

    fn status_height(&self) -> f32 {
        if self.reduced_motion() {
            return self.desired_expanded_height().unwrap_or(STATUS_HEIGHT);
        }
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
        self.transfers.active_drag_index()?;
        let entries = self.transfers.active_drag_entries();
        let preview = self.drag_preview(entries)?;
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
            .id(Id::new(SIDEBAR_SCROLL_ID))
            .on_scroll(|viewport| Message::SidebarScrolled(viewport.absolute_offset().y))
            .height(Fill),
        ];
        container(content)
            .width(SIDEBAR_WIDTH)
            .height(Fill)
            .padding(Padding::from([8, 12]))
            .style(move |theme| {
                sidebar_style(theme, self.high_contrast() || self.reduced_transparency())
            })
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
            state::NodeKind::Home
            | state::NodeKind::Place
            | state::NodeKind::Favorite
            | state::NodeKind::Recent
            | state::NodeKind::Trash => (
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
        let content = column![line.height(30), self.drag_activation_bar(&tree_row.path)].spacing(0);
        let button = button(content)
            .on_press(Message::TreeRow(tree_row.id))
            .width(Fill)
            .height(32)
            .padding(0)
            .style(move |theme, status| {
                tree_button_style(theme, status, selected, focused, drop_target)
            });
        if let Some(index) = tree_row.favorite_index {
            mouse_area(button)
                .on_press(Message::FavoritePressed(index))
                .on_release(Message::FavoriteReleased(index))
                .into()
        } else {
            button.into()
        }
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
            self.virtual_location.is_none() && self.navigation.current().parent().is_some(),
            Message::Parent,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 0,
        );
        let back = toolbar_button(
            include_bytes!("../ui/icons/back.svg"),
            "Back",
            self.navigation.can_go_back(),
            Message::Back,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 1,
        );
        let forward = toolbar_button(
            include_bytes!("../ui/icons/forward.svg"),
            "Forward",
            self.navigation.can_go_forward(),
            Message::Forward,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 2,
        );
        let refresh = toolbar_button(
            include_bytes!("../ui/icons/refresh.svg"),
            "Refresh",
            !self.navigation_loading,
            Message::Refresh,
            self.iced_theme().palette().text,
            self.iced_theme().palette().background,
            self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 3,
        );
        let options = self
            .view_preferences
            .for_directory(self.navigation.current());
        let view_label = match options.view {
            fs::ViewMode::Grid => "Grid",
            fs::ViewMode::List => "List",
        };
        let sort_label = format!(
            "{:?} {}",
            options.sort,
            if options.descending { "↓" } else { "↑" }
        );
        let location: Element<'_, Message> = if self.browser_input.mode() == InputMode::Location {
            let input = text_input("Location", &self.location_input)
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
            let accent = self.accent_color();
            column![
                container(input).width(Fill).height(33).center_y(33),
                container(Space::new().width(Fill).height(1))
                    .width(Fill)
                    .height(1)
                    .style(move |_| solid_background_style(accent)),
            ]
            .spacing(0)
            .width(Fill)
            .height(34)
            .into()
        } else if let Some(location) = self.virtual_location {
            let label = match location {
                VirtualLocation::Recent => "Recent",
                VirtualLocation::Trash => "Trash",
            };
            container(text(label).font(UI_FONT_SEMIBOLD).size(13))
                .width(Fill)
                .height(34)
                .padding(Padding::from([0, 7]))
                .center_y(34)
                .into()
        } else {
            let mut crumbs = Row::new().spacing(1).align_y(Alignment::Center);
            for (index, (label, path)) in breadcrumb_segments(self.navigation.current())
                .into_iter()
                .enumerate()
            {
                let focused =
                    self.browser_focus == BrowserFocus::Location && self.breadcrumb_cursor == index;
                crumbs = crumbs.push(
                    button(text(label).font(UI_FONT).size(13))
                        .on_press(Message::Breadcrumb(path))
                        .padding(Padding::from([4, 7]))
                        .style(move |theme, status| focusable_button_style(theme, status, focused)),
                );
            }
            container(crumbs).width(Fill).height(34).center_y(34).into()
        };
        let location = container(location)
            .width(Fill)
            .height(34)
            .style(move |theme| {
                focus_container_style(theme, self.browser_focus == BrowserFocus::Location)
            });
        container(
            row![
                parent,
                back,
                forward,
                refresh,
                location,
                toolbar_text_button(
                    view_label,
                    Message::ToggleView,
                    self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 4,
                ),
                button(text(sort_label).font(MONO_FONT_SEMIBOLD).size(11))
                    .on_press(Message::CycleSort)
                    .padding(Padding::from([1, 4]))
                    .style(move |theme, status| focusable_button_style(
                        theme,
                        status,
                        self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 5,
                    )),
                toolbar_text_button(
                    "Direction",
                    Message::ToggleSortDirection,
                    self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 6,
                ),
                toolbar_text_button(
                    "Folders",
                    Message::ToggleFoldersFirst,
                    self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 7,
                ),
                toolbar_text_button(
                    if options.show_hidden {
                        "Hidden on"
                    } else {
                        "Hidden off"
                    },
                    Message::ToggleHidden,
                    self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 8,
                ),
                toolbar_text_button(
                    if self.view_preferences.single_click_activation() {
                        "1-click"
                    } else {
                        "2-click"
                    },
                    Message::ToggleClickActivation,
                    self.browser_focus == BrowserFocus::Toolbar && self.toolbar_cursor == 9,
                ),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .height(TOOLBAR_HEIGHT)
        .padding(Padding::from([6, CONTENT_GUTTER as u16]))
        .style(browser_background_style)
        .into()
    }

    fn grid_body(&self) -> Element<'_, Message> {
        if self
            .view_preferences
            .for_directory(self.navigation.current())
            .view
            == fs::ViewMode::List
        {
            return self.list_body();
        }
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
            .style(move |theme| {
                grid_background_style(
                    theme,
                    current_drop_target,
                    self.browser_focus == BrowserFocus::Entries,
                )
            })
            .into()
    }

    fn list_body(&self) -> Element<'_, Message> {
        let visible = self
            .grid
            .list_visible_range(self.navigation.entries().len(), self.status_height());
        let top = Space::new()
            .height(visible.start as f32 * LIST_ROW_HEIGHT)
            .width(Fill);
        let bottom = Space::new()
            .height(
                self.navigation.entries().len().saturating_sub(visible.end) as f32
                    * LIST_ROW_HEIGHT,
            )
            .width(Fill);
        let mut rows = Column::new().spacing(0).push(top);
        for index in visible {
            rows = rows.push(self.file_list_row(index));
        }
        rows = rows.push(bottom);
        container(
            scrollable(rows)
                .id(Id::new(GRID_SCROLL_ID))
                .on_scroll(|viewport| Message::GridScrolled(viewport.absolute_offset().y))
                .width(Fill)
                .height(Fill),
        )
        .padding(Padding::from([
            LIST_VIEW_TOP_INSET as u16,
            CONTENT_GUTTER as u16,
        ]))
        .width(Fill)
        .height(Fill)
        .style(move |theme| {
            grid_background_style(theme, false, self.browser_focus == BrowserFocus::Entries)
        })
        .into()
    }

    fn file_list_row(&self, index: usize) -> Element<'_, Message> {
        let entry = &self.navigation.entries()[index];
        let selected = self.grid.is_selected(index);
        let focused = self.browser_focus == BrowserFocus::Entries
            && self.grid.selected_entry() == Some(index);
        let hovered = self.grid.hovered() == Some(index);
        let icon_kind = entry_icon_kind(entry);
        let kind = if entry.is_directory() {
            "Folder".to_owned()
        } else {
            entry
                .path
                .extension()
                .map(|extension| extension.to_string_lossy().to_uppercase())
                .unwrap_or_else(|| "File".to_owned())
        };
        let content = row![
            themed_svg(
                entry_icon_asset(icon_kind),
                20.0,
                self.entry_icon_color(icon_kind),
            ),
            text(fs::display_name(&entry.name)).size(13).width(Fill),
            text(kind)
                .font(MONO_FONT)
                .size(11)
                .color(self.secondary_text_color())
                .width(100),
        ]
        .spacing(9)
        .align_y(Alignment::Center);
        let content = column![
            content.height(LIST_ROW_HEIGHT - 2.0),
            self.drag_activation_bar(&entry.path)
        ]
        .spacing(0);
        let row = container(content)
            .height(LIST_ROW_HEIGHT)
            .padding(Padding::from([0, 8]))
            .style(move |theme| tile_style(theme, selected, hovered, focused, false));
        mouse_area(row)
            .on_press(Message::EntryPressed(index))
            .on_release(Message::EntryReleased(index))
            .on_double_click(Message::EntryDoubleClicked(index))
            .on_right_press(Message::EntryContext(index))
            .on_enter(Message::EntryHovered(index))
            .on_exit(Message::EntryUnhovered(index))
            .into()
    }

    fn file_tile(&self, index: usize) -> Element<'_, Message> {
        let entry = &self.navigation.entries()[index];
        let selected = self.grid.is_selected(index);
        let focused = self.browser_focus == BrowserFocus::Entries
            && self.grid.selected_entry() == Some(index);
        let hovered = self.grid.hovered() == Some(index);
        let drop_target = entry.is_directory()
            && self.drop_highlight_path().as_deref() == Some(entry.path.as_path());
        let icon_kind = entry_icon_kind(entry);
        let icon: Element<'_, Message> = self.thumbnails.handle(&entry.path).map_or_else(
            || {
                themed_svg(
                    entry_icon_asset(icon_kind),
                    48.0,
                    self.entry_icon_color(icon_kind),
                )
                .into()
            },
            |handle| {
                widget::image(handle.clone())
                    .width(48)
                    .height(48)
                    .content_fit(iced::ContentFit::Cover)
                    .border_radius(5)
                    .into()
            },
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
            self.drag_activation_bar(&entry.path),
        ]
        .spacing(4)
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
            .style(move |theme| tile_style(theme, selected, hovered, focused, drop_target));
        mouse_area(container(tile).width(Fill).center_x(Fill))
            .on_press(Message::EntryPressed(index))
            .on_release(Message::EntryReleased(index))
            .on_double_click(Message::EntryDoubleClicked(index))
            .on_right_press(Message::EntryContext(index))
            .on_enter(Message::EntryHovered(index))
            .on_exit(Message::EntryUnhovered(index))
            .into()
    }

    fn drag_activation_bar(&self, path: &Path) -> Element<'_, Message> {
        let Some(progress) = self.drag_hover.progress(path, Instant::now()) else {
            return Space::new().width(Fill).height(2).into();
        };
        let filled = ((progress * 100.0).round() as u16).clamp(1, 99);
        row![
            container(Space::new())
                .width(Length::FillPortion(filled))
                .height(2)
                .style(move |_| solid_background_style(self.accent_color())),
            Space::new()
                .width(Length::FillPortion(100 - filled))
                .height(2),
        ]
        .spacing(0)
        .width(Fill)
        .height(2)
        .into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let height = self.status_height();
        let content: Element<'_, Message> = if let Some(output) = self.command.output() {
            let header = row![
                text(&output.summary)
                    .font(MONO_FONT_SEMIBOLD)
                    .size(11)
                    .line_height(iced::Pixels(13.0))
                    .width(Fill),
                compact_text_button("Copy", Message::CopyCommandReport),
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
        } else if self.transfer_queue.expanded() && self.browser_input.mode() == InputMode::Browser
        {
            self.transfer_history_bar()
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
                    if self.transfer_queue.active_id().is_some() || self.transfer_queue.has_retry()
                    {
                        return container(compact_status_line(self.transfer_status_line()))
                            .width(Fill)
                            .height(Length::Fixed(height))
                            .clip(true)
                            .style(move |theme| {
                                status_background_style(
                                    theme,
                                    self.browser_focus == BrowserFocus::BottomBar,
                                )
                            })
                            .into();
                    }
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
            .style(move |theme| {
                status_background_style(theme, self.browser_focus == BrowserFocus::BottomBar)
            })
            .into()
    }

    fn transfer_status_line(&self) -> Element<'_, Message> {
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        if let Some(snapshot) = self.transfer_queue.snapshot() {
            line = line
                .push(self.spinner(13.0))
                .push(
                    text(format_transfer_snapshot(
                        self.transfer_queue.active_action().unwrap_or("Transfer"),
                        &snapshot,
                    ))
                    .font(MONO_FONT)
                    .size(11)
                    .width(Fill),
                )
                .push(compact_text_button("Cancel", Message::CancelTransfer));
        } else {
            line = line.push(
                text("Transfer finished with retained entries")
                    .font(MONO_FONT)
                    .size(11)
                    .width(Fill),
            );
        }
        if self.transfer_queue.has_retry() {
            line = line.push(compact_text_button("Retry", Message::RetryTransfer));
        }
        line.push(compact_text_button(
            "History",
            Message::ToggleTransferHistory,
        ))
        .into()
    }

    fn transfer_history_bar(&self) -> Element<'_, Message> {
        let mut header = Row::new()
            .push(
                text("transfers")
                    .font(MONO_FONT_SEMIBOLD)
                    .size(11)
                    .width(Fill),
            )
            .spacing(10)
            .height(25)
            .align_y(Alignment::Center);
        if self.transfer_queue.active_id().is_some() {
            header = header.push(compact_text_button("Cancel", Message::CancelTransfer));
        }
        if self.transfer_queue.has_retry() {
            header = header.push(compact_text_button("Retry", Message::RetryTransfer));
        }
        header = header
            .push(compact_text_button(
                "Copy report",
                Message::CopyTransferReport,
            ))
            .push(compact_text_button("Close", Message::ToggleTransferHistory));
        let active = self
            .transfer_queue
            .snapshot()
            .map(|snapshot| {
                format_transfer_snapshot(
                    self.transfer_queue.active_action().unwrap_or("Transfer"),
                    &snapshot,
                )
            })
            .into_iter();
        let history = self
            .transfer_queue
            .history()
            .iter()
            .rev()
            .map(|entry| entry.summary().to_owned());
        let detail = active.chain(history).collect::<Vec<_>>().join("\n");
        let detail = if detail.is_empty() {
            "No transfer history".to_owned()
        } else {
            detail
        };
        container(
            column![
                header,
                scrollable(
                    text(detail)
                        .font(MONO_FONT)
                        .size(11)
                        .line_height(iced::Pixels(14.0))
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

    fn prompt_bar(&self) -> Element<'_, Message> {
        match self.file_operations.view() {
            FileOperationView::NewFolder { value, error } => {
                self.name_prompt_bar("new folder", value, error)
            }
            FileOperationView::NewFile {
                value,
                error,
                template_label,
            } => self.name_prompt_bar(template_label, value, error),
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

    fn name_prompt_bar<'a>(
        &'a self,
        label: &'a str,
        value: &'a str,
        error: &'a str,
    ) -> Element<'a, Message> {
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
                text(label)
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
        let mut actions = Column::new();
        for (index, (label, message)) in self.context_actions().into_iter().enumerate() {
            let focused = index == self.context_menu_cursor;
            actions = actions.push(
                button(text(label).size(13))
                    .on_press(message)
                    .style(move |theme, status| context_button_style(theme, status, focused))
                    .width(Fill),
            );
        }
        let menu = container(scrollable(actions).height(Length::Shrink))
            .width(220)
            .max_height(420)
            .padding(5)
            .style(menu_style);
        let overlay =
            mouse_area(container("").width(Fill).height(Fill)).on_press(Message::CloseContext);
        stack![overlay, pin(menu).x(point.x).y(point.y)].into()
    }

    fn accent_color(&self) -> Color {
        if self.high_contrast() {
            return self.iced_theme().palette().primary;
        }
        self.accent
            .as_ref()
            .map_or(Color::from_rgb8(0, 120, 212), |colors| colors.accent)
    }

    fn secondary_text_color(&self) -> Color {
        let mut color = self.iced_theme().palette().text;
        color.a = if self.high_contrast() || self.reduced_transparency() {
            1.0
        } else {
            0.62
        };
        color
    }

    fn selection_text_color(&self) -> Color {
        if self.high_contrast() {
            return Color::BLACK;
        }
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

fn move_composite_cursor(cursor: &mut usize, len: usize, motion: Motion, count: usize) {
    let last = len.saturating_sub(1);
    let step = count.max(1);
    *cursor = match motion {
        Motion::Left | Motion::Up | Motion::HalfPageUp => cursor.saturating_sub(step),
        Motion::Right | Motion::Down | Motion::HalfPageDown => {
            cursor.saturating_add(step).min(last)
        }
        Motion::RowStart | Motion::First | Motion::ViewportTop => 0,
        Motion::RowEnd | Motion::Last | Motion::ViewportBottom => last,
        Motion::DisplayIndex(index) => index.min(last),
        Motion::ViewportMiddle => last / 2,
    };
}

fn nearest_existing_ancestor(path: &Path) -> PathBuf {
    let mut candidate = path.to_path_buf();
    loop {
        if candidate.is_dir() {
            return candidate;
        }
        if !candidate.pop() {
            return PathBuf::from("/");
        }
    }
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
    focused: bool,
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
        .style(move |theme, status| focusable_button_style(theme, status, focused))
}

fn sidebar_style(theme: &Theme, opaque: bool) -> container::Style {
    let background = theme.palette().background;
    let alternate = if background.r > 0.5 {
        Color::from_rgb8(240, 240, 240)
    } else {
        Color::from_rgb8(44, 44, 44)
    };
    if opaque {
        return container::Style::default().background(alternate);
    }
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

fn focus_container_style(theme: &Theme, focused: bool) -> container::Style {
    let mut style = container::Style::default();
    if focused {
        style.border = Border {
            width: 2.0,
            color: theme.palette().primary,
            radius: 4.0.into(),
        };
    }
    style
}

fn grid_background_style(theme: &Theme, drop_target: bool, focused: bool) -> container::Style {
    let mut style = browser_background_style(theme);
    if drop_target {
        style.border = Border {
            width: 2.0,
            color: theme.palette().primary,
            radius: 4.0.into(),
        };
    } else if focused {
        style.border = Border {
            width: 1.0,
            color: theme.palette().primary,
            radius: 0.0.into(),
        };
    }
    style
}

fn status_background_style(theme: &Theme, focused: bool) -> container::Style {
    let mut style = container::Style::default().background(lighter(theme.palette().background, 16));
    if focused {
        style.border = Border {
            width: 2.0,
            color: theme.palette().primary,
            radius: 0.0.into(),
        };
    }
    style
}

fn tree_button_style(
    theme: &Theme,
    status: button::Status,
    selected: bool,
    focused: bool,
    drop_target: bool,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let high_contrast = is_high_contrast_theme(theme);
    button::Style {
        background: if drop_target {
            Some(Background::Color(if high_contrast {
                theme.palette().warning
            } else {
                with_alpha(theme.palette().primary, 0.36)
            }))
        } else if selected {
            Some(Background::Color(if high_contrast {
                theme.palette().primary
            } else {
                with_alpha(theme.palette().primary, 0.22)
            }))
        } else if hovered {
            Some(Background::Color(with_alpha(theme.palette().text, 0.06)))
        } else {
            None
        },
        text_color: theme.palette().text,
        border: Border {
            width: if drop_target {
                3.0
            } else if focused {
                2.0
            } else if high_contrast && selected {
                1.0
            } else {
                0.0
            },
            color: if drop_target {
                theme.palette().text
            } else if focused {
                theme.palette().primary
            } else {
                theme.palette().background
            },
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

fn tile_style(
    theme: &Theme,
    selected: bool,
    hovered: bool,
    focused: bool,
    drop_target: bool,
) -> container::Style {
    let mut style = container::Style::default();
    let high_contrast = is_high_contrast_theme(theme);
    if drop_target {
        style.background = Some(Background::Color(if high_contrast {
            theme.palette().warning
        } else {
            with_alpha(theme.palette().primary, 0.30)
        }));
    } else if selected {
        style.background = Some(Background::Color(if high_contrast {
            theme.palette().primary
        } else {
            with_alpha(theme.palette().primary, 0.45)
        }));
    } else if hovered {
        style.background = Some(Background::Color(lighter(theme.palette().background, 16)));
    }
    style.border = Border {
        width: if drop_target {
            3.0
        } else if focused {
            2.0
        } else if high_contrast && selected {
            1.0
        } else {
            0.0
        },
        color: if drop_target {
            theme.palette().text
        } else if focused {
            theme.palette().primary
        } else {
            theme.palette().background
        },
        radius: 7.0.into(),
    };
    style
}

fn is_high_contrast_theme(theme: &Theme) -> bool {
    theme.palette().background == Color::BLACK && theme.palette().text == Color::WHITE
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

fn focusable_button_style(theme: &Theme, status: button::Status, focused: bool) -> button::Style {
    let mut style = toolbar_button_style(theme, status);
    if focused {
        style.border.width = 2.0;
        style.border.color = theme.palette().primary;
    }
    style
}

fn context_button_style(theme: &Theme, status: button::Status, focused: bool) -> button::Style {
    let mut style = button::text(theme, status);
    if focused {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.24)));
        style.border = Border {
            width: 2.0,
            color: theme.palette().primary,
            radius: 4.0.into(),
        };
    }
    style
}

fn breadcrumb_segments(path: &Path) -> Vec<(String, PathBuf)> {
    let mut current = PathBuf::new();
    let mut segments = Vec::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let label = match component {
            Component::RootDir => "/".to_owned(),
            Component::Prefix(prefix) => prefix.as_os_str().to_string_lossy().into_owned(),
            Component::CurDir => ".".to_owned(),
            Component::ParentDir => "..".to_owned(),
            Component::Normal(name) => name.to_string_lossy().into_owned(),
        };
        segments.push((label, current.clone()));
    }
    segments
}

fn command_failure_report(
    result: &Result<command::Completion, String>,
) -> Option<(String, String)> {
    match result {
        Ok(command::Completion::Shell(Ok(report))) if !report.successful => {
            Some((report.summary.clone(), report.detail.clone()))
        }
        Ok(command::Completion::Shell(Err(command::Failure::RequiresTerminal))) => Some((
            "interactive terminal required".to_owned(),
            "The command attempted terminal screen control and was stopped.".to_owned(),
        )),
        Ok(command::Completion::Shell(Err(command::Failure::Other(error)))) => {
            Some(("Could not run Bash".to_owned(), error.clone()))
        }
        Ok(command::Completion::Terminal {
            directory,
            result: Err(error),
        }) => Some((
            format!("Could not open a terminal in {}", directory.display()),
            error.clone(),
        )),
        Err(error) => Some(("Command operation failed".to_owned(), error.clone())),
        _ => None,
    }
}

fn compact_text_button<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    button(text(label).font(MONO_FONT_SEMIBOLD).size(11))
        .on_press(message)
        .padding(Padding::from([1, 4]))
        .style(toolbar_button_style)
        .into()
}

fn toolbar_text_button<'a>(
    label: &'a str,
    message: Message,
    focused: bool,
) -> Element<'a, Message> {
    button(text(label).font(MONO_FONT_SEMIBOLD).size(11))
        .on_press(message)
        .padding(Padding::from([1, 4]))
        .style(move |theme, status| focusable_button_style(theme, status, focused))
        .into()
}

fn format_transfer_snapshot(action: &str, snapshot: &transfer_queue::Snapshot) -> String {
    let progress = snapshot.progress;
    let eta = snapshot.estimated_remaining.map_or_else(
        || "ETA --".to_owned(),
        |remaining| format!("ETA {}s", remaining.as_secs()),
    );
    let queued = if snapshot.queued == 0 {
        String::new()
    } else {
        format!("  •  {} queued", snapshot.queued)
    };
    format!(
        "{action} {}/{}  •  {}/{}  •  {}/s  •  {eta}{queued}",
        progress.completed_entries,
        progress.total_entries,
        format_transfer_bytes(progress.completed_bytes),
        format_transfer_bytes(progress.total_bytes),
        format_transfer_bytes(snapshot.bytes_per_second),
    )
}

fn format_transfer_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
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
