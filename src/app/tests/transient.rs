use super::*;

#[test]
fn escape_dismisses_visible_output_before_the_hidden_file_prompt() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    let _ = app.show_new_folder();
    let _ = app.update(Message::PromptInputChanged("unfinished".to_owned()));
    app.show_command_output(
        "Result".to_owned(),
        "Background command finished".to_owned(),
    );

    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

    assert!(app.command.output().is_none());
    assert!(
        matches!(app.file_operations.view(), FileOperationView::NewFolder { value, .. }
        if value == "unfinished")
    );
    assert!(app.bottom_input_active());
    assert!(!app.presentation.expansion().0);
}

#[test]
fn output_suspends_and_restores_history_and_editing_modes() {
    for hidden in [
        InputMode::Browser,
        InputMode::Search,
        InputMode::Command,
        InputMode::Rename,
    ] {
        let (mut app, _) = App::new();
        app.navigation.settle_for_test();
        app.navigation
            .replace_displayed_entries(vec![entry("draft.txt")]);
        app.grid.select_only(Some(0), 1);
        match hidden {
            InputMode::Browser => {
                let _ = app.update(Message::ToggleTransferHistory);
            }
            InputMode::Search => {
                let _ = app.begin_search();
            }
            InputMode::Command => {
                let _ = app.begin_command(':');
                app.command.change("set ".to_owned());
            }
            InputMode::Rename => {
                let _ = app.show_rename(0);
            }
            _ => unreachable!(),
        }
        app.show_command_output("Result".to_owned(), "Finished".to_owned());
        assert!(!app.bottom_input_active());

        let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
        let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

        assert!(app.command.output().is_none(), "{hidden:?}");
        assert_eq!(app.browser_input.mode(), hidden);
        if hidden == InputMode::Browser {
            assert!(app.transfers.overview().expanded);
            assert!(app.presentation.expansion().0);
        } else {
            assert!(app.bottom_input_active());
            assert!(!app.presentation.expansion().0);
        }
        if hidden == InputMode::Command {
            assert_eq!(app.command.text(), "set ");
        }
    }
}

#[test]
fn copying_visible_output_cannot_confirm_a_hidden_permanent_delete() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.file_operations
        .finish_trash_transfer(vec![(entry("keep.txt"), "Trash unavailable".to_owned())]);
    app.show_command_output("Result".to_owned(), "Copy this output".to_owned());

    press(&mut app, "y");

    assert!(!app.foreground_operation_active());
    assert!(!app.file_operations.is_busy());
    assert!(app.command.output().is_some());
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::PermanentDelete { .. }
    ));
}

#[test]
fn dismissing_output_refocuses_the_restored_iced_input() {
    use iced::futures::StreamExt;
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    let _ = app.show_new_folder();
    app.show_command_output("Result".to_owned(), "Finished".to_owned());
    let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
    let task = app.update(Message::Event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: escape.clone(),
            modified_key: escape,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        }),
        event::Status::Ignored,
    ));

    let mut input = widget::text_input::State::<
        <iced::Renderer as iced::advanced::text::Renderer>::Paragraph,
    >::new();
    if let Some(mut stream) = iced_runtime::task::into_stream(task) {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Widget(mut operation) = action {
                        operation.focusable(
                            Some(&widget::Id::new(NEW_FOLDER_ID)),
                            iced::Rectangle::default(),
                            &mut input,
                        );
                    }
                }
            });
    }
    assert!(
        input.is_focused(),
        "the restored input must receive Iced focus"
    );
}

#[test]
fn replacing_output_with_a_prompt_restores_status_only_after_dismissal() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.refresh_status();
    let browser_status = app.presentation.status().to_owned();
    app.show_command_output("Result".to_owned(), "Several lines\nof output".to_owned());
    app.presentation.set_status("Working…");
    let expanded = app.presentation.status_height(true);
    assert!(expanded > STATUS_HEIGHT);
    let _ = app.update(Message::AnimationFrame(
        Instant::now() + Duration::from_secs(2),
    ));
    assert_eq!(app.presentation.status_height(false), expanded);

    let _ = app.update(Message::ContextNewFolder);
    assert!(app.command.output().is_none());
    assert_eq!(app.presentation.status(), "Working…");
    assert_eq!(app.presentation.status_height(true), STATUS_HEIGHT);

    let _ = app.update(Message::PromptCancel);
    assert_eq!(app.presentation.status(), browser_status);
    let _ = app.update(Message::AnimationFrame(
        Instant::now() + Duration::from_secs(2),
    ));
    assert_eq!(app.presentation.status_height(false), STATUS_HEIGHT);
}

#[test]
fn output_does_not_take_editing_keys_from_the_visible_location_input() {
    for (key, code, modifiers, status) in [
        (
            keyboard::Key::Character("a".into()),
            keyboard::key::Code::KeyA,
            keyboard::Modifiers::CTRL,
            event::Status::Ignored,
        ),
        (
            keyboard::Key::Named(keyboard::key::Named::Backspace),
            keyboard::key::Code::Backspace,
            keyboard::Modifiers::empty(),
            event::Status::Captured,
        ),
    ] {
        let (mut app, _) = App::new();
        app.navigation.settle_for_test();
        app.navigation
            .replace_displayed_entries(vec![entry("one"), entry("two")]);
        app.grid.select_only(Some(0), 2);
        let _ = app.update(Message::LocationFocusChanged {
            generation: 0,
            focused: true,
        });
        app.show_command_output("Result".to_owned(), "Finished".to_owned());
        let _ = app.update(Message::Event(
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Code(code),
                location: keyboard::Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            }),
            status,
        ));

        assert_eq!(app.grid.selection_count(), 1);
        assert!(!app.navigation.loading());
        assert_eq!(app.browser_input.mode(), InputMode::Location);
    }
}

#[test]
fn entering_rename_keeps_the_initial_filename_selected() {
    use iced::futures::StreamExt;
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("draft.txt")]);
    app.grid.select_only(Some(0), 1);
    app.grid.open_entry_context(0, 1);
    let task = app.update(Message::ContextRename);
    let mut input = widget::text_input::State::<
        <iced::Renderer as iced::advanced::text::Renderer>::Paragraph,
    >::new();
    let mut stream = iced_runtime::task::into_stream(task).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Widget(mut operation) = action {
                    let id = widget::Id::new(RENAME_ID);
                    operation.focusable(Some(&id), iced::Rectangle::default(), &mut input);
                    operation.text_input(Some(&id), iced::Rectangle::default(), &mut input);
                }
            }
        });
    assert!(input.is_focused());
    assert_eq!(
        input
            .cursor()
            .selection(&widget::text_input::Value::new("draft.txt")),
        Some((0, 9))
    );
}
