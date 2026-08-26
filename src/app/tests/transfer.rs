use super::*;

#[test]
fn transfer_conflict_replaces_progress_with_keyboard_choices() {
    let temp = tempfile::tempdir().unwrap();
    let source_directory = temp.path().join("source");
    let destination = temp.path().join("destination");
    std_fs::create_dir_all(&source_directory).unwrap();
    std_fs::create_dir_all(&destination).unwrap();
    let source = source_directory.join("notes.txt");
    std_fs::write(&source, "source").unwrap();
    std_fs::write(destination.join("notes.txt"), "existing").unwrap();

    let mut state = TransferState::default();
    state
        .copy(&[FileEntry {
            path: source.clone(),
            name: "notes.txt".into(),
            directory: false,
            metadata: Default::default(),
        }])
        .unwrap();
    let request = state.paste(destination.clone()).unwrap();
    let (mut app, _) = App::new();
    let work = app
        .transfers
        .enqueue_work(request.clone())
        .unwrap()
        .unwrap();
    let id = work.id();
    let outcome = work.run();
    let _ = app.finish_transfer_batch(id, outcome);

    assert!(app.transfers.overview().conflict_prompt.is_some());
    let trashed = FileEntry {
        path: temp.path().join("trash/files/notes.txt"),
        name: "notes.txt".into(),
        directory: false,
        metadata: Default::default(),
    };
    app.navigation
        .install_trash_entries(vec![super::trash::Entry {
            file: trashed,
            receipt: crate::journal::TrashReceipt {
                original: temp.path().join("restored/notes.txt"),
                trashed: temp.path().join("trash/files/notes.txt"),
                info: temp.path().join("trash/info/notes.txt.trashinfo"),
            },
        }]);
    app.grid.select_only(Some(0), 1);
    let _ = app.restore_selected_trash();
    assert!(!app.foreground_operation_active());
    assert!(app.transfers.overview().conflict_prompt.is_some());

    app.browser_input.enter(InputMode::Search);
    app.transfers.toggle_expanded();
    app.refresh_status();
    let conflict = app.browser_status_model();
    assert_eq!(conflict.presentation, BrowserStatusPresentation::Conflict);
    assert!(conflict.text.contains("r Replace"));
    assert!(conflict.text.contains("s Skip"));
    assert!(conflict.text.contains("k Keep Both"));
    assert!(conflict.text.contains("Esc cancel"));
    assert!(!conflict.retry);
    assert!(!conflict.history);

    let _ = app.cancel_transfer_conflict();
    assert!(app.transfers.overview().retry);
    app.presentation
        .set_notice("External move or removal confirmed".to_owned());
    let notice = app.browser_status_model();
    assert_eq!(notice.presentation, BrowserStatusPresentation::General);
    assert_eq!(notice.text, "External move or removal confirmed");
    assert!(notice.retry);
    assert!(notice.history);
}

#[test]
fn copy_message_uses_the_complete_visual_selection_in_display_order() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(vec![
        entry("one.txt"),
        entry("two.txt"),
        entry("three.txt"),
    ]);
    app.grid.select_only(Some(0), 3);
    app.grid.toggle_visual_selection(3);
    app.grid.move_selection(Motion::Right, 3);
    app.grid.move_selection(Motion::Right, 3);

    let _ = app.update(Message::Copy);
    let request = app.transfers.paste(PathBuf::from("/target")).unwrap();

    assert_eq!(app.presentation.status(), "Copied 3 items");
    assert_eq!(app.presentation.copy_feedback_intensity(false), 1.0);
    assert_eq!(
        request.paths,
        [
            PathBuf::from("/start/one.txt"),
            PathBuf::from("/start/two.txt"),
            PathBuf::from("/start/three.txt"),
        ]
    );
}

#[test]
fn system_clipboard_completion_enters_the_same_transfer_session() {
    let temp = tempfile::tempdir().unwrap();
    let source_directory = temp.path().join("external");
    let destination = temp.path().join("target");
    std::fs::create_dir(&source_directory).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(destination.clone());
    app.navigation.settle_for_test();
    let paths = vec![source_directory.join("one"), source_directory.join("two")];
    for path in &paths {
        std::fs::write(path, "x").unwrap();
    }

    let _ = app.update(Message::ClipboardRead(Ok(ClipboardImport {
        paths: paths.clone(),
        action: TransferAction::Copy,
        generation: None,
    })));

    let request = app.transfers.paste(destination).unwrap();
    assert_eq!(request.paths, paths);
    assert_eq!(request.action, TransferAction::Copy);
    assert!(app.transfers.overview().active);
}

#[test]
fn dd_cuts_only_the_active_item() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two"), entry("three")]);
    app.grid
        .select_only(Some(1), app.navigation.entries().len());

    press(&mut app, "d");
    assert!(app.delete_operator_pending());
    press(&mut app, "d");

    assert_eq!(
        app.transfers.pending_cut_paths(),
        [PathBuf::from("/start/two")]
    );
    assert_eq!(
        app.navigation
            .entries()
            .iter()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["one", "three"]
    );
}

#[test]
fn black_hole_delete_trashes_without_replacing_the_clipboard() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid.select_only(Some(0), 2);
    press(&mut app, "y");
    let copied = app.transfers.clipboard_payload().unwrap();

    app.grid.select_only(Some(1), 2);
    for key in ["\"", "_", "d", "d"] {
        press(&mut app, key);
    }

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Trash { message } if message.contains("two")
    ));
    assert_eq!(app.transfers.clipboard_payload(), Some(copied));
}

#[test]
fn visual_cut_hides_the_complete_selection_and_escape_restores_it() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two"), entry("three")]);
    app.grid.select_only(Some(0), 3);
    app.grid.toggle_visual_selection(3);
    app.grid.move_selection(Motion::Right, 3);
    app.grid.move_selection(Motion::Right, 3);

    press(&mut app, "x");

    assert_eq!(app.transfers.pending_cut_paths().len(), 3);
    assert!(app.navigation.entries().is_empty());

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

    assert!(app.transfers.pending_cut_paths().is_empty());
    assert_eq!(
        app.navigation.pending_path(),
        Some(app.navigation.current())
    );
}

#[test]
fn directory_events_confirm_only_observed_external_cut_moves() {
    let temp = tempfile::tempdir().unwrap();
    let source_directory = temp.path().join("source");
    let destination_directory = temp.path().join("destination");
    std::fs::create_dir(&source_directory).unwrap();
    std::fs::create_dir(&destination_directory).unwrap();
    let source = source_directory.join("item");
    std::fs::write(&source, "x").unwrap();
    let (mut app, _) = App::new();
    app.transfers
        .cut(&[FileEntry {
            path: source.clone(),
            name: "item".into(),
            directory: false,
            metadata: Default::default(),
        }])
        .unwrap();
    app.sync_location_monitoring();

    std::fs::rename(&source, destination_directory.join("item")).unwrap();
    let _ = app.update(Message::DirectoryChanged(super::directory_watch::Event {
        path: source_directory.clone(),
        removed: Vec::new(),
        watch_failed: false,
    }));
    assert_eq!(
        app.transfers.pending_cut_paths(),
        std::slice::from_ref(&source)
    );

    let _ = app.update(Message::DirectoryChanged(super::directory_watch::Event {
        path: source_directory,
        removed: vec![source],
        watch_failed: false,
    }));
    assert!(app.transfers.pending_cut_paths().is_empty());
    assert!(
        app.presentation
            .notice()
            .unwrap()
            .contains("External move or removal confirmed")
    );
}

#[test]
fn raw_mouse_drag_selects_a_grid_rectangle_and_finishes_on_release() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation.replace_displayed_entries(
        (0..9)
            .map(|index| entry(&format!("item-{index}")))
            .collect(),
    );
    let content_top =
        TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + CONTENT_GUTTER + LIST_VIEW_TOP_INSET;
    let start = iced::Point::new(SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0, content_top + 110.0);
    let column_width = app
        .grid
        .visible_range(app.navigation.entries().len(), app.status_height())
        .column_width;
    let end = iced::Point::new(
        SIDEBAR_WIDTH + CONTENT_GUTTER + column_width + 2.0,
        content_top + TILE_ROW_HEIGHT + 30.0,
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved { position: start }),
        event::Status::Ignored,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_some());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved { position: end }),
        event::Status::Ignored,
    );
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [0, 1, 5, 6]
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_none());
}

#[test]
fn raw_mouse_drag_selects_list_rows_and_finishes_on_release() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.grid.set_list_mode(true);
    app.navigation.replace_displayed_entries(
        (0..5)
            .map(|index| entry(&format!("item-{index}")))
            .collect(),
    );
    let list_top =
        TOOLBAR_HEIGHT + TOOLBAR_DIVIDER_HEIGHT + LIST_VIEW_TOP_INSET + LIST_HEADER_HEIGHT;
    let start = iced::Point::new(
        SIDEBAR_WIDTH + CONTENT_GUTTER + 2.0,
        list_top + 5.0 * LIST_ROW_HEIGHT + 8.0,
    );
    let end = iced::Point::new(
        SIDEBAR_WIDTH + CONTENT_GUTTER + 80.0,
        list_top + LIST_ROW_HEIGHT + 2.0,
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved { position: start }),
        event::Status::Ignored,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_some());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved { position: end }),
        event::Status::Ignored,
    );
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_none());
}

#[test]
fn entry_drag_activates_after_six_pixels_and_selects_the_grabbed_item() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    app.grid.move_cursor(
        iced::Point::new(100.0, 100.0),
        app.navigation.entries().len(),
    );

    let _ = app.update(Message::EntryPressed(1));
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(105.0, 100.0),
        }),
        event::Status::Ignored,
    );
    assert!(!app.transfers.overview().pointer_drag.is_active());

    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(106.0, 100.0),
        }),
        event::Status::Ignored,
    );
    assert_eq!(app.transfers.overview().pointer_drag.index(), Some(1));
    assert_eq!(app.grid.selected_entry(), Some(1));
    assert_eq!(
        app.grid
            .selected_indices()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn mouse_navigation_cancels_an_interrupted_entry_drag() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("document.txt")]);
    app.grid.move_cursor(
        iced::Point::new(100.0, 100.0),
        app.navigation.entries().len(),
    );

    let _ = app.update(Message::EntryPressed(0));
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
        event::Status::Captured,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(110.0, 100.0),
        }),
        event::Status::Ignored,
    );

    assert!(!app.transfers.overview().pointer_drag.is_active());
}

#[test]
fn navigation_transition_owns_pointer_cleanup_except_for_drag_hover() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("document.txt")]);
    app.grid.move_cursor(
        iced::Point::new(100.0, 100.0),
        app.navigation.entries().len(),
    );

    let _ = app.update(Message::EntryPressed(0));
    let _ = app.update(Message::Back);
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(110.0, 100.0),
        }),
        event::Status::Ignored,
    );
    assert!(!app.transfers.overview().pointer_drag.is_active());

    app.grid.move_cursor(
        iced::Point::new(100.0, 100.0),
        app.navigation.entries().len(),
    );
    let _ = app.update(Message::EntryPressed(0));
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(110.0, 100.0),
        }),
        event::Status::Ignored,
    );
    assert!(app.transfers.overview().pointer_drag.is_active());

    let _ = app.transition_navigation(NavigationTransition::Hover {
        requested: PathBuf::from("/hovered"),
    });
    assert!(app.transfers.overview().pointer_drag.is_active());
}

#[test]
fn internal_drag_preview_only_appears_after_the_drag_threshold() {
    let (mut app, _) = App::new();
    app.navigation.replace_displayed_entries(vec![entry("one")]);
    app.transfers.press(0, iced::Point::ORIGIN, 1);
    assert!(view::View::drag_preview_layer(&app).is_none());

    app.transfers.move_pointer(
        iced::Point::new(6.0, 0.0),
        app.navigation.entries(),
        app.grid.selected_indices(),
    );
    assert!(view::View::drag_preview_layer(&app).is_some());
}

#[test]
fn incoming_drop_targets_folders_empty_grid_and_rejects_files_and_toolbar() {
    let (mut app, _) = App::new();
    app.grid.resize(iced::Size::new(820.0, 560.0));
    app.navigation = NavigationSession::new(PathBuf::from("/start"));
    let mut folder = entry("folder");
    folder.directory = true;
    app.navigation
        .replace_displayed_entries(vec![folder.clone(), entry("file.txt")]);

    assert_eq!(
        app.drop_destination_at(iced::Point::new(250.0, 80.0), true),
        Some(folder.path)
    );
    assert_eq!(
        app.drop_destination_at(iced::Point::new(365.0, 80.0), true),
        None
    );
    assert_eq!(
        app.drop_destination_at(iced::Point::new(790.0, 400.0), true),
        Some(PathBuf::from("/start"))
    );
    assert_eq!(
        app.drop_destination_at(iced::Point::new(300.0, 20.0), true),
        None
    );
}
