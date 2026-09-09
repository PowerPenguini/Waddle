use super::*;

type InputState =
    widget::text_input::State<<iced::Renderer as iced::advanced::text::Renderer>::Paragraph>;

fn apply_widget_task(
    task: Task<Message>,
    id: &'static str,
    input: &mut InputState,
) -> Vec<Message> {
    use iced::futures::StreamExt;
    let mut messages = Vec::new();
    if let Some(mut stream) = iced_runtime::task::into_stream(task) {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                while let Some(action) = stream.next().await {
                    match action {
                        iced_runtime::Action::Widget(mut operation) => {
                            operation.focusable(
                                Some(&widget::Id::new(id)),
                                iced::Rectangle::default(),
                                input,
                            );
                            operation.text_input(
                                Some(&widget::Id::new(id)),
                                iced::Rectangle::default(),
                                input,
                            );
                            let _ = operation.finish();
                        }
                        iced_runtime::Action::Output(message) => messages.push(message),
                        _ => {}
                    }
                }
            });
    }
    messages
}

fn key(app: &mut App, key: keyboard::Key, modifiers: keyboard::Modifiers) -> Task<Message> {
    let code = match &key {
        keyboard::Key::Character(value) if value.as_str() == "l" => keyboard::key::Code::KeyL,
        keyboard::Key::Named(keyboard::key::Named::Escape) => keyboard::key::Code::Escape,
        _ => keyboard::key::Code::KeyA,
    };
    app.update(Message::Event(
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }),
        event::Status::Ignored,
    ))
}

#[test]
fn clicking_files_after_editing_location_routes_select_all_to_files() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    let _ = key(
        &mut app,
        keyboard::Key::Character("l".into()),
        keyboard::Modifiers::CTRL,
    );
    assert_eq!(app.browser_input.mode(), InputMode::Location);

    let _ = app.update(Message::EntryPressed(0));
    let _ = app.update(Message::EntryReleased(0));
    let _ = key(
        &mut app,
        keyboard::Key::Character("a".into()),
        keyboard::Modifiers::CTRL,
    );

    assert_eq!(
        app.grid.selection_count(),
        2,
        "the location editor must relinquish keyboard ownership when files are clicked"
    );
}

#[test]
fn escape_releases_location_widget_and_preserves_browser_return_target() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.focus_browser(BrowserFocus::Sidebar);
    let mut input = InputState::new();
    let task = key(
        &mut app,
        keyboard::Key::Character("l".into()),
        keyboard::Modifiers::CTRL,
    );
    apply_widget_task(task, LOCATION_ID, &mut input);
    assert!(input.is_focused());

    let task = key(
        &mut app,
        keyboard::Key::Named(keyboard::key::Named::Escape),
        keyboard::Modifiers::empty(),
    );
    apply_widget_task(task, LOCATION_ID, &mut input);

    assert_eq!(app.focus.browser(), BrowserFocus::Sidebar);
    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    assert!(
        !input.is_focused(),
        "Escape must release the actual Iced input, not just its keyboard mode"
    );
}

#[test]
fn changing_to_sidebar_cancels_an_incomplete_file_operator() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    app.grid.select_only(Some(0), 2);
    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
        id: "fixture".into(),
        path: Some(app.navigation.current().to_path_buf()),
        label: "Fixture".into(),
        can_unmount: false,
    }]);
    let row = app
        .sidebar_tree
        .rows(app.navigation.current())
        .into_iter()
        .find(|row| row.label == "Fixture")
        .unwrap();
    press(&mut app, "d");
    assert!(app.delete_operator_pending());

    let _ = app.update(Message::TreeRow(row.id));
    press(&mut app, "j");

    assert!(
        app.transfers.pending_cut_paths().is_empty(),
        "a file operator started before the focus transition must not consume sidebar navigation"
    );
    assert_eq!(app.navigation.entries().len(), 2);
}

#[test]
fn delayed_location_probe_cannot_steal_focus_from_a_new_command() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    let mut location = InputState::new();
    apply_widget_task(app.begin_location(), LOCATION_ID, &mut location);
    let probe = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Captured,
    );
    let mut replies = apply_widget_task(probe, LOCATION_ID, &mut location);
    assert_eq!(replies.len(), 1);
    let mut command = InputState::new();
    apply_widget_task(app.begin_command(':'), COMMAND_ID, &mut command);
    assert!(command.is_focused());

    let task = app.update(replies.pop().unwrap());
    apply_widget_task(task, COMMAND_ID, &mut command);

    assert_eq!(app.browser_input.mode(), InputMode::Command);
    assert!(
        command.is_focused(),
        "an obsolete observation must not redirect Iced focus"
    );
}

#[test]
fn dismissing_a_new_folder_editor_does_not_leave_location_owning_the_keyboard() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one"), entry("two")]);
    let mut location = InputState::new();
    apply_widget_task(app.begin_location(), LOCATION_ID, &mut location);
    let mut name = InputState::new();
    apply_widget_task(
        app.update(Message::ContextNewFolder),
        NEW_FOLDER_ID,
        &mut name,
    );
    assert!(name.is_focused());
    let _ = app.update(Message::PromptCancel);

    let _ = key(
        &mut app,
        keyboard::Key::Character("a".into()),
        keyboard::Modifiers::CTRL,
    );

    assert_eq!(
        app.grid.selection_count(),
        2,
        "dismissing New Folder must not revive a Location mode without a focused Location widget"
    );
}
