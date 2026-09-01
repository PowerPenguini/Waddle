use super::*;

#[test]
fn app_tests_do_not_open_the_user_operation_journal() {
    let (app, _) = App::new();

    assert!(!app.journal.uses_default_storage());
}

#[test]
fn trash_location_shows_original_path_and_requires_permanent_delete_confirmation() {
    let (mut app, _) = App::new();
    let trashed = PathBuf::from("/tmp/Trash/files/report.txt.2");
    let original = PathBuf::from("/home/user/report.txt");
    let file = crate::fs::FileEntry {
        path: trashed.clone(),
        name: "report.txt".into(),
        directory: false,
        metadata: Default::default(),
    };
    app.navigation
        .install_trash_entries(vec![crate::app::trash::Entry {
            file: file.clone(),
            receipt: crate::journal::TrashReceipt {
                original: original.clone(),
                trashed,
                info: PathBuf::from("/tmp/Trash/info/report.txt.2.trashinfo"),
            },
        }]);
    app.grid.select_only(Some(0), 1);

    app.refresh_status();
    assert!(
        app.presentation
            .status()
            .contains(&original.display().to_string())
    );
    let _ = app.update(Message::ContextDeletePermanent);
    assert!(matches!(
        app.file_operations.view(),
        crate::app::file_operation::View::PermanentDelete { detail, .. }
            if detail.contains("cannot be undone")
    ));
}

#[test]
fn r_opens_inline_rename_for_the_active_entry() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt"), entry("two.txt")]);
    app.grid
        .select_only(Some(1), app.navigation.entries().len());

    press(&mut app, "r");

    assert_eq!(app.browser_input.mode(), InputMode::Rename);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Rename {
            value: "two.txt",
            error: ""
        }
    ));
}

#[test]
fn inline_rename_validates_and_escape_cancels() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    press(&mut app, "r");

    let _ = app.update(Message::RenameChanged("bad/name".to_owned()));
    let _ = app.update(Message::RenameSubmitted);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Rename { error, .. }
            if error == "The name cannot contain a slash or NUL character."
    ));
    assert!(!app.foreground_operation_active());

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
}

#[test]
fn successful_inline_rename_returns_to_the_browser() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("one.txt");
    std::fs::write(&source, "one").unwrap();
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();
    app.navigation.replace_displayed_entries(vec![FileEntry {
        path: source,
        name: "one.txt".into(),
        directory: false,
        metadata: Default::default(),
    }]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    press(&mut app, "r");
    app.file_operations.change_name("renamed.txt".to_owned());
    let completion = app
        .file_operations
        .submit_name(app.navigation.current().to_path_buf())
        .unwrap()
        .run();
    let _ = app.finish_file_operation(completion);

    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
    assert!(temp.path().join("renamed.txt").is_file());
}

#[test]
fn new_folder_uses_an_inline_prompt_with_validation_and_escape() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();

    let _ = app.show_new_folder();
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::NewFolder {
            value: "",
            error: ""
        }
    ));

    let _ = app.update(Message::PromptInputChanged("bad/name".to_owned()));
    let _ = app.update(Message::PromptSubmit);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::NewFolder { error, .. }
            if error == "The name cannot contain a slash or NUL character."
    ));

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
}

#[test]
fn errors_expand_in_the_bottom_bar_and_escape_closes_them() {
    let (mut app, _) = App::new();
    app.show_error("Could not open the selected item".to_owned());

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Error { .. }
    ));
    assert!(app.presentation.expansion().0);
    assert!(app.presentation.expansion().1 > super::STATUS_HEIGHT);

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
    assert!(!app.presentation.expansion().0);
}

#[test]
fn trash_failure_uses_an_expanded_permanent_delete_prompt() {
    let (mut app, _) = App::new();
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());
    let _ = app.show_trash_prompt();
    let Some(FileOperationConfirmation::Trash(entries)) = app
        .file_operations
        .confirm(app.navigation.current().to_path_buf())
    else {
        panic!("Trash confirmation must start a Transfer");
    };
    app.file_operations.finish_trash_transfer(vec![(
        entries[0].clone(),
        "Trash is unavailable".to_owned(),
    )]);
    app.sync_transient_presentation();

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::PermanentDelete { message, detail }
            if message.contains("Permanently delete")
            && detail.contains("Trash is unavailable")
            && detail.contains("cannot be undone")
    ));
    assert!(app.presentation.expansion().0);

    let enter = keyboard::Key::Named(keyboard::key::Named::Enter);
    let task = app.handle_key(enter.clone(), enter, keyboard::Modifiers::empty(), None);
    assert!(app.foreground_operation_active());
    drop(task);
}

#[test]
fn live_refresh_preserves_scroll_selection_rename_and_pending_cut_by_path() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    let current = app.navigation.current().to_path_buf();
    let first = FileEntry {
        path: current.join("first"),
        name: "first".into(),
        directory: false,
        metadata: Default::default(),
    };
    let cut = FileEntry {
        path: current.join("cut"),
        name: "cut".into(),
        directory: false,
        metadata: Default::default(),
    };
    app.navigation
        .replace_displayed_entries(vec![first.clone(), cut.clone()]);
    app.grid.select_only(Some(0), 2);
    app.grid.set_scroll(173.0);
    app.browser_input.enter(InputMode::Rename);
    app.transfers.cut(std::slice::from_ref(&cut));
    let request = app.navigation.refresh_selected(vec![first.path.clone()]);

    let _ = app.finish_navigation(
        request,
        NavigationCompletion::Folder(Ok(opened(current, vec![first.clone(), cut]))),
    );

    assert_eq!(app.grid.scroll_offset(), 173.0);
    assert_eq!(app.browser_input.mode(), InputMode::Rename);
    assert_eq!(app.transfers.pending_cut_paths().len(), 1);
    assert_eq!(
        app.grid
            .selected_entry()
            .and_then(|index| app.navigation.entries().get(index))
            .map(|entry| &entry.path),
        Some(&first.path)
    );
}

#[test]
fn deletion_prompt_accepts_y_and_n_from_the_keyboard() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());

    let _ = app.show_trash_prompt();
    press(&mut app, "n");
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));

    let _ = app.show_trash_prompt();
    let key = keyboard::Key::Character("Y".into());
    let task = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("Y"));
    assert!(app.transfers.overview().active);
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
    drop(task);
}

#[test]
fn command_output_expands_and_collapses_through_the_animation_state() {
    let (mut app, _) = App::new();

    let _ = app.begin_command(':');
    app.command.change("help".to_owned());
    let _ = app.submit_command();
    assert!(app.command.output().is_some());
    assert!(app.presentation.expansion().0);
    assert!(app.presentation.expansion().1 > super::STATUS_HEIGHT);

    app.close_command_output();
    assert!(app.command.output().is_none());
    assert!(!app.presentation.expansion().0);
}

#[test]
fn closing_properties_restores_the_previous_browser_status() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("document.txt")]);
    app.grid.select_only(Some(0), 1);
    app.refresh_status();
    let previous_status = app.presentation.status().to_owned();

    let _ = app.show_properties();
    assert_eq!(app.presentation.status(), "Reading Properties…");
    let _ = app.update(Message::PropertiesFinished(Ok(properties::Info {
        name: "document.txt".to_owned(),
        detail: "Type: Plain text".to_owned(),
    })));
    let _ = app.apply_input_intent(InputIntent::CloseCommandOutput);

    assert!(app.command.output().is_none());
    assert_eq!(app.presentation.status(), previous_status);
}

#[test]
fn command_metadata_actions_accept_paths_without_a_grid_selection() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("notes from today.txt");
    std::fs::write(&path, "hello").unwrap();

    let (mut properties_app, _) = App::new();
    properties_app.navigation.settle_for_test();
    let _ = properties_app.begin_command(':');
    properties_app
        .command
        .change(format!("properties \"{}\"", path.display()));
    let properties_task = properties_app.submit_command();
    assert_eq!(properties_app.presentation.status(), "Reading Properties…");
    drop(properties_task);

    let (mut chmod_app, _) = App::new();
    chmod_app.navigation.settle_for_test();
    let _ = chmod_app.begin_command(':');
    chmod_app
        .command
        .change(format!("chmod 640 \"{}\"", path.display()));
    let chmod_task = chmod_app.submit_command();
    assert_eq!(chmod_app.presentation.status(), "Changing permissions…");
    drop(chmod_task);

    let (mut open_with_app, _) = App::new();
    open_with_app.navigation.settle_for_test();
    let _ = open_with_app.begin_command(':');
    open_with_app
        .command
        .change(format!("open-with -- \"{}\"", path.display()));
    let open_with_task = open_with_app.submit_command();
    assert!(matches!(
        open_with_app.open_with.view(),
        open_with::View::Open { target_name, .. } if target_name == "notes from today.txt"
    ));
    drop(open_with_task);
}

#[test]
fn command_output_actions_are_visually_separated() {
    assert!(super::super::bottom_bar::command_output_action_spacing() >= 8.0);
}
