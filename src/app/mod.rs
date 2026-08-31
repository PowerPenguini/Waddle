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
mod open_with;
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
mod status;
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

use std::{path::PathBuf, time::Duration};

use crate::{
    fs, journal, theme,
    transfer::{
        Action as TransferAction, ClipboardImport, Event as TransferEvent,
        Outcome as TransferOutcome,
    },
};
use browser_input::{
    BottomInput, BrowserInput, Context as InputContext, Intent as InputIntent, Mode as InputMode,
    NamedKey as InputNamedKey, Press as InputPress,
};
use command::{CommandSession, ProcessAdapter};
use file_operation::{
    FileOperationSession, GioTrashAdapter, View as FileOperationView, Work as FileOperationWork,
};
use fs::FileEntry;
use grid::{
    CONTENT_GUTTER, ContextMenu, ContextNavigation, ContextOutcome, ContextTarget, DragHoverEffect,
    DragHoverTarget, DropZone, GridInteraction, LIST_HEADER_HEIGHT, LIST_ROW_HEIGHT,
    LIST_VIEW_TOP_INSET, Motion, SIDEBAR_WIDTH, Scrollbar, TILE_HEIGHT, TILE_ROW_HEIGHT,
    TILE_WIDTH, TOOLBAR_HEIGHT,
};
use iced::time::Instant;
use iced::{
    Color, Element, Font, Point, Size, Subscription, Task, Theme, application, event, keyboard,
    system, time, widget, window,
};
use navigation::{
    Completion as NavigationCompletion, DisplayedLocation, NavigationSession,
    Outcome as NavigationOutcome, Request as NavigationRequest, Transition as NavigationTransition,
};
use operations::{Completion, Kind as OperationKind, Operations};
use presentation::{
    BrowserFocus, BrowserStatusModel, BrowserStatusPresentation, Presentation,
    TransientPresentation, TransientPresentationKind, apply_opacity, browser_background_style,
    clears_status_notice, clip_file_name, compact_status_line, compact_text_button,
    context_button_style, context_menu_button_style, entry_content_opacity, entry_icon_asset,
    entry_icon_kind, entry_svg, find_window_after_delay, flat_input_style, focus_container_style,
    format_transfer_snapshot, grid_background_style, list_row_style, marquee_style, menu_style,
    rgba, sidebar_style, solid_background_style, status_background_style, status_input_style,
    themed_svg, tile_label, tile_style, toolbar_button, toolbar_button_style,
    transient_scrollbar_style, transient_vertical_scrollbar, tree_button_style, tree_icon_asset,
    with_alpha,
};
use search::{SearchSession, Update as SearchUpdate};
use transfer_session::{
    BatchUpdate as TransferBatchUpdate, CancelUpdate as TransferCancelUpdate,
    DragRelease as TransferDragRelease, TransferSession,
};
use tree::{
    Activation as TreeActivation, LoadOutcome as TreeLoadOutcome, LoadRequest as TreeLoadRequest,
    MoveOutcome as TreeMoveOutcome, SidebarTree, TreeRow,
};

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
const MOUSE_BACK_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(350);
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
const OPEN_WITH_ID: &str = "open-with";
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

#[derive(Clone, Copy, Debug)]
enum MouseBackGesture {
    FirstPressed,
    AwaitingSecondClick { first_released_at: Instant },
    SecondPressed,
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
    LocationFocusChanged(bool),
    LocationSubmitted,
    TreeRow(u64),
    SidebarScrolled(f32),
    TreeLoaded {
        request: TreeLoadRequest,
        result: Result<Vec<PathBuf>, String>,
    },
    FavoritePressed(usize),
    FavoriteReleased(usize),
    EntryPressed(usize),
    EntryReleased(usize),
    EntryDoubleClicked(usize),
    EntryContext(usize),
    ContextFocused(usize),
    ContextNewFolder,
    ContextNewFile,
    ContextProperties,
    ContextOpenWith,
    OpenWithChanged(String),
    OpenWithSubmitted,
    OpenWithSelected(String),
    ContextRename,
    ContextTrash,
    ContextRestore,
    ContextDeletePermanent,
    ContextEmptyTrash,
    CloseContext,
    MouseBackTick(Instant),
    GridScrolled(f32),
    ScrollToSelected,
    GridPointerMoved(Point),
    NavigationFinished {
        request: NavigationRequest,
        result: Result<fs::OpenedDirectory, String>,
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
        .title("Waddle")
        .settings(iced::Settings {
            id: Some("io.github.powerpenguini.Waddle".to_owned()),
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
    sidebar_tree: SidebarTree,
    navigation: NavigationSession,
    startup: startup::State,
    recent: recent::Recent,
    trash: trash::Trash,
    operations: Operations,
    search: SearchSession,
    transfers: TransferSession,
    command: CommandSession,
    command_adapter: ProcessAdapter,
    open_with: open_with::Session,
    diagnostics: diagnostics::History,
    file_operations: FileOperationSession,
    journal: journal::Journal,
    location_monitoring: Option<location_monitoring::Native>,
    view_preferences: view_preferences::Preferences,
    thumbnails: thumbnail::Cache,
    grid: GridInteraction,
    browser_input: BrowserInput,
    presentation: Presentation,
    location_input: String,
    location_input_focused: bool,
    modifiers: keyboard::Modifiers,
    mouse_back_gesture: Option<MouseBackGesture>,
    pending_tree_navigation: Option<(NavigationRequest, TreeLoadRequest)>,
    window_size_known: bool,
    pending_reveal_scroll: bool,
    system_mode: iced::theme::Mode,
    system_accessibility: theme::AccessibilityPreferences,
    accent: Option<theme::ThemeColors>,
}

fn duration_ratio(elapsed: Duration, total: Duration) -> f32 {
    (elapsed.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
}

impl App {
    fn view(&self) -> Element<'_, Message> {
        view::View::new(self).render()
    }

    fn new() -> (Self, Task<Message>) {
        let now = Instant::now();
        let startup = startup::State::open_default();
        let view_preferences = view_preferences::Preferences::open_default();
        let preferences_error = view_preferences.error().map(str::to_owned);
        let tree_visible = view_preferences.tree_visible();
        let current =
            startup.initial_directory(view_preferences.remember_last_directory_on_startup());
        let initial_selection = startup.initial_selection();
        let recent = recent::Recent::open_default();
        let trash = trash::Trash::open_default();
        let mut locations = recent.sidebar_entry().into_iter().collect::<Vec<_>>();
        locations.push(trash.sidebar_entry());
        let sidebar_tree = SidebarTree::open_default(locations);
        let interface_settings = theme::interface_settings();
        let accent = theme::load(interface_settings.as_ref());
        let system_accessibility = theme::accessibility(interface_settings.as_ref());
        let (journal, journal_error) = match journal::Journal::open_default() {
            Ok(journal) => (journal, None),
            Err(error) => (journal::Journal::empty_default(), Some(error.to_string())),
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
            sidebar_tree,
            navigation: NavigationSession::new(current.clone()),
            startup,
            recent,
            trash,
            operations: Operations::default(),
            search: SearchSession::default(),
            transfers: TransferSession::open_default(),
            command: CommandSession::default(),
            command_adapter: ProcessAdapter,
            open_with: open_with::Session::default(),
            diagnostics: diagnostics::History::open_default(),
            file_operations: FileOperationSession::default(),
            journal,
            location_monitoring,
            view_preferences,
            thumbnails: thumbnail::Cache::default(),
            grid,
            browser_input: BrowserInput::default(),
            presentation: Presentation::new(now, startup_error),
            location_input: current.display().to_string(),
            location_input_focused: false,
            modifiers: keyboard::Modifiers::default(),
            mouse_back_gesture: None,
            pending_tree_navigation: None,
            window_size_known: false,
            pending_reveal_scroll: false,
            system_mode: iced::theme::Mode::Dark,
            system_accessibility,
            accent,
        };
        let navigation = if initial_selection.is_empty() {
            app.navigation.refresh(None)
        } else {
            app.navigation
                .transition(NavigationTransition::Reveal {
                    requested: current,
                    selected: initial_selection,
                })
                .expect("reveal navigation always creates a request")
        };
        let initial = Task::batch([
            app.request_navigation(navigation),
            system::theme().map(Message::SystemTheme),
            find_window_after_delay(),
        ]);
        (app, initial)
    }

    fn subscription(&self) -> Subscription<Message> {
        let scrollbar_visible = self.grid.scrollbar_visible();
        let reduced_motion = self.reduced_motion();
        let animation = if self.presentation.animation_active(
            reduced_motion,
            self.spinner_active(),
            self.drag_in_progress() || self.grid.marquee_drag_active(),
            scrollbar_visible,
        ) {
            time::every(if reduced_motion {
                Duration::from_millis(100)
            } else {
                Duration::from_millis(16)
            })
            .map(Message::AnimationFrame)
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
        if let Some(subscription) = self.transfers.native_subscription() {
            subscriptions.push(subscription.map(Message::NativeDndEvent));
        }
        if let Some(monitoring) = &self.location_monitoring {
            subscriptions.push(monitoring.subscription().map(Message::DirectoryChanged));
        }
        if self.transfers.overview().active {
            subscriptions
                .push(time::every(Duration::from_millis(100)).map(|_| Message::PollTransfer));
        }
        if matches!(
            self.mouse_back_gesture,
            Some(MouseBackGesture::AwaitingSecondClick { .. })
        ) {
            subscriptions
                .push(time::every(MOUSE_BACK_DOUBLE_CLICK_INTERVAL).map(Message::MouseBackTick));
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
        Theme::custom("Waddle", palette)
    }

    fn spinner_active(&self) -> bool {
        self.foreground_operation_active()
            || self.transfers.overview().active
            || self.navigation.loading()
            || self.search.is_loading()
            || self
                .sidebar_tree
                .rows(self.navigation.current())
                .iter()
                .any(|row| row.loading)
    }

    fn spinner(&self, size: f32) -> widget::Svg<'static> {
        const FRAMES: [&[u8]; 8] = [
            include_bytes!("../ui/icons/spinner-0.svg"),
            include_bytes!("../ui/icons/spinner-1.svg"),
            include_bytes!("../ui/icons/spinner-2.svg"),
            include_bytes!("../ui/icons/spinner-3.svg"),
            include_bytes!("../ui/icons/spinner-4.svg"),
            include_bytes!("../ui/icons/spinner-5.svg"),
            include_bytes!("../ui/icons/spinner-6.svg"),
            include_bytes!("../ui/icons/spinner-7.svg"),
        ];
        let frame = self.presentation.spinner_frame(self.reduced_motion());
        themed_svg(FRAMES[frame], size, view::View::new(self).accent_color())
    }

    fn foreground_operation_active(&self) -> bool {
        self.operations.foreground_active()
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
        self.transfers.stop();
    }
}
