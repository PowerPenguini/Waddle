use super::*;

#[test]
fn sidebar_returns_from_recent_and_trash_to_the_previous_folder() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for trash in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let folder = temp.path().join("folder");
            std_fs::create_dir(&folder).unwrap();
            std_fs::write(folder.join("visible.txt"), "content").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(folder.clone());
            app.view_preferences =
                super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
            app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
                id: "fixture".into(),
                path: Some(folder.clone()),
                label: "Fixture".into(),
                can_unmount: false,
            }]);
            let message = if trash {
                Message::TrashLoaded {
                    request: app.navigation.trash().request.unwrap(),
                    result: Some(Ok(Vec::new())),
                }
            } else {
                Message::RecentLoaded {
                    request: app.navigation.recent().request.unwrap(),
                    result: Some(Ok(Vec::new())),
                }
            };
            let _ = app.update(message);
            assert!(!app.navigation.folder_displayed());
            let row = app
                .sidebar_tree
                .rows(&folder)
                .into_iter()
                .find(|row| row.label == "Fixture")
                .unwrap();
            let task = app.activate_tree_row(row.id);
            tokio::time::timeout(Duration::from_secs(5), finish_tasks(&mut app, task))
                .await
                .unwrap();
            assert!(
                app.navigation.folder_displayed(),
                "clicking the previous folder must leave Recent/Trash"
            );
            assert_eq!(app.navigation.current(), folder);
            assert_eq!(app.navigation.entries().len(), 1);
            assert_eq!(app.navigation.entries()[0].name, "visible.txt");
        }
    });
}

pub(super) async fn finish_tasks(app: &mut App, task: Task<Message>) {
    use iced::futures::StreamExt;
    let mut pending = std::collections::VecDeque::from([task]);
    while let Some(task) = pending.pop_front() {
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    pending.push_back(app.update(message));
                }
            }
        }
    }
}

#[test]
fn sidebar_current_folder_supersedes_a_pending_navigation() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("current");
        let other = temp.path().join("other");
        std_fs::create_dir(&current).unwrap();
        std_fs::create_dir(&other).unwrap();
        std_fs::write(current.join("stay.txt"), "content").unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(current.clone());
        app.view_preferences =
            super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
        app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
            id: "fixture".into(),
            path: Some(current.clone()),
            label: "Fixture".into(),
            can_unmount: false,
        }]);
        let old_task = app.transition_navigation(NavigationTransition::Open {
            requested: other.clone(),
            remember: true,
            select: None,
        });
        let old_request = app.navigation.pending_request().unwrap();
        let row = app
            .sidebar_tree
            .rows(&current)
            .into_iter()
            .find(|row| row.label == "Fixture")
            .unwrap();
        let task = app.activate_tree_row(row.id);
        tokio::time::timeout(Duration::from_secs(5), finish_tasks(&mut app, task))
            .await
            .unwrap();
        // The old worker may have finished before cancellation reached it.
        let stale = app.update(Message::NavigationFinished {
            request: old_request,
            result: Ok(opened(other, Vec::new())),
        });
        tokio::time::timeout(Duration::from_secs(5), finish_tasks(&mut app, stale))
            .await
            .unwrap();
        assert_eq!(
            app.navigation.current(),
            current,
            "an older folder request must not override the latest Sidebar choice"
        );
        assert_eq!(app.navigation.entries()[0].name, "stay.txt");
        assert!(!app.navigation.loading());
        assert!(!app.navigation.can_go_back());
        drop(old_task);
    });
}

#[test]
fn hunt_refresh_during_a_folder_scan_does_not_lose_new_entries() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for external_notification in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let folder = temp.path().join("folder");
            std_fs::create_dir(&folder).unwrap();
            std_fs::write(folder.join("before.txt"), "before").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(folder.clone());
            app.view_preferences =
                super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
            app.navigation.settle_for_test();
            app.sync_location_monitoring();
            let request = app.navigation.refresh(None).request.unwrap();
            let scanned = fs::open_directory_revealing(
                &folder,
                app.view_preferences.for_directory(&folder),
                &[],
            )
            .unwrap();
            std_fs::write(folder.join("after.txt"), "after").unwrap();
            let requested_refresh = app.update(if external_notification {
                Message::DirectoryChanged(super::directory_watch::Event {
                    path: folder.clone(),
                    removed: Vec::new(),
                    watch_failed: false,
                })
            } else {
                Message::Refresh
            });
            let completion = app.update(Message::NavigationFinished {
                request,
                result: Ok(scanned),
            });
            tokio::time::timeout(
                Duration::from_secs(5),
                finish_tasks(&mut app, Task::batch([requested_refresh, completion])),
            )
            .await
            .expect("folder refresh tasks should settle");
            let names = app
                .navigation
                .entries()
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                names,
                ["after.txt", "before.txt"],
                "Refresh must rescan when the pending result predates a change"
            );
            assert!(!app.navigation.loading());
        }
    });
}

#[test]
fn sort_change_during_a_folder_scan_reaches_the_displayed_entries() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        std_fs::write(temp.path().join("a-large.txt"), "large contents").unwrap();
        std_fs::write(temp.path().join("z-small.txt"), "x").unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.view_preferences =
            super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
        app.navigation.settle_for_test();
        let request = app.navigation.refresh(None).request.unwrap();
        let scanned = fs::open_directory_revealing(
            temp.path(),
            app.view_preferences.for_directory(temp.path()),
            &[],
        )
        .unwrap();
        let sort = app.update(Message::SortBy(fs::SortKey::Size));
        let completion = app.update(Message::NavigationFinished {
            request,
            result: Ok(scanned),
        });
        tokio::time::timeout(
            Duration::from_secs(5),
            finish_tasks(&mut app, Task::batch([sort, completion])),
        )
        .await
        .expect("sort refresh should settle");
        let names = app
            .navigation
            .entries()
            .iter()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["z-small.txt", "a-large.txt"]);
        assert!(!app.navigation.loading());
    });
}

#[test]
fn deferred_refresh_cannot_undo_navigation_or_cancellation() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for cancel in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("original");
            let next = temp.path().join("next");
            std_fs::create_dir(&original).unwrap();
            std_fs::create_dir(&next).unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(original.clone());
            app.navigation.settle_for_test();
            let request = app
                .navigation
                .transition(NavigationTransition::Open {
                    requested: next.clone(),
                    remember: true,
                    select: None,
                })
                .request
                .unwrap();
            let refresh = app.update(Message::Refresh);
            if cancel {
                assert!(app.cancel_pending_navigation());
            }
            let completion = app.update(Message::NavigationFinished {
                request,
                result: Ok(opened(next.clone(), Vec::new())),
            });
            tokio::time::timeout(
                Duration::from_secs(5),
                finish_tasks(&mut app, Task::batch([refresh, completion])),
            )
            .await
            .expect("navigation should settle");
            assert_eq!(
                app.navigation.current(),
                if cancel { &original } else { &next }
            );
            assert!(!app.navigation.loading());
        }
    });
}

#[test]
fn undo_is_not_ignored_while_the_current_folder_refreshes() {
    let temp = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(temp.path().to_path_buf());
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry(".Trash-1000")]);
    let refresh = app.refresh(None);
    app.refresh_status();

    assert!(app.navigation.loading());
    assert_eq!(
        app.presentation.status(),
        format!("1 items  •  {}", temp.path().display())
    );

    let key = keyboard::Key::Character("u".into());
    let undo = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("u"));

    assert!(app.foreground_operation_active(), "u was silently ignored");
    assert_eq!(app.presentation.status(), "Undoing…");
    drop(undo);
    drop(refresh);
}

#[test]
fn mounted_tree_volume_waits_for_its_path_then_opens_the_root() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
        id: "uuid:test".to_owned(),
        path: None,
        label: "USB Stick".to_owned(),
        can_unmount: false,
    }]);
    let volume_row = app
        .sidebar_tree
        .rows(Path::new("/current"))
        .into_iter()
        .find(|row| row.label == "USB Stick")
        .unwrap();
    assert!(matches!(
        app.sidebar_tree.activate(volume_row.id),
        Some(TreeActivation::MountVolume { .. })
    ));
    let mounted_path = PathBuf::from("/run/media/user/USB Stick");

    let _ = app.finish_tree_volume_mount(
        "uuid:test",
        Ok(places::MountedVolume {
            label: "USB Stick".to_owned(),
        }),
    );
    assert!(app.pending_volume_navigation.is_some());
    assert!(app.navigation.pending_path().is_none());

    assert!(app.sidebar_tree.reconcile_volumes(vec![VolumeRoot {
        id: "uuid:test".to_owned(),
        path: Some(mounted_path.clone()),
        label: "USB Stick".to_owned(),
        can_unmount: true,
    }]));
    let _ = app.resume_tree_volume_navigation();

    assert_eq!(app.navigation.pending_path(), Some(mounted_path.as_path()));
    assert!(app.pending_volume_navigation.is_none());
    assert_eq!(
        app.sidebar_tree
            .rows(Path::new("/current"))
            .into_iter()
            .find(|row| row.id == volume_row.id)
            .unwrap()
            .path,
        Some(mounted_path)
    );
    assert!(app.presentation.status().starts_with("Opening "));
}

#[test]
fn successful_unmount_notice_is_neutral_when_leaving_the_volume() {
    let mounted_path = PathBuf::from("/run/media/user/tmp");
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(mounted_path.join("folder"));
    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
        id: "uuid:tmp".to_owned(),
        path: Some(mounted_path.clone()),
        label: "tmp".to_owned(),
        can_unmount: true,
    }]);

    let task = app.finish_tree_volume_unmount("uuid:tmp", "tmp", &mounted_path, Ok(()));

    assert_eq!(app.presentation.notice(), Some("Unmounted tmp"));
    assert!(!app.presentation.notice_is_danger());
    drop(task);
}

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
        .request
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
fn failed_folder_navigation_opens_the_error_bar() {
    let (mut app, _) = App::new();
    app.navigation = NavigationSession::new(PathBuf::from("/current"));
    let request = app
        .navigation
        .transition(NavigationTransition::Open {
            requested: PathBuf::from("/lost+found"),
            remember: true,
            select: None,
        })
        .request
        .unwrap();
    let error = "Could not read /lost+found: Permission denied (os error 13)";

    let _ = app.finish_navigation(request, NavigationCompletion::Folder(Err(error.to_owned())));

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Error { message } if message == error
    ));
    assert!(app.presentation.expansion().0);
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

    let update = app
        .transfers
        .handle_native(TransferEvent::ClipboardOwnershipLost, |_, _| None);
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
    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
        id: "uuid:data".to_owned(),
        path: Some(PathBuf::from("/data")),
        label: "Data".to_owned(),
        can_unmount: true,
    }]);
    let drive = app
        .sidebar_tree
        .rows(Path::new("/current"))
        .into_iter()
        .find(|row| row.kind == NodeKind::Drive)
        .unwrap();
    let TreeActivation::Folder {
        load: Some(request),
        ..
    } = app.sidebar_tree.activate(drive.id).unwrap()
    else {
        panic!("an unopened drive should request its children");
    };
    assert_eq!(
        app.sidebar_tree
            .complete_load(&request, Ok(vec![PathBuf::from("/data/slow")])),
        TreeLoadOutcome::Installed
    );
    let slow = app
        .sidebar_tree
        .rows(Path::new("/current"))
        .into_iter()
        .find(|row| row.path.as_deref() == Some(Path::new("/data/slow")))
        .unwrap();

    let _ = app.activate_tree_row(slow.id);
    assert_eq!(app.navigation.pending_path(), Some(Path::new("/data/slow")));
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
    let request = app.navigation.recent().request.unwrap();
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
    let request = app.navigation.trash().request.unwrap();
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
fn volume_reconciliation_preserves_existing_nodes() {
    let first = VolumeRoot {
        id: "uuid:first".to_owned(),
        path: Some(PathBuf::from("/media/first")),
        label: "First".to_owned(),
        can_unmount: true,
    };
    let mut tree = SidebarTree::new(vec![first.clone()]);
    let original_id = tree
        .rows(Path::new("/"))
        .into_iter()
        .find(|row| row.path == first.path)
        .unwrap()
        .id;
    tree.activate(original_id);

    let second = VolumeRoot {
        id: "uuid:second".to_owned(),
        path: Some(PathBuf::from("/media/second")),
        label: "Second".to_owned(),
        can_unmount: true,
    };
    assert!(tree.reconcile_volumes(vec![first, second]));
    let rows = tree.rows(Path::new("/"));
    assert_eq!(rows[1].id, original_id);
    assert!(tree.is_expanded(original_id));
    assert_eq!(rows[2].label, "Second");
}
