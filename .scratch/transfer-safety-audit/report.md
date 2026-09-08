# Transfer safety audit

Date: 2026-09-08
Revision: 2dc9c44
Scope: remaining Copy/Move/Restore transfer defects, subsequently fixed.

## Fix outcome

All four reproductions are now permanent tests in `src/fs/tests.rs` and pass.

- Cross-device Move records source identities and metadata before copying, verifies the snapshot before publishing, and removes only recorded entries. Source additions and replacements during copying produce a failure with retry information while preserving the source and the original conflict destination. Cleanup uses nonrecursive directory removal, so late additions cannot be swept up by recursive deletion.
- Copy progress is fallible and propagates cancellation through recursive copying, sparse copying and each dense block of at most 1 MiB. Interrupted work cleans up its staging copy and retains the source for retry. A same-filesystem rename that has already completed remains a completed item.
- A Transfer carries its hardlink map across merged children and conflict pauses. Staged paths enter that map only after successful publication; cached destination identity, size and modification time are checked before reuse. Cross-device Move can reuse the mapping even when unlinking a previous source sibling reduced the remaining link count to one.
- Byte accounting retains a per-source high-water mark across a late conflict. Resuming neither resets the counter nor counts the same entry's retry bytes twice.

Additional tests cover cancellation for Copy and mapped cross-device Move (the path used by Restore), ordinary/Replace/Keep Both destinations, source-path replacement during Move, and hardlinks across cross-device merged Move conflicts. Tests use temporary directories; no live transfer or removable-drive data was touched.

Validation: all 68 filesystem tests pass; the complete release gate passed (483 tests passed, 8 ignored; 2 scrollbar tests; strict Clippy; release build; FileManager1 activation; 5 real X11 tests; desktop metadata and packaged archive smoke tests). All 7 performance benchmarks passed. Logs: `fix-red.log`, `fix-green.log`, `release-gate.log`, `benchmarks.log`.

The original audit evidence below describes the pre-fix revision. The saved patch is for reproducing that revision, not for applying on top of the fix.

Four defects reproduced through the real `TransferBatch` API and temporary files. Each failed in two consecutive runs. No production implementation was changed. Regression tests are saved as `reproduction-tests.patch`, rather than leaving the normal test suite failing. `/tmp` and `/dev/shm` must be on different devices for the cross-device fixture.

## P1 — Cross-device Move deletes uncopied additions to a source folder

Source: `src/fs/mutation.rs:158`, `src/fs/tree_copy.rs:49`, `src/fs/mutation.rs:306`.

Reproduction: move a folder containing a 4 MiB file to another filesystem. After the first partial byte update, create another file in the source folder. The new file disappears from the source and is absent from the destination. The transfer completes.

Cause: recursive copying snapshots the list of directory entries before copying children. After publication, the cross-device Move path recursively removes the entire current source tree, including entries that were never copied. The source-removal path needs to preserve and report source changes rather than delete entries outside the copied snapshot. Checking only the root inode would not detect this case.

Test: `hunt_cross_device_move_preserves_files_added_while_copying`.
Observed assertion: `Move deleted a newly created file without copying it`.

The same recursive removal primitive is also used after cross-device Replace; the committed reproduction directly covers an ordinary Move.

## P2 — Cancel during one large file is ignored

Source: `src/fs/transfer_batch.rs:204`, `src/fs/tree_copy.rs:224`.

Reproduction: set the cancellation flag after the first partial byte update while copying a 4 MiB file. The whole file is published and the report returns `cancelled: false`.

Cause: cancellation is checked only before pending entries, and is not passed into recursive or chunked copying. A single pending entry can contain a whole directory tree. With no subsequent entry, the cancellation flag is never checked again. Cancellation needs to propagate through copy work and prevent publication of an unfinished cancelled item, while retaining completed items and sources for retry.

Test: `hunt_cancel_during_a_single_large_copy_stops_before_publication`.

## P2 — Merging a copied folder silently breaks hardlinks

Source: `src/fs/transfer_batch.rs:454`, `src/fs/tree_copy.rs:15`.

Reproduction: source folder contains two names hardlinked to the same inode; destination already contains a folder with the same name. Choose Replace to merge. Destination names have different inodes, with no transfer failure.

Cause: merged children become separate pending entries, each using a fresh copy context and hardlink map. Ordinary whole-tree copying preserves this relationship. Merged copies instead duplicate the data and future changes through one name no longer appear through its sibling.

Test: `hunt_merge_copy_preserves_hardlinks_between_siblings`.

## P2 — A late destination conflict resets published byte progress

Source: `src/fs/transfer_batch.rs:303`.

Reproduction: create the destination file after the first partial copy update, before the staged copy is published. Resolve the resulting conflict with Keep Both. Published byte counters are:

`0, 1048576, 2097152, 3145728, 4194304, 0, 1048576, 2097152, 3145728, 4194304, 4194304, 4194304`.

Cause: the AlreadyExists branch requeues the entry and continues before saving `copied` into `self.copied_bytes[root]`. Resuming the transfer publishes stale progress. Define and retain consistent progress accounting across retry and conflict boundaries so the counter does not move backwards.

Test: `hunt_destination_race_does_not_reset_transfer_progress`.

## Reproduce

From this revision with a clean `src/fs/tests.rs`:

```sh
git apply .scratch/transfer-safety-audit/reproduction-tests.patch
PATH=/home/powerpenguini/.cargo/bin:$PATH cargo test fs::tests::hunt_
git apply -R .scratch/transfer-safety-audit/reproduction-tests.patch
```

Expected pre-fix result: 5 existing tests pass; 4 new regression tests fail. Both observed runs are stored in `red.log` and `red-repeat.log`. All fixture data is isolated in temporary directories and removed by the test harness.

After saving and removing the reproductions, `cargo test fs::tests::` passed all 61 existing filesystem tests (`baseline.log`). `git diff --check` and `git apply --check` for the saved patch passed. This audit did not run the full release gate or interact with a live transfer.
