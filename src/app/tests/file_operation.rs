use super::*;

#[test]
fn queued_restore_completion_does_not_reopen_trash_after_back_navigation() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let folder = temp.path().join("folder");
            let root = temp.path().join("Trash");
            std_fs::create_dir(&folder).unwrap();
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let original = folder.join("restored.txt");
            let trashed = root.join("files/restored.txt");
            let info = root.join("info/restored.txt.trashinfo");
            std_fs::write(&trashed, "restored contents").unwrap();
            std_fs::write(
                &info,
                format!("[Trash Info]\nPath={}\n", original.display()),
            )
            .unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(folder.clone());
            app.navigation.settle_for_test();
            app.trash = trash::Trash::at(root);
            app.navigation
                .install_trash_entries(app.trash.entries().unwrap());
            app.grid.select_only(Some(0), 1);
            let mut stream =
                iced_runtime::task::into_stream(app.update(Message::ContextRestore)).unwrap();
            let mut queued = Vec::new();
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    queued.push(message);
                }
            }
            assert_eq!(queued.len(), 1);
            assert_eq!(
                std_fs::read_to_string(&original).unwrap(),
                "restored contents"
            );
            assert!(!trashed.exists());

            let back = app.update(Message::Back);
            navigation::finish_tasks(&mut app, back).await;
            assert!(app.navigation.folder_displayed());
            for message in queued {
                let completed = app.update(message);
                navigation::finish_tasks(&mut app, completed).await;
            }
            assert!(
                app.navigation.folder_displayed(),
                "The queued Restore completion reopened Trash after Back"
            );
            assert_eq!(app.navigation.current(), folder);
            assert_eq!(app.navigation.entries().len(), 1);
            assert_eq!(app.navigation.entries()[0].path, original);
            assert!(!info.exists());
        });
}

#[test]
fn trash_deletion_cleans_metadata_when_the_item_disappears_before_confirmation() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("Trash");
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let trashed = root.join("files/discarded.txt");
            let info = root.join("info/discarded.txt.trashinfo");
            std_fs::write(&trashed, "discarded contents").unwrap();
            std_fs::write(&info, "[Trash Info]\nPath=/original/discarded.txt\n").unwrap();
            let (mut app, _) = App::new();
            app.navigation.settle_for_test();
            app.trash = trash::Trash::at(root);
            app.navigation
                .install_trash_entries(app.trash.entries().unwrap());
            app.grid.select_only(Some(0), 1);
            let _ = app.update(Message::ContextDeletePermanent);
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::PermanentDelete { .. }
            ));

            std_fs::remove_file(&trashed).unwrap();
            let confirmed = app.update(Message::PromptConfirm);
            navigation::finish_tasks(&mut app, confirmed).await;
            assert!(!trashed.exists());
            assert!(
                !info.exists(),
                "Deletion left orphaned metadata for the absent item"
            );
            assert_eq!(
                app.browser_status_model().text,
                "Permanently deleted 1  •  0 failed",
                "Already absent Trash item was reported as a deletion failure"
            );
            assert!(
                app.command.output().is_none(),
                "Successful deletion opened an error report"
            );
            assert!(app.navigation.entries().is_empty());
        });
}

#[test]
fn trash_deletion_succeeds_when_metadata_disappears_after_confirmation_opens() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("Trash");
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let trashed = root.join("files/discarded.txt");
            let info = root.join("info/discarded.txt.trashinfo");
            std_fs::write(&trashed, "discarded contents").unwrap();
            std_fs::write(&info, "[Trash Info]\nPath=/original/discarded.txt\n").unwrap();
            let (mut app, _) = App::new();
            app.navigation.settle_for_test();
            app.trash = trash::Trash::at(root);
            app.navigation
                .install_trash_entries(app.trash.entries().unwrap());
            app.grid.select_only(Some(0), 1);
            let _ = app.update(Message::ContextDeletePermanent);
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::PermanentDelete { .. }
            ));

            std_fs::remove_file(&info).unwrap();
            let confirmed = app.update(Message::PromptConfirm);
            navigation::finish_tasks(&mut app, confirmed).await;
            assert!(!trashed.exists());
            assert!(!info.exists());
            assert_eq!(
                app.browser_status_model().text,
                "Permanently deleted 1  •  0 failed",
                "Already absent metadata was reported as a deletion failure"
            );
            assert!(
                app.command.output().is_none(),
                "Successful deletion opened an error report"
            );
            assert!(app.navigation.entries().is_empty());
        });
}

#[test]
fn empty_trash_confirmation_survives_the_resulting_refresh() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("Trash");
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let trashed = root.join("files/discarded.txt");
            let info = root.join("info/discarded.txt.trashinfo");
            std_fs::write(&trashed, "discarded contents").unwrap();
            std_fs::write(&info, "[Trash Info]\nPath=/original/discarded.txt\n").unwrap();
            let (mut app, _) = App::new();
            app.navigation.settle_for_test();
            app.trash = trash::Trash::at(root);
            app.navigation
                .install_trash_entries(app.trash.entries().unwrap());
            app.sync_location_monitoring();

            let _ = app.update(Message::ContextEmptyTrash);
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::PermanentDelete { .. }
            ));
            let confirmed = app.update(Message::PromptConfirm);
            navigation::finish_tasks(&mut app, confirmed).await;
            assert!(!trashed.exists());
            assert!(!info.exists());
            assert_eq!(
                app.navigation.displayed_location(),
                DisplayedLocation::Trash
            );
            assert!(app.navigation.entries().is_empty());
            assert_eq!(
                app.browser_status_model().text,
                "Permanently deleted 1  •  0 failed",
                "The automatic Trash refresh hid the deletion result"
            );

            let clicked = app.update(Message::Event(
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                event::Status::Captured,
            ));
            navigation::finish_tasks(&mut app, clicked).await;
            assert!(
                !app.browser_status_model()
                    .text
                    .contains("Permanently deleted")
            );
        });
}

#[test]
fn properties_failure_survives_an_automatic_directory_refresh() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let missing = temp.path().join("gone.txt");
            std_fs::write(&missing, "removed before inspection").unwrap();
            std_fs::write(temp.path().join("survivor.txt"), "still present").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.sync_location_monitoring();
            app.grid.select_only(Some(1), 2);
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged("properties gone.txt".into()));
            let properties = app.update(Message::CommandSubmitted);
            std_fs::remove_file(&missing).unwrap();
            navigation::finish_tasks(&mut app, properties).await;
            assert!(
                app.browser_status_model()
                    .text
                    .contains("Could not inspect")
            );

            // The native directory notification arrives after the explicit failure.
            let refresh = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: temp.path().to_path_buf(),
                removed: vec![missing],
                watch_failed: false,
            }));
            navigation::finish_tasks(&mut app, refresh).await;
            assert_eq!(app.navigation.entries().len(), 1);
            assert_eq!(app.navigation.entries()[0].name, "survivor.txt");
            let status = app.browser_status_model();
            assert!(
                status.text.contains("Could not inspect") && status.text.contains("gone.txt"),
                "automatic refresh hid the Properties failure: {}",
                status.text
            );
            assert!(app.presentation.notice_is_danger());

            let task = app.update(Message::Event(
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                event::Status::Captured,
            ));
            navigation::finish_tasks(&mut app, task).await;
            assert!(app.browser_status_model().text.contains("survivor.txt"));
        });
}

#[test]
fn permission_success_feedback_survives_details_refresh_until_next_input() {
    use std::os::unix::fs::PermissionsExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            for name in ["first.txt", "second.txt"] {
                let path = temp.path().join(name);
                std_fs::write(&path, name).unwrap();
                std_fs::set_permissions(path, std_fs::Permissions::from_mode(0o600)).unwrap();
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.grid.select_only(Some(0), 2);
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(
                "chmod 640 first.txt second.txt".into(),
            ));
            let task = app.update(Message::CommandSubmitted);
            navigation::finish_tasks(&mut app, task).await;

            assert_eq!(
                app.browser_status_model().text,
                "Changed permissions on 2 items to 0640"
            );
            assert!(!app.presentation.notice_is_danger());
            for name in ["first.txt", "second.txt"] {
                assert_eq!(
                    std_fs::metadata(temp.path().join(name))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o7777,
                    0o640
                );
            }
            let task = app.update(Message::Event(
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                event::Status::Captured,
            ));
            navigation::finish_tasks(&mut app, task).await;
            assert!(
                app.browser_status_model().text.contains("rw-r-----"),
                "new input should reveal the refreshed permission details: {}",
                app.browser_status_model().text
            );
        });
}

#[test]
fn queued_permission_results_preserve_newer_command_output() {
    use iced::futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for partial_failure in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let file = temp.path().join("changed.txt");
                std_fs::write(&file, "preserve contents").unwrap();
                std_fs::set_permissions(&file, std_fs::Permissions::from_mode(0o600)).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.grid.select_only(Some(0), 1);
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged(if partial_failure {
                    "chmod 644 changed.txt missing.txt".into()
                } else {
                    "chmod 644 changed.txt".into()
                }));
                let mut stream =
                    iced_runtime::task::into_stream(app.update(Message::CommandSubmitted)).unwrap();
                let mut queued = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        queued.push(message);
                    }
                }
                assert_eq!(queued.len(), 1);
                assert_eq!(
                    std_fs::metadata(&file).unwrap().permissions().mode() & 0o7777,
                    0o644
                );

                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged("help".into()));
                let task = app.update(Message::CommandSubmitted);
                navigation::finish_tasks(&mut app, task).await;
                let help = app.command.output().cloned().expect("newer help output");
                let task = app.update(queued.pop().unwrap());
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(
                    app.command.output(),
                    Some(&help),
                    "old permission result replaced help"
                );
                assert_eq!(std_fs::read_to_string(&file).unwrap(), "preserve contents");
            }
        });
}

#[test]
fn queued_rename_results_do_not_replace_a_newer_rename_editor() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for first_succeeds in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                for name in ["first.txt", "second.txt"] {
                    std_fs::write(temp.path().join(name), name).unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.grid.select_only(Some(0), 2);
                press(&mut app, "r");
                let _ = app.update(Message::RenameChanged(if first_succeeds {
                    "first-renamed.txt".into()
                } else {
                    "second.txt".into()
                }));
                let mut stream =
                    iced_runtime::task::into_stream(app.update(Message::RenameSubmitted)).unwrap();
                let mut queued = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        queued.push(message);
                    }
                }
                assert_eq!(queued.len(), 1);
                assert_eq!(
                    temp.path().join("first-renamed.txt").exists(),
                    first_succeeds
                );

                let _ = app.update(Message::EntryContext(1));
                let _ = app.update(Message::ContextRename);
                let _ = app.update(Message::RenameChanged("second-renamed.txt".into()));
                let task = app.update(queued.pop().unwrap());
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(
                    app.browser_input.mode(),
                    InputMode::Rename,
                    "old completion closed the newer editor"
                );
                assert!(
                    matches!(app.file_operations.view(), FileOperationView::Rename { value, error }
                    if value == "second-renamed.txt" && error.is_empty()),
                    "old completion changed the newer Rename input or error"
                );

                let task = app.update(Message::RenameSubmitted);
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(
                    std_fs::read_to_string(temp.path().join("second-renamed.txt")).unwrap(),
                    "second.txt"
                );
                app.journal.undo().unwrap();
                assert_eq!(
                    std_fs::read_to_string(temp.path().join("second.txt")).unwrap(),
                    "second.txt"
                );
                if first_succeeds {
                    app.journal
                        .undo()
                        .expect("the old completed Rename must also retain Undo");
                }
                assert_eq!(
                    std_fs::read_to_string(temp.path().join("first.txt")).unwrap(),
                    "first.txt"
                );
            }
        });
}

#[test]
fn queued_rename_completion_does_not_cancel_a_newer_folder_navigation() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("original");
            let next = temp.path().join("next");
            std_fs::create_dir(&original).unwrap();
            std_fs::create_dir(&next).unwrap();
            std_fs::write(original.join("before.txt"), "rename contents").unwrap();
            std_fs::write(next.join("destination.txt"), "new location").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(original.clone());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(&original).unwrap());
            app.grid.select_only(Some(0), 1);
            press(&mut app, "r");
            let _ = app.update(Message::RenameChanged("after.txt".to_owned()));
            let mut stream = iced_runtime::task::into_stream(app.update(Message::RenameSubmitted))
                .expect("Rename should start");
            let mut queued = Vec::new();
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    queued.push(message);
                }
            }
            assert_eq!(queued.len(), 1);
            assert_eq!(
                std_fs::read_to_string(original.join("after.txt")).unwrap(),
                "rename contents"
            );

            let _ = app.update(Message::LocationChanged(next.display().to_string()));
            let navigation = app.update(Message::LocationSubmitted);
            assert!(app.navigation.loading());
            let completion = app.update(queued.pop().unwrap());
            navigation::finish_tasks(&mut app, Task::batch([navigation, completion])).await;
            assert_eq!(
                app.navigation.current(),
                next,
                "Rename cancelled the newer navigation"
            );
            assert!(
                app.navigation
                    .entries()
                    .iter()
                    .any(|entry| entry.path == next.join("destination.txt"))
            );
            assert!(!app.navigation.loading());
            app.journal
                .undo()
                .expect("the completed Rename must retain Undo");
            assert_eq!(
                std_fs::read_to_string(original.join("before.txt")).unwrap(),
                "rename contents"
            );
        });
}

#[test]
fn queued_undo_completion_does_not_cancel_a_newer_folder_navigation() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("original");
            let next = temp.path().join("next");
            std_fs::create_dir(&original).unwrap();
            std_fs::create_dir(&next).unwrap();
            let file = original.join("undo.txt");
            std_fs::write(&file, "").unwrap();
            std_fs::write(next.join("destination.txt"), "new location").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(original.clone());
            app.navigation.settle_for_test();
            app.journal
                .record(journal::Action::new_file(file.clone()).unwrap())
                .unwrap();

            let key = keyboard::Key::Character("u".into());
            let task = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("u"));
            let mut stream = iced_runtime::task::into_stream(task).unwrap();
            let mut queued = Vec::new();
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    queued.push(message);
                }
            }
            assert_eq!(queued.len(), 1);
            assert!(!file.exists());
            let _ = app.update(Message::LocationChanged(next.display().to_string()));
            let navigation = app.update(Message::LocationSubmitted);
            assert!(app.navigation.loading());
            assert_eq!(app.navigation.current(), original);

            let completion = app.update(queued.pop().unwrap());
            navigation::finish_tasks(&mut app, Task::batch([navigation, completion])).await;
            assert_eq!(
                app.navigation.current(),
                next,
                "Undo cancelled the newer navigation"
            );
            assert!(
                app.navigation
                    .entries()
                    .iter()
                    .any(|entry| entry.path == next.join("destination.txt"))
            );
            assert!(!app.navigation.loading());
        });
}

#[test]
fn queued_undo_completion_preserves_a_newer_recent_or_trash_location() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for location in [DisplayedLocation::Recent, DisplayedLocation::Trash] {
                let temp = tempfile::tempdir().unwrap();
                let file = temp.path().join("undo.txt");
                std_fs::write(&file, "").unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.journal
                    .record(journal::Action::new_file(file.clone()).unwrap())
                    .unwrap();

                let key = keyboard::Key::Character("u".into());
                let task =
                    app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("u"));
                let mut stream = iced_runtime::task::into_stream(task).unwrap();
                let mut queued = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        queued.push(message);
                    }
                }
                assert_eq!(queued.len(), 1);
                assert!(!file.exists());
                assert!(!app.foreground_operation_active());

                // Deliver the new location's filesystem response before Undo's result.
                let loaded = match location {
                    DisplayedLocation::Recent => Message::RecentLoaded {
                        request: app.navigation.recent().request.unwrap(),
                        result: Some(Ok(Vec::new())),
                    },
                    DisplayedLocation::Trash => Message::TrashLoaded {
                        request: app.navigation.trash().request.unwrap(),
                        result: Some(Ok(Vec::new())),
                    },
                    DisplayedLocation::Folder => unreachable!(),
                };
                let _ = app.update(loaded);
                let refresh = app.update(queued.pop().unwrap());
                let request = app
                    .navigation
                    .pending_request()
                    .expect("refresh after Undo");
                assert_eq!(
                    request.location(),
                    location,
                    "Undo requested the old folder"
                );
                let loaded = match location {
                    DisplayedLocation::Recent => Message::RecentLoaded {
                        request,
                        result: Some(Ok(Vec::new())),
                    },
                    DisplayedLocation::Trash => Message::TrashLoaded {
                        request,
                        result: Some(Ok(Vec::new())),
                    },
                    DisplayedLocation::Folder => unreachable!(),
                };
                let _ = app.update(loaded);
                drop(refresh);
                assert_eq!(app.navigation.displayed_location(), location);
            }
        });
}

#[test]
fn partial_undo_refreshes_removed_entries_and_retains_the_failure() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let blocked = temp.path().join("blocked");
            let displayed = temp.path().join("displayed");
            let mut receipts = Vec::new();
            for (index, directory) in [&blocked, &displayed].into_iter().enumerate() {
                std_fs::create_dir(directory).unwrap();
                let source = temp.path().join(format!("source-{index}.txt"));
                let destination = directory.join("copy.txt");
                std_fs::write(&source, "original contents").unwrap();
                std_fs::copy(&source, &destination).unwrap();
                receipts.push(fs::TransferReceipt {
                    source,
                    destination,
                    replaced_existing: false,
                });
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(displayed.clone());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(&displayed).unwrap());
            app.journal
                .record(
                    journal::Action::transfer(journal::TransferKind::Copy, &receipts)
                        .unwrap()
                        .unwrap(),
                )
                .unwrap();

            // Undo removes the last copy first, then cannot remove the other one.
            std_fs::set_permissions(&blocked, std_fs::Permissions::from_mode(0o500)).unwrap();
            let key = keyboard::Key::Character("u".into());
            let task = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("u"));
            navigation::finish_tasks(&mut app, task).await;
            std_fs::set_permissions(&blocked, std_fs::Permissions::from_mode(0o700)).unwrap();

            assert!(
                !displayed.join("copy.txt").exists(),
                "Undo must have partial effects"
            );
            assert_eq!(
                std_fs::read_to_string(blocked.join("copy.txt")).unwrap(),
                "original contents"
            );
            let status = app.presentation.browser_status(None, false, false);
            assert!(status.text.contains("Permission denied"), "{}", status.text);
            assert!(
                app.navigation.entries().is_empty(),
                "the browser still displays the file removed by a partially failed Undo"
            );
        });
}

#[test]
#[ignore = "release-mode performance benchmark"]
fn benchmark_large_trash_selection_opens_delete_confirmation_promptly() {
    const COUNT: usize = 10_000;
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.install_trash_entries(
        (0..COUNT)
            .map(|index| {
                let name = format!("item-{index:05}.txt");
                crate::app::trash::Entry {
                    file: entry(&name),
                    receipt: crate::journal::TrashReceipt {
                        original: PathBuf::from("/original").join(&name),
                        trashed: PathBuf::from("/start").join(&name),
                        info: PathBuf::from("/info").join(format!("{name}.trashinfo")),
                    },
                }
            })
            .collect(),
    );
    let select_all = keyboard::Key::Character("a".into());
    let _ = app.handle_key(
        select_all.clone(),
        select_all,
        keyboard::Modifiers::CTRL,
        Some("a"),
    );
    app.modifiers = keyboard::Modifiers::CTRL;
    let _ = app.update(Message::EntryPressed(COUNT - 1));
    let _ = app.update(Message::EntryReleased(COUNT - 1));
    assert_eq!(app.grid.selection_count(), COUNT - 1);

    let delete = keyboard::Key::Named(keyboard::key::Named::Delete);
    let started = std::time::Instant::now();
    let _ = app.handle_key(delete.clone(), delete, keyboard::Modifiers::empty(), None);
    let elapsed = started.elapsed();
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::PermanentDelete { message, .. }
            if message == "Permanently delete 9999 selected Trash items?"
    ));
    eprintln!(
        "Trash Delete confirmation for {} selected items: {elapsed:?}",
        COUNT - 1
    );
    assert!(
        elapsed < Duration::from_millis(250),
        "opening Delete confirmation blocked the UI for {elapsed:?}"
    );
}

#[test]
fn context_rename_keeps_its_file_target_across_refresh() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for remove_target in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                for name in ["bravo.txt", "delta.txt", "omega.txt"] {
                    std_fs::write(temp.path().join(name), name).unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                let _ = app.update(Message::EntryContext(1));
                if remove_target {
                    std_fs::remove_file(temp.path().join("delta.txt")).unwrap();
                } else {
                    std_fs::write(temp.path().join("alpha.txt"), "inserted").unwrap();
                }
                let task = app.update(Message::Refresh);
                navigation::finish_tasks(&mut app, task).await;
                let _ = app.update(Message::ContextRename);
                if remove_target {
                    assert!(matches!(
                        app.file_operations.view(),
                        FileOperationView::Idle
                    ));
                } else {
                    let _ = app.update(Message::RenameChanged("renamed.txt".into()));
                    let task = app.update(Message::RenameSubmitted);
                    navigation::finish_tasks(&mut app, task).await;
                    assert_eq!(
                        std_fs::read_to_string(temp.path().join("renamed.txt")).unwrap(),
                        "delta.txt"
                    );
                    assert!(!temp.path().join("delta.txt").exists());
                }
                for name in ["bravo.txt", "omega.txt"] {
                    assert_eq!(
                        std_fs::read_to_string(temp.path().join(name)).unwrap(),
                        name
                    );
                }
            }
        });
}

#[test]
fn selecting_another_file_does_not_cancel_an_explicit_properties_request() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            for name in ["requested.txt", "selected.txt"] {
                std_fs::write(temp.path().join(name), "fixture").unwrap();
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged("properties requested.txt".into()));
            let properties = app.update(Message::CommandSubmitted);

            // A pointer selection queues status details while Properties is pending.
            app.modifiers = keyboard::Modifiers::CTRL;
            let _ = app.update(Message::EntryPressed(1));
            let details = app.update(Message::EntryReleased(1));
            navigation::finish_tasks(&mut app, details).await;
            navigation::finish_tasks(&mut app, properties).await;

            assert_eq!(app.grid.selected_entry(), Some(1));
            assert!(
                app.command
                    .output()
                    .is_some_and(|output| { output.summary == "Properties  •  requested.txt" }),
                "Properties disappeared after selecting another file: {}",
                app.presentation.status()
            );
        });
}

#[test]
fn queued_properties_results_cannot_replace_newer_command_output() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for old_exists in [true, false] {
                for newer_command in ["help", "properties current.txt"] {
                    let temp = tempfile::tempdir().unwrap();
                    if old_exists {
                        std_fs::write(temp.path().join("old.txt"), "old").unwrap();
                    }
                    std_fs::write(temp.path().join("current.txt"), "current").unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    press(&mut app, ":");
                    let _ = app.update(Message::CommandChanged("properties old.txt".into()));
                    let task = app.update(Message::CommandSubmitted);
                    let mut stream = iced_runtime::task::into_stream(task).unwrap();
                    let mut queued = Vec::new();
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(message) = action {
                            queued.push(message);
                        }
                    }
                    assert_eq!(queued.len(), 1);

                    press(&mut app, ":");
                    let _ = app.update(Message::CommandChanged(newer_command.into()));
                    let task = app.update(Message::CommandSubmitted);
                    navigation::finish_tasks(&mut app, task).await;
                    let output = app.command.output().cloned().unwrap();
                    assert!(output.summary.contains(if newer_command == "help" {
                        ":help"
                    } else {
                        "current.txt"
                    }));
                    let status = app.presentation.status().to_owned();

                    for message in queued {
                        let task = app.update(message);
                        navigation::finish_tasks(&mut app, task).await;
                    }
                    assert_eq!(app.command.output(), Some(&output));
                    assert_eq!(app.presentation.status(), status);
                }
            }
        });
}

#[test]
fn folder_symlinks_use_directory_applications_and_default_associations() {
    use gio::prelude::AppInfoExt;

    const CHILD_ROOT: &str = "WADDLE_FOLDER_SYMLINK_TEST_ROOT";
    const APPLICATION: &str = "waddle-test-folder.desktop";
    let Ok(root) = std::env::var(CHILD_ROOT) else {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let config = temp.path().join("config");
        std_fs::create_dir_all(data.join("applications")).unwrap();
        std_fs::create_dir_all(&config).unwrap();
        std_fs::write(
            data.join("applications").join(APPLICATION),
            "[Desktop Entry]\nType=Application\nName=Waddle Test Folder\nExec=/bin/true %u\nMimeType=inode/directory;\n",
        )
        .unwrap();
        std_fs::write(
            data.join("applications/mimeinfo.cache"),
            format!("[MIME Cache]\ninode/directory={APPLICATION};\n"),
        )
        .unwrap();
        std_fs::write(
            config.join("mimeapps.list"),
            format!("[Added Associations]\ninode/directory={APPLICATION};\n"),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::file_operation::folder_symlinks_use_directory_applications_and_default_associations",
                "--nocapture",
            ])
            .env(CHILD_ROOT, temp.path())
            .env("XDG_DATA_HOME", data)
            .env("XDG_CONFIG_HOME", config)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    };

    let root = PathBuf::from(root);
    let folder = root.join("folder");
    let link = root.join("folder.txt");
    std_fs::create_dir(&folder).unwrap();
    std::os::unix::fs::symlink(&folder, &link).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for target in [&folder, &link] {
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(root.clone());
            app.navigation.settle_for_test();
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(format!(
                "open-with -- {}",
                target.display()
            )));
            let _ = app.update(Message::CommandSubmitted);
            assert!(
                matches!(
                    app.open_with.view(),
                    open_with::View::Open { applications, .. }
                        if applications.iter().any(|application| application.id == APPLICATION)
                ),
                "{} must offer the directory application",
                target.display()
            );

            let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
            let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(format!(
                "default-app {APPLICATION} -- {}",
                target.display()
            )));
            let task = app.update(Message::CommandSubmitted);
            super::navigation::finish_tasks(&mut app, task).await;
            assert_eq!(
                gio::AppInfo::default_for_type("inode/directory", false)
                    .and_then(|application| application.id())
                    .as_deref(),
                Some(APPLICATION)
            );
            assert_ne!(
                gio::AppInfo::default_for_type("text/plain", false)
                    .and_then(|application| application.id())
                    .as_deref(),
                Some(APPLICATION)
            );

            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(format!(
                "properties {}",
                target.display()
            )));
            let task = app.update(Message::CommandSubmitted);
            super::navigation::finish_tasks(&mut app, task).await;
            let output = app.command.output().unwrap();
            assert!(
                output.detail.contains(&format!(
                    "Default application: Waddle Test Folder ({APPLICATION})"
                )),
                "Properties disagrees with the directory association for {}: {}",
                target.display(),
                output.detail
            );
            assert!(output.detail.contains("MIME type: inode/directory"));
            if target == &link {
                assert!(output.detail.contains("Type: Symbolic link"));
                let size = gio::glib::format_size(std_fs::symlink_metadata(target).unwrap().len());
                assert!(output.detail.contains(&format!("Size: {size}\n")));
            }
        }
    });
}

#[test]
fn submitting_an_unchanged_rename_preserves_the_original_filename() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for name in [
            OsString::from_vec(b"name-\xff.txt".to_vec()),
            "plain.txt".into(),
        ] {
            let temp = tempfile::tempdir().unwrap();
            std_fs::write(temp.path().join(&name), "original contents").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.grid.select_only(Some(0), 1);
            press(&mut app, "r");
            let task = app.update(Message::RenameSubmitted);
            super::navigation::finish_tasks(&mut app, task).await;

            assert_eq!(fs::read_directory(temp.path()).unwrap()[0].name, name);
            assert_eq!(app.browser_input.mode(), InputMode::Browser);
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::Idle
            ));
            assert_eq!(
                app.journal.undo().unwrap_err().to_string(),
                "Nothing to undo"
            );
        }
    });
}

#[test]
fn partial_permanent_delete_refreshes_entries_and_keeps_the_error() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        for name in ["deleted.txt", "disappeared.txt"] {
            std_fs::write(temp.path().join(name), "fixture").unwrap();
        }
        let entries = fs::read_directory(temp.path()).unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.navigation.settle_for_test();
        app.navigation.replace_displayed_entries(entries.clone());
        app.file_operations.finish_trash_transfer(
            entries
                .into_iter()
                .map(|entry| (entry, "Trash unavailable".to_owned()))
                .collect(),
        );
        let _ = app.update(Message::Noop);
        // Another process removes one of the entries before confirmation.
        std_fs::remove_file(temp.path().join("disappeared.txt")).unwrap();
        let task = app.update(Message::PromptConfirm);
        tokio::time::timeout(
            Duration::from_secs(5),
            super::navigation::finish_tasks(&mut app, task),
        )
        .await
        .unwrap();

        assert!(
            app.navigation.entries().is_empty(),
            "the browser still displays entries removed during a partial failure"
        );
        assert!(matches!(
            app.file_operations.view(),
            FileOperationView::Error { message } if message.contains("disappeared.txt")
        ));
    });
}

#[test]
fn properties_display_special_permission_bits() {
    use std::os::unix::fs::PermissionsExt;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("permissions.txt");
        std_fs::write(&path, "fixture").unwrap();
        for (mode, symbolic) in [
            (0o4755, "rwsr-xr-x"),
            (0o4644, "rwSr--r--"),
            (0o2755, "rwxr-sr-x"),
            (0o2644, "rw-r-Sr--"),
            (0o1755, "rwxr-xr-t"),
            (0o1644, "rw-r--r-T"),
        ] {
            std_fs::set_permissions(&path, std_fs::Permissions::from_mode(mode)).unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged("properties permissions.txt".into()));
            let task = app.update(Message::CommandSubmitted);
            tokio::time::timeout(
                Duration::from_secs(5),
                super::navigation::finish_tasks(&mut app, task),
            )
            .await
            .unwrap();

            let output = app.command.output().expect("Properties output");
            let expected = format!("Permissions: {symbolic} ({mode:04o})");
            assert!(output.detail.contains(&expected), "{}", output.detail);
        }
    });
}

#[test]
fn trash_keyboard_delete_opens_confirmation_for_selected_items() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .install_trash_entries(vec![crate::app::trash::Entry {
            file: entry("trashed.txt"),
            receipt: crate::journal::TrashReceipt {
                original: PathBuf::from("/original/trashed.txt"),
                trashed: PathBuf::from("/start/trashed.txt"),
                info: PathBuf::from("/info/trashed.txt.trashinfo"),
            },
        }]);
    let _ = app.update(Message::EntryPressed(0));
    let _ = app.update(Message::EntryReleased(0));
    assert_eq!(app.focus.browser(), BrowserFocus::Entries);
    for key in ["\"", "_", "d", "d"] {
        press(&mut app, key);
    }

    assert!(
        matches!(
            app.file_operations.view(),
            FileOperationView::PermanentDelete { message, detail }
                if message == "Permanently delete “trashed.txt” from Trash?"
                    && detail == "This cannot be undone."
        ),
        "Deleting a selected Trash item must open confirmation, got: {}",
        app.presentation.status()
    );
}

#[test]
fn select_all_in_trash_moves_focus_to_entries_before_delete() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation.install_trash_entries(
        ["one.txt", "two.txt"]
            .into_iter()
            .map(|name| crate::app::trash::Entry {
                file: entry(name),
                receipt: crate::journal::TrashReceipt {
                    original: PathBuf::from("/original").join(name),
                    trashed: PathBuf::from("/start").join(name),
                    info: PathBuf::from("/info").join(format!("{name}.trashinfo")),
                },
            })
            .collect(),
    );
    app.grid.select_only(Some(0), 2);
    app.focus_browser(BrowserFocus::Sidebar);
    let delete = keyboard::Key::Named(keyboard::key::Named::Delete);
    let _ = app.handle_key(
        delete.clone(),
        delete.clone(),
        keyboard::Modifiers::empty(),
        None,
    );
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));
    let select_all = keyboard::Key::Character("a".into());
    let _ = app.handle_key(
        select_all.clone(),
        select_all,
        keyboard::Modifiers::CTRL,
        Some("a"),
    );
    assert_eq!(app.grid.selection_count(), 2);
    let _ = app.handle_key(delete.clone(), delete, keyboard::Modifiers::empty(), None);

    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::PermanentDelete { message, .. }
            if message == "Permanently delete 2 selected Trash items?"
    ));
}

#[test]
fn cut_in_trash_explains_delete_instead_of_claiming_sidebar_focus() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .install_trash_entries(vec![crate::app::trash::Entry {
            file: entry("trashed.txt"),
            receipt: crate::journal::TrashReceipt {
                original: PathBuf::from("/original/trashed.txt"),
                trashed: PathBuf::from("/start/trashed.txt"),
                info: PathBuf::from("/info/trashed.txt.trashinfo"),
            },
        }]);
    let _ = app.update(Message::EntryPressed(0));
    let _ = app.update(Message::EntryReleased(0));

    for key in ["d", "x"] {
        press(&mut app, key);
        assert_eq!(
            app.presentation.status(),
            "Use Delete to delete permanently from Trash"
        );
        assert!(matches!(
            app.file_operations.view(),
            FileOperationView::Idle
        ));
        assert!(app.transfers.pending_cut_paths().is_empty());
        assert_eq!(app.navigation.entries().len(), 1);
    }
}

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
    app.file_operations
        .finish_trash_transfer(vec![(entry("one.txt"), "Trash is unavailable".to_owned())]);
    let _ = app.update(Message::Noop);

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
    let request = app
        .navigation
        .refresh_selected(vec![first.path.clone()])
        .request
        .unwrap();

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
fn permanent_delete_prompt_accepts_y_and_n_from_the_keyboard() {
    let (mut app, _) = App::new();
    app.navigation.settle_for_test();
    app.navigation
        .replace_displayed_entries(vec![entry("one.txt")]);
    app.grid
        .select_only(Some(0), app.navigation.entries().len());

    app.file_operations
        .finish_trash_transfer(vec![(entry("one.txt"), "Trash unavailable".to_owned())]);
    let _ = app.update(Message::Noop);
    press(&mut app, "n");
    assert!(matches!(
        app.file_operations.view(),
        FileOperationView::Idle
    ));

    app.file_operations
        .finish_trash_transfer(vec![(entry("one.txt"), "Trash unavailable".to_owned())]);
    let _ = app.update(Message::Noop);
    let key = keyboard::Key::Character("Y".into());
    let task = app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("Y"));
    assert!(app.foreground_operation_active());
    assert!(app.file_operations.is_busy());
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
    let _ = app.update(Message::PropertiesFinished {
        request: app.command.output_revision(),
        result: Ok(properties::Info {
            name: "document.txt".to_owned(),
            detail: "Type: Plain text".to_owned(),
        }),
    });
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
