use super::*;
use std::{error::Error as _, fs};

#[test]
fn hunt_journal_round_trips_non_utf8_file_paths() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let file = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"file-\xff.txt".to_vec()));
    let journal_path = temp.path().join("history.json");
    fs::write(&file, "").unwrap();
    let mut journal = Journal::open(journal_path.clone()).unwrap();
    journal
        .record(Action::new_file(file.clone()).unwrap())
        .expect("a valid Unix filename must not disable persistent Undo");
    let mut journal = Journal::open(journal_path).unwrap();
    journal.undo().unwrap();
    assert!(!file.exists());
    journal.redo().unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"");
}

#[test]
fn non_utf8_copy_and_move_records_support_persistent_undo_and_redo() {
    use std::os::unix::ffi::OsStringExt;
    for kind in [TransferKind::Copy, TransferKind::Move] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .join(std::ffi::OsString::from_vec(b"folder-\xfe".to_vec()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let destination = root.join("destination");
        fs::write(&source, "preserve me").unwrap();
        match kind {
            TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
            TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
        }
        let path = temp.path().join("history.json");
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
        let mut journal = Journal::open(path).unwrap();
        journal.undo().unwrap();
        assert_eq!(fs::read_to_string(&source).unwrap(), "preserve me");
        assert!(!destination.exists());
        journal.redo().unwrap();
        assert_eq!(fs::read_to_string(&destination).unwrap(), "preserve me");
        assert_eq!(source.exists(), matches!(kind, TransferKind::Copy));
    }
}

#[test]
fn non_utf8_rename_folder_and_trash_records_survive_restart() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"folder-\xfe".to_vec()));
    fs::create_dir(&root).unwrap();
    let before = root.join("before");
    let after = root.join("after");
    fs::write(&after, "recover me").unwrap();
    let path = temp.path().join("history.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(Action::rename(before.clone(), after.clone()).unwrap())
        .unwrap();
    let mut journal = Journal::open(path.clone()).unwrap();
    journal.undo().unwrap();
    assert_eq!(fs::read_to_string(&before).unwrap(), "recover me");
    journal.redo().unwrap();
    let folder = root.join("new-folder");
    fs::create_dir(&folder).unwrap();
    journal
        .record(Action::new_folder(folder.clone()).unwrap())
        .unwrap();
    let mut journal = Journal::open(path.clone()).unwrap();
    journal.undo().unwrap();
    assert!(!folder.exists());
    journal.redo().unwrap();
    assert!(folder.is_dir());

    // A private Trash fixture exercises every persisted receipt path without using desktop Trash.
    let trashed = root.join("Trash/files/item");
    let info = root.join("Trash/info/item.trashinfo");
    fs::create_dir_all(trashed.parent().unwrap()).unwrap();
    fs::create_dir_all(info.parent().unwrap()).unwrap();
    fs::rename(&after, &trashed).unwrap();
    fs::write(&info, "metadata").unwrap();
    journal
        .record(
            Action::trash(&[TrashReceipt {
                original: after.clone(),
                trashed: trashed.clone(),
                info: info.clone(),
            }])
            .unwrap()
            .unwrap(),
        )
        .unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal.undo().unwrap();
    assert_eq!(fs::read_to_string(after).unwrap(), "recover me");
    assert!(!trashed.exists());
    assert!(!info.exists());
}

#[test]
fn fingerprint_rejects_a_fifo_without_opening_it() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::mpsc, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pipe");
    let fifo = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: fifo is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        sender.send(TreeFingerprint::read(&path)).unwrap();
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("fingerprinting must not wait for a FIFO writer");
    worker.join().unwrap();
    assert!(result.unwrap_err().to_string().contains("special file"));
}

#[test]
fn malformed_journal_preserves_the_json_error_source() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("operations.json");
    fs::write(&path, b"{").unwrap();

    let error = Journal::open(path).unwrap_err();

    assert!(
        error
            .source()
            .is_some_and(|source| source.is::<serde_json::Error>())
    );
}

#[test]
fn rename_and_new_folder_round_trip_across_a_reopened_journal() {
    let temp = tempfile::tempdir().unwrap();
    let journal_path = temp.path().join("state/operations.json");
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    fs::write(&after, "content").unwrap();
    let mut journal = Journal::open(journal_path.clone()).unwrap();
    journal
        .record(Action::rename(before.clone(), after.clone()).unwrap())
        .unwrap();

    let mut journal = Journal::open(journal_path.clone()).unwrap();
    journal.undo().unwrap();
    assert!(before.exists());
    assert!(!after.exists());
    journal.redo().unwrap();
    assert!(!before.exists());
    assert!(after.exists());

    let folder = temp.path().join("folder");
    fs::create_dir(&folder).unwrap();
    journal
        .record(Action::new_folder(folder.clone()).unwrap())
        .unwrap();
    journal.undo().unwrap();
    assert!(!folder.exists());
    journal.redo().unwrap();
    assert!(folder.is_dir());
}

#[test]
fn unsafe_inverse_is_refused_and_redo_is_cleared_by_new_work() {
    let temp = tempfile::tempdir().unwrap();
    let journal_path = temp.path().join("operations.json");
    let before = temp.path().join("before");
    let after = temp.path().join("after");
    fs::write(&after, "original").unwrap();
    let mut journal = Journal::open(journal_path).unwrap();
    journal
        .record(Action::rename(before.clone(), after.clone()).unwrap())
        .unwrap();
    fs::write(&after, "changed size").unwrap();
    assert!(journal.undo().unwrap_err().to_string().contains("changed"));

    fs::remove_file(&after).unwrap();
    fs::write(&after, "original").unwrap();
    journal.stored.entries[0].action = Action::rename(before, after).unwrap();
    journal.save().unwrap();
    journal.undo().unwrap();
    let folder = temp.path().join("new");
    fs::create_dir(&folder).unwrap();
    journal.record(Action::new_folder(folder).unwrap()).unwrap();
    assert_eq!(journal.redo().unwrap_err().to_string(), "Nothing to redo");
}

#[test]
fn journal_keeps_only_one_hundred_operations_and_thirty_days() {
    let temp = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    let now = 4_000_000;
    for index in 0..105 {
        let folder = temp.path().join(format!("folder-{index}"));
        fs::create_dir(&folder).unwrap();
        journal
            .record_at(Action::new_folder(folder).unwrap(), now)
            .unwrap();
    }
    assert_eq!(journal.stored.entries.len(), 100);

    let current = temp.path().join("current");
    fs::create_dir(&current).unwrap();
    journal
        .record_at(
            Action::new_folder(current).unwrap(),
            now + MAX_AGE_SECONDS + 1,
        )
        .unwrap();
    assert_eq!(journal.stored.entries.len(), 1);
}

#[test]
fn new_folder_undo_refuses_non_empty_directory() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("folder");
    fs::create_dir(&folder).unwrap();
    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    journal
        .record(Action::new_folder(folder.clone()).unwrap())
        .unwrap();
    fs::write(folder.join("later"), "work").unwrap();

    assert!(
        journal
            .undo()
            .unwrap_err()
            .to_string()
            .contains("no longer empty")
    );
    assert!(folder.join("later").exists());
}

#[test]
fn new_file_undo_and_redo_survive_restart_and_refuse_changed_content() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("new.txt");
    let path = temp.path().join("operations.json");
    fs::write(&file, "").unwrap();
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(Action::new_file(file.clone()).unwrap())
        .unwrap();
    drop(journal);

    let mut journal = Journal::open(path).unwrap();
    fs::write(&file, "later content").unwrap();
    assert!(journal.undo().unwrap_err().to_string().contains("changed"));
    fs::write(&file, "").unwrap();
    journal.stored.entries[0].action = Action::new_file(file.clone()).unwrap();
    journal.save().unwrap();
    journal.undo().unwrap();
    assert!(!file.exists());
    journal.redo().unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"");
}

#[test]
fn copy_and_move_undo_redo_are_restart_safe() {
    let temp = tempfile::tempdir().unwrap();
    let journal_path = temp.path().join("operations.json");
    let source = temp.path().join("source");
    let copied = temp.path().join("copied");
    fs::write(&source, "content").unwrap();
    crate::fs::journal_copy(&source, &copied).unwrap();
    let copy_receipts = [crate::fs::TransferReceipt {
        source: source.clone(),
        destination: copied.clone(),
        replaced_existing: false,
    }];
    let mut journal = Journal::open(journal_path.clone()).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &copy_receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    drop(journal);

    let mut journal = Journal::open(journal_path.clone()).unwrap();
    journal.undo().unwrap();
    assert!(source.exists());
    assert!(!copied.exists());
    journal.redo().unwrap();
    assert_eq!(fs::read_to_string(&copied).unwrap(), "content");

    let moved = temp.path().join("moved");
    crate::fs::journal_move(&source, &moved).unwrap();
    let move_receipts = [crate::fs::TransferReceipt {
        source: source.clone(),
        destination: moved.clone(),
        replaced_existing: false,
    }];
    journal
        .record(
            Action::transfer(TransferKind::Move, &move_receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    journal.undo().unwrap();
    assert!(source.exists());
    assert!(!moved.exists());
    journal.redo().unwrap();
    assert!(!source.exists());
    assert!(moved.exists());
}

#[test]
fn undo_copy_replace_refuses_without_removing_the_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source/item.txt");
    let destination_directory = temp.path().join("destination");
    let destination = destination_directory.join("item.txt");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::create_dir(&destination_directory).unwrap();
    fs::write(&source, "incoming").unwrap();
    fs::write(&destination, "preexisting").unwrap();

    let crate::fs::TransferBatchOutcome::Conflict { batch, .. } = crate::fs::TransferBatch::new(
        vec![source.clone()],
        destination_directory,
        crate::transfer::Action::Copy,
    )
    .run() else {
        panic!("expected conflict");
    };
    let crate::fs::TransferBatchOutcome::Complete(report) = (*batch)
        .resolve(crate::fs::ConflictChoice::Replace, false)
        .run()
    else {
        panic!("expected completion");
    };
    assert_eq!(fs::read_to_string(&destination).unwrap(), "incoming");

    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &report.receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    let error = journal.undo().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("replaced an existing destination")
    );
    assert_eq!(fs::read_to_string(destination).unwrap(), "incoming");
    assert_eq!(fs::read_to_string(source).unwrap(), "incoming");
}

#[test]
fn undo_directory_merge_refuses_without_removing_preexisting_entries() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source/folder");
    let destination_directory = temp.path().join("destination");
    let destination = destination_directory.join("folder");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("incoming.txt"), "incoming").unwrap();
    fs::write(destination.join("preexisting.txt"), "preexisting").unwrap();

    let crate::fs::TransferBatchOutcome::Conflict { batch, .. } = crate::fs::TransferBatch::new(
        vec![source],
        destination_directory,
        crate::transfer::Action::Copy,
    )
    .run() else {
        panic!("expected conflict");
    };
    let crate::fs::TransferBatchOutcome::Complete(report) = (*batch)
        .resolve(crate::fs::ConflictChoice::Replace, false)
        .run()
    else {
        panic!("expected completion");
    };
    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &report.receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();

    let error = journal.undo().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("replaced an existing destination")
    );
    assert_eq!(
        fs::read_to_string(destination.join("preexisting.txt")).unwrap(),
        "preexisting"
    );
    assert_eq!(
        fs::read_to_string(destination.join("incoming.txt")).unwrap(),
        "incoming"
    );
}

#[test]
fn legacy_transfer_entries_refuse_undo_conservatively() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.txt");
    let destination = temp.path().join("destination.txt");
    fs::write(&source, "content").unwrap();
    fs::write(&destination, "content").unwrap();
    let action = Action::transfer(
        TransferKind::Copy,
        &[crate::fs::TransferReceipt {
            source,
            destination: destination.clone(),
            replaced_existing: false,
        }],
    )
    .unwrap()
    .unwrap();
    let mut encoded = serde_json::to_value(action).unwrap();
    encoded["Transfer"]["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("replaced_existing");
    let legacy = serde_json::from_value(encoded).unwrap();
    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    journal.record(legacy).unwrap();

    let error = journal.undo().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("replaced an existing destination")
    );
    assert_eq!(fs::read_to_string(destination).unwrap(), "content");
}

#[test]
fn copy_undo_refuses_a_changed_result_tree() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let copied = temp.path().join("copied");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file"), "content").unwrap();
    crate::fs::journal_copy(&source, &copied).unwrap();
    let receipts = [crate::fs::TransferReceipt {
        source,
        destination: copied.clone(),
        replaced_existing: false,
    }];
    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    fs::write(copied.join("later"), "user work").unwrap();

    assert!(
        journal
            .undo()
            .unwrap_err()
            .to_string()
            .contains("contents changed")
    );
    assert!(copied.join("later").exists());
}

#[test]
fn trash_undo_restores_only_the_verified_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    let trashed = temp.path().join("Trash/files/original");
    let info = temp.path().join("Trash/info/original.trashinfo");
    fs::create_dir_all(trashed.parent().unwrap()).unwrap();
    fs::create_dir_all(info.parent().unwrap()).unwrap();
    fs::write(&trashed, "content").unwrap();
    fs::write(&info, "[Trash Info]\nPath=/original\n").unwrap();
    let receipt = TrashReceipt {
        original: original.clone(),
        trashed: trashed.clone(),
        info: info.clone(),
    };
    let mut journal = Journal::open(temp.path().join("operations.json")).unwrap();
    journal
        .record(
            Action::trash(std::slice::from_ref(&receipt))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    fs::write(&trashed, "changed after trash").unwrap();
    assert!(journal.undo().unwrap_err().to_string().contains("changed"));
    assert!(!original.exists());

    fs::write(&trashed, "content").unwrap();
    journal.stored.entries[0].action = Action::trash(std::slice::from_ref(&receipt))
        .unwrap()
        .unwrap();
    journal.save().unwrap();
    journal.undo().unwrap();
    assert_eq!(fs::read_to_string(original).unwrap(), "content");
    assert!(!trashed.exists());
    assert!(!info.exists());
}

#[test]
fn restore_action_persists_the_restored_identity_for_later_undo() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("restored");
    fs::write(&original, "content").unwrap();
    let receipt = TrashReceipt {
        original,
        trashed: temp.path().join("Trash/files/restored"),
        info: temp.path().join("Trash/info/restored.trashinfo"),
    };
    let path = temp.path().join("operations.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::restore(std::slice::from_ref(&receipt), false)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    drop(journal);

    let reopened = Journal::open(path).unwrap();
    assert!(matches!(
        reopened.stored.entries[0].action,
        Action::Restore { .. }
    ));
}

#[cfg(unix)]
#[test]
fn trash_info_paths_decode_spaces_and_non_utf8_bytes() {
    use std::os::unix::ffi::OsStrExt;

    let decoded = percent_decode_path("/tmp/a%20name-%FF").unwrap();
    assert_eq!(decoded.as_bytes(), b"/tmp/a name-\xff");
}

#[test]
fn hunt_trash_undo_can_retry_after_metadata_cleanup_fails() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let files = temp.path().join("Trash/files");
    let info = temp.path().join("Trash/info");
    fs::create_dir_all(&files).unwrap();
    fs::create_dir_all(&info).unwrap();
    let receipts = ["first", "second"].map(|name| TrashReceipt {
        original: temp.path().join(name),
        trashed: files.join(name),
        info: if name == "second" {
            info.join("second.trashinfo")
        } else {
            temp.path().join("first.trashinfo")
        },
    });
    for receipt in &receipts {
        fs::write(&receipt.trashed, "recover me").unwrap();
        fs::write(&receipt.info, "metadata").unwrap();
    }
    let journal_path = temp.path().join("history.json");
    let mut journal = Journal::open(journal_path.clone()).unwrap();
    // Exercise compatibility with records saved before progress tracking existed.
    let mut legacy = serde_json::to_value(Action::trash(&receipts).unwrap().unwrap()).unwrap();
    for item in legacy["Trash"]["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("restore_pending");
    }
    journal
        .record(serde_json::from_value(legacy).unwrap())
        .unwrap();
    fs::set_permissions(&info, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = journal.undo();
    fs::set_permissions(&info, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert!(
        !receipts[0].info.exists(),
        "first item's cleanup already completed"
    );
    assert!(
        receipts[1].info.exists(),
        "second item's cleanup remains pending"
    );
    let mut journal = Journal::open(journal_path).unwrap();
    journal
        .undo()
        .expect("restoring Trash must remain retryable when metadata cleanup fails");
    for receipt in &receipts {
        assert_eq!(fs::read_to_string(&receipt.original).unwrap(), "recover me");
        assert!(!receipt.info.exists());
    }
    assert_eq!(journal.undo().unwrap_err().to_string(), "Nothing to undo");
}

#[test]
fn hunt_new_folder_undo_works_after_undoing_its_child_creation() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("folder");
    let child = folder.join("child.txt");
    let journal_path = temp.path().join("history.json");
    fs::create_dir(&folder).unwrap();
    // Pin the initial timestamp so this does not depend on filesystem clock resolution.
    fs::File::open(&folder)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000))
        .unwrap();
    let mut journal = Journal::open(journal_path.clone()).unwrap();
    journal
        .record(Action::new_folder(folder.clone()).unwrap())
        .unwrap();
    fs::write(&child, "").unwrap();
    journal
        .record(Action::new_file(child.clone()).unwrap())
        .unwrap();
    journal.undo().unwrap();
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
    let mut journal = Journal::open(journal_path).unwrap();
    journal
        .undo()
        .expect("the same now-empty folder should remain undoable");
    assert!(!folder.exists());
    journal.redo().unwrap();
    journal.redo().unwrap();
    assert!(child.is_file());
    journal.undo().unwrap();
    journal.undo().unwrap();
    assert!(!folder.exists());
}

#[test]
fn new_folder_undo_preserves_replacement_directories_and_symlinks() {
    for symlink in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("folder");
        let original = temp.path().join("original");
        fs::create_dir(&folder).unwrap();
        let mut journal = Journal::open(temp.path().join("history.json")).unwrap();
        journal
            .record(Action::new_folder(folder.clone()).unwrap())
            .unwrap();
        fs::rename(&folder, &original).unwrap();
        if symlink {
            std::os::unix::fs::symlink(&original, &folder).unwrap();
        } else {
            fs::create_dir(&folder).unwrap();
            fs::File::open(&folder)
                .unwrap()
                .set_modified(fs::metadata(&original).unwrap().modified().unwrap())
                .unwrap();
        }
        assert!(
            journal.undo().is_err(),
            "Undo must reject a different directory at the same path"
        );
        assert!(fs::symlink_metadata(&folder).is_ok());
        assert!(original.is_dir());
    }
}

#[test]
fn legacy_new_folder_records_still_undo_and_redo() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("folder");
    fs::create_dir(&folder).unwrap();
    let mut legacy = serde_json::to_value(Action::new_folder(folder.clone()).unwrap()).unwrap();
    legacy["NewFolder"]
        .as_object_mut()
        .unwrap()
        .remove("identity");
    let path = temp.path().join("history.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(serde_json::from_value(legacy).unwrap())
        .unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal.undo().unwrap();
    assert!(!folder.exists());
    journal.redo().unwrap();
    assert!(folder.is_dir());
    journal.undo().unwrap();
    assert!(!folder.exists());
}

#[test]
fn audit_redo_copy_preserves_hardlinks_between_selected_files() {
    use std::os::unix::fs::MetadataExt;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("a"), b"linked data").unwrap();
    fs::hard_link(source.join("a"), source.join("b")).unwrap();
    let crate::fs::TransferBatchOutcome::Complete(report) = crate::fs::TransferBatch::try_new(
        vec![source.join("a"), source.join("b")],
        target.clone(),
        crate::transfer::Action::Copy,
    )
    .unwrap()
    .run() else {
        panic!("no conflict")
    };
    assert!(report.failures.is_empty());
    assert_eq!(
        fs::metadata(target.join("a")).unwrap().ino(),
        fs::metadata(target.join("b")).unwrap().ino()
    );
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &report.receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    journal.undo().unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal.redo().unwrap();
    assert_eq!(
        fs::metadata(target.join("a")).unwrap().ino(),
        fs::metadata(target.join("b")).unwrap().ino(),
        "Redo broke links preserved by the original Copy"
    );
}

#[test]
fn audit_undo_cross_device_move_preserves_hardlinks() {
    use std::os::unix::fs::MetadataExt;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let target_root = tempfile::tempdir_in("/dev/shm").unwrap();
    let target = target_root.path().join("target");
    assert_ne!(
        fs::metadata(temp.path()).unwrap().dev(),
        fs::metadata(target_root.path()).unwrap().dev()
    );
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("a"), b"linked data").unwrap();
    fs::hard_link(source.join("a"), source.join("b")).unwrap();
    let crate::fs::TransferBatchOutcome::Complete(report) = crate::fs::TransferBatch::try_new(
        vec![source.join("a"), source.join("b")],
        target.clone(),
        crate::transfer::Action::Move,
    )
    .unwrap()
    .run() else {
        panic!("no conflict")
    };
    assert!(report.failures.is_empty());
    assert_eq!(
        fs::metadata(target.join("a")).unwrap().ino(),
        fs::metadata(target.join("b")).unwrap().ino()
    );
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Move, &report.receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    journal.undo().unwrap();
    assert_eq!(
        fs::metadata(source.join("a")).unwrap().ino(),
        fs::metadata(source.join("b")).unwrap().ino(),
        "Undo broke links preserved by cross-device Move"
    );
}

#[test]
fn undo_trash_across_filesystems_preserves_selected_hardlinks() {
    use std::os::unix::fs::MetadataExt;
    let temp = tempfile::tempdir().unwrap();
    let trash = tempfile::tempdir_in("/dev/shm").unwrap();
    assert_ne!(
        fs::metadata(temp.path()).unwrap().dev(),
        fs::metadata(trash.path()).unwrap().dev()
    );
    fs::write(trash.path().join("a"), b"linked contents").unwrap();
    fs::hard_link(trash.path().join("a"), trash.path().join("b")).unwrap();
    let receipts = ["a", "b"].map(|name| {
        let info = trash.path().join(format!("{name}.trashinfo"));
        fs::write(&info, "metadata").unwrap();
        TrashReceipt {
            original: temp.path().join(name),
            trashed: trash.path().join(name),
            info,
        }
    });
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(Action::trash(&receipts).unwrap().unwrap())
        .unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal.undo().unwrap();
    assert_eq!(
        fs::metadata(temp.path().join("a")).unwrap().ino(),
        fs::metadata(temp.path().join("b")).unwrap().ino()
    );
    for receipt in receipts {
        assert!(!receipt.trashed.exists() && !receipt.info.exists());
        assert_eq!(fs::read(receipt.original).unwrap(), b"linked contents");
    }
}
#[test]
fn audit_undo_copy_can_resume_after_partial_directory_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let copied = temp.path().join("copied");
    fs::create_dir(&source).unwrap();
    for name in ["first", "second"] {
        fs::create_dir(source.join(name)).unwrap();
        fs::write(source.join(name).join("data"), b"data").unwrap();
    }
    crate::fs::journal_copy(&source, &copied).unwrap();
    let locked = fs::read_dir(&copied)
        .unwrap()
        .last()
        .unwrap()
        .unwrap()
        .path();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Copy,
                &[crate::fs::TransferReceipt {
                    source: source.clone(),
                    destination: copied.clone(),
                    replaced_existing: false,
                }],
            )
            .unwrap()
            .unwrap(),
        )
        .unwrap();
    assert!(
        journal.undo().is_err(),
        "cleanup must fail at the protected child"
    );
    assert_eq!(
        fs::read_dir(&copied).unwrap().count(),
        1,
        "fixture must partially remove the copied tree"
    );
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal
        .undo()
        .expect("Undo must resume after its own partial deletion and repaired permissions");
    assert!(!copied.exists());
    assert!(source.join("first/data").exists() && source.join("second/data").exists());
    journal.redo().unwrap();
    assert_eq!(fs::read(copied.join("first/data")).unwrap(), b"data");
    assert_eq!(fs::read(copied.join("second/data")).unwrap(), b"data");
    journal.undo().unwrap();
    assert!(!copied.exists());
}

#[test]
fn resumed_copy_undo_preserves_external_changes_to_remaining_entries() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for change in ["contents", "replacement", "addition", "directory-symlink"] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let copied = temp.path().join("copied");
        fs::create_dir(&source).unwrap();
        for name in ["first", "second"] {
            fs::create_dir(source.join(name)).unwrap();
            fs::write(source.join(name).join("data"), b"data").unwrap();
        }
        crate::fs::journal_copy(&source, &copied).unwrap();
        let locked = fs::read_dir(&copied)
            .unwrap()
            .last()
            .unwrap()
            .unwrap()
            .path();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        let path = temp.path().join("journal.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        journal
            .record(
                Action::transfer(
                    TransferKind::Copy,
                    &[crate::fs::TransferReceipt {
                        source: source.clone(),
                        destination: copied.clone(),
                        replaced_existing: false,
                    }],
                )
                .unwrap()
                .unwrap(),
            )
            .unwrap();
        assert!(journal.undo().is_err());
        assert_eq!(fs::read_dir(&copied).unwrap().count(), 1);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        let remaining = locked.join("data");
        match change {
            "contents" => fs::write(&remaining, b"externally changed").unwrap(),
            "replacement" => {
                fs::rename(&remaining, temp.path().join("old-file")).unwrap();
                fs::write(&remaining, b"data").unwrap();
            }
            "addition" => fs::write(copied.join("new-file"), b"new data").unwrap(),
            "directory-symlink" => {
                let external = temp.path().join("external");
                fs::rename(&locked, &external).unwrap();
                std::os::unix::fs::symlink(&external, &locked).unwrap();
            }
            _ => unreachable!(),
        }
        let before = fs::read(&remaining).unwrap();
        let mut journal = Journal::open(path).unwrap();
        assert!(journal.undo().is_err(), "Undo accepted external {change}");
        assert_eq!(fs::read(&remaining).unwrap(), before);
        if change == "addition" {
            assert_eq!(fs::read(copied.join("new-file")).unwrap(), b"new data");
        }
        assert!(source.join("first/data").exists() && source.join("second/data").exists());
    }
}
