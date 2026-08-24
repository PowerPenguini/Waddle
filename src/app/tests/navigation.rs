use super::*;

#[test]
fn clipboard_ownership_loss_survives_refresh_as_a_notice() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("notes.txt");
    std_fs::write(&path, "notes").unwrap();
    let file = FileEntry {
        path: path.clone(),
        name: "notes.txt".into(),
        directory: false,
        metadata: Default::default(),
    };
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.replace_displayed_entries(vec![file.clone()]);
    app.navigation.settle_for_test();
    app.grid.select_only(Some(0), 1);

    let _ = app.cut_selection();
    let generation = app.transfers.clipboard_payload().unwrap().generation;
    assert!(app.navigation.entries().is_empty());

    let update = app.transfers.handle_native(
        &NoopTransferAdapter,
        TransferEvent::ClipboardOwnershipLost { generation },
        |_, _| None,
    );
    let _ = app.apply_native_update(update);
    let request = app.navigation.pending_request().unwrap();
    let requested = request.requested().unwrap().to_path_buf();
    let _ = app.finish_navigation(
        request,
        NavigationCompletion::Folder(Ok((requested, vec![file.clone()]))),
    );

    assert!(app.transfers.pending_cut_paths().is_empty());
    assert_eq!(app.navigation.entries().len(), 1);
    assert_eq!(app.navigation.entries()[0].path, file.path);
    assert_eq!(
        app.status_notice.as_deref(),
        Some("Cut restored after clipboard ownership changed")
    );
}

#[test]
fn missing_directory_falls_back_to_the_nearest_existing_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("gone/child");
    assert_eq!(nearest_existing_ancestor(&missing), temp.path());
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
    assert_eq!(
        app.navigation.pending_path(),
        Some(PathBuf::from("/back").as_path())
    );

    app.navigation.settle_for_test();
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
    app.status_notice = Some("Drop failed".to_owned());
    app.refresh_status();
    assert_eq!(app.status_notice.as_deref(), Some("Drop failed"));

    let _ = app.update(Message::Event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    ));
    assert!(app.status_notice.is_none());
}

#[test]
fn mount_reconciliation_preserves_existing_nodes() {
    let first = MountRoot {
        path: PathBuf::from("/media/first"),
        label: "First".to_owned(),
    };
    let mut state = ExplorerState::new(vec![first.clone()]);
    let original_id = state.roots[1].id;
    state.roots[1].expanded = true;

    let second = MountRoot {
        path: PathBuf::from("/media/second"),
        label: "Second".to_owned(),
    };
    assert!(state.reconcile_mounts(vec![first, second]));
    assert_eq!(state.roots[1].id, original_id);
    assert!(state.roots[1].expanded);
    assert_eq!(state.roots[2].label, "Second");
}
