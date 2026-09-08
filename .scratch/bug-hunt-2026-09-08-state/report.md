# Filename and saved-state bug hunt

Base: `b706665` on `main`. Date: 2026-09-08.

This pass examined Trash receipts, persistent Undo, Favorites, and session saving. Four user-visible failures were reproduced through separate red/green cycles and traced to two underlying defects. All fixtures used temporary files and directories; real user state and desktop Trash were not changed.

## Valid Unix filenames break persistent state

Unix filenames can contain bytes that are not UTF-8. The filesystem implementation already handles these paths, but Serde's default JSON encoding for `PathBuf` rejects them.

Confirmed failures:

- Creating a file with such a name prevented saving its Undo record: `could not encode operation journal: path contains invalid UTF-8 characters`.
- Adding a Favorite for such a directory failed with the same encoding error.
- Remembering such a directory silently stopped session saves, including subsequent window-geometry updates; restarting fell back to an older directory and state.

The shared path serializer keeps ordinary UTF-8 paths as JSON strings and stores other paths as an object containing their exact Unix bytes. Journal paths, Favorite paths, and the optional last-directory path use this encoding. Existing string-based records remain readable. Records containing the new byte representation require this updated build; older versions could not persist these paths.

Tests:

- `journal::tests::hunt_journal_round_trips_non_utf8_file_paths`
- `app::places::tests::hunt_favorites_preserve_non_utf8_folder_paths`
- `app::startup::tests::hunt_non_utf8_last_directory_does_not_block_session_saving`
- `journal::tests::non_utf8_copy_and_move_records_support_persistent_undo_and_redo`
- `journal::tests::non_utf8_rename_folder_and_trash_records_survive_restart`

Coverage verifies physical Undo/Redo results after reopening the journal, exact Favorite paths, last-directory restoration, and updated window dimensions. Trash coverage restores a private fixture without invoking desktop Trash.

## Failed Favorite saves leave uncommitted changes in memory

Add, Remove, and Reorder modified the in-memory list before trying to save it. A failed Add therefore reported an error but still added the item to the list; Retry then rejected it as an existing Favorite. Failed Remove and Reorder similarly left memory inconsistent with the saved file.

Favorite edits now build a candidate list, write and replace the saved file, and only then update memory. The regression deliberately blocks the temporary save path with a directory, checks that all three operations preserve the prior list and saved bytes, removes the obstruction, and verifies successful Retry and subsequent edits.

Test: `app::places::tests::hunt_failed_favorite_save_preserves_state_and_allows_retry`.

## Validation

- Four `*-red.log` files contain the failing reproductions before their respective fixes; corresponding `*-green.log` files contain passing results.
- `release-gate.log`: 447 application tests passed, with 7 performance benchmarks intentionally ignored in the normal suite.
- Two vendored scrollbar tests and five real-X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- `benchmarks.log`: all seven release-mode performance benchmarks passed their budgets.
- No new interactive UI claim is made for this pass.
