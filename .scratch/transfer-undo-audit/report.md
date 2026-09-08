# Transfer Undo, Redo and Restore audit

Date: 2026-09-08
Revision: 3dab5d9
Scope: diagnosis only. Three remaining issues reproduced by four tests, failing in two consecutive runs. No production implementation changes.

## Round 1 — hardlinks in history operations

The hardlink finding below is fixed for a single execution of Undo/Redo. `JournalTransfer` retains the copy context across all entries in that history operation. Transfer Undo/Redo and Trash Undo (also used by Restore Redo) use it. Safety checks and per-item error tracking remain in the journal.

Three permanent tests cover Copy Redo after reopening the journal, cross-device Move Undo, and cross-device Trash Undo. The two original hardlink regressions failed in `round-1-red.log` and passed in `round-1-green.log`; the latter deliberately also records the still-failing partial-directory Undo reproduction before that pending test was removed from the committed suite. Unimplemented reproductions remain in the original saved patch.

The complete release gate passed (`round-1-release-gate.log`): 494 tests passed, 8 ignored; 2 scrollbar tests; strict Clippy; release build; FileManager1 activation; 5 real X11 tests; desktop metadata and package smoke tests. The debug executable was rebuilt separately.

Still pending: partial-directory Undo recovery and partial Restore Retry bookkeeping. Subsequent rounds must also audit history retries after partial completion; this round does not claim all Transfer bugs are gone.

The original audit and reproduction patch below describe revision 3dab5d9, before Round 1.

## P2 — Undo cannot resume partial deletion inside a copied folder

Location: `src/journal/effects.rs:235` (whole-tree verification) and `src/journal/effects.rs:243` (recursive removal).

Reproduction: record Copy of a folder containing two child directories with data. One destination child is nonwritable when the action is recorded. Undo removes the first child and then fails on the protected child. Repair that child's permissions, reopen the journal, and retry Undo. It refuses with `contents changed after it was recorded` instead of finishing its own partial removal.

The journal tracks completed top-level items but not partial deletion within an item. The saved result fingerprint still describes the full original tree. Once recursive removal has partially mutated it, the journal's own safety check rejects that tree. The source copy remains intact, but this Undo entry blocks further history traversal. Any fix needs to retain protection against unrelated external edits while accounting for the journal's own partial work.

Test: `journal::tests::audit_undo_copy_can_resume_after_partial_directory_cleanup`.
The fixture explicitly verifies partial removal and runs as a non-root user.

## P2 — Retry of a partial Restore leaves a ghost Trash folder

Locations: `src/app/transfer_session/queue.rs:396` (Restore retry state), `src/app/transfer_session/queue.rs:113` (matching receipts for Undo), `src/app/trash.rs:369` (matching receipts for Restore completion).

Reproduction: restore a Trash folder with files `a` and `b` into an existing folder containing `a`. Choose Replace for the directory merge, then Skip on `a`; `b` restores successfully. Retry the remaining entry and choose Keep Both. The incoming `a` is restored as `a copy`, and the existing `a` remains intact. However, the empty source folder and its `.trashinfo` remain in Trash, the Restore completion counts zero restored entries, and there is no further retry.

Retry uses child source/destination mappings, while Restore bookkeeping only recognizes exact matches to the original top-level Trash source. Ancestor cleanup removed for Skip is not recreated for the retry. The child receipt is ignored by completion and Undo preparation. Retry needs to retain the originating Trash entry context and finish its bookkeeping without deleting still-retained descendants or preexisting destination files.

Test: `app::transfer_session::queue::regressions::audit_retry_partial_restore_finishes_the_trash_entry`.
This drives the real Queue, Work, conflict, Retry and `finish_restore` paths using a temporary physical Trash fixture; it does not touch desktop Trash.

## P2 — Undo/Redo breaks hardlinks preserved by the original Transfer

Location: `src/journal/effects.rs:243` and `src/journal/effects.rs:264`; the called `journal_copy` and `journal_move` wrappers create separate copy contexts per item.

Reproductions:

- Select two hardlinked files and Copy them. Their destinations share an inode. Undo, reopen the journal, and Redo: the destinations now have different inodes.
- Move two hardlinked files across filesystems. Their destinations still share an inode. Undo: the recreated source files have different inodes.

The journal executes each recorded item separately, losing the shared hardlink context now maintained by TransferBatch. This changes filesystem semantics and duplicates disk usage after history operations. The move fixture uses the ordinary temporary filesystem and `/dev/shm` and asserts different device IDs.

Tests: `audit_redo_copy_preserves_hardlinks_between_selected_files` and `audit_undo_cross_device_move_preserves_hardlinks` in `src/journal/tests.rs`.

## Reproduction and evidence

The original test files were restored after saving `reproduction-tests.patch`. On revision 3dab5d9:

```sh
git apply .scratch/transfer-undo-audit/reproduction-tests.patch
PATH=/home/powerpenguini/.cargo/bin:$PATH cargo test audit_
git apply -R .scratch/transfer-undo-audit/reproduction-tests.patch
```

Both `red.log` and `red-repeat.log` show 5 existing audit tests passing and 4 new tests failing. The patch passes `git apply --check` against the restored files. All data is isolated in temporary directories. These findings have not been fixed in this scan.

After removing the reproduction patch, `cargo test --all-targets` passed with 491 tests passing and 8 ignored (`baseline.log`). No release gate or live application restart was performed for this diagnosis-only scan.
