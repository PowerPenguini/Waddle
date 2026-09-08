// These regression tests compile the fault shim and confine it to a child process.
#[test]
#[ignore = "Child process helper for isolated filesystem fault injection"]
fn audit_history_fault_child() {
    let Some(root) = std::env::var_os("WADDLE_AUDIT_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mut journal = Journal::open(root.join("journal.json")).unwrap();
    let result = if std::env::var_os("WADDLE_AUDIT_UNDO").is_some() {
        journal.undo()
    } else {
        journal.redo()
    };
    fs::write(
        root.join("result.txt"),
        match result {
            Ok(_) => "unexpected success".to_owned(),
            Err(error) => error.to_string(),
        },
    )
    .unwrap();
}

fn audit_history_fault(kind: TransferKind, crash: bool, undo: bool) {
    audit_history_fault_with_older_check(kind, crash, undo, false, false);
}

fn audit_history_fault_with_older_check(
    kind: TransferKind,
    crash: bool,
    undo: bool,
    check_older: bool,
    replace_result: bool,
) {
    let shim_root = audit_fault_library();
    let shim = shim_root.path().join("open_fault.so");
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    let path = temp.path().join("journal.json");
    fs::write(&source, b"recover this transfer").unwrap();
    match kind {
        TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
        TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
    }
    let receipt = crate::fs::TransferReceipt {
        source: source.clone(),
        destination: destination.clone(),
        replaced_existing: false,
    };
    let mut journal = Journal::open(path.clone()).unwrap();
    let older = temp.path().join("older-unrelated-file");
    fs::write(&older, b"an earlier operation").unwrap();
    journal
        .record(Action::new_file(older.clone()).unwrap())
        .unwrap();
    journal
        .record(Action::transfer(kind, &[receipt]).unwrap().unwrap())
        .unwrap();
    if !undo {
        journal.undo().unwrap();
    }
    let armed = temp.path().join("armed");
    fs::write(&armed, "").unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    if undo {
        command.env("WADDLE_AUDIT_UNDO", "1");
    }
    let target = if undo && matches!(kind, TransferKind::Copy) {
        command.env("WADDLE_AUDIT_WRITE", "1");
        command.env("WADDLE_AUDIT_EFFECT_MISSING", &destination);
        path.with_extension("json.tmp")
    } else if undo {
        source.clone()
    } else {
        destination.clone()
    };
    let status = command
        .args([
            "--exact",
            "journal::tests::audit_history_fault_child",
            "--ignored",
            "--nocapture",
        ])
        .env("LD_PRELOAD", shim)
        .env("WADDLE_AUDIT_CHILD_ROOT", temp.path())
        .env("WADDLE_AUDIT_TARGET", &target)
        .env("WADDLE_AUDIT_ARMED", &armed)
        .env(
            "WADDLE_AUDIT_FAULT",
            if crash { "crash" } else { "read-error" },
        )
        .output()
        .unwrap();
    assert!(
        !armed.exists(),
        "fault was not reached: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    if crash {
        assert_eq!(status.status.code(), Some(86));
    } else {
        assert!(status.status.success());
        let error = fs::read_to_string(temp.path().join("result.txt")).unwrap();
        assert!(
            error.contains("Permission denied"),
            "wrong failure: {error}"
        );
        eprintln!("first failure: {error}");
    }
    if undo {
        assert_eq!(fs::read(&source).unwrap(), b"recover this transfer");
        assert!(!destination.exists());
    } else {
        assert_eq!(fs::read(&destination).unwrap(), b"recover this transfer");
        assert_eq!(source.exists(), matches!(kind, TransferKind::Copy));
    }
    let mut reopened = Journal::open(path).unwrap();
    if check_older {
        let opposite = reopened.undo();
        eprintln!(
            "Undo after the Redo failure: {opposite:?}; older file exists={}",
            older.exists()
        );
        assert!(
            older.exists(),
            "Undo crossed the unrecorded Redo effect and deleted an unrelated older file"
        );
        return;
    }
    if replace_result {
        let owned = temp.path().join("owned-result");
        let metadata = fs::metadata(&destination).unwrap();
        fs::rename(&destination, &owned).unwrap();
        fs::copy(&owned, &destination).unwrap();
        fs::File::open(&destination)
            .unwrap()
            .set_modified(metadata.modified().unwrap())
            .unwrap();
        let refused = reopened.redo();
        assert!(
            refused.is_err(),
            "matching bytes must not authorize a substituted destination"
        );
        assert_eq!(fs::read(&destination).unwrap(), b"recover this transfer");
        fs::remove_file(&destination).unwrap();
        fs::rename(&owned, &destination).unwrap();
    }
    let resumed = if undo {
        reopened.undo()
    } else {
        reopened.redo()
    };
    eprintln!(
        "kind={kind:?}, crash={crash}, undo={undo}, retry={resumed:?}; source exists={}, destination exists={}",
        source.exists(),
        destination.exists()
    );
    assert!(
        resumed.is_ok(),
        "completed filesystem effect must remain recoverable: {resumed:?}"
    );
    if undo {
        reopened.redo().unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"recover this transfer");
    } else {
        reopened.undo().unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"recover this transfer");
        assert!(!destination.exists());
    }
}

#[test]
fn audit_copy_redo_recovers_after_fingerprint_read_error() {
    audit_history_fault(TransferKind::Copy, false, false);
}
#[test]
fn audit_move_redo_recovers_after_fingerprint_read_error() {
    audit_history_fault(TransferKind::Move, false, false);
}
#[test]
fn audit_copy_redo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Copy, true, false);
}
#[test]
fn audit_move_redo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Move, true, false);
}

#[test]
fn audit_copy_undo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Copy, true, true);
}
#[test]
fn audit_move_undo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Move, true, true);
}
#[test]
fn audit_move_undo_recovers_after_fingerprint_read_error() {
    audit_history_fault(TransferKind::Move, false, true);
}

#[test]
fn audit_copy_fingerprint_error_protects_older_history() {
    audit_history_fault_with_older_check(TransferKind::Copy, false, false, true, false);
}
#[test]
fn audit_move_fingerprint_error_protects_older_history() {
    audit_history_fault_with_older_check(TransferKind::Move, false, false, true, false);
}

fn audit_fault_library() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("open_fault.c");
    fs::write(&source, include_str!("open_fault.c")).unwrap();
    let output = std::process::Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg("-ldl")
        .arg("-o")
        .arg(root.path().join("open_fault.so"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    root
}

#[test]
fn audit_recovery_refuses_a_substituted_destination_with_identical_contents() {
    for kind in [TransferKind::Copy, TransferKind::Move] {
        audit_history_fault_with_older_check(kind, true, false, false, true);
    }
}

#[test]
fn audit_cross_device_history_recovers_interrupted_source_cleanup() {
    use std::os::unix::fs::MetadataExt;
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir_in("/dev/shm").unwrap();
    let shim = audit_fault_library();
    assert_ne!(
        fs::metadata(root.path()).unwrap().dev(),
        fs::metadata(other.path()).unwrap().dev()
    );
    let source = root.path().join("source");
    let destination = other.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("first"), b"linked data").unwrap();
    fs::hard_link(source.join("first"), source.join("second")).unwrap();
    crate::fs::journal_move(&source, &destination).unwrap();
    let path = root.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Move,
                &[crate::fs::TransferReceipt {
                    source: source.clone(),
                    destination: destination.clone(),
                    replaced_existing: false,
                }],
            )
            .unwrap()
            .unwrap(),
        )
        .unwrap();
    journal.undo().unwrap();
    let armed = root.path().join("armed");
    fs::write(&armed, "").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "journal::tests::audit_history_fault_child",
            "--ignored",
            "--nocapture",
        ])
        .env("LD_PRELOAD", shim.path().join("open_fault.so"))
        .env("WADDLE_AUDIT_CHILD_ROOT", root.path())
        .env("WADDLE_AUDIT_ARMED", &armed)
        .env("WADDLE_AUDIT_UNLINK", source.join("first"))
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(source.is_dir());
    assert!(!source.join("first").exists());
    for name in ["first", "second"] {
        assert_eq!(fs::read(destination.join(name)).unwrap(), b"linked data");
    }
    // An unrelated addition must stop cleanup, even after the recorded source
    // entry has already been removed by the interrupted process.
    fs::write(source.join("external"), b"keep this").unwrap();
    let mut journal = Journal::open(path.clone()).unwrap();
    assert!(journal.redo().is_err());
    assert_eq!(fs::read(source.join("external")).unwrap(), b"keep this");
    fs::remove_file(source.join("external")).unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal.redo().unwrap();
    assert!(!source.exists());
    assert_eq!(
        fs::metadata(destination.join("first")).unwrap().ino(),
        fs::metadata(destination.join("second")).unwrap().ino()
    );
    journal.undo().unwrap();
    assert!(!destination.exists());
    assert_eq!(fs::read(source.join("first")).unwrap(), b"linked data");
}

#[test]
fn audit_history_recovers_after_intent_commit_before_publication() {
    for kind in [TransferKind::Copy, TransferKind::Move] {
        let root = tempfile::tempdir().unwrap();
        let shim = audit_fault_library();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, b"prepared result").unwrap();
        match kind {
            TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
            TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
        }
        let path = root.path().join("journal.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        journal
            .record(
                Action::transfer(
                    kind,
                    &[crate::fs::TransferReceipt {
                        source: source.clone(),
                        destination: destination.clone(),
                        replaced_existing: false,
                    }],
                )
                .unwrap()
                .unwrap(),
            )
            .unwrap();
        journal.undo().unwrap();
        let armed = root.path().join("armed");
        fs::write(&armed, "").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "journal::tests::audit_history_fault_child",
                "--ignored",
                "--nocapture",
            ])
            .env("LD_PRELOAD", shim.path().join("open_fault.so"))
            .env("WADDLE_AUDIT_CHILD_ROOT", root.path())
            .env("WADDLE_AUDIT_ARMED", &armed)
            .env("WADDLE_AUDIT_COMMIT", &path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(86),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !destination.exists(),
            "interruption must precede result publication"
        );
        assert_eq!(fs::read(&source).unwrap(), b"prepared result");
        let mut journal = Journal::open(path).unwrap();
        journal.redo().unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"prepared result");
        journal.undo().unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"prepared result");
        assert!(!destination.exists());
    }
}
