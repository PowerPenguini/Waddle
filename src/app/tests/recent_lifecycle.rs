use super::*;
use gio::prelude::FileExt;
use iced::futures::StreamExt;
use navigation::finish_tasks;
use operation_access::collection_app;

async fn queued_messages(task: Task<Message>) -> Vec<Message> {
    let mut messages = Vec::new();
    if let Some(mut stream) = iced_runtime::task::into_stream(task) {
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                messages.push(message);
            }
        }
    }
    messages
}

#[test]
fn recent_trash_restore_undo_redo_lifecycle() {
    const CHILD: &str = "WADDLE_RECENT_TRASH_LIFECYCLE_ROOT";
    let Some(root) = std::env::var_os(CHILD) else {
        // GIO chooses the home Trash only for sources on the home filesystem.
        // /tmp may be a separate tmpfs where desktop Trash is unavailable.
        let temp = tempfile::Builder::new()
            .prefix("waddle-trash-test-")
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap();
        std_fs::create_dir_all(temp.path().join("data/Trash/files")).unwrap();
        std_fs::create_dir_all(temp.path().join("data/Trash/info")).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::recent_lifecycle::recent_trash_restore_undo_redo_lifecycle",
                "--nocapture",
            ])
            .env(CHILD, temp.path())
            .env("XDG_DATA_HOME", temp.path().join("data"))
            .env("XDG_STATE_HOME", temp.path().join("state"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
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
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let (mut app, file) = collection_app(&root, true).await;
            app.trash = trash::Trash::at(root.join("data/Trash"));
            let delete = keyboard::Key::Named(keyboard::key::Named::Delete);
            let task = app.handle_key(delete.clone(), delete, keyboard::Modifiers::empty(), None);
            finish_tasks(&mut app, task).await;
            if file.exists() {
                if let FileOperationView::PermanentDelete { detail, .. } =
                    app.file_operations.view()
                {
                    panic!("Trash failed: {detail}");
                }
                panic!(
                    "Delete in Recent did not move the file to Trash: {}",
                    app.presentation.status()
                );
            }
            assert!(app.navigation.entries().is_empty());
            assert_eq!(app.trash.entries().unwrap().len(), 1);

            let stale = queued_messages(app.update(Message::Refresh)).await;
            let task = app.run_journal(false);
            finish_tasks(&mut app, task).await;
            for message in stale {
                let task = app.update(message);
                finish_tasks(&mut app, task).await;
            }
            assert_eq!(std_fs::read_to_string(&file).unwrap(), "original contents");
            assert_eq!(app.navigation.entries()[0].path, file);
            assert!(app.trash.entries().unwrap().is_empty());
            let task = app.run_journal(true);
            finish_tasks(&mut app, task).await;
            assert!(!file.exists());
            assert!(app.navigation.entries().is_empty());

            let task = app.open_trash();
            finish_tasks(&mut app, task).await;
            assert_eq!(app.navigation.entries().len(), 1);
            app.grid.select_only(Some(0), 1);
            let task = app.update(Message::ContextRestore);
            let stale = queued_messages(app.update(Message::Refresh)).await;
            finish_tasks(&mut app, task).await;
            for message in stale {
                let task = app.update(message);
                finish_tasks(&mut app, task).await;
            }
            assert_eq!(std_fs::read_to_string(&file).unwrap(), "original contents");
            assert_eq!(
                app.navigation.displayed_location(),
                DisplayedLocation::Trash
            );
            assert!(app.navigation.entries().is_empty());
            let task = app.run_journal(false);
            finish_tasks(&mut app, task).await;
            assert!(!file.exists());
            assert_eq!(app.navigation.entries().len(), 1);
            let task = app.run_journal(true);
            finish_tasks(&mut app, task).await;
            assert_eq!(std_fs::read_to_string(&file).unwrap(), "original contents");
            assert!(app.navigation.entries().is_empty());
            let task = app.open_recent();
            finish_tasks(&mut app, task).await;
            assert_eq!(app.navigation.entries()[0].path, file);
            // A later file at the same path must never be overwritten by Undo.
            let saved = file.with_file_name("saved-original.txt");
            std_fs::rename(&file, &saved).unwrap();
            std_fs::write(&file, "replacement must survive").unwrap();
            let task = app.run_journal(false);
            finish_tasks(&mut app, task).await;
            assert_eq!(
                std_fs::read_to_string(&file).unwrap(),
                "replacement must survive"
            );
            assert_eq!(std_fs::read_to_string(saved).unwrap(), "original contents");
            assert!(app.trash.entries().unwrap().is_empty());
        });
}

#[test]
fn recent_transfer_completion_during_refresh_does_not_restore_stale_entries() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, file) = collection_app(temp.path(), true).await;
            let refresh = app.update(Message::Refresh);
            let stale = queued_messages(refresh).await;
            assert!(app.navigation.loading());
            let destination = temp.path().join("destination");
            std_fs::create_dir(&destination).unwrap();
            let mut transfer = TransferState::default();
            transfer.cut(app.navigation.entries()).unwrap();
            let request = transfer.paste(destination.clone()).unwrap();
            let task = app.start_transfer(request);
            finish_tasks(&mut app, task).await;
            assert!(!file.exists());
            assert!(destination.join("match.txt").exists());
            for message in stale {
                let task = app.update(message);
                finish_tasks(&mut app, task).await;
            }
            assert_eq!(
                app.navigation.displayed_location(),
                DisplayedLocation::Recent
            );
            assert!(
                app.navigation.entries().is_empty(),
                "A stale Recent scan won over the completed Move"
            );
        });
}

#[test]
fn recent_observes_a_missing_bookmarked_file_restored_externally() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, file) = collection_app(temp.path(), true).await;
            assert!(app.location_monitoring.is_some());
            std_fs::remove_file(&file).unwrap();
            let task = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: file.parent().unwrap().to_path_buf(),
                removed: vec![file.clone()],
                watch_failed: false,
            }));
            finish_tasks(&mut app, task).await;
            assert!(app.navigation.entries().is_empty());
            std_fs::write(&file, "restored externally").unwrap();
            let task = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: file.parent().unwrap().to_path_buf(),
                removed: Vec::new(),
                watch_failed: false,
            }));
            finish_tasks(&mut app, task).await;
            assert_eq!(
                app.navigation.entries().len(),
                1,
                "Recent stopped watching the parent of a missing bookmark"
            );
            assert_eq!(app.navigation.entries()[0].path, file);
        });
}

#[test]
fn recent_multi_parent_transfer_conflicts_and_history_preserve_each_file() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for cut in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, first) = collection_app(temp.path(), true).await;
                let second = temp.path().join("other/match.txt");
                std_fs::create_dir(second.parent().unwrap()).unwrap();
                std_fs::write(&second, "second contents").unwrap();
                let mut bookmarks = gio::glib::BookmarkFile::new();
                let history = temp.path().join("history.xbel");
                bookmarks.load_from_file(&history).unwrap();
                let uri = gio::File::for_path(&second).uri();
                bookmarks.set_title(Some(&uri), "second");
                bookmarks.add_application(&uri, Some("Waddle test"), Some("waddle %u"));
                bookmarks.to_file(&history).unwrap();
                let task = app.update(Message::Refresh);
                finish_tasks(&mut app, task).await;
                app.grid.select_all(2);
                press(&mut app, if cut { "x" } else { "y" });
                let destination = temp.path().join("destination");
                std_fs::create_dir(&destination).unwrap();
                let task = app.transition_navigation(NavigationTransition::Open {
                    requested: destination.clone(),
                    remember: true,
                    select: None,
                });
                finish_tasks(&mut app, task).await;
                let task = app.update(Message::Paste);
                finish_tasks(&mut app, task).await;
                assert!(app.transfers.overview().conflict_prompt.is_some());
                // A later completion must refresh Recent without navigating back to the destination.
                let task = app.open_recent();
                finish_tasks(&mut app, task).await;
                let task = app.resolve_transfer_conflict('k', false);
                finish_tasks(&mut app, task).await;
                assert_eq!(
                    app.navigation.displayed_location(),
                    DisplayedLocation::Recent
                );
                assert_eq!(app.navigation.entries().len(), if cut { 0 } else { 2 });
                let mut contents = fs::read_directory(&destination)
                    .unwrap()
                    .iter()
                    .map(|entry| std_fs::read_to_string(&entry.path).unwrap())
                    .collect::<Vec<_>>();
                contents.sort();
                assert_eq!(contents, ["original contents", "second contents"]);
                let task = app.run_journal(false);
                finish_tasks(&mut app, task).await;
                assert_eq!(std_fs::read_to_string(&first).unwrap(), "original contents");
                assert_eq!(std_fs::read_to_string(&second).unwrap(), "second contents");
                assert!(fs::read_directory(&destination).unwrap().is_empty());
                assert_eq!(app.navigation.entries().len(), 2);
                let task = app.run_journal(true);
                finish_tasks(&mut app, task).await;
                assert_eq!(
                    app.navigation.displayed_location(),
                    DisplayedLocation::Recent
                );
                assert_eq!(app.navigation.entries().len(), if cut { 0 } else { 2 });
                assert_eq!(fs::read_directory(&destination).unwrap().len(), 2);
            }
        });
}

#[test]
fn recent_observes_external_changes_while_a_mutation_is_pending() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, file) = collection_app(temp.path(), true).await;
            let busy = app
                .operations
                .run_foreground(OperationKind::Mutation, |_| Ok(()));
            assert!(app.foreground_operation_active());
            std_fs::remove_file(&file).unwrap();
            let task = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: file.parent().unwrap().to_path_buf(),
                removed: vec![file],
                watch_failed: false,
            }));
            finish_tasks(&mut app, task).await;
            assert!(
                app.navigation.entries().is_empty(),
                "A read-only refresh was discarded while a mutation was pending"
            );
            drop(busy);
        });
}

#[test]
fn recent_partial_move_cancellation_and_retry_keep_undo_separate() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, blocked) = collection_app(temp.path(), true).await;
            let free = blocked.with_file_name("free.txt");
            std_fs::write(&free, "free contents").unwrap();
            let history = temp.path().join("history.xbel");
            let mut bookmarks = gio::glib::BookmarkFile::new();
            bookmarks.load_from_file(&history).unwrap();
            let uri = gio::File::for_path(&free).uri();
            bookmarks.set_title(Some(&uri), "free");
            bookmarks.add_application(&uri, Some("Waddle test"), Some("waddle %u"));
            bookmarks.to_file(&history).unwrap();
            let task = app.update(Message::Refresh);
            finish_tasks(&mut app, task).await;
            assert_eq!(app.navigation.entries()[0].path, free);
            app.grid.select_all(2);
            press(&mut app, "x");
            let destination = temp.path().join("destination");
            std_fs::create_dir(&destination).unwrap();
            std_fs::write(destination.join("match.txt"), "unrelated destination").unwrap();
            let task = app.transition_navigation(NavigationTransition::Open {
                requested: destination.clone(),
                remember: true,
                select: None,
            });
            finish_tasks(&mut app, task).await;
            let task = app.update(Message::Paste);
            finish_tasks(&mut app, task).await;
            assert!(app.transfers.overview().conflict_prompt.is_some());
            assert!(!free.exists());
            assert!(blocked.exists());
            let task = app.open_recent();
            finish_tasks(&mut app, task).await;
            let task = app.cancel_transfer_conflict();
            finish_tasks(&mut app, task).await;
            assert!(!app.transfers.overview().active);
            assert!(app.transfers.overview().retry);
            let task = app.update(Message::RetryTransfer);
            finish_tasks(&mut app, task).await;
            assert!(app.transfers.overview().conflict_prompt.is_some());
            let task = app.resolve_transfer_conflict('k', false);
            finish_tasks(&mut app, task).await;
            assert!(!blocked.exists());
            assert_eq!(
                app.navigation.displayed_location(),
                DisplayedLocation::Recent
            );
            assert!(app.navigation.entries().is_empty());
            let task = app.run_journal(false);
            finish_tasks(&mut app, task).await;
            assert!(blocked.exists(), "Undo retry must restore its source");
            assert!(
                !free.exists(),
                "Undo retry must leave the earlier completed part intact"
            );
            let task = app.run_journal(false);
            finish_tasks(&mut app, task).await;
            assert_eq!(std_fs::read_to_string(&free).unwrap(), "free contents");
            assert_eq!(
                std_fs::read_to_string(&blocked).unwrap(),
                "original contents"
            );
            assert_eq!(
                std_fs::read_to_string(destination.join("match.txt")).unwrap(),
                "unrelated destination"
            );
            assert_eq!(fs::read_directory(&destination).unwrap().len(), 1);
            assert_eq!(app.navigation.entries().len(), 2);
        });
}

#[test]
fn replacing_a_cut_selection_restores_the_previous_entries_in_each_view() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for location in 0..3 {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, first) = collection_app(temp.path(), location == 1).await;
                let second = first.with_file_name("match-two.txt");
                std_fs::write(&second, "second").unwrap();
                if location == 1 {
                    let history = temp.path().join("history.xbel");
                    let mut bookmarks = gio::glib::BookmarkFile::new();
                    bookmarks.load_from_file(&history).unwrap();
                    let uri = gio::File::for_path(&second).uri();
                    bookmarks.set_title(Some(&uri), "second");
                    bookmarks.add_application(&uri, Some("Waddle test"), Some("waddle %u"));
                    bookmarks.to_file(&history).unwrap();
                }
                let task = if location == 0 {
                    app.transition_navigation(NavigationTransition::Open {
                        requested: first.parent().unwrap().to_path_buf(),
                        remember: true,
                        select: None,
                    })
                } else {
                    app.update(Message::Refresh)
                };
                finish_tasks(&mut app, task).await;
                let index = app
                    .navigation
                    .entries()
                    .iter()
                    .position(|entry| entry.path == first)
                    .unwrap();
                app.grid.select_only(Some(index), 2);
                press(&mut app, "x");
                assert_eq!(app.navigation.entries().len(), 1);
                app.grid.select_only(Some(0), 1);
                let key = keyboard::Key::Character("x".into());
                let task =
                    app.handle_key(key.clone(), key, keyboard::Modifiers::empty(), Some("x"));
                finish_tasks(&mut app, task).await;
                assert_eq!(
                    app.transfers.pending_cut_paths(),
                    std::slice::from_ref(&second)
                );
                assert_eq!(
                    app.navigation.entries().len(),
                    1,
                    "Replacing Cut left the previous item hidden in location={location}"
                );
                assert_eq!(app.navigation.entries()[0].path, first);
                assert!(first.exists() && second.exists());
            }
        });
}

#[test]
fn recent_clear_and_disable_supersede_older_scans() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for command in ["recent clear", "recent disable"] {
                for displayed in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    let (mut app, file) = collection_app(temp.path(), true).await;
                    if !displayed {
                        let task = app.transition_navigation(NavigationTransition::Open {
                            requested: file.parent().unwrap().to_path_buf(),
                            remember: true,
                            select: None,
                        });
                        finish_tasks(&mut app, task).await;
                    }
                    let task = if displayed {
                        app.update(Message::Refresh)
                    } else {
                        app.open_recent()
                    };
                    let stale = queued_messages(task).await;
                    assert!(app.navigation.loading());
                    drop(app.begin_command(':'));
                    drop(app.update(Message::CommandChanged(command.into())));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    for message in stale {
                        let task = app.update(message);
                        finish_tasks(&mut app, task).await;
                    }
                    if command.ends_with("disable") {
                        assert_eq!(
                            app.navigation.displayed_location(),
                            DisplayedLocation::Folder,
                            "An older scan reopened Recent after disable"
                        );
                        assert!(app.recent.sidebar_entry().is_none());
                    } else {
                        assert_eq!(
                            app.navigation.displayed_location(),
                            DisplayedLocation::Recent
                        );
                        assert!(
                            app.navigation.entries().is_empty(),
                            "An older scan restored cleared history"
                        );
                    }
                    assert!(file.exists(), "History commands must not delete files");
                }
            }
        });
}

#[test]
fn recent_disable_does_not_replace_a_pending_folder_choice() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, _) = collection_app(temp.path(), true).await;
            let target = temp.path().join("chosen-folder");
            std_fs::create_dir(&target).unwrap();
            let task = app.transition_navigation(NavigationTransition::Open {
                requested: target.clone(),
                remember: true,
                select: None,
            });
            let pending = queued_messages(task).await;
            drop(app.begin_command(':'));
            drop(app.update(Message::CommandChanged("recent disable".into())));
            let task = app.update(Message::CommandSubmitted);
            finish_tasks(&mut app, task).await;
            for message in pending {
                let task = app.update(message);
                finish_tasks(&mut app, task).await;
            }
            assert_eq!(
                app.navigation.current(),
                target,
                "Disabling Recent replaced the user's newer folder choice"
            );
        });
}
