use super::*;

#[test]
fn failed_recent_preference_saves_preserve_the_previous_behavior() {
    const CHILD_ROOT: &str = "WADDLE_RECENT_SAVE_TEST_ROOT";
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        for enabled in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let config = temp.path().join("config");
            let data = temp.path().join("data");
            std_fs::create_dir_all(config.join("waddle/recent.json.tmp")).unwrap();
            std_fs::create_dir_all(&data).unwrap();
            std_fs::write(
                config.join("waddle/recent.json"),
                format!(r#"{{"enabled":{enabled}}}"#),
            )
            .unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::tests::navigation::failed_recent_preference_saves_preserve_the_previous_behavior", "--nocapture"])
                .env(CHILD_ROOT, temp.path())
                .env("WADDLE_RECENT_INITIAL_ENABLED", enabled.to_string())
                .env("XDG_CONFIG_HOME", config)
                .env("XDG_DATA_HOME", data)
                .output().unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let root = PathBuf::from(root);
    let enabled = std::env::var("WADDLE_RECENT_INITIAL_ENABLED").unwrap() == "true";
    let preferences = root.join("config/waddle/recent.json");
    let original_preferences = std_fs::read(&preferences).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(root.clone());
            app.navigation.settle_for_test();
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(
                if enabled {
                    "recent disable"
                } else {
                    "recent enable"
                }
                .into(),
            ));
            let task = app.update(Message::CommandSubmitted);
            finish_tasks(&mut app, task).await;
            assert!(
                app.presentation
                    .status()
                    .to_lowercase()
                    .contains("directory"),
                "Save failure was not reported: {}",
                app.presentation.status()
            );
            assert_eq!(std_fs::read(&preferences).unwrap(), original_preferences);
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged("recent open".into()));
            let task = app.update(Message::CommandSubmitted);
            finish_tasks(&mut app, task).await;
            assert_eq!(
                app.navigation.displayed_location(),
                if enabled {
                    DisplayedLocation::Recent
                } else {
                    DisplayedLocation::Folder
                },
                "Failed save changed whether Recent could be opened"
            );
            std_fs::remove_dir(root.join("config/waddle/recent.json.tmp")).unwrap();
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(
                if enabled {
                    "recent disable"
                } else {
                    "recent enable"
                }
                .into(),
            ));
            let task = app.update(Message::CommandSubmitted);
            finish_tasks(&mut app, task).await;
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged("recent open".into()));
            let task = app.update(Message::CommandSubmitted);
            finish_tasks(&mut app, task).await;
            let expected = if enabled {
                DisplayedLocation::Folder
            } else {
                DisplayedLocation::Recent
            };
            assert_eq!(
                app.navigation.displayed_location(),
                expected,
                "Retry did not apply the preference"
            );

            let (mut reopened, _) = App::new();
            reopened.navigation = NavigationSession::new(root.clone());
            reopened.navigation.settle_for_test();
            press(&mut reopened, ":");
            let _ = reopened.update(Message::CommandChanged("recent open".into()));
            let task = reopened.update(Message::CommandSubmitted);
            finish_tasks(&mut reopened, task).await;
            assert_eq!(
                reopened.navigation.displayed_location(),
                expected,
                "Successful retry was not saved"
            );
        });
}

#[test]
fn directory_refresh_preserves_location_edits_and_their_submission() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for refresh_first in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                let created = temp.path().join("new.txt");
                std_fs::create_dir(&target).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.sync_location_monitoring();
                let pending = refresh_first.then(|| app.update(Message::Refresh));
                let focus = app.begin_location();
                finish_tasks(&mut app, focus).await;
                let _ = app.update(Message::LocationChanged("target".into()));
                std_fs::write(&created, "external change").unwrap();
                let refresh = pending.unwrap_or_else(|| {
                    app.update(Message::DirectoryChanged(directory_watch::Event {
                        path: temp.path().to_path_buf(),
                        removed: Vec::new(),
                        watch_failed: false,
                    }))
                });
                finish_tasks(&mut app, refresh).await;
                assert!(
                    app.navigation
                        .entries()
                        .iter()
                        .any(|entry| entry.path == created)
                );
                assert_eq!(
                    app.location_input, "target",
                    "Refresh replaced the Location edit"
                );
                assert_eq!(app.browser_input.mode(), InputMode::Location);
                let submit = app.update(Message::LocationSubmitted);
                finish_tasks(&mut app, submit).await;
                assert_eq!(app.navigation.current(), target);
                assert_eq!(app.location_input, target.display().to_string());
                let focus = app.begin_location();
                finish_tasks(&mut app, focus).await;
                let _ = app.update(Message::LocationChanged("unsubmitted".into()));
                let parent = app.update(Message::Parent);
                finish_tasks(&mut app, parent).await;
                assert_eq!(app.navigation.current(), temp.path());
                assert_eq!(app.location_input, temp.path().display().to_string());
            }
        });
}

#[test]
fn submitting_an_unchanged_location_preserves_non_utf8_path_bytes() {
    use std::os::unix::ffi::OsStringExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let original = temp
                .path()
                .join(std::ffi::OsString::from_vec(b"folder-\xff".to_vec()));
            let twin = PathBuf::from(original.display().to_string());
            assert_ne!(original, twin);
            std_fs::create_dir(&original).unwrap();
            std_fs::create_dir(&twin).unwrap();
            std_fs::write(original.join("original.txt"), "original").unwrap();
            std_fs::write(twin.join("other.txt"), "other folder").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(original.clone());
            app.navigation.settle_for_test();
            let focus = app.begin_location();
            finish_tasks(&mut app, focus).await;
            let submitted = app.update(Message::LocationSubmitted);
            finish_tasks(&mut app, submitted).await;
            assert_eq!(
                app.navigation.current(),
                original,
                "Unedited Location entered a different folder"
            );
            assert_eq!(
                app.navigation.entries()[0].path,
                original.join("original.txt")
            );

            let focus = app.begin_location();
            finish_tasks(&mut app, focus).await;
            let _ = app.update(Message::LocationChanged(twin.display().to_string()));
            let submitted = app.update(Message::LocationSubmitted);
            finish_tasks(&mut app, submitted).await;
            assert_eq!(
                app.navigation.current(),
                twin,
                "An explicitly entered UTF-8 path must still work"
            );
        });
}

#[test]
fn shell_functions_keep_selected_paths_separate_from_function_arguments() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let selected = temp.path().join("chosen.txt");
                std_fs::write(&selected, "selected contents").unwrap();
                std_fs::write(temp.path().join("unrelated.txt"), "unrelated contents").unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                let index = app
                    .navigation
                    .entries()
                    .iter()
                    .position(|entry| entry.path == selected)
                    .unwrap();
                app.grid
                    .select_only(Some(index), app.navigation.entries().len());
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged(
                    "show() { printf '%s:' \"$1\"; cat $selected; }; show unrelated.txt".into(),
                ));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                let output = app.command.output().expect("shell output");
                assert_eq!(
                    output.detail, "unrelated.txt:selected contents",
                    "Function arguments retargeted the selected paths"
                );
                assert!(output.summary.ends_with("exit 0"));
            }
        });
}

#[test]
fn changing_shell_arguments_preserves_the_full_selection() {
    use std::os::unix::ffi::OsStringExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                for (command, expected) in [
                    ("shift; printf '%s:' \"$#\"; cat $selected", "1:AB"),
                    (
                        "true; set -- one two; shift; printf '%s:' \"$1\"; cat ${selected}",
                        "two:AB",
                    ),
                    (
                        "show() { shift; printf '%s:' \"$#\"; (cat $selected); }; show one two",
                        "1:AB",
                    ),
                ] {
                    let temp = tempfile::tempdir().unwrap();
                    std_fs::write(temp.path().join("a file.txt"), "A").unwrap();
                    std_fs::write(
                        temp.path()
                            .join(std::ffi::OsString::from_vec(b"b-\xff".to_vec())),
                        "B",
                    )
                    .unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    app.navigation
                        .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                    app.grid.select_all(app.navigation.entries().len());
                    press(&mut app, prefix);
                    let _ = app.update(Message::CommandChanged(command.into()));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    let output = app.command.output().expect("shell output");
                    assert_eq!(output.detail, expected, "{command}");
                    assert!(output.summary.ends_with("exit 0"));
                }
            }
        });
}

#[test]
fn shell_directory_changes_follow_a_renamed_working_directory() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                let renamed = temp.path().join("renamed");
                std_fs::create_dir(&target).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged(
                    "cd target; mv ../target ../renamed; printf result; false".into(),
                ));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                assert!(!target.exists());
                assert!(renamed.is_dir());
                assert_eq!(
                    app.navigation.current(),
                    if prefix == ":" {
                        renamed.as_path()
                    } else {
                        temp.path()
                    },
                    "Waddle followed stale PWD text after the shell's directory was renamed"
                );
                let output = app.command.output().expect("shell output");
                assert_eq!(output.detail, "result");
                assert!(output.summary.ends_with("exit 1"));
            }
        });
}

#[test]
fn shell_directory_reports_preserve_path_bytes_when_pwd_is_changed_or_unset() {
    use std::os::unix::ffi::OsStringExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                for change in ["unset PWD", "PWD=/unrelated", "readonly PWD=/unrelated"] {
                    let temp = tempfile::tempdir().unwrap();
                    let target = temp
                        .path()
                        .join(std::ffi::OsString::from_vec(b"target-\xff\n\n".to_vec()));
                    std_fs::create_dir(&target).unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    app.navigation
                        .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                    app.grid.select_only(Some(0), 1);
                    press(&mut app, prefix);
                    let _ = app.update(Message::CommandChanged(format!(
                        "cd $selected; {change}; printf result; false"
                    )));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    assert_eq!(
                        app.navigation.current(),
                        if prefix == ":" {
                            target.as_path()
                        } else {
                            temp.path()
                        }
                    );
                    let output = app.command.output().expect("shell output");
                    assert_eq!(output.detail, "result");
                    assert!(output.summary.ends_with("exit 1"));
                }
            }
        });
}

#[test]
fn shell_stdout_redirection_preserves_logs_and_directory_changes() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                std_fs::create_dir(&target).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged(
                    "exec 3>extra.txt; exec 4>&3; printf extra >&4; exec >log.txt; cd target; printf logged; printf 'reported error' >&2; false"
                        .into(),
                ));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                assert_eq!(
                    std_fs::read(temp.path().join("log.txt")).unwrap(),
                    b"logged",
                    "Waddle wrote its internal directory report into the user's log"
                );
                assert_eq!(std_fs::read(temp.path().join("extra.txt")).unwrap(), b"extra");
                let output = app.command.output().expect("standard error output");
                assert_eq!(output.detail, "reported error");
                assert!(output.summary.ends_with("exit 1"));
                assert_eq!(
                    app.navigation.current(),
                    if prefix == ":" {
                        target.as_path()
                    } else {
                        temp.path()
                    }
                );
            }
        });
}

#[test]
fn noisy_exit_traps_do_not_hide_shell_directory_changes() {
    use std::os::unix::ffi::OsStringExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join(std::ffi::OsString::from_vec(
                    b"target-\xff\nfolder".to_vec(),
                ));
                std_fs::create_dir(&target).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.grid.select_only(Some(0), 1);
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged(
                    "trap \"printf '%0140000d' 0\" EXIT; printf '%0140000d' 0; cd $selected; false"
                        .into(),
                ));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                let output = app.command.output().expect("truncated command output");
                assert!(output.summary.ends_with("exit 1"));
                assert!(output.detail.contains("output truncated"));
                assert!(!output.detail.contains("WADDLE_PWD"));
                assert_eq!(
                    app.navigation.current(),
                    if prefix == ":" {
                        target.as_path()
                    } else {
                        temp.path()
                    }
                );
            }
        });
}

#[test]
fn noisy_shell_commands_keep_standard_error_visible() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged(
                    "printf 'output begins\\n'; printf '%0140000d' 0; printf 'permission denied\\n' >&2; false".into(),
                ));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                let output = app.command.output().expect("shell output");
                assert!(output.summary.ends_with("exit 1"));
                assert!(output.detail.starts_with("output begins\n"));
                assert!(
                    output.detail.ends_with("stderr:\npermission denied"),
                    "Large stdout hid the command's error"
                );
                assert!(output.detail.contains("output truncated"));
                assert!(output.detail.len() < 132_000);
            }
        });
}

#[test]
fn shell_output_shares_its_limit_without_discarding_fitting_streams() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                for (stdout, stderr) in [
                    ("out".repeat(30_000), "error".into()),
                    ("output".into(), "err".repeat(30_000)),
                    ("ż".repeat(90_000), "ę".repeat(90_000)),
                    (String::new(), "err".repeat(50_000)),
                ] {
                    let temp = tempfile::tempdir().unwrap();
                    std_fs::write(temp.path().join("stdout"), &stdout).unwrap();
                    std_fs::write(temp.path().join("stderr"), &stderr).unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    press(&mut app, prefix);
                    let _ =
                        app.update(Message::CommandChanged("cat stdout; cat stderr >&2".into()));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    let output = app.command.output().expect("shell output");
                    assert!(output.summary.ends_with("exit 0"));
                    assert!(output.detail.len() <= 128 * 1024);
                    if stdout.len() + stderr.len() < 100_000 {
                        assert_eq!(
                            output.detail,
                            format!("{stdout}\n\nstderr:\n{stderr}"),
                            "A stream that fits in the output limit was truncated"
                        );
                    } else {
                        assert!(output.detail.contains("output truncated"));
                        assert!(!output.detail.contains('\u{fffd}'));
                        if stdout.is_empty() {
                            assert!(output.detail.starts_with("err"));
                        } else {
                            assert!(output.detail.starts_with('ż'));
                            assert!(output.detail.contains("stderr:\nę"));
                        }
                    }
                }
            }
        });
}

#[test]
fn shell_comments_do_not_require_selected_entries() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                for command in [
                    "printf result # $selected is optional",
                    "printf result;# \"${selected}\" is optional",
                    "# don't expand \"$selected\"\nprintf result",
                    "printf result # $selected\\\nprintf ''",
                ] {
                    let temp = tempfile::tempdir().unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    press(&mut app, prefix);
                    let _ = app.update(Message::CommandChanged(command.into()));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    let output = app.command.output().expect("shell output");
                    assert_eq!(output.detail, "result", "{command}");
                    assert!(output.summary.ends_with("exit 0"));
                }
            }
        });
}

#[test]
fn shell_comment_boundaries_preserve_selected_arguments() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                for command in [
                    "# don't expand \"$selected\" here\ncat $selected",
                    "printf '%s' x#$selected",
                    "printf '%s' \\#$selected",
                    "printf '%s' ''#$selected",
                    "printf '%s' x\\\n#$selected",
                    "# comment\n# another \"$selected\"\ncat $selected",
                ] {
                    let temp = tempfile::tempdir().unwrap();
                    let selected = temp.path().join("a file.txt");
                    std_fs::write(&selected, "selected contents").unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    app.navigation
                        .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                    app.grid.select_only(Some(0), 1);
                    press(&mut app, prefix);
                    let _ = app.update(Message::CommandChanged(command.into()));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    let output = app.command.output().expect("shell output");
                    let expected = if command.starts_with('#') {
                        "selected contents".to_owned()
                    } else if command.contains(" x") {
                        format!("x#{}", selected.display())
                    } else {
                        format!("#{}", selected.display())
                    };
                    assert_eq!(output.detail, expected, "{command}");
                    assert!(output.summary.ends_with("exit 0"));
                }
            }
        });
}

#[test]
fn selected_shell_paths_still_refer_to_the_selection_after_cd() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let source = temp.path().join("source");
                let other = temp.path().join("other");
                std_fs::create_dir(&source).unwrap();
                std_fs::create_dir(&other).unwrap();
                let selected = source.join("a file.txt");
                std_fs::write(&selected, "selected contents").unwrap();
                std_fs::write(other.join("a file.txt"), "unrelated contents").unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(source.clone());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(&source).unwrap());
                app.grid.select_only(Some(0), 1);
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged("cd ../other; cat $selected".into()));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                let output = app.command.output().expect("selected file output");
                assert_eq!(
                    output.detail, "selected contents",
                    "Changing directory retargeted $selected"
                );
                assert!(output.summary.ends_with("exit 0"));
                assert_eq!(
                    app.navigation.current(),
                    if prefix == ":" {
                        other.as_path()
                    } else {
                        source.as_path()
                    }
                );
            }
        });
}

#[test]
fn silent_shell_results_survive_refresh_until_the_next_input() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for (command, code) in [("true", 0), ("false", 1), ("cd target", 0)] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                std_fs::create_dir(&target).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged(command.into()));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                let expected = format!(":{command}  •  exit {code}");
                assert_eq!(
                    app.browser_status_model().text,
                    expected,
                    "Refresh hid the silent shell result"
                );
                assert!(app.command.output().is_none());
                let refresh = app.update(Message::Refresh);
                finish_tasks(&mut app, refresh).await;
                assert_eq!(app.browser_status_model().text, expected);
                let input = app.update(Message::Event(
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    event::Status::Captured,
                ));
                finish_tasks(&mut app, input).await;
                assert_ne!(app.browser_status_model().text, expected);
                assert_eq!(
                    app.navigation.current(),
                    if command == "cd target" {
                        target.as_path()
                    } else {
                        temp.path()
                    }
                );
            }
        });
}

#[test]
fn shell_output_preserves_stdout_from_exit_traps() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                std_fs::create_dir(&target).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, prefix);
                let _ = app.update(Message::CommandChanged(
                    r#"trap 'printf "cleanup\n"; printf "cleanup error\n" >&2' EXIT; cd target; printf "body\n"; false"#.into()
                ));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                let output = app.command.output().expect("shell output");
                assert_eq!(output.detail, "body\ncleanup\n\nstderr:\ncleanup error",
                    "Waddle dropped stdout printed during shell exit");
                assert!(output.summary.ends_with("exit 1"));
                assert_eq!(app.navigation.current(), if prefix == ":" { target.as_path() } else { temp.path() });
            }
        });
}

#[test]
fn shell_exit_status_is_not_overridden_by_a_user_status_variable() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for prefix in [":", "!"] {
                for (command, code) in [
                    ("readonly status=0; printf result; false", 1),
                    ("readonly status=7; printf result; true", 0),
                ] {
                    let temp = tempfile::tempdir().unwrap();
                    let target = temp.path().join("target");
                    std_fs::create_dir(&target).unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    press(&mut app, prefix);
                    let _ = app.update(Message::CommandChanged(format!(
                        "trap 'touch exit-ran' EXIT; cd target; {command}"
                    )));
                    let task = app.update(Message::CommandSubmitted);
                    finish_tasks(&mut app, task).await;
                    let output = app.command.output().expect("shell command result");
                    assert!(
                        output.summary.ends_with(&format!("exit {code}")),
                        "Wrong command status: {}",
                        output.summary
                    );
                    assert_eq!(output.detail, "result", "Waddle changed the command output");
                    assert!(
                        target.join("exit-ran").exists(),
                        "The user's exit trap did not run"
                    );
                    assert_eq!(
                        app.navigation.current(),
                        if prefix == ":" {
                            target.as_path()
                        } else {
                            temp.path()
                        }
                    );
                }
            }
        });
}

#[test]
fn queued_shell_directory_changes_preserve_a_newer_search_session() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recursive in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let original = temp.path().join("original");
                let target = temp.path().join("target");
                std_fs::create_dir(&original).unwrap();
                std_fs::create_dir(&target).unwrap();
                std_fs::write(original.join("needle.txt"), "selected match").unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(original.clone());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(&original).unwrap());
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged("cd ../target".into()));
                let mut stream =
                    iced_runtime::task::into_stream(app.update(Message::CommandSubmitted)).unwrap();
                let mut queued = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        queued.push(message);
                    }
                }
                press(&mut app, "/");
                let search = app.update(Message::SearchChanged(
                    if recursive { "/needle" } else { "needle" }.into(),
                ));
                finish_tasks(&mut app, search).await;
                assert_eq!(app.search.is_recursive(), recursive);
                assert_eq!(app.selected_entries()[0].path, original.join("needle.txt"));
                for message in queued {
                    let task = app.update(message);
                    finish_tasks(&mut app, task).await;
                }
                assert_eq!(
                    app.navigation.current(),
                    original,
                    "An older shell directory result replaced the newer Search session"
                );
                assert_eq!(app.search.is_recursive(), recursive);
                assert_eq!(app.search.query(), "needle");
                assert_eq!(app.selected_entries()[0].path, original.join("needle.txt"));
                let _ = app.begin_command(':');
                let _ = app.update(Message::CommandChanged("cd ../target".into()));
                let mut stream =
                    iced_runtime::task::into_stream(app.update(Message::CommandSubmitted)).unwrap();
                let mut fresh = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        fresh.push(message);
                    }
                }
                let refresh = app.update(Message::Refresh);
                finish_tasks(&mut app, refresh).await;
                for message in fresh {
                    let task = app.update(message);
                    finish_tasks(&mut app, task).await;
                }
                assert_eq!(
                    app.navigation.current(),
                    target,
                    "Refreshing an existing Search session cancelled a fresh directory command"
                );
            }
        });
}

#[test]
fn recursive_search_refresh_preserves_selected_matches_by_path() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            for name in ["bravo.txt", "delta.txt", "omega.txt"] {
                std_fs::write(temp.path().join(name), "fixture").unwrap();
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.sync_location_monitoring();
            press(&mut app, "/");
            let search = app.update(Message::SearchChanged("/txt".into()));
            finish_tasks(&mut app, search).await;
            for (index, modifiers) in [
                (0, keyboard::Modifiers::empty()),
                (2, keyboard::Modifiers::CTRL),
            ] {
                app.modifiers = modifiers;
                let _ = app.update(Message::EntryPressed(index));
                let clicked = app.update(Message::EntryReleased(index));
                finish_tasks(&mut app, clicked).await;
            }
            let selected_paths = |app: &App| {
                app.selected_entries()
                    .into_iter()
                    .map(|entry| entry.path)
                    .collect::<Vec<_>>()
            };
            let selected = vec![temp.path().join("bravo.txt"), temp.path().join("omega.txt")];
            assert_eq!(selected_paths(&app), selected);

            std_fs::write(temp.path().join("alpha.txt"), "new match").unwrap();
            let refresh = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: temp.path().to_path_buf(),
                removed: Vec::new(),
                watch_failed: false,
            }));
            finish_tasks(&mut app, refresh).await;
            assert_eq!(app.navigation.entries().len(), 4);
            assert_eq!(
                selected_paths(&app),
                selected,
                "Refresh changed the selected matches"
            );
            assert_eq!(
                app.navigation.entries()[app.grid.selected_entry().unwrap()].path,
                temp.path().join("omega.txt")
            );
            app.modifiers = keyboard::Modifiers::SHIFT;
            let _ = app.update(Message::EntryPressed(2));
            let clicked = app.update(Message::EntryReleased(2));
            finish_tasks(&mut app, clicked).await;
            assert_eq!(
                selected_paths(&app),
                [temp.path().join("delta.txt"), temp.path().join("omega.txt")],
                "Refresh lost the selection anchor"
            );
        });
}

#[test]
fn opening_a_recursive_search_match_restores_current_folder_contents() {
    use gio::prelude::AppInfoExt;
    use std::os::unix::fs::PermissionsExt;

    const CHILD_ROOT: &str = "WADDLE_SEARCH_OPEN_TEST_ROOT";
    const APPLICATION: &str = "waddle-test-search.desktop";
    let Ok(root) = std::env::var(CHILD_ROOT) else {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let config = temp.path().join("config");
        std_fs::create_dir_all(data.join("applications")).unwrap();
        std_fs::create_dir_all(&config).unwrap();
        let launcher = temp.path().join("record-open");
        std_fs::write(
            &launcher,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$WADDLE_SEARCH_OPEN_TEST_ROOT/opened\"\n",
        )
        .unwrap();
        std_fs::set_permissions(&launcher, std_fs::Permissions::from_mode(0o755)).unwrap();
        std_fs::write(
            data.join("applications").join(APPLICATION),
            format!(
                "[Desktop Entry]\nType=Application\nName=Waddle Test Search\nExec={} %f\nMimeType=text/plain;\n",
                launcher.display()
            ),
        )
        .unwrap();
        std_fs::write(
            data.join("applications/mimeinfo.cache"),
            format!("[MIME Cache]\ntext/plain={APPLICATION};\n"),
        )
        .unwrap();
        std_fs::write(
            config.join("mimeapps.list"),
            format!("[Default Applications]\ntext/plain={APPLICATION};\n"),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::navigation::opening_a_recursive_search_match_restores_current_folder_contents",
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

    assert_eq!(
        gio::AppInfo::default_for_type("text/plain", false)
            .and_then(|application| application.id())
            .as_deref(),
        Some(APPLICATION),
        "the child process must use the isolated test application"
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let root = PathBuf::from(root);
            let folder = root.join("files");
            std_fs::create_dir(&folder).unwrap();
            let matched = folder.join("match.txt");
            let old = folder.join("old.txt");
            let new = folder.join("new.txt");
            std_fs::write(&matched, "matched contents").unwrap();
            std_fs::write(&old, "old contents").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(folder.clone());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(&folder).unwrap());
            app.sync_location_monitoring();
            press(&mut app, "/");
            let search = app.update(Message::SearchChanged("/match".into()));
            finish_tasks(&mut app, search).await;

            std_fs::remove_file(&old).unwrap();
            std_fs::write(&new, "new contents").unwrap();
            let refresh = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: folder.clone(),
                removed: vec![old],
                watch_failed: false,
            }));
            finish_tasks(&mut app, refresh).await;
            assert_eq!(app.navigation.entries().len(), 1);
            assert_eq!(app.navigation.entries()[0].path, matched);
            assert_eq!(app.grid.selected_entry(), Some(0));

            let submitted = app.update(Message::SearchSubmitted);
            finish_tasks(&mut app, submitted).await;
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if std_fs::read_to_string(root.join("opened"))
                        .is_ok_and(|opened| opened.trim() == matched.to_str().unwrap())
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the default application did not receive the matched file");
            assert_eq!(app.browser_input.mode(), InputMode::Browser);
            assert!(!app.search.is_active());
            let paths: Vec<_> = app
                .navigation
                .entries()
                .iter()
                .map(|entry| &entry.path)
                .collect();
            assert_eq!(
                paths,
                [&matched, &new],
                "Opening the match restored stale folder contents"
            );
        });
}

#[test]
fn submitting_an_empty_recursive_search_restores_current_folder_contents() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let old = temp.path().join("old.txt");
            let new = temp.path().join("new.txt");
            std_fs::write(&old, "old contents").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.sync_location_monitoring();
            press(&mut app, "/");
            let search = app.update(Message::SearchChanged("/no-matching-file".into()));
            finish_tasks(&mut app, search).await;
            assert!(app.navigation.entries().is_empty());

            std_fs::remove_file(&old).unwrap();
            std_fs::write(&new, "new contents").unwrap();
            let refresh = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: temp.path().to_path_buf(),
                removed: vec![old],
                watch_failed: false,
            }));
            finish_tasks(&mut app, refresh).await;
            assert!(app.navigation.entries().is_empty());
            assert!(!app.search.is_loading());

            let submitted = app.update(Message::SearchSubmitted);
            finish_tasks(&mut app, submitted).await;
            assert_eq!(app.browser_input.mode(), InputMode::Browser);
            assert!(!app.search.is_active());
            assert_eq!(app.navigation.entries().len(), 1);
            assert_eq!(
                app.navigation.entries()[0].path,
                new,
                "Enter restored the deleted file from the search snapshot"
            );
        });
}

#[test]
fn cancelling_recursive_search_refreshes_changes_made_during_the_search() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            for name in ["keep.txt", "old.txt"] {
                std_fs::write(temp.path().join(name), name).unwrap();
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.sync_location_monitoring();
            app.grid.select_only(Some(0), 2);
            press(&mut app, "/");
            let search = app.update(Message::SearchChanged("/txt".into()));
            finish_tasks(&mut app, search).await;

            let old = temp.path().join("old.txt");
            let new = temp.path().join("new.txt");
            std_fs::remove_file(&old).unwrap();
            std_fs::write(&new, "added during search").unwrap();
            let refresh = app.update(Message::DirectoryChanged(directory_watch::Event {
                path: temp.path().to_path_buf(),
                removed: vec![old.clone()],
                watch_failed: false,
            }));
            finish_tasks(&mut app, refresh).await;
            assert!(
                app.navigation
                    .entries()
                    .iter()
                    .any(|entry| entry.path == new)
            );
            assert!(
                !app.navigation
                    .entries()
                    .iter()
                    .any(|entry| entry.path == old)
            );

            let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
            let cancel = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);
            finish_tasks(&mut app, cancel).await;
            assert!(!app.search.is_active());
            let names: Vec<_> = app
                .navigation
                .entries()
                .iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                names,
                ["keep.txt", "new.txt"],
                "Escape restored stale folder contents"
            );
            assert_eq!(
                app.navigation.entries()[app.grid.selected_entry().unwrap()].path,
                temp.path().join("keep.txt")
            );
        });
}

#[test]
fn native_queue_overflow_rescans_the_displayed_folder() {
    use iced::futures::StreamExt;

    const MARKER: &str = "WADDLE_INOTIFY_OVERFLOW_MARKER";
    let Some(marker) = std::env::var_os(MARKER) else {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("inotify_overflow.c");
        let library = fixture.path().join("inotify_overflow.so");
        std_fs::write(&source, r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/inotify.h>
#include <unistd.h>

ssize_t read(int fd, void *buffer, size_t count) {
    ssize_t (*next_read)(int, void *, size_t) = dlsym(RTLD_NEXT, "read");
    ssize_t received = next_read(fd, buffer, count);
    if (received < (ssize_t)sizeof(struct inotify_event)) return received;
    char fd_path[64], target[64];
    snprintf(fd_path, sizeof(fd_path), "/proc/self/fd/%d", fd);
    ssize_t length = readlink(fd_path, target, sizeof(target) - 1);
    if (length < 0) return received;
    target[length] = 0;
    if (strcmp(target, "anon_inode:inotify") != 0) return received;
    size_t offset = 0;
    while ((size_t)received - offset >= sizeof(struct inotify_event)) {
        struct inotify_event event;
        memcpy(&event, (char *)buffer + offset, sizeof(event));
        if (event.len > (size_t)received - offset - sizeof(event)) break;
        if ((event.mask & IN_MOVE_SELF) != 0 ||
            (event.len >= sizeof("overflow.txt") &&
            memcmp((char *)buffer + offset + sizeof(event), "overflow.txt", sizeof("overflow.txt")) == 0)) {
            const char *marker = getenv("WADDLE_INOTIFY_OVERFLOW_MARKER");
            int proof = open(marker, O_WRONLY | O_CREAT | O_TRUNC, 0600);
            if (proof >= 0) close(proof);
            struct inotify_event overflow = { .wd = -1, .mask = IN_Q_OVERFLOW };
            memcpy(buffer, &overflow, sizeof(overflow));
            return sizeof(overflow);
        }
        offset += sizeof(event) + event.len;
    }
    return received;
}
"#).unwrap();
        let compiled = std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .arg("-ldl")
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::navigation::native_queue_overflow_rescans_the_displayed_folder",
                "--nocapture",
            ])
            .env(MARKER, fixture.path().join("injected"))
            .env("LD_PRELOAD", library)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    };

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let current = temp.path().join("current");
            std_fs::create_dir(&current).unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(current.clone());
            app.navigation.settle_for_test();
            app.sidebar_tree = SidebarTree::new(Vec::new());
            app.sync_location_monitoring();
            let recipe = iced::advanced::subscription::into_recipes(
                app.location_monitoring.as_ref().unwrap().subscription(),
            )
            .pop()
            .unwrap();
            let mut events = recipe.stream(iced::futures::stream::pending().boxed());
            let ready = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    std_fs::write(current.join("ready.txt"), "ready").unwrap();
                    if let Ok(Some(event)) =
                        tokio::time::timeout(Duration::from_millis(300), events.next()).await
                        && event.path == current
                        && !event.watch_failed
                    {
                        return event;
                    }
                }
            })
            .await
            .expect("native watch should become ready");
            let task = app.update(Message::DirectoryChanged(ready));
            finish_tasks(&mut app, task).await;

            let lost = current.join("overflow.txt");
            std_fs::write(&lost, "created while notifications were lost").unwrap();
            let refreshed = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let event = events.next().await.expect("monitor must remain active");
                    let task = app.update(Message::DirectoryChanged(event));
                    finish_tasks(&mut app, task).await;
                    if app
                        .navigation
                        .entries()
                        .iter()
                        .any(|entry| entry.path == lost)
                    {
                        return;
                    }
                }
            })
            .await;
            assert!(
                PathBuf::from(marker).is_file(),
                "overflow fixture must intercept the native event"
            );
            assert!(
                refreshed.is_ok(),
                "queue overflow left the browser's file list stale"
            );

            // The overflow also hides the event that would invalidate an old watch.
            std_fs::rename(&current, temp.path().join("old-current")).unwrap();
            std_fs::create_dir(&current).unwrap();
            for name in ["replacement.txt", "future.txt"] {
                let path = current.join(name);
                std_fs::write(&path, "new directory contents").unwrap();
                let observed = tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        let event = events.next().await.expect("monitor must remain active");
                        let task = app.update(Message::DirectoryChanged(event));
                        finish_tasks(&mut app, task).await;
                        if app
                            .navigation
                            .entries()
                            .iter()
                            .any(|entry| entry.path == path)
                        {
                            return;
                        }
                    }
                })
                .await;
                assert!(
                    observed.is_ok(),
                    "{name} was missed after overflow hid the folder replacement"
                );
            }
        });
}

#[test]
fn failed_native_monitor_startup_keeps_polling_after_each_refresh() {
    use iced::futures::StreamExt;

    const CHILD: &str = "WADDLE_INOTIFY_STARTUP_FAILURE_TEST";
    if std::env::var_os(CHILD).is_none() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("inotify_failure.c");
        let library = fixture.path().join("inotify_failure.so");
        std_fs::write(
            &source,
            "#include <errno.h>\nint inotify_init1(int flags) { (void)flags; errno = EMFILE; return -1; }\n",
        )
        .unwrap();
        let compiled = std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&library)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::navigation::failed_native_monitor_startup_keeps_polling_after_each_refresh",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("LD_PRELOAD", library)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.sidebar_tree = SidebarTree::new(Vec::new());
            app.sync_location_monitoring();
            if let Some(monitoring) = app.location_monitoring.as_ref() {
                let recipe = iced::advanced::subscription::into_recipes(monitoring.subscription())
                    .pop()
                    .unwrap();
                let mut events = recipe.stream(iced::futures::stream::pending().boxed());
                if let Some(event) = tokio::time::timeout(Duration::from_secs(3), events.next())
                    .await
                    .expect("failed monitor must finish initializing")
                {
                    let _ = app.update(Message::DirectoryChanged(event));
                }
            }
            for name in ["first.txt", "second.txt"] {
                let path = temp.path().join(name);
                std_fs::write(&path, "external change").unwrap();
                let task = app.update(Message::PollSystem);
                finish_tasks(&mut app, task).await;
                assert!(
                    app.navigation
                        .entries()
                        .iter()
                        .any(|entry| entry.path == path),
                    "polling failed to display {name} after native monitoring failed"
                );
            }
        });
}

#[test]
fn location_monitoring_refreshes_while_a_file_is_continuously_written() {
    use iced::futures::{StreamExt, stream::BoxStream};

    async fn write_until_notified(
        events: &mut BoxStream<'static, super::directory_watch::Event>,
        path: &Path,
        interval: Duration,
    ) -> super::directory_watch::Event {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                std_fs::write(path, "updated").unwrap();
                if let Ok(Some(event)) = tokio::time::timeout(interval, events.next()).await
                    && event.path == path.parent().unwrap()
                    && !event.watch_failed
                {
                    return event;
                }
            }
        })
        .await
        .expect("ongoing writes must reach the browser without waiting for the writer to stop")
    }

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.sidebar_tree = SidebarTree::new(Vec::new());
            app.sync_location_monitoring();
            let subscription = app.location_monitoring.as_ref().unwrap().subscription();
            let recipe = iced::advanced::subscription::into_recipes(subscription)
                .pop()
                .unwrap();
            let mut events = recipe.stream(iced::futures::stream::pending().boxed());

            // Confirm that the native watch is installed before the sustained writes.
            let event = write_until_notified(
                &mut events,
                &temp.path().join("ready.txt"),
                Duration::from_millis(300),
            )
            .await;
            let task = app.update(Message::DirectoryChanged(event));
            finish_tasks(&mut app, task).await;

            let busy = temp.path().join("busy.txt");
            let event = write_until_notified(&mut events, &busy, Duration::from_millis(10)).await;
            let task = app.update(Message::DirectoryChanged(event));
            finish_tasks(&mut app, task).await;
            assert!(
                app.navigation
                    .entries()
                    .iter()
                    .any(|entry| entry.path == busy)
            );
        });
}

#[test]
fn queued_shell_results_preserve_newer_help_while_refreshing_changed_files() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for command in [
                "printf old-output; touch created.txt",
                "touch created.txt; false",
            ] {
                let temp = tempfile::tempdir().unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, "!");
                let _ = app.update(Message::CommandChanged(command.into()));
                let task = app.update(Message::CommandSubmitted);
                let mut stream = iced_runtime::task::into_stream(task).unwrap();
                let mut queued = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        queued.push(message);
                    }
                }
                assert_eq!(queued.len(), 1);
                assert!(temp.path().join("created.txt").is_file());
                assert!(app.navigation.entries().is_empty());

                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged("help".into()));
                let _ = app.update(Message::CommandSubmitted);
                let help = app.command.output().cloned().unwrap();
                assert!(help.summary.starts_with(":help"));
                for message in queued {
                    let task = app.update(message);
                    finish_tasks(&mut app, task).await;
                }
                assert_eq!(app.command.output(), Some(&help));
                assert!(
                    app.navigation
                        .entries()
                        .iter()
                        .any(|entry| { entry.path == temp.path().join("created.txt") })
                );
            }
        });
}

#[test]
fn refining_search_after_refresh_keeps_its_starting_file_by_path() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            for name in ["bravo.txt", "delta.txt", "omega.txt"] {
                std_fs::write(temp.path().join(name), "fixture").unwrap();
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.grid.select_only(Some(1), 3);
            press(&mut app, "/");
            let task = app.update(Message::SearchChanged("tx".into()));
            finish_tasks(&mut app, task).await;
            let active_path = |app: &App| {
                app.navigation.entries()[app.grid.selected_entry().unwrap()]
                    .path
                    .clone()
            };
            assert_eq!(active_path(&app), temp.path().join("omega.txt"));

            std_fs::write(temp.path().join("alpha.txt"), "inserted").unwrap();
            let task = app.update(Message::Refresh);
            finish_tasks(&mut app, task).await;
            assert_eq!(active_path(&app), temp.path().join("omega.txt"));
            let task = app.update(Message::SearchChanged("txt".into()));
            finish_tasks(&mut app, task).await;
            assert_eq!(active_path(&app), temp.path().join("omega.txt"));
        });
}

#[test]
fn folder_refresh_preserves_the_active_file_and_shift_selection_anchor() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for deselect_active in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                for name in ["bravo", "delta", "omega"] {
                    std_fs::write(temp.path().join(name), "fixture").unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                for (index, modifiers) in [
                    (0, keyboard::Modifiers::empty()),
                    (2, keyboard::Modifiers::CTRL),
                ] {
                    app.modifiers = modifiers;
                    let _ = app.update(Message::EntryPressed(index));
                    let _ = app.update(Message::EntryReleased(index));
                }
                if deselect_active {
                    let _ = app.update(Message::EntryPressed(2));
                    let _ = app.update(Message::EntryReleased(2));
                }
                let selected_paths = |app: &App| {
                    app.selected_entries()
                        .into_iter()
                        .map(|entry| entry.path)
                        .collect::<Vec<_>>()
                };
                let selected = selected_paths(&app);
                std_fs::write(temp.path().join("alpha"), "new").unwrap();
                let task = app.update(Message::Refresh);
                finish_tasks(&mut app, task).await;

                assert_eq!(selected_paths(&app), selected);
                let active = app.grid.selected_entry().unwrap();
                assert_eq!(
                    app.navigation.entries()[active].path,
                    temp.path().join("omega")
                );
                app.modifiers = keyboard::Modifiers::SHIFT;
                let _ = app.update(Message::EntryPressed(2));
                let _ = app.update(Message::EntryReleased(2));
                assert_eq!(
                    selected_paths(&app),
                    [temp.path().join("delta"), temp.path().join("omega")]
                );
            }
        });
}

#[test]
fn a_shell_command_without_cd_does_not_reverse_later_navigation() {
    use iced::futures::StreamExt;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        let next = temp.path().join("next");
        std_fs::create_dir(&original).unwrap();
        std_fs::create_dir(&next).unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(original.clone());
        app.navigation.settle_for_test();
        press(&mut app, ":");
        let _ = app.update(Message::CommandChanged("true".into()));
        let task = app.update(Message::CommandSubmitted);
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let mut queued = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                queued.push(message);
            }
        }
        assert_eq!(queued.len(), 1);

        let _ = app.update(Message::LocationChanged(next.display().to_string()));
        let navigation = app.update(Message::LocationSubmitted);
        finish_tasks(&mut app, navigation).await;
        assert_eq!(app.navigation.current(), next);
        for message in queued {
            let task = app.update(message);
            finish_tasks(&mut app, task).await;
        }
        assert_eq!(app.navigation.current(), next);

        // An explicit directory change in a subsequent command must still work.
        press(&mut app, ":");
        let _ = app.update(Message::CommandChanged("cd ../original".into()));
        let task = app.update(Message::CommandSubmitted);
        finish_tasks(&mut app, task).await;
        assert_eq!(app.navigation.current(), original);
    });
}

#[test]
fn refreshing_recent_and_trash_preserves_selection_by_path_and_scroll() {
    for location in [DisplayedLocation::Recent, DisplayedLocation::Trash] {
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(PathBuf::from("/start"));
        let completion = |request: NavigationRequest, entries: Vec<FileEntry>| match location {
            DisplayedLocation::Recent => Message::RecentLoaded {
                request,
                result: Some(Ok(entries)),
            },
            DisplayedLocation::Trash => Message::TrashLoaded {
                request,
                result: Some(Ok(entries
                    .into_iter()
                    .map(|file| super::trash::Entry {
                        identity: None,
                        receipt: crate::journal::TrashReceipt {
                            original: PathBuf::from("/original").join(&file.name),
                            trashed: file.path.clone(),
                            info: PathBuf::from("/info").join(&file.name),
                        },
                        file,
                    })
                    .collect())),
            },
            DisplayedLocation::Folder => unreachable!(),
        };
        let start = match location {
            DisplayedLocation::Recent => app.navigation.recent(),
            DisplayedLocation::Trash => app.navigation.trash(),
            DisplayedLocation::Folder => unreachable!(),
        };
        let _ = app.update(completion(
            start.request.unwrap(),
            vec![entry("bravo"), entry("delta"), entry("omega")],
        ));
        app.grid.select_click(0, false, false, 3);
        app.grid.select_click(2, true, false, 3);
        app.grid.set_scroll(173.0);

        let work = app.update(Message::Refresh);
        let request = app.navigation.pending_request().expect("refresh request");
        let _ = app.update(completion(
            request,
            vec![
                entry("alpha"),
                entry("bravo"),
                entry("delta"),
                entry("omega"),
            ],
        ));
        let selected = app
            .selected_entries()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        assert_eq!(
            selected,
            [PathBuf::from("/start/bravo"), PathBuf::from("/start/omega")]
        );
        let active = app.grid.selected_entry().unwrap();
        assert_eq!(
            app.navigation.entries()[active].path,
            PathBuf::from("/start/omega")
        );
        assert_eq!(app.grid.scroll_offset(), 173.0);
        drop(work);
    }
}

#[test]
fn location_monitoring_follows_a_replaced_current_folder() {
    use iced::futures::{StreamExt, stream::BoxStream};

    async fn write_until_changed(
        events: &mut BoxStream<'static, super::directory_watch::Event>,
        path: &Path,
    ) -> super::directory_watch::Event {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                std_fs::write(path, "fixture").unwrap();
                if let Ok(Some(event)) =
                    tokio::time::timeout(Duration::from_millis(300), events.next()).await
                    && event.path == path.parent().unwrap()
                    && !event.watch_failed
                {
                    return event;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("changes to {} must reach the browser", path.display()))
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("current");
        std_fs::create_dir(&current).unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(current.clone());
        app.navigation.settle_for_test();
        app.sidebar_tree = SidebarTree::new(Vec::new());
        app.sync_location_monitoring();
        let subscription = app.location_monitoring.as_ref().unwrap().subscription();
        let recipe = iced::advanced::subscription::into_recipes(subscription)
            .pop()
            .unwrap();
        let mut events = recipe.stream(iced::futures::stream::pending().boxed());

        let event = write_until_changed(&mut events, &current.join("before.txt")).await;
        let task = app.update(Message::DirectoryChanged(event));
        finish_tasks(&mut app, task).await;
        assert_eq!(app.navigation.entries()[0].name, "before.txt");

        std_fs::rename(&current, temp.path().join("old-current")).unwrap();
        std_fs::create_dir(&current).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(3), events.next())
            .await
            .unwrap()
            .unwrap();
        let task = app.update(Message::DirectoryChanged(event));
        finish_tasks(&mut app, task).await;
        assert!(app.navigation.entries().is_empty());

        let event = write_until_changed(&mut events, &current.join("after.txt")).await;
        let task = app.update(Message::DirectoryChanged(event));
        finish_tasks(&mut app, task).await;
        assert_eq!(app.navigation.entries()[0].name, "after.txt");
    });
}

#[test]
fn queued_entry_details_cannot_overwrite_newer_details_for_the_same_path() {
    use iced::futures::StreamExt;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("changing.txt");
        std_fs::write(&path, "old").unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.navigation.settle_for_test();
        app.navigation
            .install_folder_entries(fs::read_directory(temp.path()).unwrap());
        app.grid.select_only(Some(0), 1);
        let task = app.update(Message::MetadataFinished {
            request: app.command.output_revision(),
            result: Ok("Permissions changed".into()),
        });
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let mut queued = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                queued.push(message);
            }
        }
        assert_eq!(queued.len(), 1);

        std_fs::write(&path, "new size").unwrap();
        let refresh = app.update(Message::Refresh);
        finish_tasks(&mut app, refresh).await;
        assert!(app.presentation.status().contains("8 B"));
        for message in queued {
            let _ = app.update(message);
        }
        assert!(
            app.presentation.status().contains("8 B"),
            "old metadata replaced the current file details: {}",
            app.presentation.status()
        );
    });
}

#[test]
fn shell_command_completion_preserves_the_displayed_recent_or_trash_location() {
    for location in [DisplayedLocation::Recent, DisplayedLocation::Trash] {
        let temp = tempfile::tempdir().unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.navigation.settle_for_test();
        let initial = match location {
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
        let _ = app.update(initial);
        let report = super::shell::execute(temp.path(), '!', "true", &[]).unwrap();
        let work = app.update(Message::CommandFinished {
            request: app.command.output_revision(),
            navigation_revision: app.navigation.revision(),
            search_session: app.search.session_id(),
            result: Ok(super::command::Completion::Shell(Ok(report))),
        });

        let request = app.navigation.pending_request().expect("refresh request");
        assert_eq!(request.location(), location);
        let completed = match location {
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
        let _ = app.update(completed);
        assert_eq!(app.navigation.displayed_location(), location);
        drop(work);
    }
}

#[test]
fn setting_list_view_keeps_the_displayed_recent_or_trash_location() {
    for location in [DisplayedLocation::Recent, DisplayedLocation::Trash] {
        let temp = tempfile::tempdir().unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.navigation.settle_for_test();
        app.view_preferences =
            super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
        let initial = match location {
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
        let _ = app.update(initial);
        press(&mut app, ":");
        let _ = app.update(Message::CommandChanged("set view=list".to_owned()));
        let work = app.update(Message::CommandSubmitted);
        if let Some(request) = app.navigation.pending_request() {
            let completed = match request.location() {
                DisplayedLocation::Folder => Message::NavigationFinished {
                    request,
                    result: Ok(opened(temp.path().to_path_buf(), Vec::new())),
                },
                DisplayedLocation::Recent => Message::RecentLoaded {
                    request,
                    result: Some(Ok(Vec::new())),
                },
                DisplayedLocation::Trash => Message::TrashLoaded {
                    request,
                    result: Some(Ok(Vec::new())),
                },
            };
            let _ = app.update(completed);
        }
        assert_eq!(
            app.navigation.displayed_location(),
            location,
            "changing the view must not navigate to the previous folder"
        );
        assert_eq!(
            app.view_preferences.for_directory(temp.path()).view,
            fs::ViewMode::List
        );
        drop(work);
    }
}

#[test]
fn refresh_command_reloads_the_displayed_recent_or_trash_location() {
    for location in [DisplayedLocation::Recent, DisplayedLocation::Trash] {
        let temp = tempfile::tempdir().unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.navigation.settle_for_test();
        let opened = match location {
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
        let _ = app.update(opened);
        press(&mut app, ":");
        let _ = app.update(Message::CommandChanged("refresh".to_owned()));
        let work = app.update(Message::CommandSubmitted);

        let request = app.navigation.pending_request().expect("refresh request");
        assert_eq!(
            request.location(),
            location,
            ":refresh must reload the displayed view, not the previous folder"
        );
        let completed = match location {
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
        let _ = app.update(completed);
        assert_eq!(app.navigation.displayed_location(), location);
        assert!(!app.navigation.loading());
        drop(work);
    }
}

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
fn the_latest_sidebar_volume_choice_wins_regardless_of_mount_order() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for order in [[0, 1], [1, 0]] {
                let temp = tempfile::tempdir().unwrap();
                let paths = [temp.path().join("first"), temp.path().join("second")];
                for path in &paths {
                    std_fs::create_dir(path).unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                let volumes = ["First volume", "Second volume"].map(|label| VolumeRoot {
                    id: format!("uuid:{label}"),
                    path: None,
                    label: label.into(),
                    can_unmount: false,
                });
                app.sidebar_tree = SidebarTree::new(volumes.to_vec());
                let mut completions = Vec::new();
                for volume in &volumes {
                    let row = app
                        .sidebar_tree
                        .rows(temp.path())
                        .into_iter()
                        .find(|row| row.label == volume.label)
                        .unwrap();
                    // Simulate desktop mount reports through app messages; no real device is mounted.
                    drop(app.update(Message::TreeRow(row.id)));
                    completions.push(Some(Message::TreeVolumeMounted {
                        navigation_revision: app.navigation.revision(),
                        id: volume.id.clone(),
                        result: Ok(places::MountedVolume {
                            label: volume.label.clone(),
                        }),
                    }));
                }
                app.sidebar_tree.reconcile_volumes(
                    volumes
                        .iter()
                        .zip(&paths)
                        .map(|(volume, path)| VolumeRoot {
                            path: Some(path.clone()),
                            can_unmount: true,
                            ..volume.clone()
                        })
                        .collect(),
                );
                let mounting_status = app.presentation.status().to_owned();
                for index in order {
                    let task = app.update(completions[index].take().unwrap());
                    finish_tasks(&mut app, task).await;
                    if index == 0 && order[0] == 0 {
                        assert_eq!(
                            app.navigation.current(),
                            temp.path(),
                            "The earlier volume opened while the newer mount was still pending"
                        );
                        assert_eq!(app.presentation.status(), mounting_status);
                    }
                }
                assert_eq!(
                    app.navigation.current(),
                    paths[1],
                    "completion order: {order:?}"
                );
                let task = app.update(Message::Back);
                finish_tasks(&mut app, task).await;
                assert_eq!(
                    app.navigation.current(),
                    temp.path(),
                    "The abandoned mount must not enter history"
                );
            }
        });
}

#[test]
fn a_sidebar_volume_choice_supersedes_an_already_queued_folder_result() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("original");
            let mounted = temp.path().join("mounted");
            std_fs::create_dir(&original).unwrap();
            std_fs::create_dir(&mounted).unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(original.clone());
            app.navigation.settle_for_test();
            let volume = VolumeRoot {
                id: "uuid:queued-folder-test".into(),
                path: None,
                label: "Chosen volume".into(),
                can_unmount: false,
            };
            app.sidebar_tree = SidebarTree::new(vec![volume.clone()]);
            let task = app.update(Message::Parent);
            let mut stream = iced_runtime::task::into_stream(task).unwrap();
            let mut queued = Vec::new();
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    queued.push(message);
                }
            }
            assert!(!queued.is_empty());
            let row = app
                .sidebar_tree
                .rows(&original)
                .into_iter()
                .find(|row| row.label == volume.label)
                .unwrap();
            drop(app.update(Message::TreeRow(row.id)));
            let navigation_revision = app.navigation.revision();
            let status = app.presentation.status().to_owned();
            for message in queued {
                let task = app.update(message);
                finish_tasks(&mut app, task).await;
            }
            assert_eq!(
                app.navigation.current(),
                original,
                "An older folder result replaced the user's volume choice"
            );
            assert_eq!(app.presentation.status(), status);
            app.sidebar_tree.reconcile_volumes(vec![VolumeRoot {
                path: Some(mounted.clone()),
                can_unmount: true,
                ..volume.clone()
            }]);
            let task = app.update(Message::TreeVolumeMounted {
                navigation_revision,
                id: volume.id,
                result: Ok(places::MountedVolume {
                    label: volume.label,
                }),
            });
            finish_tasks(&mut app, task).await;
            assert_eq!(app.navigation.current(), mounted);
            let task = app.update(Message::Back);
            finish_tasks(&mut app, task).await;
            assert_eq!(app.navigation.current(), original);
        });
}

#[test]
fn delayed_volume_mounts_preserve_newer_navigation() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for scenario in ["settled", "pending", "returned", "refresh"] {
                let temp = tempfile::tempdir().unwrap();
                let original = temp.path().join("original");
                let mounted = temp.path().join("mounted");
                std_fs::create_dir(&original).unwrap();
                std_fs::create_dir(&mounted).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(original.clone());
                app.navigation.settle_for_test();
                let volume = VolumeRoot {
                    id: "uuid:delayed-test".into(),
                    path: None,
                    label: "Delayed volume".into(),
                    can_unmount: false,
                };
                app.sidebar_tree = SidebarTree::new(vec![volume.clone()]);
                let row = app
                    .sidebar_tree
                    .rows(&original)
                    .into_iter()
                    .find(|row| row.label == volume.label)
                    .unwrap();
                // Supply the desktop's mount completion below; do not mount a real device.
                drop(app.update(Message::TreeRow(row.id)));
                let navigation_revision = app.navigation.revision();
                let navigation = app.update(if scenario == "refresh" {
                    Message::Refresh
                } else {
                    Message::Parent
                });
                if scenario != "pending" {
                    finish_tasks(&mut app, navigation).await;
                }
                if scenario == "returned" {
                    let back = app.update(Message::Back);
                    finish_tasks(&mut app, back).await;
                }
                let status = app.presentation.status().to_owned();
                app.sidebar_tree.reconcile_volumes(vec![VolumeRoot {
                    path: Some(mounted.clone()),
                    can_unmount: true,
                    ..volume.clone()
                }]);
                let task = app.update(Message::TreeVolumeMounted {
                    navigation_revision,
                    id: volume.id,
                    result: Ok(places::MountedVolume {
                        label: volume.label,
                    }),
                });
                finish_tasks(&mut app, task).await;
                let expected = match scenario {
                    "settled" => temp.path(),
                    "refresh" => mounted.as_path(),
                    _ => original.as_path(),
                };
                assert_eq!(app.navigation.current(), expected, "scenario: {scenario}");
                if scenario == "pending" {
                    assert_eq!(app.navigation.pending_path(), Some(temp.path()));
                }
                if scenario != "refresh" {
                    assert_eq!(app.presentation.status(), status, "scenario: {scenario}");
                }
            }
        });
}

#[test]
fn deferred_volume_mount_timeouts_preserve_newer_navigation_feedback() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for navigate_away in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let original = temp.path().join("original");
                std_fs::create_dir(&original).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(original.clone());
                app.navigation.settle_for_test();
                app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
                    id: "uuid:missing-mount-path".into(),
                    path: None,
                    label: "Missing mount path".into(),
                    can_unmount: false,
                }]);
                let row = app.sidebar_tree.rows(&original).into_iter()
                    .find(|row| row.label == "Missing mount path").unwrap();
                drop(app.update(Message::TreeRow(row.id)));
                let navigation_revision = app.navigation.revision();
                drop(app.update(Message::TreeVolumeMounted {
                    navigation_revision,
                    id: "uuid:missing-mount-path".into(),
                    result: Ok(places::MountedVolume { label: "Missing mount path".into() }),
                }));
                if navigate_away {
                    let task = app.update(Message::Parent);
                    finish_tasks(&mut app, task).await;
                }
                let status = app.presentation.status().to_owned();
                // Arrange expiry of the desktop's missing-path deadline without a 10-second sleep.
                app.pending_volume_navigation.as_mut().unwrap().deadline = Instant::now();
                let task = app.update(Message::PollSystem);
                finish_tasks(&mut app, task).await;
                assert_eq!(app.navigation.current(), if navigate_away { temp.path() } else { &original });
                if navigate_away {
                    assert_eq!(app.presentation.status(), status,
                        "An abandoned volume navigation replaced newer feedback when its path timed out");
                } else {
                    assert_eq!(app.presentation.status(), "Mounted Missing mount path, but its folder is unavailable");
                }
            }
        });
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
        app.navigation.revision(),
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
fn unmount_completion_preserves_navigation_away_from_the_volume() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for into_volume in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let volume_path = temp.path().join("volume");
                let original = if into_volume {
                    temp.path().join("original")
                } else {
                    volume_path.join("original")
                };
                let destination = if into_volume {
                    volume_path.join("destination")
                } else {
                    temp.path().join("destination")
                };
                std_fs::create_dir_all(&original).unwrap();
                std_fs::create_dir_all(&destination).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(original);
                app.navigation.settle_for_test();
                app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
                    id: "uuid:unmount-navigation-test".into(),
                    path: Some(volume_path.clone()),
                    label: "Leaving volume".into(),
                    can_unmount: true,
                }]);
                // The desktop completion is supplied below without unmounting a real device.
                drop(app.update(Message::TreeVolumeUnmount(
                    "uuid:unmount-navigation-test".into(),
                )));
                drop(app.update(Message::LocationChanged(destination.display().to_string())));
                let navigation = app.update(Message::LocationSubmitted);
                let completion = app.update(Message::TreeVolumeUnmounted {
                    id: "uuid:unmount-navigation-test".into(),
                    label: "Leaving volume".into(),
                    path: volume_path.clone(),
                    result: Ok(()),
                });
                if into_volume {
                    assert!(
                        !app.navigation
                            .pending_path()
                            .unwrap()
                            .starts_with(&volume_path),
                        "Navigation into the unmounted volume must fall back to a safe folder"
                    );
                } else {
                    assert_eq!(
                        app.navigation.pending_path(),
                        Some(destination.as_path()),
                        "The unmount replaced a newer navigation out of the volume"
                    );
                }
                finish_tasks(&mut app, completion).await;
                finish_tasks(&mut app, navigation).await;
                if into_volume {
                    assert!(!app.navigation.current().starts_with(&volume_path));
                } else {
                    assert_eq!(app.navigation.current(), destination);
                }
            }
        });
}

#[test]
fn unmount_completion_preserves_pending_and_open_collection_views() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for recent in [true, false] {
                for pending in [true, false] {
                    let temp = tempfile::tempdir().unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
                        id: "uuid:collection-unmount".into(),
                        path: Some(temp.path().to_path_buf()),
                        label: "Collection volume".into(),
                        can_unmount: true,
                    }]);
                    drop(app.update(Message::TreeVolumeUnmount("uuid:collection-unmount".into())));
                    let request = if recent {
                        app.navigation.recent()
                    } else {
                        app.navigation.trash()
                    }
                    .request
                    .unwrap();
                    let mut loaded = Some(if recent {
                        Message::RecentLoaded {
                            request,
                            result: Some(Ok(Vec::new())),
                        }
                    } else {
                        Message::TrashLoaded {
                            request,
                            result: Some(Ok(Vec::new())),
                        }
                    });
                    if !pending {
                        let task = app.update(loaded.take().unwrap());
                        finish_tasks(&mut app, task).await;
                    }
                    let task = app.update(Message::TreeVolumeUnmounted {
                        id: "uuid:collection-unmount".into(),
                        label: "Collection volume".into(),
                        path: temp.path().to_path_buf(),
                        result: Ok(()),
                    });
                    finish_tasks(&mut app, task).await;
                    if let Some(message) = loaded {
                        let task = app.update(message);
                        finish_tasks(&mut app, task).await;
                    }
                    assert_eq!(
                        app.navigation.displayed_location(),
                        if recent {
                            DisplayedLocation::Recent
                        } else {
                            DisplayedLocation::Trash
                        },
                        "recent={recent}, pending={pending}"
                    );
                }
            }
        });
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
fn recursive_search_ignores_a_completed_result_queued_before_the_query_changed() {
    use iced::futures::StreamExt;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        std_fs::write(temp.path().join("old-match.txt"), "old").unwrap();
        std_fs::write(temp.path().join("new-match.txt"), "new").unwrap();
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(temp.path().to_path_buf());
        app.navigation.settle_for_test();
        app.view_preferences =
            super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
        press(&mut app, "/");
        let old_task = app.update(Message::SearchChanged("/old-match".to_owned()));
        let mut stream = iced_runtime::task::into_stream(old_task).unwrap();
        let mut queued = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(message) = action {
                queued.push(message);
            }
        }
        assert_eq!(queued.len(), 1);

        let current = app.update(Message::SearchChanged("new-match".to_owned()));
        for message in queued {
            let _ = app.update(message);
        }
        assert!(
            app.search.is_loading(),
            "a queued result for the old query must not finish the current search"
        );
        assert!(app.navigation.entries().is_empty());
        finish_tasks(&mut app, current).await;
        assert!(!app.search.is_loading());
        assert_eq!(app.navigation.entries().len(), 1);
        assert_eq!(app.navigation.entries()[0].name, "new-match.txt");
    });
}

#[test]
fn cancelling_search_after_refresh_restores_surviving_selected_paths() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for remove_selected in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            for name in ["bravo", "delta", "omega"] {
                std_fs::write(temp.path().join(name), name).unwrap();
            }
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.view_preferences =
                super::view_preferences::Preferences::empty_at(temp.path().join("waddlerc"));
            app.grid.select_click(0, false, false, 3);
            app.grid.select_click(2, true, false, 3);
            press(&mut app, "/");
            let _ = app.update(Message::SearchChanged("delta".to_owned()));
            std_fs::write(temp.path().join("alpha"), "new file").unwrap();
            if remove_selected {
                std_fs::remove_file(temp.path().join("bravo")).unwrap();
            }
            let refresh = app.update(Message::Refresh);
            finish_tasks(&mut app, refresh).await;
            let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
            let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

            let selected = app
                .grid
                .selected_items(app.navigation.entries())
                .into_iter()
                .map(|entry| entry.name.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let expected = if remove_selected {
                vec!["omega"]
            } else {
                vec!["bravo", "omega"]
            };
            assert_eq!(selected, expected, "Escape must restore files by path");
            assert_eq!(
                app.navigation.entries()[app.grid.selected_entry().unwrap()].name,
                "omega"
            );
        }
    });
}

#[test]
fn cancelling_search_restores_the_full_selection_and_active_entry() {
    for query in ["two", "/two"] {
        let (mut app, _) = App::new();
        app.navigation = NavigationSession::new(PathBuf::from("/start"));
        app.navigation
            .install_folder_entries(vec![entry("one"), entry("two"), entry("three")]);
        app.grid.select_click(0, false, false, 3);
        app.grid.select_click(2, true, false, 3);
        press(&mut app, "/");
        let _ = app.update(Message::SearchChanged(query.to_owned()));
        let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
        let _ = app.handle_key(escape.clone(), escape, keyboard::Modifiers::empty(), None);

        assert_eq!(app.browser_input.mode(), InputMode::Browser);
        assert_eq!(app.grid.selected_entry(), Some(2));
        let selected = app
            .grid
            .selected_items(app.navigation.entries())
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        assert_eq!(
            selected,
            ["one", "three"],
            "cancelling {query:?} must restore all previously selected files"
        );
    }
}

#[test]
fn captured_escape_leaves_the_recursive_search_input() {
    let (mut app, _) = App::new();
    app.browser_input.enter(InputMode::Search);
    app.search.begin(&app.navigation, &app.grid);
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
            identity: None,
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

#[test]
fn queued_shell_directory_changes_do_not_override_newer_navigation() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for scenario in ["settled", "pending", "returned", "refresh"] {
                let temp = tempfile::tempdir().unwrap();
                let original = temp.path().join("original");
                let destination = temp.path().join("command-target");
                std_fs::create_dir(&original).unwrap();
                std_fs::create_dir(&destination).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(original.clone());
                app.navigation.settle_for_test();
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged(
                    "cd ../command-target; touch created.txt".into(),
                ));
                let mut stream =
                    iced_runtime::task::into_stream(app.update(Message::CommandSubmitted)).unwrap();
                let mut queued = Vec::new();
                while let Some(action) = stream.next().await {
                    if let iced_runtime::Action::Output(message) = action {
                        queued.push(message);
                    }
                }
                let navigation = app.update(if scenario == "refresh" {
                    Message::Refresh
                } else {
                    Message::Parent
                });
                if scenario != "pending" {
                    finish_tasks(&mut app, navigation).await;
                }
                if scenario == "returned" {
                    let back = app.update(Message::Back);
                    finish_tasks(&mut app, back).await;
                    assert_eq!(app.navigation.current(), original);
                }
                for message in queued {
                    let task = app.update(message);
                    finish_tasks(&mut app, task).await;
                }
                let expected = match scenario {
                    "returned" => original.as_path(),
                    "refresh" => destination.as_path(),
                    "pending" => {
                        assert_eq!(
                            app.navigation.pending_request().unwrap().requested(),
                            Some(temp.path())
                        );
                        original.as_path()
                    }
                    _ => temp.path(),
                };
                assert_eq!(app.navigation.current(), expected, "scenario: {scenario}");
                assert!(destination.join("created.txt").exists());
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged(format!(
                    "cd {}",
                    destination.display()
                )));
                let task = app.update(Message::CommandSubmitted);
                finish_tasks(&mut app, task).await;
                assert_eq!(app.navigation.current(), destination);
            }
        });
}
