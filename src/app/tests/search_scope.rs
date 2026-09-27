use super::*;

#[test]
fn reopening_recursive_search_preserves_scope_and_escape_restores_folder() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("root");
            let nested = root.join("nested");
            std_fs::create_dir_all(&nested).unwrap();
            let file = nested.join("match.txt");
            std_fs::write(&file, "fixture").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(root.clone());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(&root).unwrap());
            app.focus_browser(BrowserFocus::Entries);
            press(&mut app, "/");
            let task = app.update(Message::SearchChanged("/match".into()));
            navigation::finish_tasks(&mut app, task).await;
            let task = app.update(Message::EntryContext(0));
            navigation::finish_tasks(&mut app, task).await;
            let task = app.update(Message::ContextRename);
            navigation::finish_tasks(&mut app, task).await;
            let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
            let task = app.handle_key(
                escape.clone(),
                escape.clone(),
                keyboard::Modifiers::empty(),
                None,
            );
            navigation::finish_tasks(&mut app, task).await;
            assert_eq!(app.browser_input.mode(), InputMode::Browser);
            assert_eq!(app.navigation.entries()[0].path, file);
            assert!(app.search.is_recursive());
            assert!(app.context_actions(ContextTarget::Background).is_empty());
            press(&mut app, "/");
            assert!(
                app.context_actions(ContextTarget::Background).is_empty(),
                "Reopening search made nested results into a writable folder view"
            );
            assert_eq!(app.search.query(), "match");
            let task = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
            navigation::finish_tasks(&mut app, task).await;
            assert_eq!(app.navigation.entries()[0].path, nested);
            assert_eq!(app.navigation.current(), root);
        });
}

#[test]
fn collections_reject_recursive_search_without_replacing_entries_or_selection() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let live = temp.path().join("live");
                std_fs::create_dir(&live).unwrap();
                std_fs::write(live.join("match-live.txt"), "not in collection").unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(live);
                app.navigation.settle_for_test();
                let files = vec![entry("first.txt"), entry("match.txt")];
                let location = if recent {
                    let request = app.navigation.recent().request.unwrap();
                    let task = app.update(Message::RecentLoaded {
                        request,
                        result: Some(Ok(files.clone())),
                    });
                    navigation::finish_tasks(&mut app, task).await;
                    DisplayedLocation::Recent
                } else {
                    app.navigation.install_trash_entries(
                        files
                            .iter()
                            .map(|file| trash::Entry {
                                identity: None,
                                file: file.clone(),
                                receipt: crate::journal::TrashReceipt {
                                    original: temp.path().join(&file.name),
                                    trashed: file.path.clone(),
                                    info: temp.path().join("unused.trashinfo"),
                                },
                            })
                            .collect(),
                    );
                    DisplayedLocation::Trash
                };
                app.focus_browser(BrowserFocus::Entries);
                app.grid.select_only(Some(0), 2);
                press(&mut app, "/");
                let task = app.update(Message::SearchChanged("/match".into()));
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(app.navigation.displayed_location(), location);
                assert_eq!(
                    app.navigation
                        .entries()
                        .iter()
                        .map(|entry| &entry.path)
                        .collect::<Vec<_>>(),
                    files.iter().map(|entry| &entry.path).collect::<Vec<_>>()
                );
                assert_eq!(app.grid.selected_entry(), Some(0));
                assert!(!app.search.is_recursive());
                assert_eq!(
                    app.presentation.status(),
                    "Recursive search requires a folder; use / to search this view"
                );
                assert_eq!(
                    app.browser_input.mode(),
                    InputMode::Browser,
                    "The search editor must not hide the rejection message"
                );
                press(&mut app, "/");
                let task = app.update(Message::SearchChanged("match".into()));
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(app.grid.selected_entry(), Some(1));
                if !recent {
                    assert_eq!(app.selected_trash_entries().len(), 1);
                }
            }
        });
}
