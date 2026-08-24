use super::*;

#[test]
fn composite_focus_order_wraps_and_context_menu_traps_then_restores_it() {
    assert_eq!(BrowserFocus::Toolbar.moved(false), BrowserFocus::Location);
    assert_eq!(BrowserFocus::Location.moved(false), BrowserFocus::Sidebar);
    assert_eq!(BrowserFocus::Sidebar.moved(false), BrowserFocus::Entries);
    assert_eq!(BrowserFocus::Entries.moved(false), BrowserFocus::BottomBar);
    assert_eq!(BrowserFocus::BottomBar.moved(false), BrowserFocus::Toolbar);
    assert_eq!(BrowserFocus::Toolbar.moved(true), BrowserFocus::BottomBar);

    let (mut app, _) = App::new();
    app.browser_focus = BrowserFocus::Sidebar;
    app.context_menu = Some((0, iced::Point::ORIGIN));
    let tab = keyboard::Key::Named(keyboard::key::Named::Tab);
    let _ = app.handle_key(tab.clone(), tab, keyboard::Modifiers::empty(), None);
    assert_eq!(app.context_menu_cursor, 1);
    assert_eq!(app.browser_focus, BrowserFocus::Sidebar);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(app.context_menu.is_none());
    assert_eq!(app.browser_focus, BrowserFocus::Sidebar);
}

#[test]
fn captured_context_menu_click_does_not_start_marquee_or_clear_selection() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation
        .replace_displayed_entries(vec![entry("selected")]);
    app.grid.select_only(Some(0), 1);
    app.context_menu = Some((0, iced::Point::new(320.0, 120.0)));

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(700.0, 300.0),
        }),
        event::Status::Ignored,
    );
    let _ = app.update(Message::CloseContext);
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Captured,
    );

    assert_eq!(app.grid.selected_entry(), Some(0));
    assert!(app.grid.marquee_bounds(app.status_height()).is_none());
}

#[test]
fn hidden_tree_is_skipped_by_focus_and_control_w_e_restores_it() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("polarexprc"));
    app.browser_focus = BrowserFocus::Sidebar;

    app.view_preferences
        .apply_command(app.navigation.current(), false, "tree=false")
        .unwrap();
    app.sync_tree_visibility();
    assert_eq!(app.browser_focus, BrowserFocus::Entries);
    assert_eq!(app.grid.sidebar_width(), 0.0);

    app.browser_focus = BrowserFocus::Location;
    app.move_browser_focus(false);
    assert_eq!(app.browser_focus, BrowserFocus::Entries);
    app.move_browser_focus(true);
    assert_eq!(app.browser_focus, BrowserFocus::Location);

    let control_w = keyboard::Key::Character("w".into());
    let _ = app.handle_key(
        control_w.clone(),
        control_w,
        keyboard::Modifiers::CTRL,
        Some("\u{17}"),
    );
    press(&mut app, "e");
    assert!(app.view_preferences.tree_visible());
    assert_eq!(app.grid.sidebar_width(), SIDEBAR_WIDTH);
    assert_eq!(app.status, "Tree shown");
}

#[test]
fn space_activates_the_focused_toolbar_control() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();
    app.browser_focus = BrowserFocus::Toolbar;
    app.toolbar_cursor = 4;
    let before = app
        .view_preferences
        .for_directory(app.navigation.current())
        .view;

    let space = keyboard::Key::Named(keyboard::key::Named::Space);
    let _ = app.handle_key(
        space.clone(),
        space,
        keyboard::Modifiers::empty(),
        Some(" "),
    );

    assert_ne!(
        app.view_preferences
            .for_directory(app.navigation.current())
            .view,
        before
    );
    assert_eq!(app.browser_focus, BrowserFocus::Toolbar);
}

#[test]
fn counted_browser_sequences_drive_grid_and_focused_sidebar_with_feedback() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(
        (0..20)
            .map(|index| entry(&format!("{index}.txt")))
            .collect(),
    );
    app.grid.select_only(Some(0), 20);

    press(&mut app, "3");
    assert_eq!(app.status, "3  •  awaiting motion or operator");
    press(&mut app, "j");
    assert_eq!(app.grid.selected_entry(), Some(15));
    press(&mut app, "g");
    assert_eq!(app.status, "g  •  awaiting g");
    press(&mut app, "g");
    assert_eq!(app.grid.selected_entry(), Some(0));

    let child_id = app.explorer.allocate_node_id();
    app.explorer.roots[0].loading = false;
    app.explorer.roots[0].loaded = true;
    app.explorer.roots[0]
        .children
        .push(crate::app::state::FolderNode::folder(
            child_id,
            PathBuf::from("/tmp"),
        ));
    app.browser_focus = BrowserFocus::Sidebar;
    app.sidebar_cursor = Some(app.explorer.roots[0].id);

    press(&mut app, "j");
    assert_eq!(app.sidebar_cursor, Some(child_id));
    press(&mut app, "g");
    press(&mut app, "g");
    assert_eq!(app.sidebar_cursor, Some(app.explorer.roots[0].id));
    let last_sidebar_id = crate::app::tree::flatten_rows(&app.explorer, app.navigation.current())
        .last()
        .unwrap()
        .id;
    press(&mut app, "G");
    assert_eq!(app.sidebar_cursor, Some(last_sidebar_id));

    press(&mut app, "3");
    press(&mut app, "q");
    assert_eq!(app.status, "Invalid Browser sequence: 3q");
}

#[test]
fn iced_browser_modes_follow_the_command_prefixes() {
    let (mut app, _) = App::new();

    press(&mut app, "/");
    assert_eq!(app.browser_input.mode(), InputMode::Search);
    app.browser_input.leave_mode();

    press(&mut app, "!");
    assert_eq!(app.browser_input.mode(), InputMode::Command);
    assert_eq!(app.command.prefix(), Some('!'));
    app.browser_input.leave_mode();

    press(&mut app, ":");
    assert_eq!(app.browser_input.mode(), InputMode::Command);
    assert_eq!(app.command.prefix(), Some(':'));
}

#[test]
fn command_submit_enter_does_not_activate_the_selected_entry() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("child");
    std_fs::create_dir(&child).unwrap();
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.replace_displayed_entries(vec![FileEntry {
        path: child.clone(),
        name: "child".into(),
        directory: true,
        metadata: Default::default(),
    }]);
    app.grid.select_only(Some(0), 1);

    let _ = app.begin_command(':');
    app.command.change("set tree=true".to_owned());
    let _ = app.submit_command();
    assert_eq!(app.browser_input.mode(), InputMode::Browser);

    let enter_event = || {
        let enter = keyboard::Key::Named(keyboard::key::Named::Enter);
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: enter.clone(),
            modified_key: enter,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    };
    let _ = app.handle_event(enter_event(), event::Status::Captured);

    assert!(app.navigation.pending_path().is_none());

    let _ = app.handle_event(enter_event(), event::Status::Ignored);
    assert_eq!(app.navigation.pending_path(), Some(child.as_path()));
}

#[test]
fn iced_vim_keys_toggle_visual_mode_and_arm_cut_operator() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());

    press(&mut app, "v");
    assert!(app.grid.visual_active());

    press(&mut app, "v");
    assert!(!app.grid.visual_active());
    press(&mut app, "d");
    assert!(app.delete_operator_pending());

    press(&mut app, "$");
    assert_eq!(
        app.transfers.pending_cut_paths(),
        [PathBuf::from("/start/one"), PathBuf::from("/start/two")]
    );
    assert!(app.navigation.entries().is_empty());
    assert_eq!(app.status, "Cut: 2 items, p paste, Esc cancel");
}

#[test]
fn focused_sidebar_does_not_apply_file_operators_to_the_grid() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(vec![entry("one")]);
    app.grid.select_only(Some(0), 1);
    app.browser_focus = BrowserFocus::Sidebar;
    app.sidebar_cursor = Some(app.explorer.roots[0].id);

    press(&mut app, "d");

    assert!(app.transfers.pending_cut_paths().is_empty());
    assert!(app.status.contains("sidebar"));
}

#[test]
fn standalone_row_edge_motions_move_the_active_selection() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(
        (0..8)
            .map(|index| entry(&format!("entry-{index}")))
            .collect(),
    );
    app.grid
        .select_only(Some(6), app.navigation.entries().len());

    press(&mut app, "0");
    assert_eq!(app.grid.selected_entry(), Some(5));
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [5]
    );

    press(&mut app, "$");
    assert_eq!(app.grid.selected_entry(), Some(7));
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn captured_browser_key_still_enters_visual_selection() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    let key = keyboard::Key::Character("v".into());

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyV),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: Some("v".into()),
            repeat: false,
        }),
        event::Status::Captured,
    );

    assert!(app.grid.visual_active());
}

#[test]
fn focused_browser_surfaces_keep_an_opaque_window_background() {
    let (app, _) = App::new();
    let theme = app.iced_theme();
    let browser = super::browser_background_style(&theme);
    let grid = super::grid_background_style(&theme, false, true);

    assert_eq!(grid.background, browser.background);
    assert!(matches!(
        super::status_background_style(&theme, true).background,
        Some(iced::Background::Color(color)) if color.a == 1.0
    ));
}
