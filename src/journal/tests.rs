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
