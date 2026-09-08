use super::*;
use std::{error::Error as _, io, os::unix::fs::MetadataExt};

use crate::transfer::Action;

use super::transfer_batch::FileIdentity;

#[cfg(target_os = "linux")]
use super::mutation::replace_exact;

fn complete(batch: TransferBatch) -> TransferReport {
    let TransferBatchOutcome::Complete(report) = batch.run() else {
        panic!("Transfer unexpectedly paused for a conflict");
    };
    report
}

#[test]
fn copying_reports_bytes_before_the_file_is_complete() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("large.bin");
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let contents = vec![0x5a; 4 * 1024 * 1024];
    fs::write(&source, &contents).unwrap();
    let mut updates = Vec::new();
    let outcome = TransferBatch::try_new(vec![source.clone()], destination.clone(), Action::Copy)
        .unwrap()
        .run_with(|| false, |update| updates.push(update));
    assert!(
        matches!(outcome, TransferBatchOutcome::Complete(ref report) if report.failures.is_empty())
    );
    assert!(
        updates.iter().any(|p| p.completed_entries == 0
            && p.completed_bytes > 0
            && p.completed_bytes < contents.len() as u64),
        "no progress during file copy: {updates:?}"
    );
    assert_eq!(
        updates.last().unwrap().completed_bytes,
        contents.len() as u64
    );
    assert_eq!(fs::read(destination.join("large.bin")).unwrap(), contents);
    assert!(source.exists());
}

#[test]
fn merged_directory_moves_report_progress_between_children() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("tree");
    let target = temp.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(target.join("tree")).unwrap();
    fs::write(source.join("one"), b"first").unwrap();
    fs::write(source.join("two"), b"second").unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::try_new(vec![source.clone()], target.clone(), Action::Move)
            .unwrap()
            .run()
    else {
        panic!("directory merge must ask")
    };
    let mut updates = Vec::new();
    let result = batch
        .resolve(ConflictChoice::Replace, false)
        .run_with(|| false, |p| updates.push(p));
    assert!(
        matches!(result, TransferBatchOutcome::Complete(ref report) if report.failures.is_empty())
    );
    assert!(
        updates
            .iter()
            .any(|p| p.completed_entries == 0 && p.completed_bytes == 5),
        "no progress between child moves: {updates:?}"
    );
    assert_eq!(updates.last().unwrap().completed_bytes, 11);
    assert!(!source.exists());
    assert_eq!(fs::read(target.join("tree/two")).unwrap(), b"second");
}

#[test]
fn skipped_transfers_do_not_inflate_byte_progress() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("target");
    fs::create_dir(&destination).unwrap();
    fs::write(&source, b"skip me").unwrap();
    fs::write(destination.join("source"), b"keep me").unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::try_new(vec![source], destination, Action::Copy)
            .unwrap()
            .run()
    else {
        panic!("expected conflict")
    };
    let mut updates = Vec::new();
    let _ = batch
        .resolve(ConflictChoice::Skip, false)
        .run_with(|| false, |p| updates.push(p));
    assert!(
        updates.iter().all(|p| p.completed_bytes == 0),
        "skipped bytes were not transferred: {updates:?}"
    );
}

#[test]
fn copy_conflicts_and_cross_filesystem_moves_and_restores_report_live_bytes() {
    let source_root = tempfile::tempdir().unwrap();
    let destination_root = tempfile::tempdir_in("/dev/shm").expect("cross-filesystem fixture");
    assert_ne!(
        fs::metadata(source_root.path()).unwrap().dev(),
        fs::metadata(destination_root.path()).unwrap().dev(),
        "fixtures must exercise EXDEV"
    );
    for action in [Action::Copy, Action::Move] {
        for choice in [
            None,
            Some(ConflictChoice::Replace),
            Some(ConflictChoice::KeepBoth),
        ] {
            let source_dir = tempfile::tempdir_in(source_root.path()).unwrap();
            let destination = tempfile::tempdir_in(destination_root.path()).unwrap();
            let source = source_dir.path().join("large.bin");
            let content = vec![0x39; 4 * 1024 * 1024];
            fs::write(&source, &content).unwrap();
            if choice.is_some() {
                fs::write(destination.path().join("large.bin"), b"old").unwrap();
            }
            // Restore uses this same mapped Move path for Trash receipts.
            let batch = TransferBatch::try_new_mapped(
                vec![(source.clone(), destination.path().join("large.bin"))],
                action,
            )
            .unwrap();
            let mut updates = Vec::new();
            let mut outcome = batch.run_with(|| false, |p| updates.push(p));
            if let TransferBatchOutcome::Conflict { batch, .. } = outcome {
                outcome = batch
                    .resolve(choice.unwrap(), false)
                    .run_with(|| false, |p| updates.push(p));
            }
            let TransferBatchOutcome::Complete(report) = outcome else {
                panic!("conflict should resolve")
            };
            assert!(report.failures.is_empty(), "{:?}", report.failures);
            assert!(
                updates.iter().any(|p| p.completed_entries == 0
                    && p.completed_bytes > 0
                    && p.completed_bytes < content.len() as u64),
                "{action:?}/{choice:?}: no intermediate bytes"
            );
            assert!(
                updates
                    .windows(2)
                    .all(|p| p[0].completed_bytes <= p[1].completed_bytes)
            );
            assert!(updates.iter().all(|p| p.completed_bytes <= p.total_bytes));
            assert_eq!(
                updates.last().unwrap().completed_bytes,
                content.len() as u64
            );
            assert_eq!(fs::read(&report.completed[0]).unwrap(), content);
            assert_eq!(source.exists(), action == Action::Copy);
        }
    }
}

#[test]
fn sparse_and_linked_directory_copy_reports_monotonic_logical_bytes() {
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("tree");
    let destination = temp.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    let mut sparse = fs::File::create(source.join("sparse")).unwrap();
    sparse.set_len(8 * 1024 * 1024).unwrap();
    sparse.seek(SeekFrom::Start(2 * 1024 * 1024)).unwrap();
    sparse.write_all(&vec![0x43; 2 * 1024 * 1024]).unwrap();
    fs::hard_link(source.join("sparse"), source.join("linked")).unwrap();
    symlink("sparse", source.join("symlink")).unwrap();
    let mut updates = Vec::new();
    let outcome = TransferBatch::try_new(vec![source], destination.clone(), Action::Copy)
        .unwrap()
        .run_with(|| false, |p| updates.push(p));
    assert!(
        matches!(outcome, TransferBatchOutcome::Complete(ref report) if report.failures.is_empty())
    );
    assert!(updates.iter().any(|p| p.completed_entries == 0
        && p.completed_bytes > 0
        && p.completed_bytes < p.total_bytes));
    assert!(
        updates
            .windows(2)
            .all(|p| p[0].completed_bytes <= p[1].completed_bytes)
    );
    assert_eq!(
        updates.last().unwrap().completed_bytes,
        16 * 1024 * 1024 + 6
    );
    assert_eq!(
        updates.last().unwrap().completed_bytes,
        updates.last().unwrap().total_bytes
    );
    assert_eq!(
        fs::metadata(destination.join("tree/sparse")).unwrap().ino(),
        fs::metadata(destination.join("tree/linked")).unwrap().ino()
    );
}

#[test]
fn copying_a_fifo_fails_without_waiting_for_a_writer() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::mpsc, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    let fifo = CString::new(source.join("pipe").as_os_str().as_bytes()).unwrap();
    // SAFETY: fifo is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let (sender, receiver) = mpsc::channel();
    let target = destination.clone();
    let worker = std::thread::spawn(move || {
        let report = complete(TransferBatch::new(vec![source], target, Action::Copy));
        sender.send(report).unwrap();
    });
    let report = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("Copy must reject a FIFO without blocking");
    worker.join().unwrap();
    assert!(report.completed.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert!(report.failures[0].error.contains("special files"));
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
}

#[test]
fn copy_and_replace_accept_maximum_length_filenames() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    for name in [
        "a".repeat(255),
        format!(".waddle-replace-{}-0", std::process::id()),
    ] {
        let source = temp.path().join(&name);
        fs::write(&source, "first").unwrap();
        let report = complete(TransferBatch::new(
            vec![source.clone()],
            destination.clone(),
            Action::Copy,
        ));
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(
            fs::read_to_string(destination.join(&name)).unwrap(),
            "first"
        );

        fs::write(&source, "replacement").unwrap();
        let TransferBatchOutcome::Conflict { batch, .. } =
            TransferBatch::new(vec![source], destination.clone(), Action::Copy).run()
        else {
            panic!("expected conflict");
        };
        let report = complete(batch.resolve(ConflictChoice::Replace, false));
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(
            fs::read_to_string(destination.join(&name)).unwrap(),
            "replacement"
        );
    }
    assert_eq!(fs::read_dir(destination).unwrap().count(), 2);
}

#[test]
fn resolving_a_conflict_defers_mutation_until_the_worker_runs() {
    for action in [Action::Copy, Action::Move] {
        for choice in [ConflictChoice::Replace, ConflictChoice::KeepBoth] {
            let temp = tempfile::tempdir().unwrap();
            let source = temp.path().join("item");
            let destination = temp.path().join("destination");
            fs::create_dir(&destination).unwrap();
            fs::write(&source, "incoming").unwrap();
            fs::write(destination.join("item"), "existing").unwrap();
            let TransferBatchOutcome::Conflict { batch, .. } =
                TransferBatch::new(vec![source.clone()], destination.clone(), action).run()
            else {
                panic!("expected conflict");
            };
            let resumed = batch.resolve(choice, false);
            assert_eq!(fs::read_to_string(&source).unwrap(), "incoming");
            assert_eq!(
                fs::read_to_string(destination.join("item")).unwrap(),
                "existing"
            );
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);

            let report = complete(resumed);
            assert!(report.failures.is_empty());
            let name = if choice == ConflictChoice::KeepBoth {
                "item copy"
            } else {
                "item"
            };
            assert_eq!(
                fs::read_to_string(destination.join(name)).unwrap(),
                "incoming"
            );
            assert_eq!(source.exists(), action == Action::Copy);
        }
    }
}

#[test]
fn cancellation_before_a_resolved_conflict_runs_keeps_both_files() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("item");
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::write(&source, "incoming").unwrap();
    fs::write(destination.join("item"), "existing").unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::new(vec![source.clone()], destination.clone(), Action::Move).run()
    else {
        panic!("expected conflict");
    };
    let TransferBatchOutcome::Complete(report) = batch
        .resolve(ConflictChoice::Replace, false)
        .run_with(|| true, |_| {})
    else {
        panic!("expected cancellation");
    };
    assert!(report.cancelled);
    assert_eq!(report.retry, [(source.clone(), destination.join("item"))]);
    assert_eq!(fs::read_to_string(source).unwrap(), "incoming");
    assert_eq!(
        fs::read_to_string(destination.join("item")).unwrap(),
        "existing"
    );
}

#[test]
fn failed_move_cleanup_retries_the_remaining_tree_once() {
    use std::os::unix::fs::PermissionsExt;

    // Root bypasses the directory permission failure used by this test.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source/folder");
    let destination = temp.path().join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(destination.join("folder")).unwrap();
    fs::write(source.join("a"), "moved once").unwrap();
    fs::write(source.join("b"), "incoming").unwrap();
    fs::write(destination.join("folder/b"), "existing").unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::new(vec![source.clone()], destination.clone(), Action::Move).run()
    else {
        panic!("expected folder conflict");
    };
    let TransferBatchOutcome::Conflict { batch, .. } =
        batch.resolve(ConflictChoice::Replace, false).run()
    else {
        panic!("expected child conflict");
    };
    assert!(!source.join("a").exists());
    fs::set_permissions(&source, fs::Permissions::from_mode(0o500)).unwrap();
    let report = complete(batch.resolve(ConflictChoice::Replace, false));
    fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(report.failures.len(), 2);
    assert_eq!(report.retry, [(source.clone(), destination.join("folder"))]);

    fs::write(destination.join("folder/a"), "edited after Move").unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::try_new_mapped(report.retry, Action::Move)
            .unwrap()
            .run()
    else {
        panic!("expected remaining folder conflict");
    };
    let report = complete(batch.resolve(ConflictChoice::Replace, true));
    assert!(report.failures.is_empty());
    assert!(!source.exists());
    assert_eq!(
        fs::read_to_string(destination.join("folder/a")).unwrap(),
        "edited after Move"
    );
    assert_eq!(
        fs::read_to_string(destination.join("folder/b")).unwrap(),
        "incoming"
    );
    assert!(!destination.join("b").exists());
}

#[test]
fn validates_names() {
    for bad in ["", ".", "..", "a/b", "a\0b"] {
        assert!(validate_name(bad).is_err());
    }
    assert!(validate_name("new folder").is_ok());
}

#[test]
fn browse_error_preserves_the_io_error_source() {
    let temp = tempfile::tempdir().unwrap();
    let error = read_directory(&temp.path().join("missing")).unwrap_err();

    assert!(
        error
            .source()
            .is_some_and(|source| source.is::<io::Error>())
    );
}

#[test]
fn reads_hidden_by_default_and_sorts_by_type_ascending() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Alpha"), "x").unwrap();
    fs::write(temp.path().join("beta"), "x").unwrap();
    fs::write(temp.path().join(".hidden"), "x").unwrap();
    fs::create_dir(temp.path().join("z-folder")).unwrap();
    let entries = read_directory(temp.path()).unwrap();
    assert!(
        entries
            .iter()
            .find(|entry| entry.name == ".hidden")
            .is_some_and(FileEntry::is_hidden)
    );
    assert!(
        entries
            .iter()
            .find(|entry| entry.name == "Alpha")
            .is_some_and(|entry| !entry.is_hidden())
    );
    let names: Vec<_> = entries.into_iter().map(|e| display_name(&e.name)).collect();
    assert_eq!(names, ["z-folder", ".hidden", "Alpha", "beta"]);
}

#[test]
fn type_sort_orders_folders_extensions_then_extensionless_files() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("a-folder")).unwrap();
    fs::write(temp.path().join("z-file"), "x").unwrap();
    fs::write(temp.path().join("notes.md"), "x").unwrap();

    let entries = read_directory_with(
        temp.path(),
        BrowseOptions {
            sort: SortKey::Type,
            ..BrowseOptions::default()
        },
    )
    .unwrap();
    let names: Vec<_> = entries
        .iter()
        .map(|entry| display_name(&entry.name))
        .collect();

    assert_eq!(names, ["a-folder", "notes.md", "z-file"]);
}

#[test]
fn directory_entries_retain_size_and_modified_metadata_for_list_details() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("report.txt"), "four").unwrap();

    let entry = read_directory(temp.path())
        .unwrap()
        .into_iter()
        .find(|entry| entry.name == "report.txt")
        .unwrap();

    assert_eq!(entry.metadata.size, Some(4));
    assert!(entry.metadata.modified.is_some());
}

#[cfg(target_os = "linux")]
#[test]
fn dormant_automount_metadata_stays_lazy() {
    use std::collections::HashSet;

    let mut statx = unsafe { std::mem::zeroed::<libc::statx>() };
    statx.stx_mask = libc::STATX_SIZE | libc::STATX_MTIME;
    statx.stx_size = 4096;
    statx.stx_mtime.tv_sec = 123;
    statx.stx_attributes = libc::STATX_ATTR_AUTOMOUNT as u64;

    let dormant = super::browse::statx_entry_metadata(&statx);
    assert_eq!(dormant.size, None);
    assert_eq!(dormant.modified, None);
    assert!(!super::browse::statx_is_watchable_directory(
        &statx,
        &HashSet::new()
    ));

    statx.stx_attributes = 0;
    statx.stx_mode = libc::S_IFDIR as u16;
    let ordinary = super::browse::statx_entry_metadata(&statx);
    assert_eq!(ordinary.size, Some(4096));
    assert_eq!(ordinary.modified, Some(123));
    assert!(super::browse::statx_is_watchable_directory(
        &statx,
        &HashSet::new()
    ));

    statx.stx_mnt_id = 45;
    assert!(!super::browse::statx_is_watchable_directory(
        &statx,
        &HashSet::from([45])
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn mountinfo_identifies_autofs_mount_ids_and_paths() {
    assert_eq!(
        super::browse::automount_mount(
            "45 1 0:44 / /home/arc\\040files rw - autofs systemd-1 rw,fd=77"
        ),
        Some((45, PathBuf::from("/home/arc files")))
    );
    assert_eq!(
        super::browse::automount_mount("46 1 8:1 / /home rw - ext4 /dev/sda1 rw"),
        None
    );
}

#[test]
fn opened_directory_reuses_one_scan_for_sidebar_folders() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("folder-b")).unwrap();
    fs::create_dir(temp.path().join("folder-a")).unwrap();
    fs::create_dir(temp.path().join(".hidden-folder")).unwrap();
    fs::write(temp.path().join("notes.txt"), "x").unwrap();
    symlink(
        temp.path().join("folder-a"),
        temp.path().join("folder-link"),
    )
    .unwrap();

    let opened = open_directory_with(
        temp.path(),
        BrowseOptions {
            show_hidden: false,
            ..BrowseOptions::default()
        },
    )
    .unwrap();

    assert_eq!(
        opened
            .child_folders
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["folder-a", "folder-b"]
    );
}

#[test]
fn revealed_hidden_entry_bypasses_filter_without_showing_its_siblings() {
    let temp = tempfile::tempdir().unwrap();
    let revealed = temp.path().join(".download");
    fs::write(&revealed, "x").unwrap();
    fs::write(temp.path().join(".other"), "x").unwrap();

    let opened = open_directory_revealing(
        temp.path(),
        BrowseOptions {
            show_hidden: false,
            ..BrowseOptions::default()
        },
        std::slice::from_ref(&revealed),
    )
    .unwrap();

    assert_eq!(
        opened
            .entries
            .iter()
            .map(|entry| entry.path.as_path())
            .collect::<Vec<_>>(),
        [revealed.as_path()]
    );
}

#[test]
fn browse_options_apply_natural_sort_metadata_keys_and_hidden_visibility() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("file-10.txt"), vec![0; 10]).unwrap();
    fs::write(temp.path().join("file-2.txt"), vec![0; 2]).unwrap();
    fs::write(temp.path().join(".hidden"), "x").unwrap();
    let natural = read_directory_with(
        temp.path(),
        BrowseOptions {
            sort: SortKey::Name,
            ..BrowseOptions::default()
        },
    )
    .unwrap();
    assert_eq!(display_name(&natural[1].name), "file-2.txt");
    assert_eq!(display_name(&natural[2].name), "file-10.txt");

    let by_size = read_directory_with(
        temp.path(),
        BrowseOptions {
            sort: SortKey::Size,
            descending: true,
            show_hidden: true,
            ..BrowseOptions::default()
        },
    )
    .unwrap();
    assert_eq!(display_name(&by_size[0].name), "file-10.txt");
    assert!(by_size.iter().any(|entry| entry.name == ".hidden"));
}

#[test]
fn size_sort_keeps_folders_first_in_both_directions() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("z-folder")).unwrap();
    fs::create_dir(temp.path().join("a-folder")).unwrap();
    fs::write(temp.path().join("small.bin"), vec![0; 2]).unwrap();
    fs::write(temp.path().join("large.bin"), vec![0; 10]).unwrap();

    for (descending, expected) in [
        (false, ["a-folder", "z-folder", "small.bin", "large.bin"]),
        (true, ["a-folder", "z-folder", "large.bin", "small.bin"]),
    ] {
        let entries = read_directory_with(
            temp.path(),
            BrowseOptions {
                sort: SortKey::Size,
                descending,
                ..BrowseOptions::default()
            },
        )
        .unwrap();
        let names: Vec<_> = entries
            .iter()
            .map(|entry| display_name(&entry.name))
            .collect();

        assert_eq!(names, expected);
    }
}

#[test]
fn hidden_preference_is_shared_by_recursive_search_and_tree_traversal() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join(".private")).unwrap();
    fs::write(temp.path().join(".private/needle.txt"), "hidden").unwrap();

    let hidden = search_directory_with_hidden(temp.path(), "needle", 100, false, || false).unwrap();
    assert!(hidden.entries.is_empty());
    assert!(
        read_child_folders_with_hidden(temp.path(), false)
            .unwrap()
            .is_empty()
    );

    let visible = search_directory_with_hidden(temp.path(), "needle", 100, true, || false).unwrap();
    assert_eq!(visible.entries.len(), 1);
    assert_eq!(
        read_child_folders_with_hidden(temp.path(), true).unwrap(),
        [temp.path().join(".private")]
    );
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
    for index in (0..10_000).rev() {
        fs::write(temp.path().join(format!("item-{index:05}")), "x").unwrap();
    }

    let entries = read_directory(temp.path()).unwrap();
    assert_eq!(entries.len(), 10_000);
    assert_eq!(display_name(&entries[0].name), "item-00000");
    assert_eq!(display_name(&entries[9_999].name), "item-09999");

    let search = search_directory_with_hidden(temp.path(), "09999", 100, false, || false).unwrap();
    assert_eq!(search.entries.len(), 1);
    assert_eq!(display_name(&search.entries[0].name), "item-09999");
}

#[test]
fn reads_child_folders_hidden_filtered_and_sorted() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("Zulu")).unwrap();
    fs::create_dir(temp.path().join("alpha")).unwrap();
    fs::create_dir(temp.path().join(".hidden")).unwrap();
    fs::write(temp.path().join("file"), "x").unwrap();

    let names: Vec<_> = read_child_folders(temp.path())
        .unwrap()
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
fn new_empty_files_never_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let created = create_file(temp.path(), "empty.txt").unwrap();

    assert_eq!(fs::read_to_string(&created).unwrap(), "");
    assert!(create_file(temp.path(), "empty.txt").is_err());
    assert_eq!(fs::read_to_string(created).unwrap(), "");
}

#[test]
fn moves_file_into_directory() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("note.txt");
    let destination_directory = temp.path().join("archive");
    fs::write(&source, "hello").unwrap();
    fs::create_dir(&destination_directory).unwrap();

    let report = complete(
        TransferBatch::try_new(
            vec![source.clone()],
            destination_directory.clone(),
            Action::Move,
        )
        .unwrap(),
    );
    let destination = report.completed.into_iter().next().unwrap();

    assert_eq!(destination, destination_directory.join("note.txt"));
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(destination).unwrap(), "hello");
}

#[test]
fn cross_filesystem_move_publishes_only_a_complete_destination() {
    let Ok(destination_root) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    let source_root = tempfile::tempdir().unwrap();
    if fs::metadata(source_root.path()).unwrap().dev()
        == fs::metadata(destination_root.path()).unwrap().dev()
    {
        return;
    }
    let source = source_root.path().join("tree");
    let destination = destination_root.path().join("tree");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("one"), "one").unwrap();
    fs::write(source.join("two"), "two").unwrap();

    move_exact(&source, &destination).unwrap();

    assert!(!source.exists());
    assert_eq!(fs::read_to_string(destination.join("one")).unwrap(), "one");
    assert_eq!(fs::read_to_string(destination.join("two")).unwrap(), "two");
    assert!(fs::read_dir(destination_root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".waddle-")
    }));
}

#[cfg(unix)]
#[test]
fn failed_cross_filesystem_move_removes_private_staging() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let Ok(destination_root) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    let source_root = tempfile::tempdir().unwrap();
    if fs::metadata(source_root.path()).unwrap().dev()
        == fs::metadata(destination_root.path()).unwrap().dev()
    {
        return;
    }
    let source = source_root.path().join("tree");
    let destination = destination_root.path().join("tree");
    fs::create_dir(&source).unwrap();
    let unreadable = source.join("unreadable");
    fs::write(&unreadable, "secret").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    assert!(move_exact(&source, &destination).is_err());

    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(source.exists());
    assert!(!destination.exists());
    assert!(fs::read_dir(destination_root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".waddle-")
    }));
}

#[cfg(unix)]
#[test]
fn cross_filesystem_move_reports_failure_and_keeps_complete_destination_if_source_removal_fails() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let Ok(destination_root) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    let source_root = tempfile::tempdir().unwrap();
    if fs::metadata(source_root.path()).unwrap().dev()
        == fs::metadata(destination_root.path()).unwrap().dev()
    {
        return;
    }
    let source = source_root.path().join("tree");
    let destination = destination_root.path().join("tree");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("kept"), "complete").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o500)).unwrap();

    let TransferBatchOutcome::Complete(report) = TransferBatch::try_new(
        vec![source.clone()],
        destination_root.path().to_path_buf(),
        Action::Move,
    )
    .unwrap()
    .run() else {
        panic!("source-removal failure must produce a report");
    };

    fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(report.failures.len(), 1);
    assert!(
        report.failures[0]
            .error
            .contains("complete destination was kept")
    );
    assert!(report.completed.is_empty());
    assert!(report.receipts.is_empty());
    assert!(source.exists());
    assert_eq!(
        fs::read_to_string(destination.join("kept")).unwrap(),
        "complete"
    );
}

#[test]
fn transfer_preflight_revalidates_sources_destination_and_descendants() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    let source = temp.path().join("source");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&source).unwrap();
    let descendant = source.join("child");
    fs::create_dir(&descendant).unwrap();

    assert!(TransferBatch::try_new(vec![source.clone()], descendant, Action::Copy,).is_err());
    assert!(
        TransferBatch::try_new(
            vec![temp.path().join("missing")],
            destination.clone(),
            Action::Copy,
        )
        .is_err()
    );
    let not_a_directory = temp.path().join("plain");
    fs::write(&not_a_directory, "x").unwrap();
    assert!(TransferBatch::try_new(vec![source], not_a_directory, Action::Copy,).is_err());
}

#[test]
fn transfer_preflight_rejects_a_move_to_the_existing_parent() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("note");
    fs::write(&source, "x").unwrap();

    assert!(
        TransferBatch::try_new(vec![source], temp.path().to_path_buf(), Action::Move,).is_err()
    );
}

#[cfg(unix)]
#[test]
fn transfer_preflight_preserves_symlinks_and_checks_destination_permissions() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};

    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let link = temp.path().join("dangling-link");
    symlink(temp.path().join("missing-target"), &link).unwrap();
    assert!(TransferBatch::try_new(vec![link], destination.clone(), Action::Copy,).is_ok());

    if unsafe { libc::geteuid() } != 0 {
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o500)).unwrap();
        let source = temp.path().join("source");
        fs::write(&source, "x").unwrap();
        assert!(TransferBatch::try_new(vec![source], destination, Action::Copy).is_err());
    }
}

#[test]
fn move_pauses_before_an_existing_destination() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("note.txt");
    let destination_directory = temp.path().join("archive");
    fs::write(&source, "source").unwrap();
    fs::create_dir(&destination_directory).unwrap();
    fs::write(destination_directory.join("note.txt"), "existing").unwrap();

    let TransferBatchOutcome::Conflict { conflict, .. } = TransferBatch::try_new(
        vec![source.clone()],
        destination_directory.clone(),
        Action::Move,
    )
    .unwrap()
    .run() else {
        panic!("existing destination must pause the Transfer");
    };

    assert_eq!(conflict.source, source);
    assert_eq!(conflict.destination, destination_directory.join("note.txt"));
    assert_eq!(fs::read_to_string(&source).unwrap(), "source");
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

    assert!(
        TransferBatch::try_new(vec![source.clone()], descendant.clone(), Action::Move).is_err()
    );
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

    let first = complete(
        TransferBatch::try_new(vec![source.clone()], destination.clone(), Action::Copy).unwrap(),
    )
    .completed
    .into_iter()
    .next()
    .unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::try_new(vec![source.clone()], destination.clone(), Action::Copy)
            .unwrap()
            .run()
    else {
        panic!("the second copy must pause for a conflict");
    };
    let second = complete(batch.resolve(ConflictChoice::KeepBoth, false))
        .completed
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(first, destination.join("note.txt"));
    assert_eq!(second, destination.join("note copy.txt"));
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

    let copied = complete(
        TransferBatch::try_new(vec![source.clone()], destination.clone(), Action::Copy).unwrap(),
    )
    .completed
    .into_iter()
    .next()
    .unwrap();

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
    let xattr_supported = set_xattr(&sparse, "user.waddle-test", b"kept").is_ok();

    let copied = complete(
        TransferBatch::try_new(vec![source.clone()], destination.clone(), Action::Copy).unwrap(),
    )
    .completed
    .into_iter()
    .next()
    .unwrap();
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
            get_xattr(&copied_sparse, "user.waddle-test").unwrap(),
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

    assert!(
        TransferBatch::try_new(vec![source.clone()], descendant.clone(), Action::Copy).is_err()
    );
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

    let report = complete(TransferBatch::new(
        vec![first, missing.clone(), second],
        destination.clone(),
        Action::Copy,
    ));

    assert_eq!(report.completed.len(), 2);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].source, missing);
    assert!(destination.join("first.txt").exists());
    assert!(destination.join("second.txt").exists());
}

#[test]
fn cancelling_a_paused_batch_move_keeps_earlier_successes() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let first = temp.path().join("first.txt");
    let conflict = temp.path().join("conflict.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&conflict, "source").unwrap();
    fs::write(destination.join("conflict.txt"), "destination").unwrap();

    let TransferBatchOutcome::Conflict {
        batch,
        conflict: paused,
    } = TransferBatch::new(
        vec![first.clone(), conflict.clone()],
        destination.clone(),
        Action::Move,
    )
    .run()
    else {
        panic!("the existing destination must pause the Transfer");
    };
    assert_eq!(paused.source, conflict);
    let report = batch.cancel();

    assert_eq!(report.completed, vec![destination.join("first.txt")]);
    assert!(report.failures.is_empty());
    assert_eq!(report.retained, [conflict]);
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
        fs::read_to_string(destination.join("first copy.txt")).unwrap(),
        "new first"
    );
    assert_eq!(
        fs::read_to_string(destination.join("second.txt")).unwrap(),
        "new second"
    );
}

#[test]
fn mapped_transfer_restores_each_source_to_its_exact_destination() {
    let temp = tempfile::tempdir().unwrap();
    let trash = temp.path().join("Trash/files");
    let first_destination = temp.path().join("one/original.txt");
    let second_destination = temp.path().join("two/renamed.txt");
    fs::create_dir_all(&trash).unwrap();
    fs::create_dir_all(first_destination.parent().unwrap()).unwrap();
    fs::create_dir_all(second_destination.parent().unwrap()).unwrap();
    let first = trash.join("original.txt.2");
    let second = trash.join("renamed.txt.8");
    fs::write(&first, "one").unwrap();
    fs::write(&second, "two").unwrap();

    let outcome = TransferBatch::new_mapped(
        [
            (first.clone(), first_destination.clone()),
            (second.clone(), second_destination.clone()),
        ],
        Action::Move,
    )
    .run();
    let TransferBatchOutcome::Complete(report) = outcome else {
        panic!("unexpected conflict");
    };

    assert_eq!(
        report.completed,
        [first_destination.clone(), second_destination.clone()]
    );
    assert!(!first.exists());
    assert!(!second.exists());
    assert_eq!(fs::read_to_string(first_destination).unwrap(), "one");
    assert_eq!(fs::read_to_string(second_destination).unwrap(), "two");
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
            .starts_with(".waddle-")
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

#[test]
fn hunt_failed_move_replace_keeps_the_incoming_source_for_retry() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("item");
    let destination_parent = temp.path().join("destination");
    fs::create_dir(&destination_parent).unwrap();
    let destination = destination_parent.join("item");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(destination.join("locked")).unwrap();
    fs::write(destination.join("locked/old.txt"), "old contents").unwrap();
    fs::write(&source, "incoming contents").unwrap();
    fs::set_permissions(
        destination.join("locked"),
        fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } = TransferBatch::new(
        vec![source.clone()],
        destination_parent.clone(),
        Action::Move,
    )
    .run() else {
        panic!("expected conflict")
    };
    let report = complete(batch.resolve(ConflictChoice::Replace, false));
    // Restore permissions at either location before asserting, even on the buggy implementation.
    for path in [&source, &destination] {
        if path.join("locked").is_dir() {
            fs::set_permissions(path.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    assert!(
        !report.failures.is_empty(),
        "fixture must make old-destination cleanup fail"
    );
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        "incoming contents",
        "failed Move must not substitute old destination data for its source"
    );
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::new(vec![source.clone()], destination_parent, Action::Move).run()
    else {
        panic!("expected retry conflict")
    };
    let retry = complete(batch.resolve(ConflictChoice::Replace, false));
    assert!(retry.failures.is_empty(), "{:?}", retry.failures);
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        "incoming contents"
    );
}

#[test]
fn hunt_keep_both_can_duplicate_a_maximum_length_name() {
    for name in [
        "a".repeat(255),
        "é".repeat(127),
        format!("{}.png", "a".repeat(251)),
        format!("{}.png", "é".repeat(125)),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join(&name);
        fs::write(&source, "data").unwrap();
        for _ in 0..12 {
            let report = complete(TransferBatch::new(
                vec![source.clone()],
                temp.path().to_path_buf(),
                Action::Copy,
            ));
            assert!(report.failures.is_empty(), "{:?}", report.failures);
            assert_eq!(report.receipts.len(), 1);
            assert_ne!(report.receipts[0].destination, source);
            assert_eq!(
                report.receipts[0].destination.extension(),
                source.extension()
            );
            assert_eq!(
                fs::read_to_string(&report.receipts[0].destination).unwrap(),
                "data"
            );
            assert!(
                report.receipts[0]
                    .destination
                    .file_name()
                    .unwrap()
                    .to_str()
                    .is_some()
            );
            assert_eq!(fs::read_to_string(&source).unwrap(), "data");
        }
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 13);
    }
}

#[test]
fn hunt_keep_both_preserves_dotted_directory_names_and_non_utf8_file_extensions() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("folder.v1");
    fs::create_dir(&source).unwrap();
    let report = complete(TransferBatch::new(
        vec![source],
        temp.path().to_path_buf(),
        Action::Copy,
    ));
    assert!(report.failures.is_empty());
    assert_eq!(
        report.receipts[0].destination,
        temp.path().join("folder.v1 copy")
    );
    let mut name = vec![0xff; 251];
    name.extend(b".png");
    let source = temp.path().join(std::ffi::OsString::from_vec(name));
    fs::write(&source, "data").unwrap();
    let report = complete(TransferBatch::new(
        vec![source],
        temp.path().to_path_buf(),
        Action::Copy,
    ));
    assert!(report.failures.is_empty());
    let destination = &report.receipts[0].destination;
    assert_eq!(destination.extension().unwrap(), "png");
    assert!(destination.file_name().unwrap().as_bytes().len() <= 255);
    assert_eq!(fs::read_to_string(destination).unwrap(), "data");
}

#[test]
fn hunt_failed_copy_cleans_staging_with_read_only_descendants() {
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    };
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let protected = source.join("a-protected");
    let destination = temp.path().join("destination");
    fs::create_dir_all(&protected).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(protected.join("keep.txt"), "source contents").unwrap();
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o500)).unwrap();
    std::os::unix::fs::symlink(&protected, source.join("a-link")).unwrap();
    let fifo = CString::new(source.join("z-pipe").as_os_str().as_bytes()).unwrap();
    // SAFETY: fifo is a live NUL-terminated path to a temporary fixture.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);

    let report = complete(
        TransferBatch::try_new(vec![source.clone()], destination.clone(), Action::Copy).unwrap(),
    );
    let leftovers = fs::read_dir(&destination)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    // Repair fixture permissions even when running against the broken cleanup.
    for path in &leftovers {
        if path.join("a-protected").is_dir() {
            fs::set_permissions(path.join("a-protected"), fs::Permissions::from_mode(0o700))
                .unwrap();
        }
    }
    let source_mode = fs::metadata(&protected).unwrap().permissions().mode() & 0o777;
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(report.failures.len(), 1);
    assert!(report.failures[0].error.contains("special files"));
    assert_eq!(source_mode, 0o500, "cleanup must not chmod the original");
    assert_eq!(
        fs::read_to_string(protected.join("keep.txt")).unwrap(),
        "source contents"
    );
    assert!(
        leftovers.is_empty(),
        "failed Copy leaked staging trees: {leftovers:?}"
    );
}

#[test]
fn hunt_failed_copy_replace_restores_the_destination() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("item");
    let parent = temp.path().join("destination");
    let destination = parent.join("item");
    fs::create_dir_all(destination.join("locked")).unwrap();
    fs::write(destination.join("locked/old.txt"), "old contents").unwrap();
    fs::write(&source, "incoming contents").unwrap();
    fs::set_permissions(
        destination.join("locked"),
        fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::try_new(vec![source.clone()], parent.clone(), Action::Copy)
            .unwrap()
            .run()
    else {
        panic!("expected a replacement conflict");
    };
    let report = complete(batch.resolve(ConflictChoice::Replace, false));
    let entries = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    for path in &entries {
        if path.join("locked").is_dir() {
            fs::set_permissions(path.join("locked"), fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    assert_eq!(report.failures.len(), 1, "protected cleanup must fail");
    assert_eq!(fs::read_to_string(&source).unwrap(), "incoming contents");
    assert!(
        destination.is_dir(),
        "failed Replace must restore the old destination path"
    );
    assert_eq!(
        fs::read_to_string(destination.join("locked/old.txt")).unwrap(),
        "old contents"
    );
    assert_eq!(
        entries.as_slice(),
        std::slice::from_ref(&destination),
        "failed Replace must not leak a hidden tree"
    );

    let TransferBatchOutcome::Conflict { batch, .. } =
        TransferBatch::try_new(vec![source], parent, Action::Copy)
            .unwrap()
            .run()
    else {
        panic!("expected retry conflict");
    };
    let retry = complete(batch.resolve(ConflictChoice::Replace, false));
    assert!(retry.failures.is_empty(), "{:?}", retry.failures);
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        "incoming contents"
    );
}
