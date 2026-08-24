use super::*;

#[test]
fn set_tree_from_the_bottom_bar_updates_layout_without_persisting() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("polarexprc");
    let (mut app, _) = App::new();
    app.view_preferences = super::view_preferences::Preferences::empty_at(config.clone());
    app.sync_tree_visibility();

    let _ = app.begin_command(':');
    app.command.change("set tree=false".to_owned());
    let _ = app.submit_command();

    assert!(!app.view_preferences.tree_visible());
    assert_eq!(app.grid.sidebar_width(), 0.0);
    assert!(!config.exists());
}

#[test]
fn folders_first_is_a_setting_not_a_toolbar_control() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();
    assert!(
        app.view_preferences
            .for_directory(app.navigation.current())
            .folders_first
    );

    app.view_preferences
        .apply_command(app.navigation.current(), false, "folders-first=false")
        .unwrap();
    app.browser_focus = BrowserFocus::Toolbar;
    app.toolbar_cursor = usize::MAX;
    let _ = app.move_focused(Motion::Last, 1, false);

    assert_eq!(app.toolbar_cursor, 4);
    assert_eq!(app.status, "Toolbar control 5 of 5");
    assert!(
        !app.view_preferences
            .for_directory(app.navigation.current())
            .folders_first
    );
}

#[test]
fn list_headers_select_and_reverse_each_sort_property() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();

    let _ = app.update(Message::SortBy(crate::fs::SortKey::Size));
    let options = app.view_preferences.for_directory(app.navigation.current());
    assert_eq!(options.sort, crate::fs::SortKey::Size);
    assert!(!options.descending);

    let _ = app.update(Message::SortBy(crate::fs::SortKey::Size));
    assert!(
        app.view_preferences
            .for_directory(app.navigation.current())
            .descending
    );

    let _ = app.update(Message::SortBy(crate::fs::SortKey::Modified));
    let options = app.view_preferences.for_directory(app.navigation.current());
    assert_eq!(options.sort, crate::fs::SortKey::Modified);
    assert!(!options.descending);
}

#[test]
fn list_columns_preserve_name_space_as_the_window_narrows() {
    let (mut app, _) = App::new();

    app.grid.resize(iced::Size::new(660.0, 600.0));
    assert_eq!(app.list_column_visibility(), (false, false));
    assert_eq!(app.list_name_character_limit(), 33);

    app.grid.resize(iced::Size::new(760.0, 600.0));
    assert_eq!(app.list_column_visibility(), (true, false));
    assert_eq!(app.list_name_character_limit(), 32);

    app.grid.resize(iced::Size::new(920.0, 600.0));
    assert_eq!(app.list_column_visibility(), (true, true));
    assert_eq!(app.list_name_character_limit(), 32);

    app.grid.set_sidebar_visible(false);
    assert!(app.list_name_character_limit() > 32);

    app.grid.resize(iced::Size::new(660.0, 600.0));
    assert_eq!(app.list_column_visibility(), (true, false));
}

#[test]
fn list_header_name_aligns_with_entry_names() {
    assert_eq!(
        LIST_HEADER_ICON_SLOT_WIDTH + f32::from(LIST_HEADER_HORIZONTAL_PADDING),
        LIST_ENTRY_ICON_WIDTH
    );
}

#[test]
fn single_click_activation_keeps_modified_clicks_for_selection() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.replace_displayed_entries(vec![FileEntry {
        path: child.clone(),
        name: "child".into(),
        directory: true,
        metadata: Default::default(),
    }]);
    app.view_preferences
        .apply_command(app.navigation.current(), false, "click=single")
        .unwrap();

    app.modifiers = keyboard::Modifiers::CTRL;
    let _ = app.activate_entry(0, false);
    assert!(app.navigation.pending_path().is_none());
    assert_eq!(app.grid.selected_entry(), Some(0));

    app.modifiers = keyboard::Modifiers::empty();
    let _ = app.activate_entry(0, false);
    assert_eq!(app.navigation.pending_path(), Some(child.as_path()));
}

#[test]
fn popular_file_formats_use_distinct_icons() {
    use super::EntryIconKind;

    for (name, expected) in [
        ("main.rs", EntryIconKind::Code),
        ("Dockerfile", EntryIconKind::Code),
        ("manual.pdf", EntryIconKind::Pdf),
        ("notes.md", EntryIconKind::Document),
        ("photo.JPG", EntryIconKind::Image),
        ("song.flac", EntryIconKind::Audio),
        ("movie.mkv", EntryIconKind::Video),
        ("backup.tar.gz", EntryIconKind::Archive),
        ("budget.xlsx", EntryIconKind::Spreadsheet),
        ("slides.pptx", EntryIconKind::Presentation),
        ("unknown.bin", EntryIconKind::Generic),
    ] {
        assert_eq!(super::entry_icon_kind(&entry(name)), expected, "{name}");
    }

    let mut folder = entry("archive.zip");
    folder.directory = true;
    assert_eq!(super::entry_icon_kind(&folder), EntryIconKind::Folder);
}

#[test]
fn standard_sidebar_places_use_distinct_custom_icons() {
    let kinds = [
        NodeKind::Home,
        NodeKind::Desktop,
        NodeKind::Documents,
        NodeKind::Downloads,
        NodeKind::Music,
        NodeKind::Pictures,
        NodeKind::Videos,
        NodeKind::Recent,
        NodeKind::Trash,
    ];
    let icons = kinds.map(super::tree_icon_asset);

    for (index, icon) in icons.iter().enumerate() {
        assert!(icon.starts_with(b"<svg"));
        for other in &icons[index + 1..] {
            assert_ne!(icon, other);
        }
    }
    assert_eq!(
        super::tree_icon_asset(NodeKind::Favorite),
        super::tree_icon_asset(NodeKind::Folder)
    );
}

#[test]
fn pdf_icon_keeps_a_contrasting_white_mark() {
    let icon = std::str::from_utf8(super::entry_icon_asset(super::EntryIconKind::Pdf)).unwrap();

    assert!(icon.contains("#fff"));
}

#[test]
fn tile_label_breaks_at_separators_without_splitting_the_extension() {
    assert_eq!(
        super::tile_label("Mateusz_Jaskiewicz_CV_ATS.pdf"),
        "Mateusz_\u{200b}Jaskiewicz_\u{200b}CV_\u{200b}ATS\u{00a0}.pdf"
    );
}

#[test]
fn long_file_names_are_clipped_without_losing_the_extension() {
    assert_eq!(
        super::clip_file_name("Quarterly_financial_report_2026.xlsx", 20),
        "Quarterly_fina….xlsx"
    );
    assert_eq!(super::clip_file_name("abcdefghijklmnop", 8), "abcdefg…");
    assert_eq!(
        super::clip_file_name("zażółćgęślą_report.txt", 12),
        "zażółćg….txt"
    );
}

#[test]
fn scrollbar_fades_in_holds_and_fades_out_after_scrolling() {
    let now = iced::time::Instant::now();
    let mut visibility = super::ScrollbarVisibility::default();

    assert!(!visibility.is_visible());
    assert_eq!(visibility.opacity(now, false), 0.0);
    visibility.show(now);
    assert!(visibility.is_visible());
    assert_eq!(visibility.opacity(now, false), 0.0);
    assert_eq!(
        visibility.opacity(now + super::SCROLLBAR_FADE_IN / 2, false),
        0.5
    );
    assert_eq!(
        visibility.opacity(now + super::SCROLLBAR_FADE_IN, false),
        1.0
    );
    assert_eq!(visibility.opacity(now + super::SCROLLBAR_HOLD, false), 1.0);
    assert_eq!(
        visibility.opacity(
            now + super::SCROLLBAR_HOLD + super::SCROLLBAR_FADE_OUT / 2,
            false
        ),
        0.5
    );
    assert_eq!(visibility.opacity(now, true), 1.0);

    visibility.hide_if_elapsed(
        now + super::SCROLLBAR_HOLD + super::SCROLLBAR_FADE_OUT - Duration::from_millis(1),
    );
    assert!(visibility.is_visible());

    visibility.hide_if_elapsed(now + super::SCROLLBAR_HOLD + super::SCROLLBAR_FADE_OUT);
    assert!(!visibility.is_visible());
}

#[test]
fn transient_scrollbar_is_thin_rounded_and_has_no_rail() {
    let status = iced::widget::scrollable::Status::Active {
        is_horizontal_scrollbar_disabled: true,
        is_vertical_scrollbar_disabled: false,
    };
    let hidden = super::transient_scrollbar_style(&iced::Theme::Dark, status, 0.0);
    let visible = super::transient_scrollbar_style(&iced::Theme::Dark, status, 1.0);

    assert_eq!(hidden.vertical_rail.background, None);
    assert!(matches!(
        hidden.vertical_rail.scroller.background,
        iced::Background::Color(color) if color.a == 0.0
    ));
    assert!(matches!(
        visible.vertical_rail.scroller.background,
        iced::Background::Color(color) if color.a > 0.0
    ));
    assert_eq!(
        visible.vertical_rail.scroller.border.radius.top_left,
        super::SCROLLBAR_THUMB_WIDTH / 2.0
    );
}

#[test]
fn entering_an_earlier_tile_is_not_cleared_by_the_previous_tiles_exit() {
    let (mut app, _) = App::new();
    app.grid.enter(1);

    let _ = app.update(Message::EntryHovered(0));
    let _ = app.update(Message::EntryUnhovered(1));

    assert_eq!(app.grid.hovered(), Some(0));
}

#[test]
fn sidebar_tree_hover_and_selection_remain_translucent() {
    let (app, _) = App::new();
    let theme = app.iced_theme();
    let hover = super::tree_button_style(
        &theme,
        iced::widget::button::Status::Hovered,
        false,
        false,
        false,
    );
    let selected = super::tree_button_style(
        &theme,
        iced::widget::button::Status::Active,
        true,
        false,
        false,
    );

    assert!(matches!(
        hover.background,
        Some(iced::Background::Color(color)) if color.a == 0.06
    ));
    assert!(matches!(
        selected.background,
        Some(iced::Background::Color(color)) if color.a == 0.22
    ));
}

#[test]
fn list_selection_rounds_only_the_outer_edges_of_each_block() {
    let theme = iced::Theme::Dark;
    let single = super::list_row_style(&theme, true, false, false, false, false);
    let first = super::list_row_style(&theme, true, false, false, false, true);
    let middle = super::list_row_style(&theme, true, false, false, true, true);
    let last = super::list_row_style(&theme, true, false, false, true, false);
    let tile = super::tile_style(&theme, true, false, false, false);

    assert_eq!(single.border.radius, tile.border.radius);
    assert_eq!(first.border.radius.top_left, tile.border.radius.top_left);
    assert_eq!(first.border.radius.bottom_left, 0.0);
    assert_eq!(middle.border.radius.top_left, 0.0);
    assert_eq!(middle.border.radius.bottom_left, 0.0);
    assert_eq!(last.border.radius.top_left, 0.0);
    assert_eq!(
        last.border.radius.bottom_left,
        tile.border.radius.bottom_left
    );
}

#[test]
fn spinner_animation_runs_for_tree_and_background_loading() {
    let (mut app, _) = App::new();
    assert!(app.spinner_active());

    app.explorer.roots[0].loading = false;
    app.navigation.settle_for_test();
    assert!(!app.spinner_active());

    app.busy = true;
    assert!(app.spinner_active());
}
