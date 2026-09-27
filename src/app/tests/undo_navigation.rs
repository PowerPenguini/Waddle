use super::*;

fn app_at(path: &Path) -> App {
    let (mut app, _) = App::new();
    app.journal = crate::journal::Journal::in_memory();
    app.transfers = transfer_session::TransferSession::open(path.join("history.json"));
    app.navigation = NavigationSession::new(path.to_path_buf());
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("first"), entry("second")]);
    app.grid.select_only(Some(0), 2);
    app.focus_browser(BrowserFocus::Entries);
    app
}

#[test]
fn undo_navigation_keyboard_selection_remains_available_during_undo_and_redo() {
    for redo in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut app = app_at(temp.path());
        let task = app.run_journal(redo);
        assert!(app.foreground_operation_active());
        press(&mut app, "G");
        assert_eq!(
            app.grid.selected_entry(),
            Some(1),
            "Undo/Redo swallowed navigation"
        );
        press(&mut app, "v");
        assert!(app.grid.visual_active(), "Undo/Redo swallowed selection");
        assert!(!app.operation_access().history);
        assert!(!app.operation_access().entries);
        assert!(!app.operation_access().destination);
        drop(task);
    }
}

#[test]
fn undo_navigation_collections_remain_available_during_undo_and_redo() {
    for redo in [false, true] {
        for trash in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let mut app = app_at(temp.path());
            let undo = app.run_journal(redo);
            let navigation = if trash {
                app.open_trash()
            } else {
                app.open_recent()
            };
            assert!(
                app.navigation.loading(),
                "Undo/Redo blocked collection navigation"
            );
            drop(navigation);
            drop(undo);
        }
    }
}

#[test]
fn undo_navigation_completion_preserves_a_newer_selection() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let before = temp.path().join("a-before");
            let renamed = temp.path().join("a-renamed");
            let keep = temp.path().join("z-keep");
            std_fs::write(&renamed, b"undo rename").unwrap();
            std_fs::write(&keep, b"keep selected").unwrap();
            let mut app = app_at(temp.path());
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.grid.select_only(Some(0), 2);
            app.journal
                .record(crate::journal::Action::rename(before.clone(), renamed).unwrap())
                .unwrap();
            let undo = app.run_journal(false);
            // Mouse selection can change while the journal worker is pending.
            app.grid.select_only(Some(1), 2);
            super::navigation::finish_tasks(&mut app, undo).await;
            assert!(before.exists());
            assert_eq!(
                app.selected_entries()
                    .iter()
                    .map(|entry| &entry.path)
                    .collect::<Vec<_>>(),
                [&keep]
            );
        });
}

#[test]
fn undo_navigation_keeps_other_foreground_operations_blocking() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path());
    let undo = app.run_journal(false);
    let blocking = app.operations.begin_foreground();
    press(&mut app, "G");
    assert_eq!(app.grid.selected_entry(), Some(0));
    drop(blocking);
    press(&mut app, "G");
    assert_eq!(app.grid.selected_entry(), Some(1));
    press(&mut app, "!");
    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    press(&mut app, ":");
    assert_eq!(app.browser_input.mode(), InputMode::Browser);
    drop(undo);
    assert!(!app.foreground_operation_active());
    assert!(app.operation_access().history);
}

#[test]
fn undo_navigation_allows_mouse_marquee_selection() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path());
    app.grid.resize(iced::Size::new(820.0, 560.0));
    let undo = app.run_journal(false);
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::CursorMoved {
            position: iced::Point::new(700.0, 300.0),
        }),
        event::Status::Ignored,
    );
    let _ = app.handle_event(
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        event::Status::Ignored,
    );
    assert!(app.grid.marquee_bounds(app.status_height()).is_some());
    drop(undo);
}

#[test]
fn undo_navigation_completion_keeps_the_newer_folder() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let before = temp.path().join("before");
            let renamed = temp.path().join("renamed");
            let other = temp.path().join("other");
            let keep = other.join("keep");
            std_fs::write(&renamed, b"undo rename").unwrap();
            std_fs::create_dir(&other).unwrap();
            std_fs::write(&keep, b"selected in new folder").unwrap();
            let mut app = app_at(temp.path());
            app.journal
                .record(crate::journal::Action::rename(before.clone(), renamed).unwrap())
                .unwrap();
            let undo = app.run_journal(false);
            let navigation = app.transition_navigation(NavigationTransition::Open {
                requested: other.clone(),
                remember: true,
                select: None,
            });
            super::navigation::finish_tasks(&mut app, navigation).await;
            app.grid.select_only(Some(0), 1);
            super::navigation::finish_tasks(&mut app, undo).await;
            assert!(before.exists());
            assert_eq!(app.navigation.current(), other);
            assert_eq!(
                app.selected_entries()
                    .iter()
                    .map(|entry| &entry.path)
                    .collect::<Vec<_>>(),
                [&keep]
            );
        });
}
