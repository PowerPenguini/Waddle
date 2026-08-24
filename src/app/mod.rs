mod bottom_bar;
mod browser_input;
mod command;
mod diagnostics;
mod directory_watch;
mod drag_hover;
mod file_operation;
mod grid;
mod location_monitoring;
mod navigation;
mod navigation_integration;
mod operation_integration;
mod operations;
mod places;
mod presentation;
mod properties;
mod recent;
mod runtime;
mod search;
mod shell;
mod startup;
mod state;
mod status;
mod templates;
mod thumbnail;
mod transfer_integration;
mod transfer_queue;
mod transfer_session;
mod trash;
mod tree;
mod view;
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
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    fs, journal, theme,
    transfer::{
        Action as TransferAction, Adapter, ClipboardAdapter, ClipboardImport,
        Event as TransferEvent, NativeUpdate, Outcome as TransferOutcome,
        Preview as TransferPreview, Release as TransferRelease, Request as TransferRequest,
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
    CONTENT_GUTTER, DropZone, GridInteraction, LIST_HEADER_HEIGHT, LIST_ROW_HEIGHT,
    LIST_VIEW_TOP_INSET, Motion, SIDEBAR_WIDTH, TILE_ROW_HEIGHT, TILE_WIDTH, TOOLBAR_HEIGHT,
};
use iced::time::Instant;
use iced::{
    Alignment, Animation, Background, Border, Color, Element, Fill, Font, Length, Padding, Point,
    Shadow, Size, Subscription, Task, Theme, Vector,
    animation::Easing,
    application, event, gradient, keyboard, mouse, system, time,
    widget::{
        self, Button, Column, Grid, Id, Row, Space, button, container, mouse_area, pin, rule,
        scrollable, stack, svg, text, text_input,
    },
    window,
};
use navigation::{
    Completion as NavigationCompletion, DisplayedLocation, NavigationSession,
    Outcome as NavigationOutcome, Request as NavigationRequest,
};
use operations::{Completion, Kind as OperationKind, Operations};
use presentation::*;
use search::{SearchSession, Update as SearchUpdate};
use state::ExplorerState;
use transfer_session::{
    BatchUpdate as TransferBatchUpdate, CancelUpdate as TransferCancelUpdate,
    CompletedBatch as TransferCompletedBatch, TransferSession,
};
use tree::{TreeRow, find_node, find_node_mut, flatten_rows, mounted_roots};

const STATUS_HEIGHT: f32 = 25.0;
const SEARCH_LIMIT: usize = 1000;
const LIST_HORIZONTAL_PADDING: u16 = 8;
const LIST_HEADER_HORIZONTAL_PADDING: u16 = 4;
const LIST_COLUMN_SPACING: f32 = 9.0;
const LIST_ENTRY_ICON_WIDTH: f32 = 20.0;
const LIST_HEADER_ICON_SLOT_WIDTH: f32 =
    LIST_ENTRY_ICON_WIDTH - LIST_HEADER_HORIZONTAL_PADDING as f32;
const _: () = assert!(LIST_HEADER_HORIZONTAL_PADDING > 0);
const LIST_TYPE_WIDTH: f32 = 90.0;
const LIST_SIZE_WIDTH: f32 = 100.0;
const LIST_MODIFIED_WIDTH: f32 = 148.0;
const LIST_SHOW_SIZE_AT: f32 = 760.0;
const LIST_SHOW_MODIFIED_AT: f32 = 920.0;
const LIST_NAME_APPROX_CHARACTER_WIDTH: f32 = 8.0;
const LIST_NAME_MIN_CHARACTERS: usize = 8;
const GRID_NAME_MAX_CHARACTERS: usize = 26;
const TOOLBAR_ICON_SIZE: f32 = 16.0;
const SCROLLBAR_TRACK_WIDTH: f32 = 6.0;
const SCROLLBAR_THUMB_WIDTH: f32 = 4.0;
const SCROLLBAR_FADE_IN: Duration = Duration::from_millis(100);
const SCROLLBAR_HOLD: Duration = Duration::from_millis(500);
const SCROLLBAR_FADE_OUT: Duration = Duration::from_millis(200);
const _: () = assert!(SCROLLBAR_THUMB_WIDTH < SCROLLBAR_TRACK_WIDTH);
const _: () = assert!(SCROLLBAR_TRACK_WIDTH < 10.0);
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
    SortBy(fs::SortKey),
    Parent,
    Back,
    Forward,
    LocationChanged(String),
    LocationSubmitted,
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
        request: NavigationRequest,
        result: Result<(PathBuf, Vec<FileEntry>), String>,
    },
    NavigationCancelled(NavigationRequest),
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
    RecentLoaded {
        request: NavigationRequest,
        result: Option<Result<Vec<FileEntry>, String>>,
    },
    TrashLoaded {
        request: NavigationRequest,
        result: Option<Result<Vec<trash::Entry>, String>>,
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
    templates: Vec<templates::Template>,
    operations: Operations,
    search: SearchSession,
    transfers: TransferSession,
    command: CommandSession,
    command_adapter: ProcessAdapter,
    diagnostics: diagnostics::History,
    file_operations: FileOperationSession,
    journal: journal::Journal,
    location_monitoring: Option<location_monitoring::Native>,
    view_preferences: view_preferences::Preferences,
    thumbnails: thumbnail::Cache,
    trash_adapter: GioTrashAdapter,
    grid: GridInteraction,
    sidebar_scrollbar: ScrollbarVisibility,
    entry_scrollbar: ScrollbarVisibility,
    drag_hover: drag_hover::State,
    browser_input: BrowserInput,
    browser_focus: BrowserFocus,
    toolbar_cursor: usize,
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

#[derive(Default)]
struct ScrollbarVisibility {
    shown_at: Option<Instant>,
    fade_out_at: Option<Instant>,
}

impl ScrollbarVisibility {
    fn show(&mut self, now: Instant) {
        self.shown_at.get_or_insert(now);
        self.fade_out_at = Some(now + SCROLLBAR_HOLD);
    }

    fn hide_if_elapsed(&mut self, now: Instant) {
        if self
            .fade_out_at
            .is_some_and(|fade_out_at| now >= fade_out_at + SCROLLBAR_FADE_OUT)
        {
            self.shown_at = None;
            self.fade_out_at = None;
        }
    }

    fn is_visible(&self) -> bool {
        self.fade_out_at.is_some()
    }

    fn opacity(&self, now: Instant, reduced_motion: bool) -> f32 {
        let (Some(shown_at), Some(fade_out_at)) = (self.shown_at, self.fade_out_at) else {
            return 0.0;
        };
        if reduced_motion {
            return 1.0;
        }
        if now < fade_out_at {
            return duration_ratio(now.saturating_duration_since(shown_at), SCROLLBAR_FADE_IN);
        }

        1.0 - duration_ratio(
            now.saturating_duration_since(fade_out_at),
            SCROLLBAR_FADE_OUT,
        )
    }
}

fn duration_ratio(elapsed: Duration, total: Duration) -> f32 {
    (elapsed.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserStatusPresentation {
    Conflict,
    Transfer,
    General,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrowserStatusModel<'a> {
    presentation: BrowserStatusPresentation,
    text: &'a str,
    retry: bool,
    history: bool,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let now = Instant::now();
        let startup = startup::State::open_default();
        let view_preferences = view_preferences::Preferences::open_default();
        let preferences_error = view_preferences.error().map(str::to_owned);
        let tree_visible = view_preferences.tree_visible();
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
        let (location_monitoring, watch_error) = match location_monitoring::Native::open() {
            Ok(monitoring) => (Some(monitoring), None),
            Err(error) => (None, Some(error)),
        };
        let startup_error = startup
            .error()
            .map(str::to_owned)
            .or(preferences_error)
            .or(journal_error)
            .or(watch_error);
        let mut grid = GridInteraction::default();
        grid.set_sidebar_visible(tree_visible);
        let mut app = Self {
            explorer,
            navigation: NavigationSession::new(current.clone()),
            startup,
            places,
            recent,
            trash,
            templates: templates::discover(),
            operations: Operations::default(),
            search: SearchSession::default(),
            transfers: TransferSession::open_default(),
            command: CommandSession::default(),
            command_adapter: ProcessAdapter,
            diagnostics: diagnostics::History::open_default(),
            file_operations: FileOperationSession::default(),
            journal,
            location_monitoring,
            view_preferences,
            thumbnails: thumbnail::Cache::default(),
            trash_adapter: GioTrashAdapter,
            grid,
            sidebar_scrollbar: ScrollbarVisibility::default(),
            entry_scrollbar: ScrollbarVisibility::default(),
            drag_hover: drag_hover::State::default(),
            browser_input: BrowserInput::default(),
            browser_focus: BrowserFocus::Entries,
            toolbar_cursor: 0,
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
            status: String::new(),
            status_notice: startup_error,
            busy: false,
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
        let scrollbar_visible =
            self.sidebar_scrollbar.is_visible() || self.entry_scrollbar.is_visible();
        let animation = if !self.reduced_motion()
            && (self.output_expansion.is_animating(self.animation_now)
                || self.spinner_active()
                || self.drag_in_progress()
                || scrollbar_visible)
        {
            time::every(Duration::from_millis(16)).map(Message::AnimationFrame)
        } else if self.reduced_motion() && (self.drag_in_progress() || scrollbar_visible) {
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
        if let Some(monitoring) = &self.location_monitoring {
            subscriptions.push(monitoring.subscription().map(Message::DirectoryChanged));
        }
        if self.transfers.active() {
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
            || self.transfers.active()
            || self.navigation.loading()
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
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(source) = self.native_dnd.take() {
            self.transfers.stop(&source);
        }
    }
}
