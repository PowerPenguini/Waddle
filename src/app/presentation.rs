use super::*;

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
        format!("{stem}\u{00a0}.{extension}")
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

pub(super) fn tree_icon_asset(kind: state::NodeKind) -> &'static [u8] {
    match kind {
        state::NodeKind::Computer => include_bytes!("../ui/icons/computer.svg"),
        state::NodeKind::Drive => include_bytes!("../ui/icons/drive.svg"),
        state::NodeKind::Folder | state::NodeKind::Favorite => {
            include_bytes!("../ui/icons/folder.svg")
        }
        state::NodeKind::Home => include_bytes!("../ui/icons/place-home.svg"),
        state::NodeKind::Desktop => include_bytes!("../ui/icons/place-desktop.svg"),
        state::NodeKind::Documents => include_bytes!("../ui/icons/place-documents.svg"),
        state::NodeKind::Downloads => include_bytes!("../ui/icons/place-downloads.svg"),
        state::NodeKind::Music => include_bytes!("../ui/icons/place-music.svg"),
        state::NodeKind::Pictures => include_bytes!("../ui/icons/place-pictures.svg"),
        state::NodeKind::Videos => include_bytes!("../ui/icons/place-videos.svg"),
        state::NodeKind::Recent => include_bytes!("../ui/icons/place-recent.svg"),
        state::NodeKind::Trash => include_bytes!("../ui/icons/place-trash.svg"),
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

pub(super) fn nearest_existing_ancestor(path: &Path) -> PathBuf {
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

pub(super) fn status_background_style(theme: &Theme, focused: bool) -> container::Style {
    let background = lighter(theme.palette().background, 16);
    let mut style = container::Style::default().background(background);
    if focused {
        style.background = Some(Background::Color(blend_colors(
            background,
            theme.palette().primary,
            0.10,
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
    if selected {
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
    container::Style::default()
        .background(with_alpha(accent, 0.18))
        .border(Border {
            width: 1.0,
            color: accent,
            ..Border::default()
        })
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
