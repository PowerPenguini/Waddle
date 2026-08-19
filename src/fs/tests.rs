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

#[test]
fn previews_directory_contents() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("folder");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("note.txt"), "hello").unwrap();
    let entry = FileEntry {
        path: folder,
        name: "folder".into(),
        directory: true,
    };

    let PreviewData::Directory(entries) = read_preview(&entry).unwrap() else {
        panic!("expected directory preview");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(display_name(&entries[0].name), "note.txt");
}

#[test]
fn previews_text_and_truncates_large_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("note.txt");
    fs::write(&path, "a".repeat(70 * 1024)).unwrap();
    let entry = FileEntry {
        path,
        name: "note.txt".into(),
        directory: false,
    };

    let PreviewData::Text {
        text, truncated, ..
    } = read_preview(&entry).unwrap()
    else {
        panic!("expected text preview");
    };
    assert!(truncated);
    assert_eq!(text.len(), 64 * 1024);
}

#[test]
fn binary_preview_contains_metadata_only() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("data.bin");
    fs::write(&path, [0, 1, 2, 3]).unwrap();
    let entry = FileEntry {
        path,
        name: "data.bin".into(),
        directory: false,
    };

    let PreviewData::Metadata(metadata) = read_preview(&entry).unwrap() else {
        panic!("expected metadata preview");
    };
    assert!(metadata.contains("4 B"));
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
