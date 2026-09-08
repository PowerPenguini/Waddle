# Bug hunt after the main commit

Base commit: `1fae6c6` on `main`. Date: 2026-09-08.

Examined filesystem mutations, conflict resolution, Trash recovery, persistent Undo/Redo, and the filename/thumbnail boundary. Four defects were reproduced with failing Rust tests and fixed. This is a focused audit, not a claim that the entire codebase is bug-free.

## 1. Move/Replace can substitute old destination data for the incoming source

Replacing a nonempty directory with a file exchanges their paths. If deleting a protected descendant then fails, the old destination remains at the source path, which is subsequently offered for Retry. The minimized test checks that the incoming source is still a file with its original contents and then completes Retry. Before the fix, reading that source failed with `Is a directory`.

The failure path now exchanges the entries back before reporting the cleanup error. The regression uses an unprivileged temporary directory with a protected nested folder, restores permissions, and verifies that Retry publishes the incoming data. If rollback itself fails, the error reports both failures. Replacement is not a crash-atomic recursive filesystem transaction; some authorized deletion of old destination contents can already have occurred before a cleanup failure.

Test: `fs::tests::hunt_failed_move_replace_keeps_the_incoming_source_for_retry`.

## 2. Keep Both fails for valid maximum-length filenames

Appending ` copy` to a 255-byte name produces an invalid destination. The failing Transfer test reported `File name too long`.

Copy-name allocation now reads the destination filesystem's component limit, reserves space for the suffix and extension, truncates the stem at a valid UTF-8 boundary when applicable, and propagates lookup failures instead of treating every error as an available path. Coverage includes ASCII and multibyte names, repeated collisions through double-digit suffixes, and non-UTF-8 names.

Tests: `fs::tests::hunt_keep_both_can_duplicate_a_maximum_length_name` and `fs::tests::hunt_keep_both_preserves_dotted_directory_names_and_non_utf8_file_extensions`.

## 3. Undo Trash cannot resume after metadata cleanup fails

The original implementation restored the physical file before deleting its `.trashinfo` file. If metadata cleanup failed, the journal cursor stayed unchanged without recording the physical move. Retrying Undo then failed while fingerprinting the now-missing trashed path.

The journal now persists per-item restore progress on failure, verifies already-restored contents, skips completed moves, tolerates metadata already removed by an earlier batch step, and clears progress after the full restore step succeeds. The regression restores two files, fails cleanup of the second, reopens the journal, and retries successfully. It also exercises a legacy record without the new optional progress field. Filesystem effects and journal persistence remain separate operations; this does not claim crash-atomic recovery.

Test: `journal::tests::hunt_trash_undo_can_retry_after_metadata_cleanup_fails`.

## 4. Keep Both removes thumbnail eligibility by changing the file extension

Duplicating `photo.png` previously produced `photo.png copy`. The thumbnail cache no longer recognized the extension and queued no decoder. A test using a real copied PNG reproduced the missing request.

Names now take the form `photo copy.png`. Directory names retain their existing suffix style, including dotted directory names. The end-to-end regression passes the copied file through the real thumbnail queue and decoder and obtains a rendered-image handle.

Test: `app::thumbnail::tests::hunt_keep_both_images_still_receive_thumbnails`.

## Validation

- Before-fix evidence: `reproductions-before.log` and `thumbnail-before.log`.
- Reproduction command: `cargo test hunt_ -- --nocapture`.
- Final release gate: `scripts/release-gate.sh` passed; see `release-gate.log`.
- 432 application tests, 2 vendored scrollbar tests, and 5 X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- Permission-based tests ran as UID 1000. Fixtures were temporary; real user files and Trash were not used.
- These fixes were checked through actual filesystem effects and compiled tests. No new interactive UI claim is made for this pass. The prior seven performance benchmarks covered the committed appearance work; this pass changes filesystem/journal behavior, with only a test addition in the thumbnail module.
