use super::*;

#[test]
fn validates_names() {
    for bad in ["", ".", "..", "a/b", "a\0b"] {
        assert!(validate_name(bad).is_err());
    }
    assert!(validate_name("new folder").is_ok());
}

#[test]
fn reads_hidden_filtered_and_directories_first() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Alpha"), "x").unwrap();
    fs::write(temp.path().join("beta"), "x").unwrap();
    fs::write(temp.path().join(".hidden"), "x").unwrap();
    fs::create_dir(temp.path().join("z-folder")).unwrap();
    let names: Vec<_> = read_directory(temp.path())
        .unwrap()
        .into_iter()
        .map(|e| display_name(&e.name))
        .collect();
    assert_eq!(names, ["z-folder", "Alpha", "beta"]);
}

#[test]
fn recursive_search_matches_relative_paths_and_skips_hidden_entries() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src/nested")).unwrap();
    fs::create_dir_all(temp.path().join(".hidden")).unwrap();
    fs::write(temp.path().join("src/nested/needle.txt"), "match").unwrap();
    fs::write(temp.path().join("src/other.txt"), "other").unwrap();
    fs::write(temp.path().join(".hidden/needle.txt"), "hidden").unwrap();

    let result = super::search_directory(temp.path(), "nested/need", 100, || false).unwrap();

    assert!(!result.truncated);
    assert_eq!(result.entries.len(), 1);
    assert_eq!(
        result.entries[0].path,
        temp.path().join("src/nested/needle.txt")
    );
}

#[test]
fn recursive_search_reports_truncation_at_the_result_limit() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["match-a", "match-b", "match-c"] {
        fs::write(temp.path().join(name), name).unwrap();
    }

    let result = super::search_directory(temp.path(), "match", 2, || false).unwrap();

    assert!(result.truncated);
    assert_eq!(result.entries.len(), 2);
}

#[test]
fn recursive_search_stops_when_cancellation_is_requested() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("nested/deeper")).unwrap();
    fs::write(temp.path().join("nested/deeper/needle.txt"), "match").unwrap();

    let result = super::search_directory(temp.path(), "needle", 100, || true).unwrap();

    assert!(result.entries.is_empty());
    assert!(!result.truncated);
}

#[cfg(unix)]
#[test]
fn sorts_a_directory_symlink_with_directories() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    symlink(&target, temp.path().join("a-link")).unwrap();
    fs::write(temp.path().join("z-file"), "x").unwrap();

    let entries = read_directory(temp.path()).unwrap();
    let names: Vec<_> = entries
        .iter()
        .map(|entry| display_name(&entry.name))
        .collect();
    assert_eq!(names, ["a-link", "target", "z-file"]);
    assert!(entries[0].is_directory());
    fs::remove_dir(&target).unwrap();
    assert!(
        entries[0].is_directory(),
        "directory classification must not touch the filesystem again"
    );
}

#[test]
fn scans_a_large_directory_completely_and_in_order() {
    let temp = tempfile::tempdir().unwrap();
    for index in (0..1000).rev() {
        fs::write(temp.path().join(format!("item-{index:04}")), "x").unwrap();
    }

    let entries = read_directory(temp.path()).unwrap();
    assert_eq!(entries.len(), 1000);
    assert_eq!(display_name(&entries[0].name), "item-0000");
    assert_eq!(display_name(&entries[999].name), "item-0999");
}

#[test]
fn reads_child_folders_hidden_filtered_and_sorted() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("Zulu")).unwrap();
    fs::create_dir(temp.path().join("alpha")).unwrap();
    fs::create_dir(temp.path().join(".hidden")).unwrap();
    fs::write(temp.path().join("file"), "x").unwrap();

    let names: Vec<_> = read_child_folders(temp.path())
        .into_iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    assert_eq!(names, ["alpha", "Zulu"]);
}

#[cfg(unix)]
#[test]
fn reads_permissions_size_and_owner_for_status() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("details.bin");
    fs::write(&path, vec![0; 1536]).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

    let details = read_entry_details(&path).unwrap();
    assert!(details.contains("-rw-r-----"));
    assert!(details.contains("1.5 KiB"));
    assert!(details.rsplit_once(':').is_some());
}

#[test]
fn creates_renames_and_deletes() {
    let temp = tempfile::tempdir().unwrap();
    let created = create_folder(temp.path(), "first").unwrap();
    let renamed = rename_entry(&created, "second").unwrap();
    fs::write(renamed.join("nested"), "x").unwrap();
    delete_permanently(&renamed).unwrap();
    assert!(!renamed.exists());
}

#[test]
fn moves_file_into_directory() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("note.txt");
    let destination_directory = temp.path().join("archive");
    fs::write(&source, "hello").unwrap();
    fs::create_dir(&destination_directory).unwrap();

    let destination = move_entry(&source, &destination_directory).unwrap();

    assert_eq!(destination, destination_directory.join("note.txt"));
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(destination).unwrap(), "hello");
}

#[test]
fn move_rejects_an_existing_destination() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("note.txt");
    let destination_directory = temp.path().join("archive");
    fs::write(&source, "source").unwrap();
    fs::create_dir(&destination_directory).unwrap();
    fs::write(destination_directory.join("note.txt"), "existing").unwrap();

    assert!(move_entry(&source, &destination_directory).is_err());
    assert_eq!(fs::read_to_string(source).unwrap(), "source");
    assert_eq!(
        fs::read_to_string(destination_directory.join("note.txt")).unwrap(),
        "existing"
    );
}

#[test]
fn move_rejects_a_directory_descendant() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("folder");
    let descendant = source.join("child");
    fs::create_dir_all(&descendant).unwrap();

    assert!(move_entry(&source, &descendant).is_err());
    assert!(source.exists());
    assert!(descendant.exists());
}

#[test]
fn copies_files_without_overwriting_existing_entries() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("note.txt");
    let destination = temp.path().join("archive");
    fs::write(&source, "hello").unwrap();
    fs::create_dir(&destination).unwrap();

    let first = copy_entry(&source, &destination).unwrap();
    let second = copy_entry(&source, &destination).unwrap();

    assert_eq!(first, destination.join("note.txt"));
    assert_eq!(second, destination.join("note.txt copy"));
    assert_eq!(fs::read_to_string(first).unwrap(), "hello");
    assert_eq!(fs::read_to_string(second).unwrap(), "hello");
    assert!(source.exists());
}

#[test]
fn copies_directories_recursively() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("folder");
    let destination = temp.path().join("archive");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("nested/note.txt"), "hello").unwrap();
    fs::create_dir(&destination).unwrap();

    let copied = copy_entry(&source, &destination).unwrap();

    assert_eq!(copied, destination.join("folder"));
    assert_eq!(
        fs::read_to_string(copied.join("nested/note.txt")).unwrap(),
        "hello"
    );
    assert!(source.join("nested/note.txt").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn copy_preserves_sparse_hardlink_symlink_timestamp_permission_and_xattr_metadata() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("tree");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    let sparse = source.join("sparse");
    let file = fs::File::create(&sparse).unwrap();
    file.set_len(8 * 1024 * 1024).unwrap();
    fs::set_permissions(&sparse, fs::Permissions::from_mode(0o640)).unwrap();
    set_times(
        &sparse,
        1_700_000_000,
        123_000_000,
        1_700_000_000,
        123_000_000,
    )
    .unwrap();
    let hardlink = source.join("hardlink");
    fs::hard_link(&sparse, &hardlink).unwrap();
    symlink("sparse", source.join("symlink")).unwrap();
    let xattr_supported = set_xattr(&sparse, "user.polarexp-test", b"kept").is_ok();

    let copied = copy_entry(&source, &destination).unwrap();
    let copied_sparse = copied.join("sparse");
    let copied_hardlink = copied.join("hardlink");
    let metadata = fs::symlink_metadata(&copied_sparse).unwrap();

    assert_eq!(metadata.permissions().mode() & 0o777, 0o640);
    assert_eq!(metadata.mtime(), 1_700_000_000);
    assert_eq!(metadata.mtime_nsec(), 123_000_000);
    assert!(metadata.blocks() * 512 < metadata.len());
    assert_eq!(
        metadata.ino(),
        fs::symlink_metadata(copied_hardlink).unwrap().ino()
    );
    assert_eq!(
        fs::read_link(copied.join("symlink")).unwrap(),
        PathBuf::from("sparse")
    );
    if xattr_supported {
        assert_eq!(
            get_xattr(&copied_sparse, "user.polarexp-test").unwrap(),
            b"kept"
        );
    }
}

#[test]
fn metadata_warning_is_kept_separate_from_content_failure() {
    let mut warnings = Vec::new();
    record_metadata_result(
        "extended attributes",
        Err(io::Error::new(io::ErrorKind::Unsupported, "not supported")),
        &mut warnings,
    );
    assert_eq!(warnings, ["extended attributes: not supported"]);

    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let report = TransferBatch::new(
        vec![temp.path().join("missing")],
        destination,
        crate::transfer::Action::Copy,
    )
    .run();
    let TransferBatchOutcome::Complete(report) = report else {
        panic!("missing content cannot be a conflict");
    };
    assert!(report.completed.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn copy_rejects_a_directory_descendant() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("folder");
    let descendant = source.join("child");
    fs::create_dir_all(&descendant).unwrap();

    assert!(copy_entry(&source, &descendant).is_err());
    assert_eq!(fs::read_dir(&descendant).unwrap().count(), 0);
}

#[test]
fn batch_copy_keeps_successes_when_one_source_fails() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let first = temp.path().join("first.txt");
    let missing = temp.path().join("missing.txt");
    let second = temp.path().join("second.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();

    let report = transfer_entries(
        &[first, missing.clone(), second],
        &destination,
        crate::transfer::Action::Copy,
    );

    assert_eq!(report.completed.len(), 2);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].source, missing);
    assert!(destination.join("first.txt").exists());
    assert!(destination.join("second.txt").exists());
}

#[test]
fn batch_move_rejects_conflicts_without_rolling_back_successes() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let first = temp.path().join("first.txt");
    let conflict = temp.path().join("conflict.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&conflict, "source").unwrap();
    fs::write(destination.join("conflict.txt"), "destination").unwrap();

    let report = transfer_entries(
        &[first.clone(), conflict.clone()],
        &destination,
        crate::transfer::Action::Move,
    );

    assert_eq!(report.completed, vec![destination.join("first.txt")]);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].source, conflict);
    assert!(!first.exists());
}

#[test]
fn transfer_batch_pauses_before_a_conflict_and_escape_retains_pending_sources() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let first = temp.path().join("first.txt");
    let conflict = temp.path().join("conflict.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&conflict, "source").unwrap();
    fs::write(destination.join("conflict.txt"), "destination").unwrap();

    let outcome = TransferBatch::new(
        vec![first.clone(), conflict.clone()],
        destination.clone(),
        crate::transfer::Action::Move,
    )
    .run();
    let TransferBatchOutcome::Conflict {
        batch,
        conflict: prompt,
    } = outcome
    else {
        panic!("expected a conflict");
    };

    assert_eq!(prompt.source, conflict);
    assert!(!first.exists());
    assert!(prompt.source.exists());
    let report = (*batch).cancel();
    assert_eq!(report.completed, [destination.join("first.txt")]);
    assert_eq!(report.retained, [prompt.source]);
}

#[test]
fn transfer_batch_supports_one_shot_and_batch_conflict_choices() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let first = temp.path().join("first.txt");
    let second = temp.path().join("second.txt");
    fs::write(&first, "new first").unwrap();
    fs::write(&second, "new second").unwrap();
    fs::write(destination.join("first.txt"), "old first").unwrap();
    fs::write(destination.join("second.txt"), "old second").unwrap();

    let TransferBatchOutcome::Conflict { batch, .. } = TransferBatch::new(
        vec![first.clone(), second.clone()],
        destination.clone(),
        crate::transfer::Action::Copy,
    )
    .run() else {
        panic!("expected first conflict");
    };
    let TransferBatchOutcome::Conflict { batch, conflict } =
        (*batch).resolve(ConflictChoice::KeepBoth, false).run()
    else {
        panic!("expected second conflict");
    };
    assert_eq!(conflict.source, second);
    let TransferBatchOutcome::Complete(report) =
        (*batch).resolve(ConflictChoice::Replace, true).run()
    else {
        panic!("expected completion");
    };

    assert!(report.failures.is_empty());
    assert_eq!(
        fs::read_to_string(destination.join("first.txt copy")).unwrap(),
        "new first"
    );
    assert_eq!(
        fs::read_to_string(destination.join("second.txt")).unwrap(),
        "new second"
    );
}

#[test]
fn skipping_a_conflict_keeps_the_source_and_existing_destination() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("item.txt");
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::write(&source, "source").unwrap();
    fs::write(destination.join("item.txt"), "destination").unwrap();

    let TransferBatchOutcome::Conflict { batch, .. } = TransferBatch::new(
        vec![source.clone()],
        destination.clone(),
        crate::transfer::Action::Move,
    )
    .run() else {
        panic!("expected conflict");
    };
    let TransferBatchOutcome::Complete(report) =
        (*batch).resolve(ConflictChoice::Skip, false).run()
    else {
        panic!("expected completion");
    };

    assert_eq!(report.retained.as_slice(), std::slice::from_ref(&source));
    assert_eq!(fs::read_to_string(source).unwrap(), "source");
    assert_eq!(
        fs::read_to_string(destination.join("item.txt")).unwrap(),
        "destination"
    );
}

#[test]
fn cancelled_batch_keeps_completed_results_and_removes_private_incomplete_names() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    let first = source.join("first");
    let second = source.join("second");
    fs::write(&first, vec![1_u8; 4096]).unwrap();
    fs::write(&second, vec![2_u8; 4096]).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&cancelled);

    let outcome = TransferBatch::new(
        vec![first, second.clone()],
        destination.clone(),
        crate::transfer::Action::Copy,
    )
    .run_with(
        || cancelled.load(Ordering::Acquire),
        move |progress| {
            if progress.completed_entries == 1 {
                observed.store(true, Ordering::Release);
            }
        },
    );
    let TransferBatchOutcome::Complete(report) = outcome else {
        panic!("cancelled batch must finish with a report");
    };

    assert!(report.cancelled);
    assert_eq!(report.completed, [destination.join("first")]);
    assert_eq!(report.retained, [second]);
    assert!(!destination.join("second").exists());
    assert!(fs::read_dir(&destination).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".polarexp-")
    }));
}

#[test]
fn replacing_conflicting_directories_merges_without_deleting_the_existing_tree() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source/folder");
    let destination = temp.path().join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(destination.join("folder")).unwrap();
    fs::write(source.join("incoming.txt"), "incoming").unwrap();
    fs::write(destination.join("folder/existing.txt"), "existing").unwrap();

    let TransferBatchOutcome::Conflict { batch, .. } = TransferBatch::new(
        vec![source],
        destination.clone(),
        crate::transfer::Action::Copy,
    )
    .run() else {
        panic!("expected directory conflict");
    };
    let TransferBatchOutcome::Complete(report) =
        (*batch).resolve(ConflictChoice::Replace, false).run()
    else {
        panic!("expected merged completion");
    };

    assert!(report.failures.is_empty());
    assert_eq!(
        fs::read_to_string(destination.join("folder/incoming.txt")).unwrap(),
        "incoming"
    );
    assert_eq!(
        fs::read_to_string(destination.join("folder/existing.txt")).unwrap(),
        "existing"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn atomic_rename_does_not_clobber_a_concurrently_created_destination() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::write(&source, "source").unwrap();
    let destination = temp.path().join("destination");
    fs::write(&destination, "concurrent").unwrap();

    let error = rename_noreplace(&source, &destination).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(source).unwrap(), "source");
    assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent");
}

#[cfg(unix)]
#[test]
fn replace_preserves_symlink_identity_and_rejects_a_changed_destination() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let other = temp.path().join("other");
    fs::write(&target, "target").unwrap();
    fs::write(&other, "other").unwrap();
    let source = temp.path().join("source-link");
    let destination = temp.path().join("destination-link");
    symlink(&target, &source).unwrap();
    symlink(&target, &destination).unwrap();
    let observed = FileIdentity::read(&destination).unwrap();
    fs::remove_file(&destination).unwrap();
    symlink(&other, &destination).unwrap();

    assert!(
        replace_exact(
            &source,
            &destination,
            crate::transfer::Action::Move,
            observed,
        )
        .is_err()
    );
    assert_eq!(fs::read_link(source).unwrap(), target);
    assert_eq!(fs::read_link(destination).unwrap(), other);
}

#[cfg(unix)]
#[test]
fn deleting_symlink_preserves_target() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    let link = temp.path().join("link");
    symlink(&target, &link).unwrap();
    delete_permanently(&link).unwrap();
    assert!(target.exists());
    assert!(!link.exists());
}
