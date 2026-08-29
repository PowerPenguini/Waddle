use std::time::Duration;

use iced::{
    Animation, Background, Border, Color, Element, Fill, Padding, Shadow, Task, Theme, Vector,
    animation::Easing,
    gradient, keyboard, mouse,
    time::Instant,
    widget::{self, Button, button, container, scrollable, svg, text, text_input},
};

use crate::fs::FileEntry;

use super::{
    CONTENT_GUTTER, EntryIconKind, MONO_FONT_SEMIBOLD, Message, Motion, SCROLLBAR_THUMB_WIDTH,
    SCROLLBAR_TRACK_WIDTH, STATUS_HEIGHT, TOOLBAR_ICON_SIZE, command, duration_ratio,
    transfer_session, tree,
};

const COPY_FEEDBACK_HOLD: Duration = Duration::from_millis(320);
const COPY_FEEDBACK_FADE: Duration = Duration::from_millis(680);
const SPINNER_FRAME_DURATION: Duration = Duration::from_millis(100);
const SPINNER_FRAME_COUNT: u128 = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum BrowserFocus {
    Toolbar,
    Location,
    Sidebar,
    #[default]
    Entries,
    BottomBar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusDirection {
    Left,
    Down,
    Up,
    Right,
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

    fn label(self) -> &'static str {
        match self {
            Self::Toolbar => "toolbar",
            Self::Location => "location",
            Self::Sidebar => "sidebar",
            Self::Entries => "files",
            Self::BottomBar => "bottom bar",
        }
    }

    fn moved_in(self, direction: FocusDirection, tree_visible: bool) -> Self {
        match (self, direction) {
            (Self::Toolbar, FocusDirection::Left) if tree_visible => Self::Sidebar,
            (Self::Toolbar, FocusDirection::Right) => Self::Location,
            (Self::Toolbar, FocusDirection::Down) => Self::Entries,
            (Self::Location, FocusDirection::Left) => Self::Toolbar,
            (Self::Location, FocusDirection::Down) => Self::Entries,
            (Self::Sidebar, FocusDirection::Right) => Self::Entries,
            (Self::Entries, FocusDirection::Left) if tree_visible => Self::Sidebar,
            (Self::Entries, FocusDirection::Up) => Self::Location,
            (Self::BottomBar, FocusDirection::Left) if tree_visible => Self::Sidebar,
            (Self::BottomBar, FocusDirection::Up) => Self::Location,
            (Self::BottomBar, _) => Self::Entries,
            _ => self,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrowserStatusPresentation {
    Conflict,
    Transfer,
    General,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BrowserStatusModel<'a> {
    pub(super) presentation: BrowserStatusPresentation,
    pub(super) text: &'a str,
    pub(super) retry: bool,
    pub(super) history: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum TransientPresentationKind {
    Conflict,
    OpenWith,
    CommandOutput,
    FileOperation,
    TransferHistory,
    #[default]
    Standard,
}

impl TransientPresentationKind {
    fn overrides_browser_status(self) -> bool {
        self != Self::Standard
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TransientPresentation {
    kind: TransientPresentationKind,
    expanded_height: Option<f32>,
}

impl TransientPresentation {
    pub(super) fn standard() -> Self {
        Self::default()
    }

    pub(super) fn conflict() -> Self {
        Self {
            kind: TransientPresentationKind::Conflict,
            expanded_height: None,
        }
    }

    pub(super) fn open_with(height: f32) -> Self {
        Self {
            kind: TransientPresentationKind::OpenWith,
            expanded_height: Some(height),
        }
    }

    pub(super) fn command_output(detail: &str) -> Self {
        Self {
            kind: TransientPresentationKind::CommandOutput,
            expanded_height: Some(expanded_bar_height(detail)),
        }
    }

    pub(super) fn file_operation(detail: Option<&str>) -> Self {
        Self {
            kind: TransientPresentationKind::FileOperation,
            expanded_height: detail.map(expanded_bar_height),
        }
    }

    pub(super) fn transfer_history() -> Self {
        Self {
            kind: TransientPresentationKind::TransferHistory,
            expanded_height: Some(190.0),
        }
    }

    pub(super) fn kind(self) -> TransientPresentationKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CopyFeedback {
    started_at: Option<Instant>,
}

impl CopyFeedback {
    fn trigger(&mut self, now: Instant) {
        self.started_at = Some(now);
    }

    fn intensity(self, now: Instant, reduced_motion: bool) -> f32 {
        let Some(started_at) = self.started_at else {
            return 0.0;
        };
        let elapsed = now.saturating_duration_since(started_at);
        if elapsed <= COPY_FEEDBACK_HOLD {
            return 1.0;
        }
        if reduced_motion {
            return 0.0;
        }
        1.0 - duration_ratio(
            elapsed.saturating_sub(COPY_FEEDBACK_HOLD),
            COPY_FEEDBACK_FADE,
        )
    }

    fn active(self, now: Instant) -> bool {
        self.started_at.is_some_and(|started_at| {
            now.saturating_duration_since(started_at) < COPY_FEEDBACK_HOLD + COPY_FEEDBACK_FADE
        })
    }

    fn finish_if_elapsed(&mut self, now: Instant) {
        if !self.active(now) {
            self.started_at = None;
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Presentation {
    focus: BrowserFocus,
    toolbar_cursor: usize,
    bottom_cursor: usize,
    expanded_bar_height: f32,
    output_expansion: Animation<bool>,
    transient: TransientPresentationKind,
    transient_expanded: bool,
    copy_feedback: CopyFeedback,
    now: Instant,
    spinner_started: Instant,
    status: String,
    notice: Option<String>,
}

impl Presentation {
    pub(super) fn new(now: Instant, notice: Option<String>) -> Self {
        Self {
            focus: BrowserFocus::Entries,
            toolbar_cursor: 0,
            bottom_cursor: 0,
            expanded_bar_height: STATUS_HEIGHT,
            output_expansion: Animation::new(false)
                .duration(Duration::from_millis(140))
                .easing(Easing::EaseOut),
            transient: TransientPresentationKind::Standard,
            transient_expanded: false,
            copy_feedback: CopyFeedback::default(),
            now,
            spinner_started: now,
            status: String::new(),
            notice,
        }
    }

    pub(super) fn focus(&self) -> BrowserFocus {
        self.focus
    }

    pub(super) fn focus_is(&self, focus: BrowserFocus) -> bool {
        self.focus == focus
    }

    pub(super) fn set_focus(&mut self, focus: BrowserFocus) {
        self.focus = focus;
    }

    pub(super) fn move_focus(&mut self, reverse: bool, tree_visible: bool) {
        self.focus = self.focus.moved(reverse);
        if !tree_visible && self.focus == BrowserFocus::Sidebar {
            self.focus = self.focus.moved(reverse);
        }
    }

    pub(super) fn move_focus_in(&mut self, direction: FocusDirection, tree_visible: bool) {
        self.focus = self.focus.moved_in(direction, tree_visible);
    }

    pub(super) fn focus_label(&self) -> &'static str {
        self.focus.label()
    }

    pub(super) fn toolbar_cursor(&self) -> usize {
        self.toolbar_cursor
    }

    pub(super) fn move_toolbar_cursor(&mut self, motion: Motion, count: usize) -> String {
        move_composite_cursor(&mut self.toolbar_cursor, 5, motion, count);
        format!("Toolbar control {} of 5", self.toolbar_cursor + 1)
    }

    pub(super) fn bottom_cursor(&self) -> usize {
        self.bottom_cursor
    }

    pub(super) fn reset_bottom_cursor(&mut self) {
        self.bottom_cursor = 0;
    }

    pub(super) fn move_bottom_cursor(
        &mut self,
        action_count: usize,
        motion: Motion,
        count: usize,
    ) -> String {
        let available = action_count.max(1);
        move_composite_cursor(&mut self.bottom_cursor, available, motion, count);
        if action_count == 0 {
            "Bottom bar has no actions".to_owned()
        } else {
            format!(
                "Bottom bar action {} of {available}",
                self.bottom_cursor + 1
            )
        }
    }

    #[cfg(test)]
    pub(super) fn status(&self) -> &str {
        &self.status
    }

    pub(super) fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub(super) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub(super) fn set_notice(&mut self, notice: impl Into<String>) {
        self.notice = Some(notice.into());
    }

    pub(super) fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub(super) fn sync_transient(&mut self, next: TransientPresentation) -> bool {
        let closed =
            self.transient.overrides_browser_status() && !next.kind.overrides_browser_status();
        self.transient = next.kind;
        self.transient_expanded = next.expanded_height.is_some();
        if let Some(height) = next.expanded_height {
            self.expanded_bar_height = height;
        }
        self.now = Instant::now();
        self.output_expansion
            .go_mut(next.expanded_height.is_some(), self.now);
        closed
    }

    pub(super) fn status_height(&self, reduced_motion: bool) -> f32 {
        if reduced_motion {
            return if self.transient_expanded {
                self.expanded_bar_height
            } else {
                STATUS_HEIGHT
            };
        }
        self.output_expansion
            .interpolate(STATUS_HEIGHT, self.expanded_bar_height, self.now)
    }

    pub(super) fn animation_active(
        &self,
        reduced_motion: bool,
        spinner_active: bool,
        pointer_activity: bool,
        scrollbar_visible: bool,
    ) -> bool {
        if reduced_motion {
            return pointer_activity || scrollbar_visible || self.copy_feedback.active(self.now);
        }
        self.output_expansion.is_animating(self.now)
            || spinner_active
            || pointer_activity
            || scrollbar_visible
            || self.copy_feedback.active(self.now)
    }

    pub(super) fn tick(&mut self, now: Instant) {
        self.now = now;
        self.copy_feedback.finish_if_elapsed(now);
    }

    pub(super) fn now(&self) -> Instant {
        self.now
    }

    pub(super) fn set_now(&mut self, now: Instant) {
        self.now = now;
    }

    pub(super) fn spinner_frame(&self, reduced_motion: bool) -> usize {
        if reduced_motion {
            0
        } else {
            ((self.now.duration_since(self.spinner_started).as_millis()
                / SPINNER_FRAME_DURATION.as_millis())
                % SPINNER_FRAME_COUNT) as usize
        }
    }

    pub(super) fn flash_copy_feedback(&mut self) {
        let now = Instant::now();
        self.now = now;
        self.copy_feedback.trigger(now);
    }

    pub(super) fn copy_feedback_intensity(&self, reduced_motion: bool) -> f32 {
        self.copy_feedback.intensity(self.now, reduced_motion)
    }

    pub(super) fn browser_status<'a>(
        &'a self,
        conflict: Option<&'a str>,
        transfer_active: bool,
        retry: bool,
    ) -> BrowserStatusModel<'a> {
        if let Some(prompt) = conflict {
            BrowserStatusModel {
                presentation: BrowserStatusPresentation::Conflict,
                text: prompt,
                retry: false,
                history: false,
            }
        } else if transfer_active {
            BrowserStatusModel {
                presentation: BrowserStatusPresentation::Transfer,
                text: "",
                retry: false,
                history: true,
            }
        } else {
            BrowserStatusModel {
                presentation: BrowserStatusPresentation::General,
                text: self.notice.as_deref().unwrap_or(&self.status),
                retry,
                history: retry,
            }
        }
    }

    #[cfg(test)]
    pub(super) fn expansion(&self) -> (bool, f32) {
        (self.output_expansion.value(), self.expanded_bar_height)
    }

    #[cfg(test)]
    pub(super) fn set_toolbar_cursor(&mut self, cursor: usize) {
        self.toolbar_cursor = cursor;
    }
}

pub(super) fn compact_status_line<'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .center_y(Fill)
        .padding(Padding::from([0, CONTENT_GUTTER as u16]))
        .into()
}

pub(super) fn tile_label(name: &str) -> String {
    let (stem, extension) = name
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
        .unwrap_or((name, ""));
    let stem = stem.replace('_', "_\u{200b}");

    if extension.is_empty() {
        stem
    } else {
        format!("{stem}\u{2060}.{extension}")
    }
}

pub(super) fn clip_file_name(name: &str, max_characters: usize) -> String {
    let character_count = name.chars().count();
    if character_count <= max_characters {
        return name.to_owned();
    }
    if max_characters == 0 {
        return String::new();
    }
    if max_characters == 1 {
        return "…".to_owned();
    }

    let extension = name
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
        .map(|(_, extension)| format!(".{extension}"));
    if let Some(extension) = extension {
        let extension_length = extension.chars().count();
        if extension_length + 2 <= max_characters {
            let stem_length = max_characters - extension_length - 1;
            let stem = name.chars().take(stem_length).collect::<String>();
            return format!("{stem}…{extension}");
        }
    }

    let stem = name.chars().take(max_characters - 1).collect::<String>();
    format!("{stem}…")
}

pub(super) fn entry_icon_kind(entry: &FileEntry) -> EntryIconKind {
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

pub(super) fn entry_icon_asset(kind: EntryIconKind) -> &'static [u8] {
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

pub(super) fn tree_icon_asset(kind: tree::NodeKind) -> &'static [u8] {
    match kind {
        tree::NodeKind::Computer => include_bytes!("../ui/icons/computer.svg"),
        tree::NodeKind::Drive => include_bytes!("../ui/icons/drive.svg"),
        tree::NodeKind::Folder | tree::NodeKind::Favorite => {
            include_bytes!("../ui/icons/folder.svg")
        }
        tree::NodeKind::Home => include_bytes!("../ui/icons/place-home.svg"),
        tree::NodeKind::Desktop => include_bytes!("../ui/icons/place-desktop.svg"),
        tree::NodeKind::Documents => include_bytes!("../ui/icons/place-documents.svg"),
        tree::NodeKind::Downloads => include_bytes!("../ui/icons/place-downloads.svg"),
        tree::NodeKind::Music => include_bytes!("../ui/icons/place-music.svg"),
        tree::NodeKind::Pictures => include_bytes!("../ui/icons/place-pictures.svg"),
        tree::NodeKind::Videos => include_bytes!("../ui/icons/place-videos.svg"),
        tree::NodeKind::Recent => include_bytes!("../ui/icons/place-recent.svg"),
        tree::NodeKind::Trash => include_bytes!("../ui/icons/place-trash.svg"),
    }
}

pub(super) fn find_window_after_delay() -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_millis(100)).await;
        },
        |()| Message::FindWindow,
    )
}

pub(super) fn clears_status_notice(event: &iced::Event) -> bool {
    matches!(
        event,
        iced::Event::Keyboard(keyboard::Event::KeyPressed { .. })
            | iced::Event::Mouse(mouse::Event::ButtonPressed(_))
    )
}

pub(super) fn move_composite_cursor(cursor: &mut usize, len: usize, motion: Motion, count: usize) {
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

pub(super) fn rgba(color: Color, alpha: f32) -> [u8; 4] {
    [
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

pub(super) fn expanded_bar_height(detail: &str) -> f32 {
    let lines = detail.lines().count().max(1) as f32;
    (40.0 + lines * 16.0).clamp(56.0, 280.0)
}

pub(super) fn toolbar_button(
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
        TOOLBAR_ICON_SIZE,
        blend_colors(background, color, if enabled { 0.98 } else { 0.30 }),
    );
    button(icon)
        .on_press_maybe(enabled.then_some(message))
        .width(26)
        .height(30)
        .padding(0)
        .style(move |theme, status| focusable_button_style(theme, status, focused))
}

pub(super) fn sidebar_style(theme: &Theme, opaque: bool) -> container::Style {
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

pub(super) fn browser_background_style(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().background)
}

pub(super) fn entry_content_opacity(
    hidden: bool,
    emphasized: bool,
    reduced_transparency: bool,
) -> f32 {
    if hidden && !emphasized && !reduced_transparency {
        0.70
    } else {
        1.0
    }
}

pub(super) fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a *= opacity.clamp(0.0, 1.0);
    color
}

pub(super) fn transient_vertical_scrollbar() -> scrollable::Direction {
    scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(SCROLLBAR_TRACK_WIDTH)
            .scroller_width(SCROLLBAR_THUMB_WIDTH)
            .margin((SCROLLBAR_TRACK_WIDTH - SCROLLBAR_THUMB_WIDTH) / 2.0),
    )
}

pub(super) fn transient_scrollbar_style(
    theme: &Theme,
    status: scrollable::Status,
    opacity: f32,
) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let thumb_color = with_alpha(theme.palette().text, 0.38 * opacity.clamp(0.0, 1.0));
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: thumb_color.into(),
            border: Border {
                radius: (SCROLLBAR_THUMB_WIDTH / 2.0).into(),
                ..Border::default()
            },
        },
    };
    style.vertical_rail = rail;
    style.horizontal_rail = rail;
    style.gap = None;
    style
}

pub(super) fn focus_container_style(theme: &Theme, focused: bool) -> container::Style {
    if focused {
        container::Style::default().background(with_alpha(theme.palette().primary, 0.08))
    } else {
        container::Style::default()
    }
}

pub(super) fn grid_background_style(
    theme: &Theme,
    drop_target: bool,
    _focused: bool,
) -> container::Style {
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

pub(super) fn status_background_style(
    theme: &Theme,
    focused: bool,
    copy_feedback: f32,
) -> container::Style {
    let background = lighter(theme.palette().background, 16);
    let mut style = container::Style::default().background(background);
    let accent_mix = (if focused { 0.10_f32 } else { 0.0 }).max(copy_feedback * 0.22);
    if accent_mix > 0.0 {
        style.background = Some(Background::Color(blend_colors(
            background,
            theme.palette().primary,
            accent_mix,
        )));
    }
    style
}

pub(super) fn tree_button_style(
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
        } else if focused {
            Some(Background::Color(with_alpha(theme.palette().primary, 0.10)))
        } else if hovered {
            Some(Background::Color(with_alpha(theme.palette().text, 0.06)))
        } else {
            None
        },
        text_color: theme.palette().text,
        border: Border {
            width: if drop_target {
                3.0
            } else if high_contrast && selected {
                1.0
            } else {
                0.0
            },
            color: if drop_target {
                theme.palette().text
            } else {
                theme.palette().background
            },
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

pub(super) fn tile_style(
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
    } else if focused {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.10)));
    } else if hovered {
        style.background = Some(Background::Color(lighter(theme.palette().background, 16)));
    }
    style.border = Border {
        width: if drop_target {
            3.0
        } else if high_contrast && selected {
            1.0
        } else {
            0.0
        },
        color: if drop_target {
            theme.palette().text
        } else {
            theme.palette().background
        },
        radius: 7.0.into(),
    };
    style
}

pub(super) fn list_row_style(
    theme: &Theme,
    selected: bool,
    hovered: bool,
    focused: bool,
    selected_above: bool,
    selected_below: bool,
) -> container::Style {
    let mut style = tile_style(theme, selected, hovered, focused, false);
    let outer_radius = style.border.radius.top_left;
    style.border.radius = 0.0.into();
    if hovered && !selected {
        style.border.radius = outer_radius.into();
    } else if selected {
        if !selected_above {
            style.border.radius.top_left = outer_radius;
            style.border.radius.top_right = outer_radius;
        }
        if !selected_below {
            style.border.radius.bottom_left = outer_radius;
            style.border.radius.bottom_right = outer_radius;
        }
    }
    style
}

pub(super) fn is_high_contrast_theme(theme: &Theme) -> bool {
    theme.palette().background == Color::BLACK && theme.palette().text == Color::WHITE
}

pub(super) fn marquee_style(accent: Color) -> container::Style {
    container::Style::default().background(with_alpha(accent, 0.18))
}

pub(super) fn themed_svg(icon: &'static [u8], size: f32, color: Color) -> widget::Svg<'static> {
    svg(svg::Handle::from_memory(icon))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
}

pub(super) fn entry_svg(kind: EntryIconKind, size: f32, color: Color) -> widget::Svg<'static> {
    let icon = entry_icon_asset(kind);
    if kind == EntryIconKind::Pdf {
        svg(svg::Handle::from_memory(icon)).width(size).height(size)
    } else {
        themed_svg(icon, size, color)
    }
}

pub(super) fn toolbar_button_style(theme: &Theme, status: button::Status) -> button::Style {
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

pub(super) fn focusable_button_style(
    theme: &Theme,
    status: button::Status,
    focused: bool,
) -> button::Style {
    let mut style = toolbar_button_style(theme, status);
    if focused {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.18)));
    }
    style
}

pub(super) fn context_button_style(
    theme: &Theme,
    status: button::Status,
    focused: bool,
) -> button::Style {
    let mut style = button::text(theme, status);
    if focused {
        style.background = Some(Background::Color(with_alpha(theme.palette().primary, 0.24)));
    }
    style
}

pub(super) fn context_menu_button_style(theme: &Theme, focused: bool) -> button::Style {
    context_button_style(theme, button::Status::Active, focused)
}

pub(super) fn command_failure_report(
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

pub(super) fn compact_text_button<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    button(text(label).font(MONO_FONT_SEMIBOLD).size(11))
        .on_press(message)
        .padding(Padding::from([1, 4]))
        .style(toolbar_button_style)
        .into()
}

pub(super) fn format_transfer_snapshot(
    action: &str,
    snapshot: &transfer_session::Snapshot,
) -> String {
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

pub(super) fn format_transfer_bytes(bytes: u64) -> String {
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

pub(super) fn solid_background_style(color: Color) -> container::Style {
    container::Style::default().background(color)
}

pub(super) fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

pub(super) fn lighter(color: Color, amount: u8) -> Color {
    let amount = amount as f32 / 255.0;
    Color::from_rgba(
        (color.r + amount).min(1.0),
        (color.g + amount).min(1.0),
        (color.b + amount).min(1.0),
        color.a,
    )
}

pub(super) fn blend_colors(background: Color, foreground: Color, opacity: f32) -> Color {
    Color::from_rgb(
        background.r + (foreground.r - background.r) * opacity,
        background.g + (foreground.g - background.g) * opacity,
        background.b + (foreground.b - background.b) * opacity,
    )
}

pub(super) fn flat_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    style.background = Background::Color(Color::TRANSPARENT);
    style.border = Border {
        width: 0.0,
        radius: 4.0.into(),
        color: Color::TRANSPARENT,
    };
    style
}

pub(super) fn status_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = flat_input_style(theme, status);
    style.background = Background::Color(Color::TRANSPARENT);
    style
}

pub(super) fn menu_style(theme: &Theme) -> container::Style {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_menu_draws_only_its_shared_active_item() {
        let theme = Theme::Dark;

        assert!(
            context_menu_button_style(&theme, false)
                .background
                .is_none()
        );
        assert!(context_menu_button_style(&theme, true).background.is_some());
    }

    #[test]
    fn hidden_entry_opacity_yields_to_interaction_and_accessibility() {
        assert_eq!(entry_content_opacity(false, false, false), 1.0);
        assert_eq!(entry_content_opacity(true, false, false), 0.70);
        assert_eq!(entry_content_opacity(true, true, false), 1.0);
        assert_eq!(entry_content_opacity(true, false, true), 1.0);

        let color = apply_opacity(Color::from_rgba8(10, 20, 30, 0.5), 0.70);
        assert!((color.a - 0.35).abs() < f32::EPSILON);
    }

    #[test]
    fn copy_feedback_holds_then_fades_to_the_normal_background() {
        let now = Instant::now();
        let mut feedback = CopyFeedback::default();

        assert_eq!(feedback.intensity(now, false), 0.0);
        feedback.trigger(now);
        assert_eq!(feedback.intensity(now, false), 1.0);
        assert_eq!(
            feedback.intensity(now + COPY_FEEDBACK_HOLD + COPY_FEEDBACK_FADE / 2, false),
            0.5
        );
        let finished = now + COPY_FEEDBACK_HOLD + COPY_FEEDBACK_FADE;
        assert_eq!(feedback.intensity(finished, false), 0.0);
        feedback.finish_if_elapsed(finished);
        assert!(!feedback.active(finished));
    }

    #[test]
    fn reduced_motion_keeps_the_hold_but_skips_the_fade() {
        let now = Instant::now();
        let mut feedback = CopyFeedback::default();
        feedback.trigger(now);

        assert_eq!(feedback.intensity(now + COPY_FEEDBACK_HOLD, true), 1.0);
        assert_eq!(
            feedback.intensity(now + COPY_FEEDBACK_HOLD + Duration::from_millis(1), true),
            0.0
        );
    }

    #[test]
    fn transient_lifecycle_restores_status_only_after_the_last_overlay_closes() {
        let now = Instant::now();
        let mut presentation = Presentation::new(now, None);

        assert!(!presentation.sync_transient(TransientPresentation::open_with(120.0)));
        assert!(
            !presentation.sync_transient(TransientPresentation::command_output("command detail"))
        );
        assert!(presentation.sync_transient(TransientPresentation::standard()));
        assert!(!presentation.sync_transient(TransientPresentation::standard()));
    }

    #[test]
    fn transient_lifecycle_owns_reduced_motion_height() {
        let now = Instant::now();
        let mut presentation = Presentation::new(now, None);

        presentation.sync_transient(TransientPresentation::open_with(120.0));
        assert_eq!(presentation.status_height(true), 120.0);

        presentation.sync_transient(TransientPresentation::conflict());
        assert_eq!(presentation.status_height(true), STATUS_HEIGHT);
    }
}
