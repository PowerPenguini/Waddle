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

#[test]
fn redo_copy_resumes_after_a_later_failure_and_failed_rollback() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("source-one");
    let second = temp.path().join("source-two");
    let first_parent = temp.path().join("target-one");
    let second_parent = temp.path().join("target-two");
    fs::create_dir_all(first.join("locked")).unwrap();
    fs::create_dir(&first_parent).unwrap();
    fs::create_dir(&second_parent).unwrap();
    fs::write(first.join("locked/data"), b"first data").unwrap();
    fs::set_permissions(first.join("locked"), fs::Permissions::from_mode(0o500)).unwrap();
    fs::write(&second, b"second data").unwrap();
    let receipts = [
        (first.clone(), first_parent.join("copy")),
        (second, second_parent.join("copy")),
    ]
    .map(|(source, destination)| {
        crate::fs::journal_copy(&source, &destination).unwrap();
        crate::fs::TransferReceipt {
            source,
            destination,
            replaced_existing: false,
        }
    });
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    assert!(journal.undo().is_err());
    fs::set_permissions(
        receipts[0].destination.join("locked"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    journal.undo().unwrap();
    fs::set_permissions(&second_parent, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = journal.redo();
    fs::set_permissions(&second_parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert!(
        receipts[0].destination.join("locked/data").exists(),
        "fixture must leave an already copied item"
    );
    let mut journal = Journal::open(path).unwrap();
    let resumed = journal.redo();
    fs::set_permissions(first.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(
        receipts[0].destination.join("locked"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    resumed.expect("Redo must resume its own partial work after repairing permissions");
    assert_eq!(
        fs::read(receipts[0].destination.join("locked/data")).unwrap(),
        b"first data"
    );
    assert_eq!(fs::read(&receipts[1].destination).unwrap(), b"second data");
}

#[test]
fn partial_redo_preserves_completed_entries_and_checks_them_before_resuming() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for kind in [TransferKind::Copy, TransferKind::Move] {
        for change_completed in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let receipts = ["one", "two"].map(|name| {
                let source = temp.path().join(name);
                let parent = temp.path().join(format!("target-{name}"));
                fs::create_dir(&parent).unwrap();
                let destination = parent.join(name);
                fs::write(&source, name).unwrap();
                match kind {
                    TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
                    TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
                }
                crate::fs::TransferReceipt {
                    source,
                    destination,
                    replaced_existing: false,
                }
            });
            let path = temp.path().join("journal.json");
            let mut journal = Journal::open(path.clone()).unwrap();
            journal
                .record(Action::transfer(kind, &receipts).unwrap().unwrap())
                .unwrap();
            journal.undo().unwrap();
            let blocked = receipts[1].destination.parent().unwrap();
            fs::set_permissions(blocked, fs::Permissions::from_mode(0o500)).unwrap();
            let failed = journal.redo();
            fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(failed.is_err());
            let completed = &receipts[0].destination;
            assert_eq!(fs::read(completed).unwrap(), b"one");
            let inode = fs::metadata(completed).unwrap().ino();
            assert!(!receipts[1].destination.exists());
            assert_eq!(
                receipts[0].source.exists(),
                matches!(kind, TransferKind::Copy)
            );
            if change_completed {
                fs::write(completed, "external edit").unwrap();
            }
            let mut journal = Journal::open(path).unwrap();
            let resumed = journal.redo();
            if change_completed {
                assert!(
                    resumed.is_err(),
                    "a changed completed entry must block the retry"
                );
                assert_eq!(fs::read(completed).unwrap(), b"external edit");
                assert!(!receipts[1].destination.exists());
                assert_eq!(fs::read(&receipts[1].source).unwrap(), b"two");
            } else {
                resumed.unwrap();
                assert_eq!(fs::metadata(completed).unwrap().ino(), inode);
                assert_eq!(fs::read(&receipts[1].destination).unwrap(), b"two");
                journal.undo().unwrap();
                for receipt in &receipts {
                    assert!(receipt.source.exists());
                    assert!(!receipt.destination.exists());
                }
                journal.redo().unwrap();
                assert_eq!(fs::read(completed).unwrap(), b"one");
                assert_eq!(fs::read(&receipts[1].destination).unwrap(), b"two");
            }
        }
    }
}

#[test]
fn partial_history_retry_preserves_hardlinks_after_reopening_the_journal() {
    use std::os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, PermissionsExt},
    };
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for kind in [TransferKind::Copy, TransferKind::Move] {
        let temp = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir_in("/dev/shm").unwrap();
        assert_ne!(
            fs::metadata(temp.path()).unwrap().dev(),
            fs::metadata(target.path()).unwrap().dev()
        );
        let first = temp
            .path()
            .join("first")
            .join(std::ffi::OsString::from_vec(b"a-\xff".to_vec()));
        let second = temp.path().join("second/b");
        let first_target = target.path().join("first").join(first.file_name().unwrap());
        let second_target = target.path().join("second/b");
        for path in [&first, &second, &first_target, &second_target] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        fs::write(&first, b"linked history data").unwrap();
        fs::hard_link(&first, &second).unwrap();
        let action = match kind {
            TransferKind::Copy => crate::transfer::Action::Copy,
            TransferKind::Move => crate::transfer::Action::Move,
        };
        let mut transfer = crate::fs::JournalTransfer::default();
        let receipts = [
            (first.clone(), first_target.clone()),
            (second.clone(), second_target.clone()),
        ]
        .map(|(source, destination)| {
            transfer.apply(action, &source, &destination).unwrap();
            crate::fs::TransferReceipt {
                source,
                destination,
                replaced_existing: false,
            }
        });
        let journal_path = temp.path().join("journal.json");
        let mut journal = Journal::open(journal_path.clone()).unwrap();
        journal
            .record(Action::transfer(kind, &receipts).unwrap().unwrap())
            .unwrap();
        let blocked = match kind {
            TransferKind::Copy => {
                journal.undo().unwrap();
                second_target.parent().unwrap()
            }
            TransferKind::Move => first.parent().unwrap(),
        };
        fs::set_permissions(blocked, fs::Permissions::from_mode(0o500)).unwrap();
        let failed = match kind {
            TransferKind::Copy => journal.redo(),
            TransferKind::Move => journal.undo(),
        };
        fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(failed.is_err());
        let mut journal = Journal::open(journal_path).unwrap();
        let (left, right) = match kind {
            TransferKind::Copy => {
                journal.redo().unwrap();
                (&first_target, &second_target)
            }
            TransferKind::Move => {
                journal.undo().unwrap();
                (&first, &second)
            }
        };
        assert_eq!(fs::read(left).unwrap(), b"linked history data");
        assert_eq!(fs::read(right).unwrap(), b"linked history data");
        assert_eq!(
            fs::metadata(left).unwrap().ino(),
            fs::metadata(right).unwrap().ino(),
            "retrying the same history operation must preserve its hardlinks across journal reopen"
        );
    }
}

#[test]
fn resumed_move_undo_refuses_edited_completed_files_even_when_timestamps_match() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir_in("/dev/shm").unwrap();
    let first = temp.path().join("first/a");
    let second = temp.path().join("second/b");
    fs::create_dir_all(first.parent().unwrap()).unwrap();
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    fs::write(&first, b"original").unwrap();
    fs::hard_link(&first, &second).unwrap();
    let mut transfer = crate::fs::JournalTransfer::default();
    let receipts = [first.clone(), second.clone()].map(|source| {
        let destination = target.path().join(source.file_name().unwrap());
        transfer
            .apply(crate::transfer::Action::Move, &source, &destination)
            .unwrap();
        crate::fs::TransferReceipt {
            source,
            destination,
            replaced_existing: false,
        }
    });
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Move, &receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    fs::set_permissions(first.parent().unwrap(), fs::Permissions::from_mode(0o500)).unwrap();
    let failed = journal.undo();
    fs::set_permissions(first.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    let modified = fs::metadata(&second).unwrap().modified().unwrap();
    fs::write(&second, b"modified").unwrap();
    fs::File::open(&second)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let mut journal = Journal::open(path).unwrap();
    assert!(
        journal.undo().is_err(),
        "Undo must verify completed source contents before reusing their hardlinks"
    );
    assert!(!first.exists());
    assert_eq!(fs::read(&second).unwrap(), b"modified");
    assert_eq!(fs::read(&receipts[0].destination).unwrap(), b"original");
}

#[test]
fn trash_undo_preserves_hardlinks_after_cleanup_failure_and_journal_reopen() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let restored = tempfile::tempdir_in("/dev/shm").unwrap();
    let files = temp.path().join("Trash/files");
    let info = temp.path().join("Trash/info");
    fs::create_dir_all(&files).unwrap();
    fs::create_dir(&info).unwrap();
    fs::write(files.join("a"), b"linked contents").unwrap();
    fs::hard_link(files.join("a"), files.join("b")).unwrap();
    let receipts = ["a", "b"].map(|name| {
        let info = info.join(format!("{name}.trashinfo"));
        fs::write(&info, "fixture metadata").unwrap();
        TrashReceipt {
            original: restored.path().join(name),
            trashed: files.join(name),
            info,
        }
    });
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(Action::trash(&receipts).unwrap().unwrap())
        .unwrap();
    let newer = temp.path().join("unrelated-newer.txt");
    fs::write(&newer, "").unwrap();
    journal
        .record(Action::new_file(newer.clone()).unwrap())
        .unwrap();
    journal.undo().unwrap();
    fs::set_permissions(&info, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = journal.undo();
    fs::set_permissions(&info, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert!(receipts[0].original.exists());
    assert!(receipts[1].trashed.exists());
    let mut journal = Journal::open(path).unwrap();
    assert!(
        journal
            .redo()
            .unwrap_err()
            .to_string()
            .contains("retry Undo")
    );
    assert!(!newer.exists());
    journal.undo().unwrap();
    for receipt in &receipts {
        assert_eq!(fs::read(&receipt.original).unwrap(), b"linked contents");
        assert!(!receipt.trashed.exists());
        assert!(!receipt.info.exists());
    }
    assert_eq!(
        fs::metadata(&receipts[0].original).unwrap().ino(),
        fs::metadata(&receipts[1].original).unwrap().ino()
    );
}

#[test]
fn transfer_history_without_saved_link_context_still_supports_undo_and_redo() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::write(&source, "legacy history").unwrap();
    crate::fs::journal_copy(&source, &destination).unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Copy,
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
    // Exercise the older on-disk schema, which did not include this optional field.
    let mut legacy: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    legacy["entries"][0]["action"]["Transfer"]
        .as_object_mut()
        .unwrap()
        .remove("transfer");
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut journal = Journal::open(path).unwrap();
    journal.undo().unwrap();
    assert!(!destination.exists());
    journal.redo().unwrap();
    assert_eq!(fs::read(source).unwrap(), b"legacy history");
    assert_eq!(fs::read(destination).unwrap(), b"legacy history");
}

#[test]
fn undo_does_not_cross_an_incomplete_redo_into_an_older_operation() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let previous = temp.path().join("previous.txt");
    fs::write(&previous, "unrelated earlier operation").unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(Action::new_file(previous.clone()).unwrap())
        .unwrap();
    let receipts = ["one", "two"].map(|name| {
        let source = temp.path().join(name);
        let destination = temp.path().join(format!("target-{name}/{name}"));
        fs::create_dir(destination.parent().unwrap()).unwrap();
        fs::write(&source, name).unwrap();
        crate::fs::journal_copy(&source, &destination).unwrap();
        crate::fs::TransferReceipt {
            source,
            destination,
            replaced_existing: false,
        }
    });
    journal
        .record(
            Action::transfer(TransferKind::Copy, &receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    journal.undo().unwrap();
    let blocked = receipts[1].destination.parent().unwrap();
    fs::set_permissions(blocked, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = journal.redo();
    fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    let mut journal = Journal::open(path).unwrap();
    let undo = journal.undo();
    assert!(
        undo.is_err(),
        "Undo must not affect older history while Redo is incomplete"
    );
    assert!(undo.unwrap_err().to_string().contains("retry Redo"));
    assert_eq!(fs::read(&previous).unwrap(), b"unrelated earlier operation");
    assert!(receipts[0].destination.exists());
    assert!(!receipts[1].destination.exists());
    journal.redo().unwrap();
    journal.undo().unwrap();
    assert!(previous.exists());
    journal.undo().unwrap();
    assert!(!previous.exists());
}

#[test]
fn redo_does_not_cross_an_incomplete_undo_into_a_newer_operation() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    let receipts = ["one", "two"].map(|name| {
        let source = temp.path().join(name);
        let destination = temp.path().join(format!("target-{name}/{name}"));
        fs::create_dir(destination.parent().unwrap()).unwrap();
        fs::write(&source, name).unwrap();
        crate::fs::journal_copy(&source, &destination).unwrap();
        crate::fs::TransferReceipt {
            source,
            destination,
            replaced_existing: false,
        }
    });
    journal
        .record(
            Action::transfer(TransferKind::Copy, &receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    let newer = temp.path().join("newer.txt");
    fs::write(&newer, "").unwrap();
    journal
        .record(Action::new_file(newer.clone()).unwrap())
        .unwrap();
    journal.undo().unwrap();
    let blocked = receipts[0].destination.parent().unwrap();
    fs::set_permissions(blocked, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = journal.undo();
    fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert!(!receipts[1].destination.exists());
    let mut journal = Journal::open(path).unwrap();
    let redo = journal.redo();
    assert!(
        !newer.exists(),
        "Redo must not recreate unrelated newer history while Undo is incomplete"
    );
    assert!(redo.unwrap_err().to_string().contains("retry Undo"));
    assert!(receipts[0].destination.exists());
    journal.undo().unwrap();
    journal.redo().unwrap();
    for receipt in &receipts {
        assert!(receipt.destination.exists());
    }
    assert!(!newer.exists());
    journal.redo().unwrap();
    assert!(newer.exists());
}

#[test]
fn partial_single_directory_undo_keeps_newer_history_blocked_until_cleanup_finishes() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    for name in ["first", "second"] {
        fs::create_dir(source.join(name)).unwrap();
        fs::write(source.join(name).join("data"), "needs permission repair").unwrap();
    }
    crate::fs::journal_copy(&source, &destination).unwrap();
    let locked = fs::read_dir(&destination)
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
                    destination: destination.clone(),
                    replaced_existing: false,
                }],
            )
            .unwrap()
            .unwrap(),
        )
        .unwrap();
    let newer = temp.path().join("newer");
    fs::write(&newer, "").unwrap();
    journal
        .record(Action::new_file(newer.clone()).unwrap())
        .unwrap();
    journal.undo().unwrap();
    let failed = journal.undo();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    let mut journal = Journal::open(path).unwrap();
    assert!(
        journal
            .redo()
            .unwrap_err()
            .to_string()
            .contains("retry Undo")
    );
    assert!(!newer.exists());
    assert_eq!(
        fs::read(locked.join("data")).unwrap(),
        b"needs permission repair"
    );
    journal.undo().unwrap();
    assert!(!destination.exists());
}

#[test]
fn new_operations_preserve_partial_redo_and_resume_after_their_undo() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for kind in [TransferKind::Copy, TransferKind::Move] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journal.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        let receipts = ["one", "two"].map(|name| {
            let source = temp.path().join(name);
            let destination = temp.path().join(format!("target-{name}/{name}"));
            fs::create_dir(destination.parent().unwrap()).unwrap();
            fs::write(&source, name).unwrap();
            match kind {
                TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
                TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
            }
            crate::fs::TransferReceipt {
                source,
                destination,
                replaced_existing: false,
            }
        });
        journal
            .record(Action::transfer(kind, &receipts).unwrap().unwrap())
            .unwrap();
        journal.undo().unwrap();
        let blocked = receipts[1].destination.parent().unwrap();
        fs::set_permissions(blocked, fs::Permissions::from_mode(0o500)).unwrap();
        let failed = journal.redo();
        fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(failed.is_err());
        let renamed = temp.path().join("renamed");
        fs::rename(&receipts[0].destination, &renamed).unwrap();
        journal
            .record(Action::rename(receipts[0].destination.clone(), renamed.clone()).unwrap())
            .unwrap();
        let newer = temp.path().join("newer");
        fs::write(&newer, "").unwrap();
        journal
            .record(Action::new_file(newer.clone()).unwrap())
            .unwrap();
        let mut journal = Journal::open(path.clone()).unwrap();
        journal.undo().unwrap();
        journal.undo().unwrap();
        assert_eq!(fs::read(&receipts[0].destination).unwrap(), b"one");
        let error = journal.undo().unwrap_err().to_string();
        assert!(
            error.contains("retry Redo"),
            "incomplete transfer was lost: {error}"
        );
        let mut journal = Journal::open(path.clone()).unwrap();
        fs::set_permissions(blocked, fs::Permissions::from_mode(0o500)).unwrap();
        let failed_again = journal.redo();
        fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(failed_again.is_err());
        let mut journal = Journal::open(path).unwrap();
        journal
            .redo()
            .expect("resume the retained partial transfer after another failure");
        assert_eq!(fs::read(&receipts[1].destination).unwrap(), b"two");
        assert!(
            !renamed.exists(),
            "resuming must not replay the newer rename"
        );
        journal.undo().unwrap();
        for receipt in &receipts {
            assert!(!receipt.destination.exists());
            assert!(receipt.source.exists());
        }
        journal.redo().unwrap();
        journal.redo().unwrap();
        journal.redo().unwrap();
        assert_eq!(fs::read(&renamed).unwrap(), b"one");
        assert!(newer.exists());
    }
}

#[test]
fn partial_retrashing_preserves_metadata_and_retries_after_reopen() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for restore in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let files = temp.path().join("Trash/files");
        let info = temp.path().join("Trash/info");
        fs::create_dir_all(&files).unwrap();
        fs::create_dir_all(&info).unwrap();
        let receipts = ["one", "two"].map(|name| {
            let original = temp.path().join(format!("source-{name}/{name}"));
            fs::create_dir(original.parent().unwrap()).unwrap();
            let receipt = TrashReceipt {
                original,
                trashed: files.join(name),
                info: info.join(format!("{name}.trashinfo")),
            };
            fs::write(
                if restore {
                    &receipt.original
                } else {
                    &receipt.trashed
                },
                name,
            )
            .unwrap();
            if !restore {
                fs::write(&receipt.info, "original metadata").unwrap();
            }
            receipt
        });
        let path = temp.path().join("journal.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        if restore {
            journal
                .record(Action::restore(&receipts, false).unwrap().unwrap())
                .unwrap();
        } else {
            journal
                .record(Action::trash(&receipts).unwrap().unwrap())
                .unwrap();
            journal.undo().unwrap();
        }
        let blocked = receipts[0].original.parent().unwrap().to_path_buf();
        let blocked_backend = blocked.clone();
        let expected_file = files.join("new-one");
        let expected_info = info.join("new-one.trashinfo");
        let mut fail_once = true;
        trash_receipt::test_backend::with(
            move |source| {
                let name = source.file_name().unwrap().to_str().unwrap();
                if name == "two" && fail_once {
                    fail_once = false;
                    // Model a mount/permission change after the first successful Trash.
                    fs::set_permissions(&blocked_backend, fs::Permissions::from_mode(0o500))
                        .unwrap();
                    return Err(Error::message("desktop Trash failed for second entry"));
                }
                let receipt = TrashReceipt {
                    original: source.to_path_buf(),
                    trashed: files.join(format!("new-{name}")),
                    info: info.join(format!("new-{name}.trashinfo")),
                };
                fs::rename(source, &receipt.trashed).unwrap();
                fs::write(
                    &receipt.info,
                    format!("[Trash Info]\nPath={}\n", source.display()),
                )
                .unwrap();
                Ok(receipt)
            },
            || {
                let failed = if restore {
                    journal.undo()
                } else {
                    journal.redo()
                };
                fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
                assert!(failed.is_err());
                assert_eq!(fs::read(&expected_file).unwrap(), b"one");
                assert!(
                    expected_info.exists(),
                    "failed rollback must not orphan the first Trash entry"
                );
                let newer = temp.path().join("newer");
                fs::write(&newer, "").unwrap();
                journal
                    .record(Action::new_file(newer.clone()).unwrap())
                    .unwrap();
                let mut journal = Journal::open(path).unwrap();
                journal.undo().unwrap();
                assert!(!newer.exists());
                // Refuse to resume if a completed Trash entry was edited.
                let modified = fs::metadata(&expected_file).unwrap().modified().unwrap();
                fs::write(&expected_file, "external edit").unwrap();
                let changed = if restore {
                    journal.undo()
                } else {
                    journal.redo()
                };
                assert!(changed.is_err());
                assert_eq!(fs::read(&receipts[1].original).unwrap(), b"two");
                assert_eq!(fs::read(&expected_file).unwrap(), b"external edit");
                assert!(expected_info.exists());
                fs::write(&expected_file, "one").unwrap();
                fs::File::open(&expected_file)
                    .unwrap()
                    .set_modified(modified)
                    .unwrap();
                let opposite = if restore {
                    journal.redo()
                } else {
                    journal.undo()
                };
                assert!(
                    opposite.is_err(),
                    "partial Trash must block the opposite history direction"
                );
                if restore {
                    journal.undo().unwrap();
                } else {
                    journal.redo().unwrap();
                }
                assert!(expected_info.exists());
                for receipt in &receipts {
                    assert!(!receipt.original.exists());
                }
                if restore {
                    journal.redo().unwrap();
                } else {
                    journal.undo().unwrap();
                }
                assert!(!expected_info.exists());
                for receipt in &receipts {
                    assert_eq!(
                        fs::read(&receipt.original).unwrap(),
                        receipt.original.file_name().unwrap().as_encoded_bytes()
                    );
                }
            },
        );
    }
}

include!("../../.scratch/transfer-audit-14/history_probes.rs");

include!("../../.scratch/transfer-round-16/trash_probes.rs");

#[cfg(target_os = "linux")]
fn set_test_attribute(path: &std::path::Path, name: &str, value: &[u8]) {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = CString::new(name).unwrap();
    // SAFETY: path/name are NUL-terminated; value is live for this call.
    assert_eq!(
        unsafe {
            libc::lsetxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn copy_undo_preserves_metadata_edits_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    fs::write(&source, b"unchanged contents").unwrap();
    crate::fs::journal_copy(&source, &destination).unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Copy,
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
    set_test_attribute(
        &destination,
        "user.comment",
        b"new information added after copying",
    );
    let mut journal = Journal::open(path).unwrap();
    let result = journal.undo();
    assert!(
        result.is_err(),
        "Undo silently discarded metadata edited after Copy: {result:?}"
    );
    assert_eq!(fs::read(&destination).unwrap(), b"unchanged contents");
    assert_eq!(fs::read(&source).unwrap(), b"unchanged contents");
}

#[cfg(target_os = "linux")]
#[test]
fn resumed_copy_undo_preserves_new_directory_attributes() {
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    fs::create_dir_all(source.join("locked")).unwrap();
    fs::write(source.join("locked/data"), b"keep this tree").unwrap();
    fs::set_permissions(source.join("locked"), fs::Permissions::from_mode(0o500)).unwrap();
    crate::fs::journal_copy(&source, &destination).unwrap();
    fs::set_permissions(source.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Copy,
                &[crate::fs::TransferReceipt {
                    source,
                    destination: destination.clone(),
                    replaced_existing: false,
                }],
            )
            .unwrap()
            .unwrap(),
        )
        .unwrap();
    assert!(
        journal.undo().is_err(),
        "fixture must pause before deleting the locked file"
    );
    fs::set_permissions(
        destination.join("locked"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    set_test_attribute(&destination, "user.comment", b"new folder annotation");
    let mut journal = Journal::open(path).unwrap();
    let result = journal.undo();
    assert!(
        result.is_err(),
        "resumed Undo silently deleted a newly annotated directory: {result:?}"
    );
    assert_eq!(
        fs::read(destination.join("locked/data")).unwrap(),
        b"keep this tree"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn transfer_history_rejects_attribute_changes_in_both_directions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for kind in [TransferKind::Copy, TransferKind::Move] {
        for redo in [false, true] {
            for change in ["file-tag", "removed-tag", "directory-tag", "acl"] {
                let temp = tempfile::tempdir().unwrap();
                let source = temp.path().join("source");
                let destination = temp.path().join("result");
                fs::create_dir(&source).unwrap();
                fs::write(source.join("data"), b"original content").unwrap();
                fs::set_permissions(source.join("data"), fs::Permissions::from_mode(0o640))
                    .unwrap();
                set_test_attribute(&source.join("data"), "user.comment", b"original annotation");
                match kind {
                    TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
                    TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
                }
                let path = temp.path().join("journal.json");
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
                if redo {
                    journal.undo().unwrap();
                }
                let retained = if redo { &source } else { &destination };
                let file = retained.join("data");
                let before = fs::metadata(&file).unwrap();
                match change {
                    "file-tag" => set_test_attribute(&file, "user.comment", b"updated annotation"),
                    "removed-tag" => {
                        use std::{ffi::CString, os::unix::ffi::OsStrExt};
                        let path = CString::new(file.as_os_str().as_bytes()).unwrap();
                        // SAFETY: both arguments are live NUL-terminated strings.
                        assert_eq!(
                            unsafe { libc::lremovexattr(path.as_ptr(), c"user.comment".as_ptr()) },
                            0
                        );
                    }
                    "directory-tag" => {
                        set_test_attribute(retained, "user.comment", b"new folder annotation")
                    }
                    _ => {
                        // Add named-user read access while preserving the 0640 mode.
                        let mut acl = 2_u32.to_le_bytes().to_vec();
                        for (tag, permissions, id) in [
                            (1_u16, 6_u16, u32::MAX),
                            (2, 4, 65534),
                            (4, 4, u32::MAX),
                            (16, 4, u32::MAX),
                            (32, 0, u32::MAX),
                        ] {
                            acl.extend_from_slice(&tag.to_le_bytes());
                            acl.extend_from_slice(&permissions.to_le_bytes());
                            acl.extend_from_slice(&id.to_le_bytes());
                        }
                        set_test_attribute(&file, "system.posix_acl_access", &acl);
                    }
                }
                let after = fs::metadata(&file).unwrap();
                assert_eq!(before.mode(), after.mode());
                assert_eq!(before.modified().unwrap(), after.modified().unwrap());
                let mut journal = Journal::open(path).unwrap();
                let result = if redo { journal.redo() } else { journal.undo() };
                assert!(
                    result.is_err(),
                    "{kind:?} redo={redo} ignored {change}: {result:?}"
                );
                assert_eq!(fs::read(file).unwrap(), b"original content");
            }
        }
    }
}

#[test]
fn legacy_transfer_fingerprints_keep_undo_redo_available() {
    fn remove_new_fields(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                object.remove("attributes_digest");
                object.remove("directory_attributes");
                for value in object.values_mut() {
                    remove_new_fields(value);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    remove_new_fields(value);
                }
            }
            _ => {}
        }
    }
    for kind in [TransferKind::Copy, TransferKind::Move] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("result");
        fs::write(&source, b"legacy contents").unwrap();
        match kind {
            TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
            TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
        }
        let path = temp.path().join("journal.json");
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
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        remove_new_fields(&mut legacy);
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut journal = Journal::open(path.clone()).unwrap();
        journal.undo().unwrap();
        assert!(!destination.exists());
        assert_eq!(fs::read(&source).unwrap(), b"legacy contents");
        let mut journal = Journal::open(path).unwrap();
        journal.redo().unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"legacy contents");
        assert_eq!(source.exists(), matches!(kind, TransferKind::Copy));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn trash_and_restore_history_preserve_attribute_edits() {
    for restore in [false, true] {
        for redo in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let receipt = TrashReceipt {
                original: temp.path().join("original"),
                trashed: temp.path().join("Trash/files/item"),
                info: temp.path().join("Trash/info/item.trashinfo"),
            };
            fs::create_dir_all(receipt.trashed.parent().unwrap()).unwrap();
            fs::create_dir_all(receipt.info.parent().unwrap()).unwrap();
            fs::write(
                if restore {
                    &receipt.original
                } else {
                    &receipt.trashed
                },
                b"keep this item",
            )
            .unwrap();
            if !restore {
                fs::write(&receipt.info, "private fixture metadata").unwrap();
            }
            let path = temp.path().join("journal.json");
            let mut journal = Journal::open(path.clone()).unwrap();
            let action = if restore {
                Action::restore(std::slice::from_ref(&receipt), false)
            } else {
                Action::trash(std::slice::from_ref(&receipt))
            };
            journal.record(action.unwrap().unwrap()).unwrap();
            let backend_receipt = receipt.clone();
            trash_receipt::test_backend::with(
                move |source| {
                    fs::rename(source, &backend_receipt.trashed).unwrap();
                    fs::write(&backend_receipt.info, "private fixture metadata").unwrap();
                    Ok(backend_receipt.clone())
                },
                || {
                    if redo {
                        journal.undo().unwrap();
                    }
                    let retained = if restore != redo {
                        &receipt.original
                    } else {
                        &receipt.trashed
                    };
                    set_test_attribute(retained, "user.comment", b"new annotation");
                    let mut journal = Journal::open(path).unwrap();
                    let result = if redo { journal.redo() } else { journal.undo() };
                    assert!(
                        result.is_err(),
                        "restore={restore} redo={redo} ignored updated metadata: {result:?}"
                    );
                    assert_eq!(fs::read(retained).unwrap(), b"keep this item");
                },
            );
        }
    }
}

#[test]
fn recording_transfer_history_preserves_preexisting_temporary_entries() {
    for kind in ["symlink", "hardlink", "file", "directory"] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("copy");
        fs::write(&source, b"copied data").unwrap();
        crate::fs::journal_copy(&source, &destination).unwrap();
        let path = temp.path().join("journal.json");
        let temporary = path.with_extension("json.tmp");
        let unrelated = temp.path().join("unrelated");
        fs::write(&unrelated, b"must not be overwritten").unwrap();
        match kind {
            "symlink" => std::os::unix::fs::symlink(&unrelated, &temporary).unwrap(),
            "hardlink" => fs::hard_link(&unrelated, &temporary).unwrap(),
            "file" => fs::write(&temporary, b"must not be overwritten").unwrap(),
            _ => fs::create_dir(&temporary).unwrap(),
        }
        let mut journal = Journal::open(path.clone()).unwrap();
        let result = journal.record(
            Action::transfer(
                TransferKind::Copy,
                &[crate::fs::TransferReceipt {
                    source,
                    destination: destination.clone(),
                    replaced_existing: false,
                }],
            )
            .unwrap()
            .unwrap(),
        );
        assert_eq!(
            fs::read(&unrelated).unwrap(),
            b"must not be overwritten",
            "journal save followed {kind}"
        );
        if kind == "directory" {
            assert!(temporary.is_dir());
        } else {
            assert_eq!(fs::read(&temporary).unwrap(), b"must not be overwritten");
            if kind == "symlink" {
                assert_eq!(fs::read_link(&temporary).unwrap(), unrelated);
            }
        }
        result.expect("an occupied temporary name must not disable transfer history");
        let mut reopened = Journal::open(path).unwrap();
        reopened.undo().unwrap();
        assert!(!destination.exists());
    }
}

#[test]
fn saved_transfer_history_is_private_to_its_owner() {
    use std::os::unix::fs::MetadataExt;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("private-source");
    let destination = temp.path().join("copy");
    fs::write(&source, b"private paths must stay private").unwrap();
    crate::fs::journal_copy(&source, &destination).unwrap();
    let path = temp.path().join("journal.json");
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Copy,
                &[crate::fs::TransferReceipt {
                    source,
                    destination,
                    replaced_existing: false,
                }],
            )
            .unwrap()
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().mode() & 0o777,
        0o600,
        "transfer history exposes file paths and metadata to other users"
    );
}

#[test]
#[ignore = "Child process helper for isolated journal synchronization failure"]
fn journal_save_fault_child() {
    let root =
        PathBuf::from(std::env::var_os("WADDLE_JOURNAL_SAVE_FIXTURE").expect("parent fixture"));
    let mut journal = Journal::open(root.join("state/journal.json")).unwrap();
    let result = journal.record(
        Action::transfer(
            TransferKind::Copy,
            &[crate::fs::TransferReceipt {
                source: root.join("source"),
                destination: root.join("copy"),
                replaced_existing: false,
            }],
        )
        .unwrap()
        .unwrap(),
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("could not flush operation journal")
    );
}

#[test]
fn failed_journal_sync_keeps_previous_history_and_removes_owned_temporary_file() {
    let shim = audit_fault_library();
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let path = state.join("journal.json");
    let older = temp.path().join("older");
    fs::write(&older, b"older operation").unwrap();
    let mut journal = Journal::open(path.clone()).unwrap();
    journal
        .record(Action::new_file(older.clone()).unwrap())
        .unwrap();
    let before = fs::read(&path).unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    fs::write(&source, b"new transfer").unwrap();
    crate::fs::journal_copy(&source, &destination).unwrap();
    let armed = temp.path().join("armed");
    fs::write(&armed, "").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "journal::tests::journal_save_fault_child",
            "--ignored",
            "--nocapture",
        ])
        .env("LD_PRELOAD", shim.path().join("open_fault.so"))
        .env("WADDLE_JOURNAL_SAVE_FIXTURE", temp.path())
        .env("WADDLE_AUDIT_SYNC_TARGET", &state)
        .env("WADDLE_AUDIT_SYNC_ERRNO", libc::EIO.to_string())
        .env("WADDLE_AUDIT_ARMED", &armed)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!armed.exists(), "fsync fault must be reached");
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut entries = fs::read_dir(&state)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [
            std::ffi::OsString::from("journal.json"),
            "journal.lock".into()
        ],
        "failed save left a temporary file"
    );
    let mut journal = Journal::open(path).unwrap();
    journal
        .record(
            Action::transfer(
                TransferKind::Copy,
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
    assert!(!destination.exists());
    assert_eq!(fs::read(source).unwrap(), b"new transfer");
    assert_eq!(fs::read(older).unwrap(), b"older operation");
}

#[test]
fn failed_history_checkpoint_does_not_orphan_prepared_copy() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for kind in [TransferKind::Copy, TransferKind::Move] {
        for redo in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let inputs = temp.path().join("inputs");
            fs::create_dir(&inputs).unwrap();
            let source = inputs.join("source");
            let target = tempfile::tempdir_in("/dev/shm").unwrap();
            assert_ne!(
                fs::metadata(&inputs).unwrap().dev(),
                fs::metadata(target.path()).unwrap().dev()
            );
            let state = temp.path().join("state");
            fs::write(&source, b"retry without leaking prepared data").unwrap();
            let destination = target.path().join("copy");
            match kind {
                TransferKind::Copy => crate::fs::journal_copy(&source, &destination).unwrap(),
                TransferKind::Move => crate::fs::journal_move(&source, &destination).unwrap(),
            }
            let path = state.join("journal.json");
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
            if redo {
                journal.undo().unwrap();
            }
            let input_count = fs::read_dir(&inputs).unwrap().count();
            let target_count = fs::read_dir(target.path()).unwrap().count();
            fs::set_permissions(&state, fs::Permissions::from_mode(0o500)).unwrap();
            let failed = if redo { journal.redo() } else { journal.undo() };
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(failed.is_err());
            assert_eq!(
                fs::read_dir(&inputs).unwrap().count(),
                input_count,
                "{kind:?} redo={redo}: source staging leaked"
            );
            assert_eq!(
                fs::read_dir(target.path()).unwrap().count(),
                target_count,
                "{kind:?} redo={redo}: destination staging leaked"
            );
            let mut journal = Journal::open(path).unwrap();
            if redo {
                journal.redo().unwrap();
            } else {
                journal.undo().unwrap();
            }
            assert_eq!(source.exists(), matches!(kind, TransferKind::Copy) || !redo);
            assert_eq!(destination.exists(), redo);
            let retained = if redo { &destination } else { &source };
            assert_eq!(
                fs::read(retained).unwrap(),
                b"retry without leaking prepared data"
            );
            assert_eq!(
                fs::read_dir(&inputs).unwrap().count(),
                usize::from(source.exists())
            );
            assert_eq!(
                fs::read_dir(target.path()).unwrap().count(),
                usize::from(destination.exists())
            );
        }
    }
}

#[test]
fn history_checkpoint_sync_errors_distinguish_saved_and_unsaved_intent() {
    use std::os::unix::fs::MetadataExt;
    let shim = audit_fault_library();
    for committed in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("a"), b"prepared once").unwrap();
        fs::hard_link(source.join("a"), source.join("b")).unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        let destination = target.join("copy");
        crate::fs::journal_copy(&source, &destination).unwrap();
        let state = temp.path().join("state");
        let path = state.join("journal.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        journal
            .record(
                Action::transfer(
                    TransferKind::Copy,
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
        let armed = temp.path().join("armed");
        fs::write(&armed, "").unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        if committed {
            command.env("WADDLE_AUDIT_SYNC_EXACT", "1");
        }
        let output = command
            .args([
                "--exact",
                "journal::tests::audit_history_fault_child",
                "--ignored",
                "--nocapture",
            ])
            .env("LD_PRELOAD", shim.path().join("open_fault.so"))
            .env("WADDLE_AUDIT_CHILD_ROOT", &state)
            .env("WADDLE_AUDIT_SYNC_TARGET", &state)
            .env("WADDLE_AUDIT_SYNC_ERRNO", libc::EIO.to_string())
            .env("WADDLE_AUDIT_ARMED", &armed)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!armed.exists(), "sync fault must be reached");
        assert!(
            fs::read_to_string(state.join("result.txt"))
                .unwrap()
                .contains("could not flush operation journal")
        );
        assert!(!destination.exists());
        let prepared = fs::read_dir(&target)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(
            prepared.len(),
            usize::from(committed),
            "incorrect staging ownership after committed={committed}"
        );
        let prepared_inode = prepared.first().map(|p| fs::metadata(p).unwrap().ino());
        let mut journal = Journal::open(path).unwrap();
        if committed {
            assert!(
                journal.undo().is_err(),
                "cannot cross a recorded pending Redo"
            );
        }
        journal.redo().unwrap();
        assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
        if let Some(inode) = prepared_inode {
            assert_eq!(
                fs::metadata(&destination).unwrap().ino(),
                inode,
                "recovery must publish the existing prepared copy"
            );
        }
        assert_eq!(fs::read(destination.join("a")).unwrap(), b"prepared once");
        assert_eq!(
            fs::metadata(destination.join("a")).unwrap().ino(),
            fs::metadata(destination.join("b")).unwrap().ino()
        );
        journal.undo().unwrap();
        assert!(!destination.exists());
        assert_eq!(fs::read(source.join("a")).unwrap(), b"prepared once");
    }
}

#[test]
fn restoring_from_trash_cleans_unrecorded_prepared_copies() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    assert_ne!(unsafe { libc::geteuid() }, 0);
    for restore in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let inputs = temp.path().join("inputs");
        fs::create_dir(&inputs).unwrap();
        let trash = tempfile::tempdir_in("/dev/shm").unwrap();
        assert_ne!(
            fs::metadata(&inputs).unwrap().dev(),
            fs::metadata(trash.path()).unwrap().dev()
        );
        if restore {
            // Native Trash preserves the inode on its own filesystem. Start the
            // original location there, then model a remounted destination before Redo.
            let original_directory = trash.path().join("original-location");
            fs::create_dir(&original_directory).unwrap();
            fs::remove_dir(&inputs).unwrap();
            std::os::unix::fs::symlink(&original_directory, &inputs).unwrap();
        }
        let receipt = TrashReceipt {
            original: inputs.join("original"),
            trashed: trash.path().join("item"),
            info: trash.path().join("item.trashinfo"),
        };
        fs::write(
            if restore {
                &receipt.original
            } else {
                &receipt.trashed
            },
            b"restore without orphans",
        )
        .unwrap();
        if !restore {
            fs::write(&receipt.info, "fixture metadata").unwrap();
        }
        let state = temp.path().join("state");
        let path = state.join("journal.json");
        let mut journal = Journal::open(path.clone()).unwrap();
        let action = if restore {
            Action::restore(std::slice::from_ref(&receipt), false)
        } else {
            Action::trash(std::slice::from_ref(&receipt))
        };
        journal.record(action.unwrap().unwrap()).unwrap();
        if restore {
            let saved = receipt.clone();
            trash_receipt::test_backend::with(
                move |source| {
                    crate::fs::journal_move(source, &saved.trashed).unwrap();
                    fs::write(&saved.info, "fixture metadata").unwrap();
                    Ok(saved.clone())
                },
                || {
                    journal.undo().unwrap();
                },
            );
        }
        if restore {
            fs::remove_file(&inputs).unwrap();
            fs::create_dir(&inputs).unwrap();
        }
        assert_ne!(
            fs::metadata(&inputs).unwrap().dev(),
            fs::metadata(trash.path()).unwrap().dev()
        );
        fs::set_permissions(&state, fs::Permissions::from_mode(0o500)).unwrap();
        let failed = if restore {
            journal.redo()
        } else {
            journal.undo()
        };
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(failed.is_err());
        assert_eq!(
            fs::read_dir(&inputs).unwrap().count(),
            0,
            "restore={restore}: unrecorded staging remains"
        );
        assert_eq!(
            fs::read(&receipt.trashed).unwrap(),
            b"restore without orphans"
        );
        assert!(receipt.info.exists());
        let mut journal = Journal::open(path).unwrap();
        if restore {
            journal.redo().unwrap();
        } else {
            journal.undo().unwrap();
        }
        assert_eq!(
            fs::read(&receipt.original).unwrap(),
            b"restore without orphans"
        );
        assert!(!receipt.trashed.exists());
        assert!(!receipt.info.exists());
        assert_eq!(fs::read_dir(inputs).unwrap().count(), 1);
    }
}

#[test]
#[ignore = "Child process helper for isolated history metadata failure"]
fn history_metadata_warning_child() {
    let root = PathBuf::from(std::env::var_os("WADDLE_METADATA_FIXTURE").expect("parent fixture"));
    let mut journal = Journal::open(root.join("journal.json")).unwrap();
    let effect = if std::env::var_os("WADDLE_METADATA_UNDO").is_some() {
        journal.undo()
    } else {
        journal.redo()
    }
    .unwrap();
    fs::write(root.join("result.txt"), effect.status).unwrap();
}

#[test]
fn redo_copy_reports_metadata_that_could_not_be_preserved() {
    let shim = audit_fault_library();
    for crash in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        let destination = target.join("copy");
        fs::write(&source, b"copied contents").unwrap();
        set_test_attribute(&source, "user.comment", b"preserve this annotation");
        crate::fs::journal_copy(&source, &destination).unwrap();
        let mut journal = Journal::open(temp.path().join("journal.json")).unwrap();
        journal
            .record(
                Action::transfer(
                    TransferKind::Copy,
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
        let armed = temp.path().join("attribute-armed");
        fs::write(&armed, "").unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        if crash {
            let crash_armed = temp.path().join("crash-armed");
            fs::write(&crash_armed, "").unwrap();
            command
                .env("WADDLE_AUDIT_COMMIT", temp.path().join("journal.json"))
                .env("WADDLE_AUDIT_ARMED", crash_armed);
        }
        let output = command
            .args([
                "--exact",
                "journal::tests::history_metadata_warning_child",
                "--ignored",
                "--nocapture",
            ])
            .env("LD_PRELOAD", shim.path().join("open_fault.so"))
            .env("WADDLE_METADATA_FIXTURE", temp.path())
            .env("WADDLE_AUDIT_XATTR_TARGET", &target)
            .env("WADDLE_AUDIT_XATTR_ARMED", &armed)
            .output()
            .unwrap();
        assert!(
            if crash {
                output.status.code() == Some(86)
            } else {
                output.status.success()
            },
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!armed.exists(), "metadata fault must be reached");
        let status = if crash {
            assert!(!destination.exists(), "fault must occur before publication");
            Journal::open(temp.path().join("journal.json"))
                .unwrap()
                .redo()
                .unwrap()
                .status
        } else {
            fs::read_to_string(temp.path().join("result.txt")).unwrap()
        };
        assert_eq!(fs::read(&destination).unwrap(), b"copied contents");
        assert!(
            crate::fs::read_xattrs(&destination)
                .unwrap()
                .iter()
                .all(|(name, _)| name.to_bytes() != b"user.comment")
        );
        assert!(
            status.contains("warning") && status.contains("extended attributes"),
            "lost metadata was reported as complete success: {status}"
        );
        let mut journal = Journal::open(temp.path().join("journal.json")).unwrap();
        assert!(!journal.undo().unwrap().status.contains("warning"));
        assert!(!journal.redo().unwrap().status.contains("warning"));
        assert!(
            crate::fs::read_xattrs(&destination)
                .unwrap()
                .iter()
                .any(|(name, value)| name.to_bytes() == b"user.comment"
                    && value == b"preserve this annotation")
        );
    }
}

#[test]
fn move_and_restore_history_report_metadata_warnings() {
    use std::os::unix::fs::MetadataExt;
    let shim = audit_fault_library();
    for case in ["move-undo", "move-redo", "trash-undo", "restore-redo"] {
        let temp = tempfile::tempdir().unwrap();
        let inputs = temp.path().join("inputs");
        let other = tempfile::tempdir_in("/dev/shm").unwrap();
        if case == "restore-redo" {
            let original_directory = other.path().join("original-location");
            fs::create_dir(&original_directory).unwrap();
            std::os::unix::fs::symlink(original_directory, &inputs).unwrap();
        } else {
            fs::create_dir(&inputs).unwrap();
        }
        let receipt = TrashReceipt {
            original: inputs.join("original"),
            trashed: other.path().join("item"),
            info: other.path().join("item.trashinfo"),
        };
        fs::write(&receipt.original, b"keep file contents").unwrap();
        set_test_attribute(&receipt.original, "user.comment", b"metadata to preserve");
        let mut journal = Journal::open(temp.path().join("journal.json")).unwrap();
        if case != "restore-redo" {
            crate::fs::journal_move(&receipt.original, &receipt.trashed).unwrap();
        }
        let action = match case {
            "trash-undo" => {
                fs::write(&receipt.info, "private fixture").unwrap();
                Action::trash(std::slice::from_ref(&receipt))
            }
            "restore-redo" => Action::restore(std::slice::from_ref(&receipt), false),
            _ => Action::transfer(
                TransferKind::Move,
                &[crate::fs::TransferReceipt {
                    source: receipt.original.clone(),
                    destination: receipt.trashed.clone(),
                    replaced_existing: false,
                }],
            ),
        };
        journal.record(action.unwrap().unwrap()).unwrap();
        if case == "move-redo" {
            journal.undo().unwrap();
        }
        if case == "restore-redo" {
            let saved = receipt.clone();
            trash_receipt::test_backend::with(
                move |source| {
                    fs::rename(source, &saved.trashed).unwrap();
                    fs::write(&saved.info, "private fixture").unwrap();
                    Ok(saved.clone())
                },
                || {
                    journal.undo().unwrap();
                },
            );
            // Model a remounted original location before Restore Redo.
            fs::remove_file(&inputs).unwrap();
            fs::create_dir(&inputs).unwrap();
        }
        assert_ne!(
            fs::metadata(&inputs).unwrap().dev(),
            fs::metadata(other.path()).unwrap().dev()
        );
        let destination = if case == "move-redo" {
            &receipt.trashed
        } else {
            &receipt.original
        };
        let source = if case == "move-redo" {
            &receipt.original
        } else {
            &receipt.trashed
        };
        let armed = temp.path().join("attribute-armed");
        fs::write(&armed, "").unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        if case.ends_with("-undo") {
            command.env("WADDLE_METADATA_UNDO", "1");
        }
        let output = command
            .args([
                "--exact",
                "journal::tests::history_metadata_warning_child",
                "--ignored",
                "--nocapture",
            ])
            .env("LD_PRELOAD", shim.path().join("open_fault.so"))
            .env("WADDLE_METADATA_FIXTURE", temp.path())
            .env("WADDLE_AUDIT_XATTR_TARGET", destination.parent().unwrap())
            .env("WADDLE_AUDIT_XATTR_ARMED", &armed)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!armed.exists(), "{case}: metadata fault not reached");
        assert_eq!(fs::read(destination).unwrap(), b"keep file contents");
        assert!(!source.exists());
        let status = fs::read_to_string(temp.path().join("result.txt")).unwrap();
        assert!(
            status.contains("metadata warnings:") && status.contains("extended attributes"),
            "{case}: {status}"
        );
        let verb = match case {
            "move-undo" => "Undid Move",
            "move-redo" => "Redid Move",
            "trash-undo" => "Undid Trash",
            _ => "Redid Restore",
        };
        assert!(status.starts_with(verb), "{case}: {status}");
        assert!(
            status.contains(destination.to_str().unwrap()),
            "{case}: missing affected path"
        );
    }
}

#[test]
#[ignore = "Child process helper for hardlinks on metadata-limited filesystems"]
fn hardlink_metadata_fault_child() {
    use std::os::unix::fs::MetadataExt;
    let root = PathBuf::from(
        std::env::var_os("WADDLE_HARDLINK_METADATA_FIXTURE").expect("parent fixture"),
    );
    let target = PathBuf::from(std::env::var_os("WADDLE_HARDLINK_TARGET").unwrap());
    let action = if std::env::var_os("WADDLE_HARDLINK_MOVE").is_some() {
        crate::transfer::Action::Move
    } else {
        crate::transfer::Action::Copy
    };
    let change = std::env::var("WADDLE_HARDLINK_CHANGE").unwrap();
    let batch = crate::fs::TransferBatch::try_new(
        vec![root.join("source/a"), root.join("source/b")],
        target.clone(),
        action,
    )
    .unwrap();
    fs::write(target.join("b"), b"conflict").unwrap();
    let crate::fs::TransferBatchOutcome::Conflict { batch, .. } = batch.run() else {
        panic!("expected conflict on the second hardlink")
    };
    let report = batch.cancel();
    assert!(
        !report.warnings.is_empty(),
        "unsupported metadata must still be reported"
    );
    fs::remove_file(target.join("b")).unwrap();
    let changed = match change.as_str() {
        "source" => Some(root.join("source/b")),
        "destination" => Some(target.join("a")),
        _ => None,
    };
    if let Some(path) = changed {
        set_test_attribute(&path, "user.edit", b"external edit");
    }
    let crate::fs::TransferBatchOutcome::Complete(report) =
        report.retry_plan().into_batch(action).unwrap().run()
    else {
        panic!("unexpected conflict on retry")
    };
    assert!(report.failures.is_empty(), "{report:?}");
    assert_eq!(fs::read(target.join("a")).unwrap(), b"shared contents");
    assert_eq!(fs::read(target.join("b")).unwrap(), b"shared contents");
    assert_eq!(
        fs::metadata(target.join("a")).unwrap().ino()
            == fs::metadata(target.join("b")).unwrap().ino(),
        change == "none",
        "unsupported attributes broke the source hardlink relationship"
    );
}

#[test]
fn copying_hardlinks_preserves_relationship_when_attributes_are_unsupported() {
    let shim = audit_fault_library();
    for moving in [false, true] {
        for change in ["none", "source", "destination"] {
            let temp = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir_in("/dev/shm").unwrap();
            let source = temp.path().join("source");
            let target = destination.path();
            assert_ne!(
                std::os::unix::fs::MetadataExt::dev(&fs::metadata(temp.path()).unwrap()),
                std::os::unix::fs::MetadataExt::dev(&fs::metadata(target).unwrap())
            );
            fs::create_dir(&source).unwrap();
            fs::write(source.join("a"), b"shared contents").unwrap();
            fs::hard_link(source.join("a"), source.join("b")).unwrap();
            set_test_attribute(&source.join("a"), "user.comment", b"source metadata");
            let armed = temp.path().join("attribute-armed");
            fs::write(&armed, "").unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            if moving {
                command.env("WADDLE_HARDLINK_MOVE", "1");
            }
            let output = command
                .args([
                    "--exact",
                    "journal::tests::hardlink_metadata_fault_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("LD_PRELOAD", shim.path().join("open_fault.so"))
                .env("WADDLE_HARDLINK_METADATA_FIXTURE", temp.path())
                .env("WADDLE_HARDLINK_TARGET", target)
                .env("WADDLE_HARDLINK_CHANGE", change)
                .env("WADDLE_AUDIT_XATTR_TARGET", target)
                .env("WADDLE_AUDIT_XATTR_ARMED", &armed)
                .output()
                .unwrap();
            assert!(!armed.exists(), "attribute failure must be reached");
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn hardlinks_with_metadata_warnings_survive_history_retry_after_restart() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let shim = audit_fault_library();
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("first")).unwrap();
    fs::create_dir_all(target.join("second")).unwrap();
    let first = temp.path().join("a");
    let second = temp.path().join("b");
    let first_target = target.join("first/a");
    let second_target = target.join("second/b");
    fs::write(&first, b"linked history data").unwrap();
    fs::hard_link(&first, &second).unwrap();
    set_test_attribute(&first, "user.comment", b"source metadata");
    let mut transfer = crate::fs::JournalTransfer::default();
    let receipts =
        [(&first, &first_target), (&second, &second_target)].map(|(source, destination)| {
            transfer
                .apply(crate::transfer::Action::Copy, source, destination)
                .unwrap();
            crate::fs::TransferReceipt {
                source: source.clone(),
                destination: destination.clone(),
                replaced_existing: false,
            }
        });
    let mut journal = Journal::open(temp.path().join("journal.json")).unwrap();
    journal
        .record(
            Action::transfer(TransferKind::Copy, &receipts)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
    journal.undo().unwrap();
    let armed = temp.path().join("attribute-armed");
    fs::write(&armed, "").unwrap();
    fs::set_permissions(target.join("second"), fs::Permissions::from_mode(0o500)).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "journal::tests::audit_history_fault_child",
            "--ignored",
            "--nocapture",
        ])
        .env("LD_PRELOAD", shim.path().join("open_fault.so"))
        .env("WADDLE_AUDIT_CHILD_ROOT", temp.path())
        .env("WADDLE_AUDIT_XATTR_TARGET", &target)
        .env("WADDLE_AUDIT_XATTR_ARMED", &armed)
        .output()
        .unwrap();
    fs::set_permissions(target.join("second"), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!armed.exists(), "attribute failure must be reached");
    assert!(
        fs::read_to_string(temp.path().join("result.txt"))
            .unwrap()
            .contains("Permission denied")
    );
    assert!(first_target.exists());
    assert!(!second_target.exists());
    // A fresh process must reuse the persisted destination even though its
    // missing user.comment no longer matches the source's metadata.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "journal::tests::history_metadata_warning_child",
            "--ignored",
            "--nocapture",
        ])
        .env("LD_PRELOAD", shim.path().join("open_fault.so"))
        .env("WADDLE_METADATA_FIXTURE", temp.path())
        .env("WADDLE_AUDIT_XATTR_TARGET", &target)
        .env("WADDLE_AUDIT_XATTR_ARMED", &armed)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        fs::read_to_string(temp.path().join("result.txt"))
            .unwrap()
            .contains("metadata warnings")
    );
    assert_eq!(fs::read(&first_target).unwrap(), b"linked history data");
    assert_eq!(fs::read(&second_target).unwrap(), b"linked history data");
    assert_eq!(
        fs::metadata(first_target).unwrap().ino(),
        fs::metadata(second_target).unwrap().ino(),
        "history retry lost the hardlink after reopening"
    );
}

#[test]
#[ignore = "Child process helper for unsupported attribute enumeration"]
fn attribute_enumeration_transfer_child() {
    use std::os::unix::fs::MetadataExt;
    let root = PathBuf::from(std::env::var_os("WADDLE_ENUMERATION_FIXTURE").unwrap());
    let target = PathBuf::from(std::env::var_os("WADDLE_ENUMERATION_DESTINATION").unwrap());
    let action = if std::env::var_os("WADDLE_ENUMERATION_MOVE").is_some() {
        crate::transfer::Action::Move
    } else {
        crate::transfer::Action::Copy
    };
    let enumeration_unsupported = std::env::var("WADDLE_AUDIT_LIST_XATTR_ERRNO")
        .unwrap()
        .parse::<i32>()
        .unwrap()
        == libc::ENOTSUP;
    let batch = crate::fs::TransferBatch::try_new(
        vec![root.join("source/a"), root.join("source/b")],
        target.clone(),
        action,
    )
    .unwrap();
    let crate::fs::TransferBatchOutcome::Complete(report) = batch.run() else {
        panic!("unexpected conflict")
    };
    assert!(report.failures.is_empty(), "{report:?}");
    assert_eq!(fs::read(target.join("a")).unwrap(), b"shared contents");
    assert_eq!(fs::read(target.join("b")).unwrap(), b"shared contents");
    assert_eq!(
        fs::metadata(target.join("a")).unwrap().ino()
            == fs::metadata(target.join("b")).unwrap().ino(),
        enumeration_unsupported,
        "unsupported attribute enumeration broke the hardlink relationship"
    );
    assert_eq!(
        root.join("source/a").exists(),
        action == crate::transfer::Action::Copy
    );
    assert_eq!(
        root.join("source/b").exists(),
        action == crate::transfer::Action::Copy
    );
    if !enumeration_unsupported {
        return;
    }
    let kind = if action == crate::transfer::Action::Move {
        TransferKind::Move
    } else {
        TransferKind::Copy
    };
    let path = root.join("journal.json");
    Journal::open(path.clone())
        .unwrap()
        .record(Action::transfer(kind, &report.receipts).unwrap().unwrap())
        .unwrap();
    Journal::open(path.clone()).unwrap().undo().unwrap();
    assert_eq!(fs::read(root.join("source/a")).unwrap(), b"shared contents");
    assert_eq!(
        fs::metadata(root.join("source/a")).unwrap().ino(),
        fs::metadata(root.join("source/b")).unwrap().ino()
    );
    Journal::open(path).unwrap().redo().unwrap();
    assert_eq!(
        fs::metadata(target.join("a")).unwrap().ino(),
        fs::metadata(target.join("b")).unwrap().ino()
    );
}

#[test]
fn transfers_preserve_hardlinks_without_attribute_enumeration() {
    let shim = audit_fault_library();
    for moving in [false, true] {
        for source_fault in [false, true] {
            for error in [libc::ENOTSUP, libc::EIO, libc::EACCES] {
                let temp = tempfile::tempdir().unwrap();
                let target = tempfile::tempdir_in("/dev/shm").unwrap();
                if source_fault && error == libc::ENOTSUP {
                    // Source without ACL support must not gain named users
                    // inherited from the destination's default ACL.
                    let mut acl = 2_u32.to_le_bytes().to_vec();
                    for (tag, permissions, id) in [
                        (1_u16, 7_u16, u32::MAX),
                        (2, 7, 65534),
                        (4, 5, u32::MAX),
                        (16, 7, u32::MAX),
                        (32, 0, u32::MAX),
                    ] {
                        acl.extend_from_slice(&tag.to_le_bytes());
                        acl.extend_from_slice(&permissions.to_le_bytes());
                        acl.extend_from_slice(&id.to_le_bytes());
                    }
                    set_test_attribute(target.path(), "system.posix_acl_default", &acl);
                }
                let source = temp.path().join("source");
                fs::create_dir(&source).unwrap();
                fs::write(source.join("a"), b"shared contents").unwrap();
                fs::hard_link(source.join("a"), source.join("b")).unwrap();
                let armed = temp.path().join("enumeration-armed");
                fs::write(&armed, "").unwrap();
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                if moving {
                    command.env("WADDLE_ENUMERATION_MOVE", "1");
                }
                let output = command
                    .args([
                        "--exact",
                        "journal::tests::attribute_enumeration_transfer_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("LD_PRELOAD", shim.path().join("open_fault.so"))
                    .env("WADDLE_ENUMERATION_FIXTURE", temp.path())
                    .env("WADDLE_ENUMERATION_DESTINATION", target.path())
                    .env(
                        "WADDLE_AUDIT_LIST_XATTR_TARGET",
                        if source_fault {
                            source.as_path()
                        } else {
                            target.path()
                        },
                    )
                    .env("WADDLE_AUDIT_LIST_XATTR_ERRNO", error.to_string())
                    .env("WADDLE_AUDIT_LIST_XATTR_ARMED", &armed)
                    .output()
                    .unwrap();
                assert!(!armed.exists(), "enumeration failure must be reached");
                if source_fault && error == libc::ENOTSUP && output.status.success() {
                    for name in ["a", "b"] {
                        assert!(
                            crate::fs::read_xattrs(&target.path().join(name))
                                .unwrap()
                                .iter()
                                .all(|(name, _)| name.as_bytes() != b"system.posix_acl_access"),
                            "copy inherited a named-user ACL absent from its source"
                        );
                    }
                }
                assert!(
                    output.status.success(),
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

#[test]
fn undo_copy_refuses_unreadable_attributes_that_were_successfully_listed() {
    let shim = audit_fault_library();
    for error in [libc::ENOTSUP, libc::EIO, libc::EACCES] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::write(&source, b"original contents").unwrap();
        crate::fs::journal_copy(&source, &destination).unwrap();
        let mut journal = Journal::open(temp.path().join("journal.json")).unwrap();
        journal
            .record(
                Action::transfer(
                    TransferKind::Copy,
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
        // An external edit must not disappear just because its value is unreadable.
        set_test_attribute(&destination, "user.comment", b"added after copy");
        let armed = temp.path().join("value-armed");
        fs::write(&armed, "").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "journal::tests::audit_history_fault_child",
                "--ignored",
                "--nocapture",
            ])
            .env("LD_PRELOAD", shim.path().join("open_fault.so"))
            .env("WADDLE_AUDIT_CHILD_ROOT", temp.path())
            .env("WADDLE_AUDIT_UNDO", "1")
            .env("WADDLE_AUDIT_GET_XATTR_TARGET", &destination)
            .env("WADDLE_AUDIT_GET_XATTR_ARMED", &armed)
            .env("WADDLE_AUDIT_GET_XATTR_ERRNO", error.to_string())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(!armed.exists(), "attribute read failure must be reached");
        assert!(
            destination.exists(),
            "Undo deleted an edited copy after lgetxattr failed with {error}"
        );
        assert_eq!(fs::read(&destination).unwrap(), b"original contents");
        assert!(
            fs::read_to_string(temp.path().join("result.txt"))
                .unwrap()
                .contains("could not fingerprint attributes")
        );
        assert!(
            Journal::open(temp.path().join("journal.json"))
                .unwrap()
                .undo()
                .is_err(),
            "a later readable attribute must still protect the external edit"
        );
    }
}

#[test]
#[ignore = "Child process helper for metadata faults with inherited ACLs"]
fn metadata_fault_acl_transfer_child() {
    let root = PathBuf::from(std::env::var_os("WADDLE_ACL_FIXTURE").unwrap());
    let target = PathBuf::from(std::env::var_os("WADDLE_ACL_TARGET").unwrap());
    let action = if std::env::var_os("WADDLE_ACL_MOVE").is_some() {
        crate::transfer::Action::Move
    } else {
        crate::transfer::Action::Copy
    };
    let directory = root.join("source").is_dir();
    let batch =
        crate::fs::TransferBatch::try_new(vec![root.join("source")], target.clone(), action)
            .unwrap();
    let crate::fs::TransferBatchOutcome::Complete(report) = batch.run() else {
        panic!("unexpected conflict")
    };
    if std::env::var_os("WADDLE_ACL_EXPECT_FAILURE").is_some() {
        assert!(
            !report.failures.is_empty(),
            "ACL failure was ignored: {report:?}"
        );
        assert!(
            root.join("source").exists(),
            "failed Move removed the source"
        );
        assert!(
            !target.join("source").exists(),
            "published a copy with invalid access policy"
        );
        assert_eq!(
            fs::read_dir(&target).unwrap().count(),
            0,
            "abandoned staging after ACL failure"
        );
        return;
    }
    assert!(report.failures.is_empty(), "{report:?}");
    assert!(!report.warnings.is_empty(), "missing metadata warning");
    assert_eq!(
        fs::read(target.join(if directory { "source/note" } else { "source" })).unwrap(),
        b"private data"
    );
    assert_eq!(
        root.join("source").exists(),
        action == crate::transfer::Action::Copy
    );
}

fn transfer_test_acl(uid: u32) -> Vec<u8> {
    let mut acl = 2_u32.to_le_bytes().to_vec();
    for (tag, permissions, id) in [
        (1_u16, 7_u16, u32::MAX),
        (2, 7, uid),
        (4, 5, u32::MAX),
        (16, 7, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend_from_slice(&tag.to_le_bytes());
        acl.extend_from_slice(&permissions.to_le_bytes());
        acl.extend_from_slice(&id.to_le_bytes());
    }
    acl
}

#[test]
fn failed_optional_attribute_read_does_not_grant_inherited_acl_access() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let shim = audit_fault_library();
    for moving in [false, true] {
        for directory in [false, true] {
            for explicit_acl in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let target = tempfile::tempdir_in("/dev/shm").unwrap();
                assert_ne!(
                    fs::metadata(temp.path()).unwrap().dev(),
                    fs::metadata(target.path()).unwrap().dev()
                );
                let source = temp.path().join("source");
                if directory {
                    fs::create_dir(&source).unwrap();
                }
                fs::write(
                    if directory {
                        source.join("note")
                    } else {
                        source.clone()
                    },
                    b"private data",
                )
                .unwrap();
                fs::set_permissions(
                    &source,
                    fs::Permissions::from_mode(if directory { 0o750 } else { 0o640 }),
                )
                .unwrap();
                set_test_attribute(&source, "user.comment", b"annotation");
                if explicit_acl {
                    set_test_attribute(
                        &source,
                        "system.posix_acl_access",
                        &transfer_test_acl(65533),
                    );
                    if directory {
                        set_test_attribute(
                            &source,
                            "system.posix_acl_default",
                            &transfer_test_acl(65533),
                        );
                    }
                }
                let acl_attributes = |path: &std::path::Path| {
                    crate::fs::read_xattrs(path)
                        .unwrap()
                        .into_iter()
                        .filter(|(name, _)| name.to_bytes().starts_with(b"system.posix_acl_"))
                        .collect::<Vec<_>>()
                };
                let expected = acl_attributes(&source);
                let expected_child = directory.then(|| acl_attributes(&source.join("note")));
                set_test_attribute(
                    target.path(),
                    "system.posix_acl_default",
                    &transfer_test_acl(65534),
                );
                let armed = temp.path().join("attribute-armed");
                fs::write(&armed, "").unwrap();
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                if moving {
                    command.env("WADDLE_ACL_MOVE", "1");
                }
                let output = command
                    .args([
                        "--exact",
                        "journal::tests::metadata_fault_acl_transfer_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("LD_PRELOAD", shim.path().join("open_fault.so"))
                    .env("WADDLE_ACL_FIXTURE", temp.path())
                    .env("WADDLE_ACL_TARGET", target.path())
                    .env("WADDLE_AUDIT_GET_XATTR_TARGET", &source)
                    .env("WADDLE_AUDIT_GET_XATTR_ARMED", &armed)
                    .env("WADDLE_AUDIT_GET_XATTR_ERRNO", libc::EIO.to_string())
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{output:?}");
                assert!(!armed.exists(), "attribute read failure must be reached");
                assert_eq!(
                    acl_attributes(&target.path().join("source")),
                    expected,
                    "optional metadata failure changed the source access policy"
                );
                if let Some(expected) = expected_child {
                    assert_eq!(acl_attributes(&target.path().join("source/note")), expected);
                }
            }
        }
    }
}

#[test]
fn acl_transfer_faults_preserve_safe_results_and_recoverability() {
    let shim = audit_fault_library();
    for moving in [false, true] {
        for directory in [false, true] {
            for fault in ["read", "write", "remove", "unsupported"] {
                let temp = tempfile::tempdir().unwrap();
                let target = tempfile::tempdir_in("/dev/shm").unwrap();
                let source = temp.path().join("source");
                if directory {
                    fs::create_dir(&source).unwrap();
                }
                let contents = if directory {
                    source.join("note")
                } else {
                    source.clone()
                };
                fs::write(&contents, b"private data").unwrap();
                if fault != "remove" {
                    set_test_attribute(
                        &source,
                        "system.posix_acl_access",
                        &transfer_test_acl(65533),
                    );
                }
                set_test_attribute(
                    target.path(),
                    "system.posix_acl_default",
                    &transfer_test_acl(65534),
                );
                let armed = temp.path().join("acl-armed");
                fs::write(&armed, "").unwrap();
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                if moving {
                    command.env("WADDLE_ACL_MOVE", "1");
                }
                if fault != "unsupported" {
                    command.env("WADDLE_ACL_EXPECT_FAILURE", "1");
                }
                match fault {
                    "read" => {
                        command
                            .env("WADDLE_AUDIT_GET_XATTR_TARGET", &source)
                            .env("WADDLE_AUDIT_GET_XATTR_ARMED", &armed)
                            .env("WADDLE_AUDIT_GET_XATTR_NAME", "system.posix_acl_access")
                            .env("WADDLE_AUDIT_GET_XATTR_ERRNO", libc::EIO.to_string());
                    }
                    "remove" => {
                        command
                            .env("WADDLE_AUDIT_REMOVE_ACL_TARGET", target.path())
                            .env("WADDLE_AUDIT_REMOVE_ACL_ARMED", &armed);
                    }
                    _ => {
                        command
                            .env("WADDLE_AUDIT_XATTR_TARGET", target.path())
                            .env("WADDLE_AUDIT_XATTR_ARMED", &armed)
                            .env("WADDLE_AUDIT_SET_XATTR_NAME", "system.posix_acl_access")
                            .env(
                                "WADDLE_AUDIT_SET_XATTR_ERRNO",
                                if fault == "unsupported" {
                                    libc::ENOTSUP
                                } else {
                                    libc::EACCES
                                }
                                .to_string(),
                            );
                    }
                }
                let output = command
                    .args([
                        "--exact",
                        "journal::tests::metadata_fault_acl_transfer_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("LD_PRELOAD", shim.path().join("open_fault.so"))
                    .env("WADDLE_ACL_FIXTURE", temp.path())
                    .env("WADDLE_ACL_TARGET", target.path())
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{fault} move={moving} directory={directory}: {output:?}"
                );
                assert!(!armed.exists(), "ACL fault must be reached");
                if fault == "unsupported" {
                    assert!(
                        crate::fs::read_xattrs(&target.path().join("source"))
                            .unwrap()
                            .iter()
                            .all(|(name, _)| name.to_bytes() != b"system.posix_acl_access"),
                        "unsupported source ACL left an inherited named-user grant"
                    );
                    continue;
                }
                assert_eq!(fs::read(&contents).unwrap(), b"private data");
                let action = if moving {
                    crate::transfer::Action::Move
                } else {
                    crate::transfer::Action::Copy
                };
                let crate::fs::TransferBatchOutcome::Complete(report) =
                    crate::fs::TransferBatch::try_new(
                        vec![source],
                        target.path().to_owned(),
                        action,
                    )
                    .unwrap()
                    .run()
                else {
                    panic!("unexpected retry conflict")
                };
                assert!(
                    report.failures.is_empty() && report.warnings.is_empty(),
                    "{report:?}"
                );
                assert_eq!(
                    fs::read(
                        target
                            .path()
                            .join(if directory { "source/note" } else { "source" })
                    )
                    .unwrap(),
                    b"private data"
                );
            }
        }
    }
}
