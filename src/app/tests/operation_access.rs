use super::*;
use gio::prelude::FileExt;
use navigation::finish_tasks;

#[test]
fn collection_background_is_not_a_drop_destination() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, _) = collection_app(temp.path(), recent).await;
                app.grid.resize(iced::Size::new(1000.0, 700.0));
                let empty = iced::Point::new(950.0, 600.0);
                assert_eq!(
                    app.drop_destination_at(empty, true),
                    None,
                    "Dropping on a collection would write to its hidden underlying folder"
                );
                app.navigation.install_trash_entries(Vec::new());
                assert_eq!(app.drop_destination_at(empty, true), None);
            }
        });
}

pub(super) async fn collection_app(temp: &Path, recent: bool) -> (App, PathBuf) {
    let root = temp.join("root");
    let nested = root.join("nested");
    std_fs::create_dir_all(&nested).unwrap();
    let file = nested.join("match.txt");
    std_fs::write(&file, "original contents").unwrap();
    let (mut app, _) = App::new();
    app.transfers = transfer_session::TransferSession::open(temp.join("transfers.json"));
    app.view_preferences = view_preferences::Preferences::empty_at(temp.join("view.toml"));
    app.navigation = NavigationSession::new(root.clone());
    app.navigation.settle_for_test();
    app.navigation
        .install_folder_entries(fs::read_directory(&root).unwrap());
    if recent {
        let history = temp.join("history.xbel");
        let uri = gio::File::for_path(&file).uri();
        let mut bookmarks = gio::glib::BookmarkFile::new();
        bookmarks.set_title(Some(&uri), "fixture");
        bookmarks.add_application(&uri, Some("Waddle test"), Some("waddle %u"));
        bookmarks.to_file(&history).unwrap();
        app.recent = recent::Recent::open_at(history, temp.join("preferences.json"));
        let task = app.open_recent();
        finish_tasks(&mut app, task).await;
    } else {
        press(&mut app, "/");
        let task = app.update(Message::SearchChanged("/match".into()));
        finish_tasks(&mut app, task).await;
        // Keep the Search session while moving keyboard input to the entries.
        app.browser_input.leave_mode();
    }
    app.focus_browser(BrowserFocus::Entries);
    app.grid.select_only(Some(0), 1);
    assert_eq!(app.navigation.entries()[0].path, file);
    (app, file)
}

fn assert_collection(app: &App, recent: bool) {
    if recent {
        assert_eq!(
            app.navigation.displayed_location(),
            DisplayedLocation::Recent
        );
    } else {
        assert!(app.search.is_recursive());
    }
}

#[test]
fn collection_cut_refresh_and_cancel_preserve_the_displayed_collection() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, file) = collection_app(temp.path(), recent).await;
                press(&mut app, "d");
                press(&mut app, "d");
                assert_eq!(
                    app.transfers.pending_cut_paths(),
                    std::slice::from_ref(&file)
                );
                assert!(app.navigation.entries().is_empty());
                let task = app.update(Message::Refresh);
                finish_tasks(&mut app, task).await;
                assert_collection(&app, recent);
                assert!(
                    app.navigation.entries().is_empty(),
                    "refresh resurrected pending Cut in recent={recent}"
                );
                let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
                let task =
                    app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
                finish_tasks(&mut app, task).await;
                assert_collection(&app, recent);
                assert_eq!(app.navigation.entries()[0].path, file);
                assert!(app.transfers.pending_cut_paths().is_empty());
                assert_eq!(std_fs::read_to_string(file).unwrap(), "original contents");
            }
        });
}

#[test]
fn collection_rename_and_undo_operate_on_the_selected_path() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, file) = collection_app(temp.path(), recent).await;
                let task = app.update(Message::EntryContext(0));
                finish_tasks(&mut app, task).await;
                let task = app.update(Message::ContextRename);
                finish_tasks(&mut app, task).await;
                assert!(matches!(
                    app.file_operations.view(),
                    FileOperationView::Rename { .. }
                ));
                drop(app.update(Message::RenameChanged("renamed.txt".into())));
                let task = app.update(Message::RenameSubmitted);
                finish_tasks(&mut app, task).await;
                assert!(!file.exists());
                let renamed = file.with_file_name("renamed.txt");
                assert_eq!(
                    std_fs::read_to_string(&renamed).unwrap(),
                    "original contents"
                );
                assert_collection(&app, recent);
                assert!(app.navigation.entries().is_empty());
                let task = app.run_journal(false);
                finish_tasks(&mut app, task).await;
                assert!(file.exists(), "Undo was rejected in recent={recent}");
                assert!(!renamed.exists());
                assert_collection(&app, recent);
                assert_eq!(app.navigation.entries()[0].path, file);
            }
        });
}

#[test]
fn collection_cut_can_be_pasted_only_after_opening_a_destination_folder() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, file) = collection_app(temp.path(), recent).await;
                press(&mut app, "x");
                assert_eq!(
                    app.transfers.pending_cut_paths(),
                    std::slice::from_ref(&file)
                );
                let task = app.update(Message::Paste);
                finish_tasks(&mut app, task).await;
                assert!(file.exists());
                assert!(!app.transfers.overview().active);
                for message in [Message::ContextNewFolder, Message::ContextNewFile] {
                    let task = app.update(message);
                    finish_tasks(&mut app, task).await;
                    assert!(matches!(
                        app.file_operations.view(),
                        FileOperationView::Idle
                    ));
                }
                assert!(app.context_actions(ContextTarget::Background).is_empty());
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
                assert!(!file.exists());
                assert_eq!(
                    std_fs::read_to_string(destination.join("match.txt")).unwrap(),
                    "original contents"
                );
                assert!(app.transfers.pending_cut_paths().is_empty());
            }
        });
}

#[test]
fn copy_shortcuts_agree_in_collections_and_sidebar() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [true, false] {
                for sidebar in [true, false] {
                    for control in [true, false] {
                        let temp = tempfile::tempdir().unwrap();
                        let (mut app, file) = collection_app(temp.path(), recent).await;
                        if sidebar {
                            // Make this independent of the user's saved sidebar visibility.
                            drop(app.view_preferences.apply_command(
                                app.navigation.current(),
                                false,
                                "tree=true",
                            ));
                            app.focus_browser(BrowserFocus::Sidebar);
                        }
                        let key = keyboard::Key::Character(if control { "c" } else { "y" }.into());
                        let modifiers = if control {
                            keyboard::Modifiers::CTRL
                        } else {
                            keyboard::Modifiers::empty()
                        };
                        let task = app.handle_key(key.clone(), key, modifiers, None);
                        finish_tasks(&mut app, task).await;
                        let payload = app.transfers.clipboard_payload();
                        if sidebar {
                            assert!(payload.is_none(), "Copy stole the hidden entry selection");
                        } else {
                            assert_eq!(payload.unwrap().paths, [file]);
                        }
                    }
                }
            }
        });
}
