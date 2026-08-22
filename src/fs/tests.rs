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
