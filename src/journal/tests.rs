use super::*;
use std::{error::Error as _, fs};

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
            Action::restore(std::slice::from_ref(&receipt))
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
