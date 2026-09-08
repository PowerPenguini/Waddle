#[test]
#[ignore = "Subprocess entry point for isolated Trash recovery"]
fn trash_recovery_child() {
    let Some(root) = std::env::var_os("WADDLE_TRASH_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let operation = std::env::var("WADDLE_TRASH_OPERATION").unwrap();
    let kill = std::env::var_os("WADDLE_TRASH_KILL").is_some();
    let backend_root = root.clone();
    trash_receipt::test_backend::with(
        move |original| {
            let receipt = TrashReceipt {
                original: original.to_owned(),
                trashed: backend_root.join("Trash/files/new-item"),
                info: backend_root.join("Trash/info/new-item.trashinfo"),
            };
            fs::rename(original, &receipt.trashed).unwrap();
            fs::write(
                &receipt.info,
                format!("[Trash Info]\nPath={}\n", original.display()),
            )
            .unwrap();
            if kill {
                std::process::exit(86);
            }
            Ok(receipt)
        },
        || {
            let mut journal = Journal::open(root.join("journal.json")).unwrap();
            let result = if operation == "undo" {
                journal.undo()
            } else {
                journal.redo()
            };
            result.unwrap();
        },
    );
}

#[test]
fn trash_and_restore_history_resume_after_process_exit() {
    for restore in [false, true] {
        for returning_from_trash in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir_all(root.path().join("Trash/files")).unwrap();
            fs::create_dir_all(root.path().join("Trash/info")).unwrap();
            let receipt = TrashReceipt {
                original: root.path().join("original"),
                trashed: root.path().join("Trash/files/item"),
                info: root.path().join("Trash/info/item.trashinfo"),
            };
            let path = root.path().join("journal.json");
            let mut journal = Journal::open(path).unwrap();
            // Construct the state immediately before the requested direction.
            if restore {
                fs::write(&receipt.original, b"recover Trash data").unwrap();
                journal
                    .record(
                        Action::restore(std::slice::from_ref(&receipt), false)
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap();
                if returning_from_trash {
                    let copy = receipt.clone();
                    trash_receipt::test_backend::with(
                        move |original| {
                            fs::rename(original, &copy.trashed).unwrap();
                            fs::write(&copy.info, "metadata").unwrap();
                            Ok(copy.clone())
                        },
                        || {
                            journal.undo().unwrap();
                        },
                    );
                }
            } else {
                fs::write(&receipt.trashed, b"recover Trash data").unwrap();
                fs::write(&receipt.info, "metadata").unwrap();
                journal
                    .record(
                        Action::trash(std::slice::from_ref(&receipt))
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap();
                if !returning_from_trash {
                    journal.undo().unwrap();
                }
            }
            let operation = if restore == returning_from_trash {
                "redo"
            } else {
                "undo"
            };
            let shim = audit_fault_library();
            let armed = root.path().join("armed");
            fs::write(&armed, "").unwrap();
            let run = |interrupt: bool| {
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "journal::tests::trash_recovery_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("WADDLE_TRASH_CHILD", root.path())
                    .env("WADDLE_TRASH_OPERATION", operation)
                    .env("XDG_DATA_HOME", root.path());
                if interrupt && returning_from_trash {
                    command
                        .env("LD_PRELOAD", shim.path().join("open_fault.so"))
                        .env("WADDLE_AUDIT_TARGET", &receipt.original)
                        .env("WADDLE_AUDIT_ARMED", &armed)
                        .env("WADDLE_AUDIT_FAULT", "crash");
                } else if interrupt {
                    command.env("WADDLE_TRASH_KILL", "1");
                }
                command.output().unwrap()
            };
            let killed = run(true);
            assert_eq!(
                killed.status.code(),
                Some(86),
                "{}",
                String::from_utf8_lossy(&killed.stderr)
            );
            let resumed = run(false);
            assert!(
                resumed.status.success(),
                "restore={restore}, returning={returning_from_trash}: {}",
                String::from_utf8_lossy(&resumed.stderr)
            );
            if returning_from_trash {
                assert_eq!(fs::read(&receipt.original).unwrap(), b"recover Trash data");
                assert!(!receipt.info.exists());
            } else {
                assert!(!receipt.original.exists());
                assert_eq!(
                    fs::read(root.path().join("Trash/files/new-item")).unwrap(),
                    b"recover Trash data"
                );
                assert!(root.path().join("Trash/info/new-item.trashinfo").exists());
            }
        }
    }
}

#[test]
#[ignore = "Subprocess entry point for isolated storage errors"]
fn storage_error_child() {
    let Some(root) = std::env::var_os("WADDLE_STORAGE_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let action = if std::env::var_os("WADDLE_STORAGE_MOVE").is_some() {
        crate::transfer::Action::Move
    } else {
        crate::transfer::Action::Copy
    };
    let target = PathBuf::from(std::env::var_os("WADDLE_AUDIT_SYNC_TARGET").unwrap());
    let batch =
        crate::fs::TransferBatch::try_new(vec![root.join("source")], target.clone(), action)
            .unwrap();
    let outcome = match batch.run() {
        crate::fs::TransferBatchOutcome::Conflict { batch, .. } => batch
            .resolve(crate::fs::ConflictChoice::Replace, false)
            .run(),
        complete => complete,
    };
    let crate::fs::TransferBatchOutcome::Complete(report) = outcome else {
        panic!("unexpected conflict")
    };
    assert!(
        !report.failures.is_empty(),
        "unflushed data must not be reported as successful: {report:?}"
    );
}

#[test]
fn transfer_rejects_delayed_storage_errors_before_publication() {
    for error in [libc::ENOSPC, libc::EIO] {
        for moving in [false, true] {
            for replace in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let target = if moving {
                    tempfile::tempdir_in("/dev/shm").unwrap()
                } else {
                    tempfile::tempdir().unwrap()
                };
                let shim = audit_fault_library();
                fs::write(root.path().join("source"), vec![b'S'; 1024 * 1024]).unwrap();
                if replace {
                    fs::write(target.path().join("source"), b"original destination").unwrap();
                }
                let armed = root.path().join("armed");
                fs::write(&armed, "").unwrap();
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "journal::tests::storage_error_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("WADDLE_STORAGE_CHILD", root.path())
                    .env("LD_PRELOAD", shim.path().join("open_fault.so"))
                    .env("WADDLE_AUDIT_SYNC_TARGET", target.path())
                    .env("WADDLE_AUDIT_SYNC_ERRNO", error.to_string())
                    .env("WADDLE_AUDIT_ARMED", &armed);
                if moving {
                    command.env("WADDLE_STORAGE_MOVE", "1");
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "error={error}, moving={moving}, replace={replace}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    !armed.exists(),
                    "the simulated delayed write error must actually be reached"
                );
                assert_eq!(
                    fs::read(root.path().join("source")).unwrap(),
                    vec![b'S'; 1024 * 1024]
                );
                assert_eq!(
                    fs::read(target.path().join("source")).ok(),
                    replace.then(|| b"original destination".to_vec())
                );
                assert_eq!(
                    fs::read_dir(target.path()).unwrap().count(),
                    usize::from(replace),
                    "failed copy left staging behind"
                );
            }
        }
    }
}

#[test]
#[ignore = "Subprocess entry point with an isolated native GIO Trash"]
fn native_trash_identity_child() {
    let Some(root) = std::env::var_os("WADDLE_NATIVE_TRASH_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let original = root.join("original");
    let receipt = trash(&original).unwrap();
    assert_eq!(
        fs::read(&receipt.trashed).unwrap(),
        b"new data",
        "Trash returned an unrelated older receipt"
    );
}

#[test]
fn native_trash_receipt_identifies_the_moved_file_among_duplicate_original_paths() {
    let root = tempfile::tempdir_in(std::env::var_os("HOME").unwrap()).unwrap();
    fs::create_dir_all(root.path().join("Trash/files")).unwrap();
    fs::create_dir_all(root.path().join("Trash/info")).unwrap();
    let original = root.path().join("original");
    fs::write(&original, b"new data").unwrap();
    fs::write(root.path().join("Trash/files/old-item"), b"old data").unwrap();
    let old_info = root.path().join("Trash/info/old-item.trashinfo");
    fs::write(
        &old_info,
        format!(
            "[Trash Info]\nPath={}\nDeletionDate=2020-01-01T00:00:00\n",
            original.display()
        ),
    )
    .unwrap();
    fs::File::open(&old_info)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
        .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "journal::tests::native_trash_identity_child",
            "--ignored",
            "--nocapture",
        ])
        .env("WADDLE_NATIVE_TRASH_CHILD", root.path())
        .env("XDG_DATA_HOME", root.path())
        .env("GIO_USE_VFS", "local")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(root.path().join("Trash/files/old-item")).unwrap(),
        b"old data"
    );
}
