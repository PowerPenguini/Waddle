# Follow-up bug hunt

Base: `b28f78b` on `main`. Date: 2026-09-08.

Three additional defects were reproduced and fixed, one failing regression followed by its implementation at a time. Tests exercise the existing TransferBatch and persistent Journal interfaces with real temporary filesystem fixtures. Permission failures ran as UID 1000; the user's files and Trash were not used.

## Failed Copy leaves hidden staging directories

Copying a directory first preserves a read-only descendant, then encounters an unsupported FIFO. Cleanup cannot remove the read-only directory's contents and silently leaves `.waddle-replace-*` behind. The failing test observed a leftover staging tree.

Cleanup now grants the owner access to directories in the unpublished copy before recursively removing them. The regression checks that the destination has no leftovers and that source contents and permissions remain intact, including when the copied tree contains a symlink back to the source.

Test: `fs::tests::hunt_failed_copy_cleans_staging_with_read_only_descendants`.

## Failed Copy/Replace publishes the incoming copy without a successful result

Replacing a directory with a file exchanges the copy's staging path with the old destination. If removing a protected descendant fails, the old directory stays hidden at staging while the incoming copy occupies the destination, even though the Transfer reports failure and has no success receipt.

This failure now exchanges the paths back, cleans the unpublished incoming copy, and reports the error. The regression checks the old destination's location and contents, absence of hidden staging trees, source preservation, and successful Retry after permissions are repaired. The same staging helper also serves cross-filesystem Move/Replace.

Test: `fs::tests::hunt_failed_copy_replace_restores_the_destination`.

As with the earlier Move/Replace fix, restoring the destination path is not an atomic rollback of every recursive deletion: some old contents may have been removed before cleanup fails. A failure of the exchange itself reports both the cleanup and rollback errors.

## New Folder Undo fails after undoing creation of a child

Create a folder, create a file inside it, Undo the file, and Undo the folder. The final operation incorrectly refuses the same empty folder because its modification timestamp changed. A pinned initial timestamp makes this reproduction independent of clock resolution.

New Folder records now include directory device/inode identity. Undo checks that identity and still requires the directory to be empty; Redo refreshes the identity of the recreated directory. Records without the optional identity field remain readable and retain their older conservative fingerprint check.

Tests:

- `journal::tests::hunt_new_folder_undo_works_after_undoing_its_child_creation` exercises reopening the journal and a complete subsequent Redo/Undo cycle.
- `journal::tests::new_folder_undo_preserves_replacement_directories_and_symlinks` rejects different objects at the recorded path, even a replacement empty directory with a matching timestamp.
- `journal::tests::legacy_new_folder_records_still_undo_and_redo` checks backward compatibility.

## Validation

- The three `*-red.log` files contain the observed failures before their respective fixes.
- The three `*-green.log` files contain the corresponding passing reproductions.
- `release-gate.log`: 437 application tests passed; 7 benchmark tests intentionally ignored by the normal suite.
- Two vendored scrollbar tests and five real-X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- No UI rendering or input behavior changed in this pass. No additional interactive UI claim is made.

The scan also inspected Transfer conflict/retry handling, journal batch recovery, and navigation/search transitions. Only the three reproduced defects above are claimed as findings.
