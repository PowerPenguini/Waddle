# Concurrent Favorites and Restore retry bug hunt

Base: `6c3b7e8` on `main`. Date: 2026-09-08.

This pass reproduced two defects with failing tests before fixing them. All filesystem and Trash fixtures used temporary directories; real user settings and desktop Trash were not changed.

## Concurrent windows overwrite Favorites

Two windows load the same Favorites list. Adding a Favorite in the first window and then adding another in the second overwrote the first addition because each mutation saved its cached list.

Favorite mutations now hold a stable sidecar file lock, reload the latest saved list, apply the edit, and atomically replace the JSON file. Remove and Reorder capture the displayed folder paths before reloading so another window's edits cannot redirect an old index to the wrong Favorite. A missing target produces an error. Failed writes still leave the proposed edit uncommitted and retryable.

Tests cover sequential additions from two windows, simultaneous additions from two workers, and stale Remove/Reorder indices after another window reorders or deletes a Favorite. Idle windows still display their cached list until refreshed through an edit; this change protects saved mutations rather than adding live synchronization.

- `app::places::tests::hunt_two_windows_keep_both_favorite_additions`
- `app::places::tests::stale_favorite_indices_keep_their_original_targets`
- `app::places::tests::simultaneous_windows_serialize_favorite_writes`

## Failed Restore retries an older Copy

Restore completion did not update the Transfer session's retry target. After an older failed Copy and a newer failed Restore, Retry selected the Copy. With no older failure, Restore offered no retry.

Restore completion now retains its failed source/destination mappings and Trash receipts as the retry target, or clears an older target when Restore succeeds. Retry remains a Restore operation, preserving Trash metadata cleanup and Undo. Validation failures leave Retry available. The Transfer session keeps the foreground activity guard until Restore retry completion is applied.

The regression fails Restore by omitting its destination parent, repairs the parent, and confirms Retry restores the intended file without running the older Copy. An additional test executes the actual Iced task stream, verifies activity lifetime, checks Undo preparation, and confirms source and metadata cleanup. Existing behavior excluding Restore from transfer history is preserved.

- `app::transfer_queue::tests::hunt_failed_restore_retries_itself_instead_of_an_older_copy`
- `app::transfer_session::tests::restore_retry_stays_foreground_until_completion_and_keeps_undo`

## Validation

- `favorites-red.log` and `restore-red.log` contain failing reproductions before their fixes; corresponding green logs contain passing results.
- `favorites-checks.log`: all seven Favorites tests passed, including earlier failed-save and Unix path regressions.
- `release-gate.log`: 452 application tests passed; seven performance benchmarks were intentionally ignored in the normal suite.
- Two vendored scrollbar tests and five real-X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- `benchmarks.log`: all seven release-mode performance benchmarks passed their budgets.
- No new interactive UI claim is made for this pass.
