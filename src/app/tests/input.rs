use super::*;

fn press_window_motion(app: &mut App, motion: &'static str) {
    let control_w = keyboard::Key::Character("w".into());
    let _ = app.handle_key(
        control_w.clone(),
        control_w,
        keyboard::Modifiers::CTRL,
        Some("\u{17}"),
    );
    press(app, motion);
}

#[test]
fn context_menu_traps_focus_then_restores_it() {
    let (mut app, _) = App::new();
    app.focus_browser(BrowserFocus::Sidebar);
    assert!(app.grid.open_entry_context(0, 1));
    let tab = keyboard::Key::Named(keyboard::key::Named::Tab);
    let _ = app.handle_key(tab.clone(), tab, keyboard::Modifiers::empty(), None);
    assert_eq!(app.grid.context_menu().unwrap().focused, 1);
    assert_eq!(app.focus.browser(), BrowserFocus::Sidebar);

    let _ = app.update(Message::ContextFocused(0));
    assert_eq!(app.grid.context_menu().unwrap().focused, 0);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(app.grid.context_menu().is_none());
    assert_eq!(app.focus.browser(), BrowserFocus::Sidebar);
}

#[test]
fn control_w_hjkl_moves_spatially_without_targeting_the_bottom_bar() {
    let (mut app, _) = App::new();

    press_window_motion(&mut app, "h");
    assert_eq!(app.focus.browser(), BrowserFocus::Sidebar);
    press_window_motion(&mut app, "l");
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);
    for direction in ["k", "j"] {
        press_window_motion(&mut app, direction);
        assert_eq!(app.focus.browser(), BrowserFocus::Entries);
    }
    assert_eq!(app.presentation.status(), "Focus: files");
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
fn marquee_selection_takes_keyboard_focus_from_the_sidebar_before_copy() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation
        .replace_displayed_entries(vec![entry("selected.txt")]);
    app.focus_browser(BrowserFocus::Sidebar);

    for event in [
        mouse::Event::CursorMoved {
            position: iced::Point::new(700.0, 300.0),
        },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::CursorMoved {
            position: iced::Point::new(
                SIDEBAR_WIDTH + 1.0,
                TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + 1.0,
            ),
        },
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        let _ = app.update(Message::Event(
            iced::Event::Mouse(event),
            event::Status::Ignored,
        ));
    }
    assert_eq!(app.grid.selected_entry(), Some(0));
    press(&mut app, "y");
    assert_eq!(app.presentation.status(), "Copied selected.txt");
    assert_eq!(
        app.transfers
            .clipboard_payload()
            .expect("Copy must act on the selected file")
            .paths,
        [PathBuf::from("/start/selected.txt")]
    );
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);
}

#[test]
fn trash_marquee_selects_entries_in_grid_and_list_views() {
    for list in [false, true] {
        let (mut app, _) = App::new();
        app.navigation.install_trash_entries(
            ["one.txt", "two.txt"]
                .into_iter()
                .map(|name| super::trash::Entry {
                    file: entry(name),
                    receipt: crate::journal::TrashReceipt {
                        original: PathBuf::from("/original").join(name),
                        trashed: PathBuf::from("/start").join(name),
                        info: PathBuf::from("/info").join(format!("{name}.trashinfo")),
                    },
                })
                .collect(),
        );
        app.grid.resize(iced::Size::new(820.0, 560.0));
        app.grid.set_sidebar_visible(true);
        app.grid.set_icon_size(48);
        app.grid.set_list_mode(list);
        app.focus_browser(BrowserFocus::Sidebar);
        for event in [
            mouse::Event::CursorMoved {
                position: iced::Point::new(700.0, 300.0),
            },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::CursorMoved {
                position: iced::Point::new(
                    SIDEBAR_WIDTH + 1.0,
                    TOOLBAR_HEIGHT
                        + TOOLBAR_DIVIDER_HEIGHT
                        + LIST_VIEW_TOP_INSET
                        + LIST_HEADER_HEIGHT,
                ),
            },
        ] {
            let _ = app.update(Message::Event(
                iced::Event::Mouse(event),
                event::Status::Ignored,
            ));
        }
        assert!(app.grid.marquee_drag_active(), "Trash marquee, list={list}");
        assert_eq!(
            app.grid.selected_indices(),
            &std::collections::BTreeSet::from([0, 1]),
            "Trash selection, list={list}",
        );
        assert_eq!(app.focus.browser(), BrowserFocus::Entries);
        let _ = app.update(Message::Event(
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            event::Status::Ignored,
        ));
        assert!(!app.grid.marquee_drag_active());
        assert_eq!(app.grid.selection_count(), 2);
    }
}

#[test]
fn right_clicking_one_of_multiple_selected_entries_keeps_the_selection() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(vec![
        entry("one.txt"),
        entry("two.txt"),
        entry("three.txt"),
    ]);
    app.grid.select_click(0, false, false, 3);
    app.grid.select_click(1, true, false, 3);

    let _ = app.update(Message::EntryContext(0));

    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(app.grid.selected_entry(), Some(0));
    assert_eq!(
        app.grid.context_menu().map(|menu| menu.target),
        Some(ContextTarget::Entry(0))
    );
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
    app.focus_browser(BrowserFocus::Sidebar);

    app.view_preferences
        .apply_command(app.navigation.current(), false, "tree=false")
        .unwrap();
    app.sync_tree_visibility();
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);
    assert_eq!(app.grid.sidebar_width(), 0.0);

    app.move_browser_focus(false);
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);
    app.move_browser_focus(true);
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);

    app.focus_browser(BrowserFocus::Entries);
    press_window_motion(&mut app, "h");
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);

    press_window_motion(&mut app, "e");
    assert!(app.view_preferences.tree_visible());
    assert_eq!(app.grid.sidebar_width(), SIDEBAR_WIDTH);
    assert_eq!(app.presentation.status(), "Tree shown");

    app.focus_browser(BrowserFocus::Entries);
    press_window_motion(&mut app, "h");
    assert_eq!(app.focus.browser(), BrowserFocus::Sidebar);
}

#[test]
fn clicking_toolbar_toggle_preserves_browser_focus() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences =
        super::view_preferences::Preferences::empty_at(temp.path().join("view-preferences.json"));
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();
    app.focus_browser(BrowserFocus::Sidebar);
    let before = app
        .view_preferences
        .for_directory(app.navigation.current())
        .view;

    let _ = app.update(Message::ToggleView);

    assert_ne!(
        app.view_preferences
            .for_directory(app.navigation.current())
            .view,
        before
    );
    assert_eq!(app.focus.browser(), BrowserFocus::Sidebar);
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

    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
        id: "uuid:data".to_owned(),
        path: Some(PathBuf::from("/data")),
        label: "Data".to_owned(),
        can_unmount: true,
    }]);
    let rows = app.sidebar_tree.rows(app.navigation.current());
    let root_id = rows[0].id;
    let drive_id = rows[1].id;
    let TreeActivation::Folder {
        load: Some(request),
        ..
    } = app.sidebar_tree.activate(drive_id).unwrap()
    else {
        panic!("an unopened drive should request its children");
    };
    assert_eq!(
        app.sidebar_tree
            .complete_load(&request, Ok(vec![PathBuf::from("/data/tmp")])),
        TreeLoadOutcome::Installed
    );
    let child_id = app.sidebar_tree.rows(app.navigation.current())[2].id;
    app.focus_browser(BrowserFocus::Sidebar);
    app.sidebar_tree.focus(drive_id);

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
fn directional_entry_keys_reach_the_penultimate_row_before_scrolling() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(
        (0..20)
            .map(|index| entry(&format!("{index}.txt")))
            .collect(),
    );
    app.grid.select_only(Some(0), 20);

    press(&mut app, "j");
    assert_eq!(app.grid.selected_entry(), Some(5));
    assert!(!app.grid.scroll_animation_active());

    press(&mut app, "j");
    assert_eq!(app.grid.selected_entry(), Some(10));
    assert!(!app.grid.scroll_animation_active());

    press(&mut app, "j");
    assert_eq!(app.grid.selected_entry(), Some(15));
    assert!(app.grid.scroll_animation_active());

    press(&mut app, "g");
    press(&mut app, "g");
    assert_eq!(app.grid.selected_entry(), Some(0));
    assert!(!app.grid.scroll_animation_active());

    let arrow_down = keyboard::Key::Named(keyboard::key::Named::ArrowDown);
    let _ = app.handle_key(
        arrow_down.clone(),
        arrow_down,
        keyboard::Modifiers::empty(),
        None,
    );
    assert_eq!(app.grid.selected_entry(), Some(5));
    assert!(!app.grid.scroll_animation_active());
}

#[test]
fn modifier_presses_do_not_cancel_a_pending_browser_sequence() {
    let (mut app, _) = App::new();
    let shift = keyboard::Key::Named(keyboard::key::Named::Shift);

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("'".into()),
            modified_key: keyboard::Key::Character("\"".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Quote),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::SHIFT,
            text: Some("\"".into()),
            repeat: false,
        }),
        event::Status::Ignored,
    );
    assert_eq!(app.presentation.status(), "\"  •  awaiting _");

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key: shift.clone(),
            modified_key: shift.clone(),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::ShiftLeft),
            location: keyboard::Location::Left,
            modifiers: keyboard::Modifiers::empty(),
        }),
        event::Status::Ignored,
    );
    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: shift.clone(),
            modified_key: shift,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::ShiftLeft),
            location: keyboard::Location::Left,
            modifiers: keyboard::Modifiers::SHIFT,
            text: None,
            repeat: false,
        }),
        event::Status::Ignored,
    );

    assert_eq!(app.presentation.status(), "\"  •  awaiting _");

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("-".into()),
            modified_key: keyboard::Key::Character("_".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Minus),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::SHIFT,
            text: Some("_".into()),
            repeat: false,
        }),
        event::Status::Ignored,
    );

    assert_eq!(app.presentation.status(), "\"_  •  awaiting d or x");
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
    app.focus_browser(BrowserFocus::Sidebar);
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
    app.focus_browser(BrowserFocus::Entries);

    let _ = app.update(Message::CopyCommandReport);

    assert_eq!(app.focus.browser(), BrowserFocus::Entries);
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
    let focus = app.update(Message::LocationFocusChanged {
        generation: 0,
        focused: true,
    });
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
    app.focus_browser(BrowserFocus::Sidebar);
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
        super::status_background_style(&theme, 0.0).background,
        Some(iced::Background::Color(color)) if color.a == 1.0
    ));
}

#[test]
fn icon_zoom_keyboard_wheel_and_command_share_the_same_preference() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.view_preferences = view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid.select_only(Some(1), 2);
    app.browser_input.leave_mode();
    for (value, expected) in [("=", 56), ("+", 64), ("-", 56)] {
        let key = keyboard::Key::Character(value.into());
        let _ = app.handle_key(key.clone(), key, keyboard::Modifiers::CTRL, None);
        assert_eq!(app.view_preferences.icon_size(), expected);
        assert_eq!(app.grid.icon_size(), f32::from(expected));
        assert_eq!(app.grid.selected_entry(), Some(1));
    }
    let _ = app.update(Message::IconsZoomed(mouse::ScrollDelta::Lines {
        x: 0.0,
        y: -1.0,
    }));
    assert_eq!(app.view_preferences.icon_size(), 48);
    let _ = app.begin_command(':');
    app.command.change("set icon-size=96".to_owned());
    let _ = app.submit_command();
    assert_eq!(app.view_preferences.icon_size(), 96);
    assert_eq!(app.grid.icon_size(), 96.0);
    assert_eq!(app.grid.selected_entry(), Some(1));
    assert!(!app.navigation.loading());
}
