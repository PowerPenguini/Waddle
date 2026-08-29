use super::*;

#[test]
fn startup_reveal_waits_for_actual_window_geometry_before_final_scroll() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/start"));
    app.window_size_known = false;
    let selected = PathBuf::from("/start/revealed.txt");
    let request = app
        .navigation
        .transition(NavigationTransition::Reveal {
            requested: PathBuf::from("/start"),
            selected: vec![selected.clone()],
        })
        .unwrap();

    let _ = app.finish_navigation(
        request,
        NavigationCompletion::Folder(Ok(opened(
            PathBuf::from("/start"),
            vec![entry("first.txt"), entry("revealed.txt")],
        ))),
    );

    assert!(app.pending_reveal_scroll);
    let _ = app.update(Message::WindowResized(iced::Size::new(420.0, 513.0)));
    assert!(app.window_size_known);
    assert!(!app.pending_reveal_scroll);
    assert_eq!(
        app.grid
            .selected_entry()
            .map(|index| &app.navigation.entries()[index].path),
        Some(&selected)
    );
}

#[test]
fn clipboard_ownership_loss_keeps_the_internal_cut_pending() {
    let temp = tempfile::tempdir().unwrap();
    let paths = [temp.path().join("notes.txt"), temp.path().join("todo.txt")];
    for path in &paths {
        std_fs::write(path, "notes").unwrap();
    }
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.replace_displayed_entries(
        paths
            .iter()
            .map(|path| FileEntry {
                path: path.clone(),
                name: path.file_name().unwrap().to_os_string(),
                directory: false,
                metadata: Default::default(),
            })
            .collect(),
    );
    app.navigation.settle_for_test();
    app.grid.select_click(0, false, false, 2);
    app.grid.select_click(1, true, false, 2);

    press(&mut app, "d");
    assert!(app.navigation.entries().is_empty());

    let update = app.transfers.handle_native_with_adapter(
        &NoopTransferAdapter,
        TransferEvent::ClipboardOwnershipLost,
        |_, _| None,
    );
    let _ = app.apply_native_update(update);

    assert_eq!(app.transfers.pending_cut_paths(), paths);
    assert!(app.navigation.entries().is_empty());
    assert!(app.navigation.pending_request().is_none());
    assert_eq!(
        app.presentation.status(),
        "Cut: 2 items, p paste, Esc cancel"
    );
    assert_eq!(app.presentation.notice(), None);

    let request = app
        .transfers
        .paste(temp.path().join("destination"))
        .unwrap();
    assert_eq!(request.paths, paths);
    assert_eq!(request.action, TransferAction::Move);
}

#[test]
fn captured_escape_leaves_the_recursive_search_input() {
    let (mut app, _) = App::new();
    app.browser_input.enter(InputMode::Search);
    app.search.begin(&app.grid);
    let _ = app
        .search
        .update(&mut app.navigation, &mut app.grid, "/needle".to_owned());
    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);

    let _ = app.handle_event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: escape.clone(),
            modified_key: escape,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        }),
        event::Status::Captured,
    );

    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(!app.search.is_active());
    assert!(app.search.query().is_empty());
}

#[test]
fn escape_cancels_pending_navigation_immediately() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    let _ = app.transition_navigation(NavigationTransition::Open {
        requested: PathBuf::from("/slow"),
        remember: true,
        select: None,
    });
    assert_eq!(app.navigation.pending_path(), Some(Path::new("/slow")));

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

    assert!(app.navigation.pending_path().is_none());
    assert_eq!(app.navigation.current(), Path::new("/current"));
}

#[test]
fn first_back_cancels_tree_navigation_and_second_back_uses_history() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    app.navigation
        .seed_history(vec![PathBuf::from("/back")], Vec::new());
    let root = app.sidebar_tree.rows(Path::new("/current"))[0].id;
    assert!(
        app.sidebar_tree
            .install_children(root, Path::new("/"), vec![PathBuf::from("/slow")],)
    );
    let slow = app
        .sidebar_tree
        .rows(Path::new("/current"))
        .into_iter()
        .find(|row| row.path == Path::new("/slow"))
        .unwrap();

    let _ = app.activate_tree_row(slow.id);
    assert_eq!(app.navigation.pending_path(), Some(Path::new("/slow")));
    assert!(
        app.sidebar_tree
            .rows(Path::new("/current"))
            .into_iter()
            .find(|row| row.id == slow.id)
            .unwrap()
            .loading
    );

    let _ = app.update(Message::Back);
    assert!(app.navigation.pending_path().is_none());
    assert_eq!(app.navigation.current(), Path::new("/current"));
    let slow_after_cancel = app
        .sidebar_tree
        .rows(Path::new("/current"))
        .into_iter()
        .find(|row| row.id == slow.id)
        .unwrap();
    assert!(!slow_after_cancel.loading);
    assert!(!app.sidebar_tree.is_expanded(slow.id));

    let _ = app.update(Message::Back);
    assert_eq!(app.navigation.pending_path(), Some(Path::new("/back")));
}

#[test]
fn mouse_side_buttons_request_back_and_forward_navigation() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    app.navigation.seed_history(
        vec![PathBuf::from("/back")],
        vec![PathBuf::from("/forward")],
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    assert!(app.navigation.pending_path().is_none());
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Back)),
        event::Status::Captured,
    );
    let Some(MouseBackGesture::AwaitingSecondClick { first_released_at }) = app.mouse_back_gesture
    else {
        panic!("first Back click should wait for a possible double click");
    };
    assert!(app.navigation.pending_path().is_none());
    let _ = app.update(Message::MouseBackTick(
        first_released_at + MOUSE_BACK_DOUBLE_CLICK_INTERVAL,
    ));
    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/back").as_path())
    );
    app.navigation.settle_for_test();
    let _ = app.update(Message::MouseBackTick(
        first_released_at + MOUSE_BACK_DOUBLE_CLICK_INTERVAL,
    ));
    assert!(app.navigation.pending_path().is_none());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)),
        event::Status::Captured,
    );
    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/forward").as_path())
    );
}

#[test]
fn double_clicking_mouse_back_navigates_to_parent_without_using_history() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current/folder"));
    app.navigation
        .seed_history(vec![PathBuf::from("/history")], Vec::new());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    assert!(app.navigation.pending_path().is_none());
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Back)),
        event::Status::Captured,
    );
    let Some(MouseBackGesture::AwaitingSecondClick { first_released_at }) = app.mouse_back_gesture
    else {
        panic!("first Back click should wait for a possible double click");
    };
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    assert!(app.navigation.pending_path().is_none());
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Back)),
        event::Status::Captured,
    );

    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/current").as_path())
    );
    app.navigation.settle_for_test();
    let _ = app.update(Message::MouseBackTick(
        first_released_at + MOUSE_BACK_DOUBLE_CLICK_INTERVAL,
    ));
    assert!(app.navigation.pending_path().is_none());
}

#[test]
fn holding_mouse_back_no_longer_navigates_to_parent() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current/folder"));

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    let _ = app.update(Message::MouseBackTick(
        Instant::now() + MOUSE_BACK_DOUBLE_CLICK_INTERVAL,
    ));

    assert!(app.navigation.pending_path().is_none());
}

#[test]
fn another_navigation_cancels_a_pending_single_mouse_back_click() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    app.navigation.seed_history(
        vec![PathBuf::from("/back")],
        vec![PathBuf::from("/forward")],
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Back)),
        event::Status::Captured,
    );
    let Some(MouseBackGesture::AwaitingSecondClick { first_released_at }) = app.mouse_back_gesture
    else {
        panic!("first Back click should wait for a possible double click");
    };

    let _ = app.update(Message::Forward);
    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/forward").as_path())
    );
    app.navigation.settle_for_test();
    let _ = app.update(Message::MouseBackTick(
        first_released_at + MOUSE_BACK_DOUBLE_CLICK_INTERVAL,
    ));
    assert!(app.navigation.pending_path().is_none());
}

#[test]
fn displayed_locations_install_watches_from_the_newly_displayed_entries() {
    let temp = tempfile::tempdir().unwrap();
    let recent_parent = temp.path().join("recent-parent");
    let trash_files = temp.path().join("volume-trash/files");
    let trash_info = temp.path().join("volume-trash/info");
    for directory in [&recent_parent, &trash_files, &trash_info] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let (mut app, _) = App::new();
    let recent_file = recent_parent.join("recent.txt");
    std::fs::write(&recent_file, "x").unwrap();
    let request = app.navigation.recent();
    let _ = app.update(Message::RecentLoaded {
        request,
        result: Some(Ok(vec![FileEntry {
            path: recent_file,
            name: "recent.txt".into(),
            directory: false,
            metadata: Default::default(),
        }])),
    });
    assert!(app.displayed_watch_paths().contains(&recent_parent));

    let trashed = trash_files.join("trashed.txt");
    let info = trash_info.join("trashed.txt.trashinfo");
    std::fs::write(&trashed, "x").unwrap();
    std::fs::write(&info, "[Trash Info]").unwrap();
    let request = app.navigation.trash();
    let _ = app.update(Message::TrashLoaded {
        request,
        result: Some(Ok(vec![super::trash::Entry {
            file: FileEntry {
                path: trashed.clone(),
                name: "trashed.txt".into(),
                directory: false,
                metadata: Default::default(),
            },
            receipt: crate::journal::TrashReceipt {
                original: temp.path().join("original.txt"),
                trashed,
                info,
            },
        }])),
    });
    let watched = app.displayed_watch_paths();
    assert!(watched.contains(&trash_files));
    assert!(watched.contains(&trash_info));
}

#[test]
fn drag_notice_survives_refresh_and_clears_on_the_next_interaction() {
    let (mut app, _) = App::new();
    app.presentation.set_notice("Drop failed".to_owned());
    app.refresh_status();
    assert_eq!(app.presentation.notice(), Some("Drop failed"));

    let _ = app.update(Message::Event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    ));
    assert!(app.presentation.notice().is_none());
}

#[test]
fn mount_reconciliation_preserves_existing_nodes() {
    let first = MountRoot {
        path: PathBuf::from("/media/first"),
        label: "First".to_owned(),
    };
    let mut tree = SidebarTree::new(vec![first.clone()]);
    let original_id = tree
        .rows(Path::new("/"))
        .into_iter()
        .find(|row| row.path == first.path)
        .unwrap()
        .id;
    tree.activate(original_id);

    let second = MountRoot {
        path: PathBuf::from("/media/second"),
        label: "Second".to_owned(),
    };
    assert!(tree.reconcile_mounts(vec![first, second]));
    let rows = tree.rows(Path::new("/"));
    assert_eq!(rows[1].id, original_id);
    assert!(tree.is_expanded(original_id));
    assert_eq!(rows[2].label, "Second");
}
