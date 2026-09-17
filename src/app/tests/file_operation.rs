use super::*;

#[test]
fn queued_trash_preserves_replaced_source_items() {
    const CHILD: &str = "WADDLE_QUEUED_TRASH_ROOT";
    let Some(root) = std::env::var_os(CHILD) else {
        let temp = tempfile::Builder::new()
            .prefix("waddle-queued-trash-test-")
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap();
        std_fs::create_dir_all(temp.path().join("data/Trash/files")).unwrap();
        std_fs::create_dir_all(temp.path().join("data/Trash/info")).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::file_operation::queued_trash_preserves_replaced_source_items",
                "--nocapture",
            ])
            .env(CHILD, temp.path())
            .env("XDG_DATA_HOME", temp.path().join("data"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("XDG_STATE_HOME", temp.path().join("state"))
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
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let root = PathBuf::from(root);
            for kind in ["file", "directory", "symlink"] {
                let folder = root.join(kind);
                std_fs::create_dir(&folder).unwrap();
                let source = folder.join("source");
                let retained = root.join(format!("retained-{kind}"));
                let unchanged = folder.join("unchanged.txt");
                let target = root.join(format!("target-{kind}"));
                match kind {
                    "file" => std_fs::write(&source, b"original item").unwrap(),
                    "directory" => {
                        std_fs::create_dir(&source).unwrap();
                        std_fs::write(source.join("child"), b"original item").unwrap();
                    }
                    _ => {
                        std_fs::write(&target, b"link target").unwrap();
                        std::os::unix::fs::symlink(&target, &source).unwrap();
                    }
                }
                std_fs::write(&unchanged, b"trash this selected item").unwrap();
                let (mut app, _) = App::new();
                app.trash = trash::Trash::at(root.join("data/Trash"));
                app.navigation = NavigationSession::new(folder.clone());
                app.navigation.settle_for_test();
                app.navigation.install_folder_entries(fs::read_directory(&folder).unwrap());
                app.grid.select_click(0, false, false, 2);
                app.grid.select_click(1, true, false, 2);
                let task = app.update(Message::ContextTrash);
                std_fs::rename(&source, &retained).unwrap();
                match kind {
                    "file" => std_fs::write(&source, b"replacement item").unwrap(),
                    "directory" => {
                        std_fs::create_dir(&source).unwrap();
                        std_fs::write(source.join("child"), b"replacement item").unwrap();
                    }
                    // A new symlink to the same target is still a replacement.
                    _ => std::os::unix::fs::symlink(&target, &source).unwrap(),
                }
                // Editing the same inode is allowed; replacing it is not.
                std_fs::write(&unchanged, b"updated selected contents").unwrap();
                navigation::finish_tasks(&mut app, task).await;
                assert!(source.exists(), "Queued Trash moved a replacement {kind}");
                match kind {
                    "file" => {
                        assert_eq!(std_fs::read(&source).unwrap(), b"replacement item");
                        assert_eq!(std_fs::read(&retained).unwrap(), b"original item");
                    }
                    "directory" => {
                        assert_eq!(std_fs::read(source.join("child")).unwrap(), b"replacement item");
                        assert_eq!(std_fs::read(retained.join("child")).unwrap(), b"original item");
                    }
                    _ => {
                        assert_eq!(std_fs::read_link(&source).unwrap(), target);
                        assert_eq!(std_fs::read_link(&retained).unwrap(), target);
                        assert_eq!(std_fs::read(&target).unwrap(), b"link target");
                    }
                }
                assert!(!unchanged.exists(), "The same selected inode should still be trashed");
                let trashed = app.trash.entries().unwrap().into_iter()
                    .find(|entry| entry.receipt.original == unchanged).unwrap();
                assert_eq!(std_fs::read(trashed.receipt.trashed).unwrap(), b"updated selected contents");
                assert!(matches!(app.file_operations.view(), FileOperationView::PermanentDelete { detail, .. } if detail.contains("changed")));
            }
        });
}

#[test]
fn queued_volume_command_feedback_preserves_newer_folder_navigation() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for scenario in ["pending", "settled", "returned", "refresh", "current"] {
                for succeeds in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    let original = temp.path().join("original");
                    std_fs::create_dir(&original).unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(original.clone());
                    app.navigation.settle_for_test();
                    press(&mut app, ":");
                    drop(app.update(Message::CommandChanged(
                        "volume invalid-action unused".into(),
                    )));
                    let task = app.update(Message::CommandSubmitted);
                    let mut stream = iced_runtime::task::into_stream(task).unwrap();
                    let mut queued = Vec::new();
                    while let Some(action) = stream.next().await {
                        if let iced_runtime::Action::Output(mut message) = action {
                            if succeeds {
                                // Supply desktop success without changing a real volume.
                                let Message::VolumeFinished { result, .. } = &mut message else {
                                    panic!("expected a volume command completion");
                                };
                                *result = Ok("Volume action completed".into());
                            }
                            queued.push(message);
                        }
                    }
                    assert!(!queued.is_empty());
                    let navigation = match scenario {
                        "current" => None,
                        "refresh" => Some(app.update(Message::Refresh)),
                        _ => Some(app.update(Message::Parent)),
                    };
                    let mut pending = None;
                    if let Some(task) = navigation {
                        if scenario == "pending" {
                            pending = Some(task);
                        } else {
                            navigation::finish_tasks(&mut app, task).await;
                            if scenario == "returned" {
                                let task = app.update(Message::Back);
                                navigation::finish_tasks(&mut app, task).await;
                            }
                        }
                    }
                    app.sidebar_tree = SidebarTree::new(vec![VolumeRoot {
                        id: "uuid:volume-command-feedback-test".into(),
                        path: None,
                        label: "Obsolete test volume".into(),
                        can_unmount: false,
                    }]);
                    let status = app.presentation.status().to_owned();
                    for message in queued {
                        let task = app.update(message);
                        navigation::finish_tasks(&mut app, task).await;
                    }
                    let current = matches!(scenario, "current" | "refresh");
                    assert_eq!(
                        app.presentation.status(),
                        if !current {
                            status.as_str()
                        } else if succeeds {
                            "Volume action completed"
                        } else {
                            "unknown volume action: invalid-action"
                        },
                        "scenario={scenario}, succeeds={succeeds}"
                    );
                    if succeeds {
                        assert!(
                            !app.sidebar_tree
                                .rows(app.navigation.current())
                                .iter()
                                .any(|row| row.label == "Obsolete test volume"),
                            "Success must refresh volumes even when its feedback is obsolete"
                        );
                    }
                    if let Some(task) = pending {
                        navigation::finish_tasks(&mut app, task).await;
                    }
                    assert_eq!(
                        app.navigation.current(),
                        if matches!(scenario, "pending" | "settled") {
                            temp.path()
                        } else {
                            original.as_path()
                        }
                    );
                }
            }
        });
}

#[test]
fn queued_volume_errors_preserve_newer_command_feedback() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(
                "volume invalid-action unused".into(),
            ));
            let task = app.update(Message::CommandSubmitted);
            let mut stream = iced_runtime::task::into_stream(task).unwrap();
            let mut queued = Vec::new();
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    queued.push(message);
                }
            }
            assert!(!queued.is_empty());
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged("set view=list".into()));
            let task = app.update(Message::CommandSubmitted);
            navigation::finish_tasks(&mut app, task).await;
            let status = app.presentation.status().to_owned();
            for message in queued {
                let task = app.update(message);
                navigation::finish_tasks(&mut app, task).await;
            }
            assert_eq!(
                app.presentation.status(),
                status,
                "An obsolete volume result replaced the newer command's feedback"
            );
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(
                "volume current-invalid unused".into(),
            ));
            let task = app.update(Message::CommandSubmitted);
            navigation::finish_tasks(&mut app, task).await;
            assert_eq!(
                app.presentation.status(),
                "unknown volume action: current-invalid",
                "A current volume error must still be displayed"
            );
        });
}

#[test]
fn built_in_commands_accept_tabs_before_their_arguments() {
    for (command, expected) in [
        ("favorite", "unknown favorite command: invalid-subcommand"),
        ("recent", "unknown Recent command: invalid-subcommand"),
        ("volume", "Waiting for desktop volume authorization…"),
        ("chmod", "Select entries or pass paths to :chmod"),
    ] {
        for separator in [" ", "\t", "\t "] {
            let temp = tempfile::tempdir().unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            press(&mut app, ":");
            let _ = app.update(Message::CommandChanged(format!(
                "{command}{separator}invalid-subcommand"
            )));
            // Inspect immediate dispatch without running a volume or shell worker.
            let task = app.update(Message::CommandSubmitted);
            assert!(
                app.presentation.status().starts_with(expected),
                "{command} with {separator:?} was dispatched incorrectly: {}",
                app.presentation.status()
            );
            drop(task);
        }
        for (prefix, text) in [
            ("!", format!("{command}\tinvalid-subcommand")),
            (":", format!("{command}-external\tinvalid-subcommand")),
        ] {
            let (mut app, _) = App::new();
            app.navigation.settle_for_test();
            press(&mut app, prefix);
            let _ = app.update(Message::CommandChanged(text));
            let task = app.update(Message::CommandSubmitted);
            assert!(app.presentation.status().starts_with("Running "));
            drop(task);
        }
    }
}

#[test]
fn tab_separated_chmod_uses_selection_and_quoted_explicit_paths() {
    use std::os::unix::fs::PermissionsExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for explicit in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target file.txt");
                let other = temp.path().join("other.txt");
                for path in [&target, &other] {
                    std_fs::write(path, "contents").unwrap();
                    std_fs::set_permissions(path, std_fs::Permissions::from_mode(0o700)).unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                let selected = app
                    .navigation
                    .entries()
                    .iter()
                    .position(|entry| {
                        entry.path == if explicit { &other } else { &target }.as_path()
                    })
                    .unwrap();
                app.grid.select_only(Some(selected), 2);
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged(if explicit {
                    "chmod\t640\t\"target file.txt\"".into()
                } else {
                    "chmod\t640".into()
                }));
                let task = app.update(Message::CommandSubmitted);
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(
                    std_fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
                    0o640
                );
                assert_eq!(
                    std_fs::metadata(&other).unwrap().permissions().mode() & 0o7777,
                    0o700
                );
            }
        });
}

#[test]
#[cfg(target_os = "linux")]
fn permission_changes_preserve_items_replaced_during_chmod() {
    use std::os::unix::fs::PermissionsExt;

    const CHILD_ROOT: &str = "WADDLE_CHMOD_RACE_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(PathBuf::from(root));
                app.navigation.settle_for_test();
                press(&mut app, ":");
                let input = std::env::var("WADDLE_CHMOD_INPUT").unwrap();
                let _ = app.update(Message::CommandChanged(format!("chmod 755 {input}")));
                let task = app.update(Message::CommandSubmitted);
                navigation::finish_tasks(&mut app, task).await;
            });
        return;
    }

    let fixture = tempfile::tempdir().unwrap();
    let source = fixture.path().join("chmod_race.c");
    let library = fixture.path().join("chmod_race.so");
    std_fs::write(
        &source,
        r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int chmod(const char *path, mode_t mode) {
    int (*next_chmod)(const char *, mode_t) = dlsym(RTLD_NEXT, "chmod");
    const char *target = getenv("WADDLE_CHMOD_TARGET");
    const char *replacement = getenv("WADDLE_CHMOD_REPLACEMENT");
    const char *retained = getenv("WADDLE_CHMOD_RETAINED");
    const char *armed = getenv("WADDLE_CHMOD_ARMED");
    char resolved[PATH_MAX];
    if (target && replacement && retained && armed &&
        realpath(path, resolved) && !strcmp(resolved, target) && !unlink(armed)) {
        if (rename(target, retained) || rename(replacement, target)) return -1;
    }
    return next_chmod(path, mode);
}
"#,
    )
    .unwrap();
    let compiled = std::process::Command::new("cc")
        .args(["-shared", "-fPIC", "-o"])
        .arg(&library)
        .arg(&source)
        .arg("-ldl")
        .output()
        .unwrap();
    assert!(compiled.status.success(), "{compiled:?}");
    for directory in [false, true] {
        for symlink in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let target = temp.path().join("target");
            let replacement = temp.path().join("replacement");
            let retained = temp.path().join("retained");
            let armed = temp.path().join("armed");
            for (path, contents) in [(&target, "original"), (&replacement, "replacement")] {
                if directory {
                    std_fs::create_dir(path).unwrap();
                } else {
                    std_fs::write(path, contents).unwrap();
                }
                std_fs::set_permissions(path, std_fs::Permissions::from_mode(0o700)).unwrap();
            }
            if symlink {
                std::os::unix::fs::symlink(&target, temp.path().join("link")).unwrap();
            }
            std_fs::write(&armed, "").unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::tests::file_operation::permission_changes_preserve_items_replaced_during_chmod", "--nocapture"])
                .env("LD_PRELOAD", &library)
                .env(CHILD_ROOT, temp.path())
                .env("WADDLE_CHMOD_INPUT", if symlink { "link" } else { "target" })
                .env("WADDLE_CHMOD_TARGET", &target)
                .env("WADDLE_CHMOD_REPLACEMENT", &replacement)
                .env("WADDLE_CHMOD_RETAINED", &retained)
                .env("WADDLE_CHMOD_ARMED", &armed)
                .output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(!armed.exists(), "The replacement race must be reached");
            assert_eq!(
                std_fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
                0o700,
                "chmod changed permissions on the replacement item"
            );
            assert_eq!(
                std_fs::metadata(&retained).unwrap().permissions().mode() & 0o7777,
                0o755
            );
            if !directory {
                assert_eq!(std_fs::read_to_string(&target).unwrap(), "replacement");
                assert_eq!(std_fs::read_to_string(&retained).unwrap(), "original");
            }
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn permission_changes_support_unreadable_items_and_fifos() {
    use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};

    for kind in ["file", "directory", "fifo"] {
        for symlink in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let target = temp.path().join("target");
            match kind {
                "file" => std_fs::write(&target, "contents").unwrap(),
                "directory" => std_fs::create_dir(&target).unwrap(),
                "fifo" => {
                    let path = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
                    // SAFETY: path is a valid NUL-terminated fixture pathname.
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                }
                _ => unreachable!(),
            }
            let mode = if kind == "fifo" { 0o600 } else { 0 };
            std_fs::set_permissions(&target, std_fs::Permissions::from_mode(mode)).unwrap();
            if symlink {
                std::os::unix::fs::symlink(&target, temp.path().join("link")).unwrap();
            }
            // A normal read-open would block forever on the FIFO. Bound the child.
            let output = std::process::Command::new("timeout")
                .arg("10")
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "app::tests::file_operation::permission_changes_preserve_items_replaced_during_chmod", "--nocapture"])
                .env("WADDLE_CHMOD_RACE_ROOT", temp.path())
                .env("WADDLE_CHMOD_INPUT", if symlink { "link" } else { "target" })
                .output().unwrap();
            assert!(output.status.success(), "{kind}: {output:?}");
            assert_eq!(
                std_fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
                0o755,
                "Could not change permissions on {kind}"
            );
            if kind == "file" {
                assert_eq!(std_fs::read_to_string(&target).unwrap(), "contents");
            }
        }
    }
}

#[test]
fn queued_permission_changes_verify_selected_symlink_targets() {
    use std::os::unix::fs::PermissionsExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for directory in [false, true] {
                for change in ["unchanged", "replaced", "created"] {
                    let temp = tempfile::tempdir().unwrap();
                    let target = temp.path().join("target");
                    let retained = temp.path().join("retained");
                    let link = temp.path().join("link");
                    let create_target = || {
                        if directory {
                            std_fs::create_dir(&target).unwrap();
                        } else {
                            std_fs::write(&target, "contents").unwrap();
                        }
                        std_fs::set_permissions(&target, std_fs::Permissions::from_mode(0o700))
                            .unwrap();
                    };
                    if change != "created" {
                        create_target();
                    }
                    std::os::unix::fs::symlink(&target, &link).unwrap();
                    let (mut app, _) = App::new();
                    app.navigation = NavigationSession::new(temp.path().to_path_buf());
                    app.navigation.settle_for_test();
                    let entries = fs::read_directory(temp.path()).unwrap();
                    let selected = entries.iter().position(|entry| entry.path == link).unwrap();
                    let count = entries.len();
                    app.navigation.install_folder_entries(entries);
                    app.grid.select_only(Some(selected), count);
                    press(&mut app, ":");
                    let _ = app.update(Message::CommandChanged("chmod 755".into()));
                    let task = app.update(Message::CommandSubmitted);
                    if change == "replaced" {
                        std_fs::rename(&target, &retained).unwrap();
                    }
                    if change != "unchanged" {
                        create_target();
                    }
                    navigation::finish_tasks(&mut app, task).await;
                    let expected = if change == "unchanged" { 0o755 } else { 0o700 };
                    assert_eq!(
                        std_fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
                        expected,
                        "Queued chmod followed the wrong symlink target: {change}"
                    );
                    if change == "replaced" {
                        assert_eq!(
                            std_fs::metadata(&retained).unwrap().permissions().mode() & 0o7777,
                            0o700
                        );
                    }
                    if change != "unchanged" {
                        assert!(app.command.output().unwrap().detail.contains("1 failed"));
                    }
                }
            }
        });
}

#[test]
fn queued_permission_changes_preserve_replaced_targets() {
    use std::os::unix::fs::PermissionsExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for directory in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                let retained = temp.path().join("retained");
                let valid = temp.path().join("valid");
                if directory {
                    std_fs::create_dir(&target).unwrap();
                } else {
                    std_fs::write(&target, "original").unwrap();
                }
                std_fs::write(&valid, "unchanged target").unwrap();
                for path in [&target, &valid] {
                    std_fs::set_permissions(path, std_fs::Permissions::from_mode(0o700)).unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                press(&mut app, ":");
                let _ = app.update(Message::CommandChanged("chmod 755 target valid".into()));
                let task = app.update(Message::CommandSubmitted);
                std_fs::rename(&target, &retained).unwrap();
                if directory {
                    std_fs::create_dir(&target).unwrap();
                } else {
                    std_fs::write(&target, "replacement").unwrap();
                }
                std_fs::set_permissions(&target, std_fs::Permissions::from_mode(0o700)).unwrap();
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(
                    std_fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
                    0o700,
                    "Queued chmod changed a replacement item"
                );
                assert_eq!(
                    std_fs::metadata(&retained).unwrap().permissions().mode() & 0o7777,
                    0o700
                );
                assert_eq!(
                    std_fs::metadata(&valid).unwrap().permissions().mode() & 0o7777,
                    0o755
                );
                assert!(app.command.output().unwrap().detail.contains("1 failed"));
            }
        });
}

#[test]
fn creation_through_a_directory_symlink_checks_the_target_identity() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for (directory, replaced) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let temp = tempfile::tempdir().unwrap();
                let target = temp.path().join("target");
                let alias = temp.path().join("alias");
                std_fs::create_dir(&target).unwrap();
                std::os::unix::fs::symlink(&target, &alias).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(alias.clone());
                app.navigation.settle_for_test();
                if directory {
                    let _ = app.show_new_folder();
                } else {
                    let _ = app.show_new_file();
                }
                let _ = app.update(Message::PromptInputChanged("created".into()));
                if replaced {
                    std_fs::rename(&target, temp.path().join("retained")).unwrap();
                    std_fs::create_dir(&target).unwrap();
                }
                let submitted = app.update(Message::PromptSubmit);
                navigation::finish_tasks(&mut app, submitted).await;
                assert_eq!(std_fs::read_link(&alias).unwrap(), target);
                if replaced {
                    assert!(!target.join("created").exists());
                    assert!(!temp.path().join("retained/created").exists());
                    assert!(app.journal.undo().is_err());
                } else {
                    assert!(target.join("created").exists());
                    assert_eq!(target.join("created").is_dir(), directory);
                    app.journal.undo().unwrap();
                    assert!(!target.join("created").exists());
                }
            }
        });
}

#[test]
fn creation_preserves_a_replaced_parent_after_the_prompt_opens() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for (directory, queued) in [(false, false), (true, false), (false, true), (true, true)] {
                let temp = tempfile::tempdir().unwrap();
                let parent = temp.path().join("parent");
                let retained = temp.path().join("retained");
                std_fs::create_dir(&parent).unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(parent.clone());
                app.navigation.settle_for_test();
                if directory { let _ = app.show_new_folder(); } else { let _ = app.show_new_file(); }
                let _ = app.update(Message::PromptInputChanged("created".into()));
                let pending = queued.then(|| app.update(Message::PromptSubmit));
                std_fs::rename(&parent, &retained).unwrap();
                std_fs::create_dir(&parent).unwrap();
                let task = pending.unwrap_or_else(|| app.update(Message::PromptSubmit));
                navigation::finish_tasks(&mut app, task).await;
                assert!(!parent.join("created").exists(), "Creation wrote into a replacement parent folder");
                assert!(!retained.join("created").exists());
                assert!(matches!(app.file_operations.view(), FileOperationView::NewFile { error, .. } | FileOperationView::NewFolder { error, .. } if !error.is_empty()));
                assert!(app.journal.undo().is_err());
            }
        });
}

#[test]
fn rename_preserves_replacements_after_the_editor_opens() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for (queued, directory) in [(false, false), (true, false), (false, true), (true, true)] {
                let temp = tempfile::tempdir().unwrap();
                let source = temp.path().join("before.txt");
                let destination = temp.path().join("after.txt");
                let retained = temp.path().join("retained.txt");
                let contents = |path: &std::path::Path| {
                    if directory { path.join("contents") } else { path.to_path_buf() }
                };
                if directory {
                    std_fs::create_dir(&source).unwrap();
                }
                std_fs::write(contents(&source), "original").unwrap();
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation.install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.grid.select_only(Some(0), 1);
                press(&mut app, "r");
                let _ = app.update(Message::RenameChanged("after.txt".into()));
                let pending = queued.then(|| app.update(Message::RenameSubmitted));
                std_fs::rename(&source, &retained).unwrap();
                if directory {
                    std_fs::create_dir(&source).unwrap();
                }
                std_fs::write(contents(&source), "replaced").unwrap();
                let task = pending.unwrap_or_else(|| app.update(Message::RenameSubmitted));
                navigation::finish_tasks(&mut app, task).await;
                assert!(!destination.exists(), "Rename moved an external replacement");
                assert_eq!(std_fs::read_to_string(contents(&source)).unwrap(), "replaced");
                assert_eq!(std_fs::read_to_string(contents(&retained)).unwrap(), "original");
                assert!(matches!(app.file_operations.view(), FileOperationView::Rename { error, .. } if !error.is_empty()));
                assert!(app.journal.undo().is_err(), "Rejected Rename must not add an Undo record");
            }
        });
}

#[test]
fn queued_permanent_delete_fallback_preserves_a_replacement_folder() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let item = temp.path().join("folder");
            let retained = temp.path().join("retained");
            std_fs::create_dir(&item).unwrap();
            std_fs::write(item.join("file"), "original").unwrap();
            let entries = fs::read_directory(temp.path()).unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation.install_folder_entries(entries.clone());
            app.file_operations.finish_trash_transfer(
                entries
                    .into_iter()
                    .map(|entry| (entry, "Trash unavailable".into()))
                    .collect(),
            );
            let _ = app.update(Message::Noop);
            let confirmed = app.update(Message::PromptConfirm);
            std_fs::rename(&item, &retained).unwrap();
            std_fs::create_dir(&item).unwrap();
            std_fs::write(item.join("file"), "replaced").unwrap();
            navigation::finish_tasks(&mut app, confirmed).await;
            assert_eq!(
                std_fs::read_to_string(item.join("file")).unwrap(),
                "replaced"
            );
            assert_eq!(
                std_fs::read_to_string(retained.join("file")).unwrap(),
                "original"
            );
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::Error { .. }
            ));
        });
}

#[test]
fn permanent_delete_fallback_preserves_a_replacement_after_confirmation_opens() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let item = temp.path().join("item.txt");
            let retained = temp.path().join("retained.txt");
            std_fs::write(&item, "original").unwrap();
            let entries = fs::read_directory(temp.path()).unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation.install_folder_entries(entries.clone());
            app.file_operations.finish_trash_transfer(
                entries
                    .into_iter()
                    .map(|entry| (entry, "Trash unavailable".into()))
                    .collect(),
            );
            let _ = app.update(Message::Noop);
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::PermanentDelete { .. }
            ));
            std_fs::rename(&item, &retained).unwrap();
            std_fs::write(&item, "replaced").unwrap();
            let confirmed = app.update(Message::PromptConfirm);
            navigation::finish_tasks(&mut app, confirmed).await;
            assert!(
                item.exists(),
                "the old confirmation deleted a replacement file"
            );
            assert_eq!(std_fs::read_to_string(&item).unwrap(), "replaced");
            assert_eq!(std_fs::read_to_string(&retained).unwrap(), "original");
            assert!(matches!(
                app.file_operations.view(),
                FileOperationView::Error { .. }
            ));
        });
}

#[test]
fn queued_rename_completion_refreshes_a_newer_recursive_search() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("original");
            let next = temp.path().to_path_buf();
            let nested = next.join("nested");
            std_fs::create_dir(&original).unwrap();
            std_fs::create_dir_all(&nested).unwrap();
            let before = original.join("before.bin");
            let after = original.join("after.bin");
            std_fs::write(&before, "rename contents").unwrap();
            let matched = nested.join("match.txt");
            std_fs::write(&matched, "search match").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(original.clone());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(&original).unwrap());
            app.grid.select_only(Some(0), 1);
            press(&mut app, "r");
            let _ = app.update(Message::RenameChanged("after.bin".into()));
            let mut stream =
                iced_runtime::task::into_stream(app.update(Message::RenameSubmitted)).unwrap();
            let mut queued = Vec::new();
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    queued.push(message);
                }
            }
            assert_eq!(queued.len(), 1);
            assert_eq!(std_fs::read_to_string(&after).unwrap(), "rename contents");
            let navigation = app.update(Message::Parent);
            navigation::finish_tasks(&mut app, navigation).await;
            press(&mut app, "/");
            let search = app.update(Message::SearchChanged("/txt".into()));
            navigation::finish_tasks(&mut app, search).await;
            let paths = |app: &App| {
                app.navigation
                    .entries()
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(paths(&app), std::slice::from_ref(&matched));
            let added = nested.join("new.txt");
            std_fs::write(&added, "new match").unwrap();

            let completion = app.update(queued.pop().unwrap());
            navigation::finish_tasks(&mut app, completion).await;
            assert_eq!(
                paths(&app),
                [matched, added],
                "The old Rename completion replaced recursive matches with the folder listing"
            );
            assert!(app.search.is_recursive());
            assert_eq!(app.navigation.current(), next);
            app.journal.undo().unwrap();
            assert_eq!(std_fs::read_to_string(&before).unwrap(), "rename contents");
            assert!(!after.exists());
        });
}

#[test]
fn undo_and_redo_results_survive_the_resulting_folder_refresh() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let file = temp.path().join("created.txt");
            std_fs::write(&file, "").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.journal
                .record(journal::Action::new_file(file.clone()).unwrap())
                .unwrap();

            for (key, modifiers, exists, expected) in [
                ("u", keyboard::Modifiers::empty(), false, "Undid New File"),
                ("r", keyboard::Modifiers::CTRL, true, "Redid New File"),
            ] {
                let named = keyboard::Key::Character(key.into());
                let task = app.handle_key(named.clone(), named, modifiers, Some(key));
                navigation::finish_tasks(&mut app, task).await;
                assert_eq!(file.exists(), exists);
                assert_eq!(
                    app.navigation
                        .entries()
                        .iter()
                        .any(|entry| entry.path == file),
                    exists
                );
                assert_eq!(
                    app.browser_status_model().text,
                    expected,
                    "The automatic folder refresh hid the journal result"
                );
                let clicked = app.update(Message::Event(
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    event::Status::Captured,
                ));
                navigation::finish_tasks(&mut app, clicked).await;
                assert_ne!(app.browser_status_model().text, expected);
            }
        });
}

#[test]
fn queued_restore_does_not_move_a_replacement_trash_item() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("Trash");
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let original = temp.path().join("original.txt");
            let trashed = root.join("files/item.txt");
            let info = root.join("info/item.txt.trashinfo");
            std_fs::write(&trashed, "original item").unwrap();
            std_fs::write(
                &info,
                format!("[Trash Info]\nPath={}\n", original.display()),
            )
            .unwrap();
            let (mut app, _) = App::new();
            app.navigation.settle_for_test();
            app.trash = trash::Trash::at(root);
            app.navigation
                .install_trash_entries(app.trash.entries().unwrap());
            app.grid.select_only(Some(0), 1);
            let restore = app.update(Message::ContextRestore);
            let recovered = temp.path().join("recovered.txt");
            std_fs::rename(&trashed, &recovered).unwrap();
            std_fs::write(&trashed, "replacement item").unwrap();
            std_fs::write(&info, "[Trash Info]\nPath=/replacement/item.txt\n").unwrap();

            navigation::finish_tasks(&mut app, restore).await;
            assert!(
                trashed.exists(),
                "Queued Restore moved a replacement Trash item to the old location"
            );
            assert!(!original.exists());
            assert_eq!(
                std_fs::read_to_string(&trashed).unwrap(),
                "replacement item"
            );
            assert_eq!(
                std_fs::read_to_string(&info).unwrap(),
                "[Trash Info]\nPath=/replacement/item.txt\n"
            );
            assert_eq!(std_fs::read_to_string(&recovered).unwrap(), "original item");
            assert_eq!(
                app.browser_status_model().text,
                "Restored 0  •  1 failed  •  0 kept"
            );
        });
}

#[test]
fn trash_delete_confirmation_does_not_delete_a_replacement_item() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("Trash");
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let trashed = root.join("files/item.txt");
            let info = root.join("info/item.txt.trashinfo");
            std_fs::write(&trashed, "original item").unwrap();
            std_fs::write(&info, "[Trash Info]\nPath=/original/item.txt\n").unwrap();
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
            let recovered = temp.path().join("recovered.txt");
            std_fs::rename(&trashed, &recovered).unwrap();
            std_fs::write(&trashed, "replacement item").unwrap();
            std_fs::write(&info, "[Trash Info]\nPath=/replacement/item.txt\n").unwrap();

            let confirmed = app.update(Message::PromptConfirm);
            navigation::finish_tasks(&mut app, confirmed).await;
            assert!(
                trashed.exists(),
                "The old confirmation deleted a replacement Trash item"
            );
            assert_eq!(
                std_fs::read_to_string(&trashed).unwrap(),
                "replacement item"
            );
            assert_eq!(
                std_fs::read_to_string(&info).unwrap(),
                "[Trash Info]\nPath=/replacement/item.txt\n"
            );
            assert_eq!(std_fs::read_to_string(&recovered).unwrap(), "original item");
            assert_eq!(
                app.browser_status_model().text,
                "Permanently deleted 0  •  1 failed"
            );
        });
}

#[test]
fn queued_undo_completion_refreshes_the_active_recursive_search() {
    use iced::futures::StreamExt;

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let nested = temp.path().join("nested");
            std_fs::create_dir(&nested).unwrap();
            let matched = nested.join("match.txt");
            std_fs::write(&matched, "search match").unwrap();
            let undone = temp.path().join("undo.bin");
            std_fs::write(&undone, "").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.journal
                .record(journal::Action::new_file(undone.clone()).unwrap())
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
            assert!(!undone.exists());
            assert!(!app.foreground_operation_active());
            press(&mut app, "/");
            let search = app.update(Message::SearchChanged("/txt".into()));
            navigation::finish_tasks(&mut app, search).await;
            let paths = |app: &App| {
                app.navigation
                    .entries()
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(paths(&app), std::slice::from_ref(&matched));
            let added = nested.join("new.txt");
            std_fs::write(&added, "new match").unwrap();

            let completion = app.update(queued.pop().unwrap());
            navigation::finish_tasks(&mut app, completion).await;
            assert_eq!(
                paths(&app),
                [matched, added],
                "Undo completion must refresh recursive matches instead of listing the root folder"
            );
            assert!(app.search.is_recursive());
            assert!(!undone.exists());
        });
}

#[test]
fn restore_confirmation_survives_the_resulting_trash_refresh() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("Trash");
            std_fs::create_dir_all(root.join("files")).unwrap();
            std_fs::create_dir_all(root.join("info")).unwrap();
            let original = temp.path().join("restored.txt");
            let trashed = root.join("files/restored.txt");
            let info = root.join("info/restored.txt.trashinfo");
            std_fs::write(&trashed, "restored contents").unwrap();
            std_fs::write(
                &info,
                format!("[Trash Info]\nPath={}\n", original.display()),
            )
            .unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.trash = trash::Trash::at(root);
            app.navigation
                .install_trash_entries(app.trash.entries().unwrap());
            app.grid.select_only(Some(0), 1);

            let restore = app.update(Message::ContextRestore);
            navigation::finish_tasks(&mut app, restore).await;
            assert_eq!(
                std_fs::read_to_string(&original).unwrap(),
                "restored contents"
            );
            assert!(!trashed.exists());
            assert!(!info.exists());
            assert_eq!(
                app.navigation.displayed_location(),
                DisplayedLocation::Trash
            );
            assert!(app.navigation.entries().is_empty());
            assert_eq!(
                app.browser_status_model().text,
                "Restored 1  •  0 failed  •  0 kept",
                "The automatic Trash refresh hid the Restore result"
            );

            let clicked = app.update(Message::Event(
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                event::Status::Captured,
            ));
            navigation::finish_tasks(&mut app, clicked).await;
            assert!(!app.browser_status_model().text.contains("Restored"));
        });
}

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
                    identity: None,
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
fn explicitly_retyping_a_lossy_filename_renames_its_original_bytes() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            for directory in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let original_name = OsString::from_vec(b"name-\xff".to_vec());
                let entered = original_name.to_string_lossy().into_owned();
                let original = temp.path().join(&original_name);
                let renamed = temp.path().join(&entered);
                if directory {
                    std_fs::create_dir(&original).unwrap();
                } else {
                    std_fs::write(&original, "original contents").unwrap();
                }
                let (mut app, _) = App::new();
                app.navigation = NavigationSession::new(temp.path().to_path_buf());
                app.navigation.settle_for_test();
                app.navigation
                    .install_folder_entries(fs::read_directory(temp.path()).unwrap());
                app.grid.select_only(Some(0), 1);
                press(&mut app, "r");
                let _ = app.update(Message::RenameChanged("temporary input".into()));
                let _ = app.update(Message::RenameChanged(entered));
                let task = app.update(Message::RenameSubmitted);
                navigation::finish_tasks(&mut app, task).await;
                assert!(
                    !original.exists(),
                    "Explicitly entered UTF-8 name was ignored"
                );
                assert!(renamed.exists());
                app.journal.undo().unwrap();
                assert!(original.exists());
                assert!(!renamed.exists());
                app.journal.redo().unwrap();
                assert!(!original.exists());
                assert!(renamed.exists());
                if !directory {
                    assert_eq!(
                        std_fs::read_to_string(&renamed).unwrap(),
                        "original contents"
                    );
                }
            }
        });
}

#[test]
fn explicitly_retyping_a_lossy_filename_reports_existing_name_collisions() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let original_name = OsString::from_vec(b"name-\xff".to_vec());
            let entered = original_name.to_string_lossy().into_owned();
            let original = temp.path().join(&original_name);
            let existing = temp.path().join(&entered);
            std_fs::write(&original, "original contents").unwrap();
            std_fs::write(&existing, "unrelated contents").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation.settle_for_test();
            app.navigation.install_folder_entries(fs::read_directory(temp.path()).unwrap());
            let selected = app.navigation.entries().iter().position(|entry| entry.path == original).unwrap();
            app.grid.select_only(Some(selected), 2);
            press(&mut app, "r");
            let _ = app.update(Message::RenameChanged(entered));
            let task = app.update(Message::RenameSubmitted);
            navigation::finish_tasks(&mut app, task).await;
            assert!(matches!(app.file_operations.view(), FileOperationView::Rename { error, .. } if !error.is_empty()));
            assert_eq!(std_fs::read_to_string(&original).unwrap(), "original contents");
            assert_eq!(std_fs::read_to_string(&existing).unwrap(), "unrelated contents");
            assert_eq!(app.journal.undo().unwrap_err().to_string(), "Nothing to undo");
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
        for (name, retype) in [
            (OsString::from_vec(b"name-\xff.txt".to_vec()), false),
            ("plain.txt".into(), false),
            ("plain.txt".into(), true),
        ] {
            let temp = tempfile::tempdir().unwrap();
            std_fs::write(temp.path().join(&name), "original contents").unwrap();
            let (mut app, _) = App::new();
            app.navigation = NavigationSession::new(temp.path().to_path_buf());
            app.navigation
                .install_folder_entries(fs::read_directory(temp.path()).unwrap());
            app.grid.select_only(Some(0), 1);
            press(&mut app, "r");
            if retype {
                let _ = app.update(Message::RenameChanged("temporary input".into()));
                let _ = app.update(Message::RenameChanged(name.to_str().unwrap().into()));
            }
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
            identity: None,
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
                identity: None,
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
            identity: None,
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
            identity: None,
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
