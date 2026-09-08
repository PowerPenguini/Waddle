// Run via run-history-probes.sh: the injected fault is confined to a child process.
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
    audit_history_fault_with_older_check(kind, crash, undo, false);
}

fn audit_history_fault_with_older_check(
    kind: TransferKind,
    crash: bool,
    undo: bool,
    check_older: bool,
) {
    let shim = std::env::var_os("WADDLE_AUDIT_SHIM")
        .expect("run .scratch/transfer-audit-14/run-history-probes.sh");
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
#[ignore = "Known defect under audit: fingerprint read error strands Copy Redo"]
fn audit_copy_redo_recovers_after_fingerprint_read_error() {
    audit_history_fault(TransferKind::Copy, false, false);
}
#[test]
#[ignore = "Known defect under audit: fingerprint read error strands Move Redo"]
fn audit_move_redo_recovers_after_fingerprint_read_error() {
    audit_history_fault(TransferKind::Move, false, false);
}
#[test]
#[ignore = "Known defect under audit: process exit strands Copy Redo"]
fn audit_copy_redo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Copy, true, false);
}
#[test]
#[ignore = "Known defect under audit: process exit strands Move Redo"]
fn audit_move_redo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Move, true, false);
}

#[test]
#[ignore = "Known defect under audit: process exit strands Copy Undo"]
fn audit_copy_undo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Copy, true, true);
}
#[test]
#[ignore = "Known defect under audit: process exit strands Move Undo"]
fn audit_move_undo_recovers_after_process_exit() {
    audit_history_fault(TransferKind::Move, true, true);
}
#[test]
#[ignore = "Control requiring the isolated fault-injection runner"]
fn audit_move_undo_recovers_after_fingerprint_read_error() {
    audit_history_fault(TransferKind::Move, false, true);
}

#[test]
#[ignore = "Known defect under audit: unreadable Copy result lets Undo cross history"]
fn audit_copy_fingerprint_error_protects_older_history() {
    audit_history_fault_with_older_check(TransferKind::Copy, false, false, true);
}
#[test]
#[ignore = "Known defect under audit: unreadable Move result lets Undo cross history"]
fn audit_move_fingerprint_error_protects_older_history() {
    audit_history_fault_with_older_check(TransferKind::Move, false, false, true);
}
