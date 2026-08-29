use super::*;

#[test]
fn composite_focus_order_wraps_and_context_menu_traps_then_restores_it() {
    let (mut app, _) = App::new();
    app.presentation.set_focus(BrowserFocus::Toolbar);
    for expected in [
        BrowserFocus::Location,
        BrowserFocus::Sidebar,
        BrowserFocus::Entries,
        BrowserFocus::BottomBar,
        BrowserFocus::Toolbar,
    ] {
        app.presentation.move_focus(false, true);
        assert_eq!(app.presentation.focus(), expected);
    }
    app.presentation.move_focus(true, true);
    assert_eq!(app.presentation.focus(), BrowserFocus::BottomBar);

    app.presentation.set_focus(BrowserFocus::Sidebar);
    assert!(app.grid.open_entry_context(0, 1));
    let tab = keyboard::Key::Named(keyboard::key::Named::Tab);
    let _ = app.handle_key(tab.clone(), tab, keyboard::Modifiers::empty(), None);
    assert_eq!(app.grid.context_menu().unwrap().focused, 1);
    assert_eq!(app.presentation.focus(), BrowserFocus::Sidebar);

    let _ = app.update(Message::ContextFocused(0));
    assert_eq!(app.grid.context_menu().unwrap().focused, 0);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(app.grid.context_menu().is_none());
    assert_eq!(app.presentation.focus(), BrowserFocus::Sidebar);
}

#[test]
fn captured_context_menu_click_does_not_start_marquee_or_clear_selection() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation
        .replace_displayed_entries(vec![entry("selected")]);
    app.grid.select_only(Some(0), 1);
    app.grid.move_cursor(iced::Point::new(320.0, 120.0), 1);
    assert!(app.grid.open_entry_context(0, 1));

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
fn right_clicking_empty_grid_space_opens_a_creation_context_menu() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation
        .replace_displayed_entries(vec![entry("selected")]);
    app.grid.select_only(Some(0), 1);
    let empty_space = iced::Point::new(700.0, 300.0);

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: empty_space,
        }),
        event::Status::Ignored,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        event::Status::Ignored,
    );

    assert!(app.grid.context_menu().is_some());
    assert_eq!(app.grid.selected_entry(), None);
    let labels = app
        .context_actions(ContextTarget::Background)
        .into_iter()
        .map(|(label, _)| label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["New Folder", "New Empty File"]);
}

#[test]
fn hidden_tree_is_skipped_by_focus_and_control_w_e_restores_it() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
    app.presentation.set_focus(BrowserFocus::Sidebar);

    app.view_preferences
        .apply_command(app.navigation.current(), false, "tree=false")
        .unwrap();
    app.sync_tree_visibility();
    assert_eq!(app.presentation.focus(), BrowserFocus::Entries);
    assert_eq!(app.grid.sidebar_width(), 0.0);

    app.presentation.set_focus(BrowserFocus::Location);
    app.move_browser_focus(false);
    assert_eq!(app.presentation.focus(), BrowserFocus::Entries);
    app.move_browser_focus(true);
    assert_eq!(app.presentation.focus(), BrowserFocus::Location);

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
    assert_eq!(app.presentation.status(), "Tree shown");
}

#[test]
fn space_activates_the_focused_toolbar_control() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();
    app.presentation.set_focus(BrowserFocus::Toolbar);
    app.presentation.set_toolbar_cursor(4);
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
    assert_eq!(app.presentation.focus(), BrowserFocus::Toolbar);
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
    assert_eq!(
        app.presentation.status(),
        "3  •  awaiting motion or operator"
    );
    press(&mut app, "j");
    assert_eq!(app.grid.selected_entry(), Some(15));
    press(&mut app, "g");
    assert_eq!(app.presentation.status(), "g  •  awaiting g");
    press(&mut app, "g");
    assert_eq!(app.grid.selected_entry(), Some(0));

    let root_id = app.sidebar_tree.rows(app.navigation.current())[0].id;
    assert!(app.sidebar_tree.install_children(
        root_id,
        Path::new("/"),
        vec![PathBuf::from("/tmp")],
    ));
    let child_id = app.sidebar_tree.rows(app.navigation.current())[1].id;
    app.presentation.set_focus(BrowserFocus::Sidebar);
    app.sidebar_tree.focus(root_id);

    press(&mut app, "j");
    assert_eq!(app.sidebar_tree.focused_id(), Some(child_id));
    press(&mut app, "g");
    press(&mut app, "g");
    assert_eq!(app.sidebar_tree.focused_id(), Some(root_id));
    let last_sidebar_id = app
        .sidebar_tree
        .rows(app.navigation.current())
        .last()
        .unwrap()
        .id;
    press(&mut app, "G");
    assert_eq!(app.sidebar_tree.focused_id(), Some(last_sidebar_id));

    press(&mut app, "3");
    press(&mut app, "q");
    assert_eq!(app.presentation.status(), "Invalid Browser sequence: 3q");
}

#[test]
fn focused_sidebar_can_move_above_home_to_computer() {
    let (mut app, _) = App::new();
    let home = PathBuf::from("/home/tester");
    app.sidebar_tree.install_places(vec![places::Entry {
        path: home,
        label: "Home".to_owned(),
        kind: NodeKind::Home,
        favorite_index: None,
    }]);
    let rows = app.sidebar_tree.rows(app.navigation.current());
    let home_id = rows
        .iter()
        .find(|row| row.kind == NodeKind::Home)
        .unwrap()
        .id;
    let computer_id = rows
        .iter()
        .find(|row| row.kind == NodeKind::Computer)
        .unwrap()
        .id;
    app.presentation.set_focus(BrowserFocus::Sidebar);
    app.sidebar_tree.focus(home_id);

    press(&mut app, "h");

    assert_eq!(app.sidebar_tree.focused_id(), Some(computer_id));
}

#[test]
fn context_menu_does_not_offer_template_files() {
    let (app, _) = App::new();
    let labels = app
        .context_actions(ContextTarget::Entry(0))
        .into_iter()
        .map(|(label, _)| label)
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        [
            "New Folder",
            "New Empty File",
            "Properties",
            "Open With…",
            "Rename",
            "Move to Trash",
        ]
    );
}

#[test]
fn open_with_context_shows_compatible_options_and_a_manual_input() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("document.txt");
    std_fs::write(&path, "hello").unwrap();
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(vec![FileEntry {
        path,
        name: "document.txt".into(),
        directory: false,
        metadata: Default::default(),
    }]);
    assert!(app.grid.open_entry_context(0, 1));

    let _ = app.update(Message::ContextOpenWith);

    assert_eq!(app.browser_input.mode(), InputMode::OpenWith);
    assert!(matches!(
        app.open_with.view(),
        open_with::View::Open {
            target_name: "document.txt",
            custom: "",
            ..
        }
    ));
    assert!(app.command.output().is_none());
    assert!(app.grid.context_menu().is_none());

    let _ = app.update(Message::OpenWithChanged(
        "org.example.Custom.desktop".to_owned(),
    ));
    assert!(matches!(
        app.open_with.view(),
        open_with::View::Open {
            custom: "org.example.Custom.desktop",
            ..
        }
    ));

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(!app.open_with.is_open());
}

#[test]
fn copying_command_output_does_not_leave_the_bottom_bar_focused() {
    let (mut app, _) = App::new();
    let _ = app.begin_command(':');
    app.command.change("help".to_owned());
    let _ = app.submit_command();
    app.presentation.set_focus(BrowserFocus::Entries);

    let _ = app.update(Message::CopyCommandReport);

    assert_eq!(app.presentation.focus(), BrowserFocus::Entries);
    assert_eq!(app.presentation.copy_feedback_intensity(false), 1.0);
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
fn focused_location_input_owns_control_a_even_when_the_event_is_ignored() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two"), entry("three")]);
    app.grid
        .select_only(Some(1), app.navigation.entries().len());
    let focus = app.update(Message::LocationFocusChanged(true));
    let key = keyboard::Key::Character("a".into());

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
            repeat: false,
        }),
        event::Status::Ignored,
    );

    assert_eq!(app.browser_input.mode(), InputMode::Location);
    assert_eq!(focus.units(), 1);
    assert_eq!(app.grid.selected_indices(), &[1].into_iter().collect());
}

#[test]
fn captured_mouse_click_refocuses_an_active_bottom_input() {
    let (mut app, _) = App::new();
    let _ = app.begin_command(':');

    let task = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Captured,
    );

    assert_eq!(task.units(), 2);

    let (mut browser, _) = App::new();
    let task = browser.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Captured,
    );
    assert_eq!(task.units(), 1);
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
    assert_eq!(
        app.presentation.status(),
        "Cut: 2 items, p paste, Esc cancel"
    );
}

#[test]
fn focused_sidebar_does_not_apply_file_operators_to_the_grid() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(vec![entry("one")]);
    app.grid.select_only(Some(0), 1);
    app.presentation.set_focus(BrowserFocus::Sidebar);
    let root_id = app.sidebar_tree.rows(app.navigation.current())[0].id;
    app.sidebar_tree.focus(root_id);

    press(&mut app, "d");

    assert!(app.transfers.pending_cut_paths().is_empty());
    assert!(app.presentation.status().contains("sidebar"));
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
        super::status_background_style(&theme, true, 0.0).background,
        Some(iced::Background::Color(color)) if color.a == 1.0
    ));
}
