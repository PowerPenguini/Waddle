# Bug hunting, 2026-09-09

Each round uses a failing behavioral regression before its fix, followed by a
commit and push to `main`. The user approved filesystem operations, journal
Undo/Redo, and app messages as test boundaries. Existing scratch artifacts are
outside this work.

## Round 1: queued recursive search results

- Reproduction: finish a real recursive search worker, retain its output message,
  change the query, then deliver the retained message through `App::update`.
- Red: `cargo test recursive_search_ignores_a_completed_result_queued_before_the_query_changed -- --nocapture`
  failed because the old result marked the new search finished.
- Cause: worker cancellation cannot retract an output already queued for the UI;
  search completions lacked request identity.
- Fix: Search sessions assign revisions to requests and ignore obsolete results.
- Green: the same regression passed; `cargo test --all-targets --quiet` passed
  (596 passed, 19 ignored); Clippy with warnings denied and formatting passed.

## Round 2: Search session cancellation loses a multi-file selection

- Reproduction: select two nonadjacent entries, search, then press Escape through
  the app's keyboard boundary. Repeat for local and recursive searches.
- Red: `cargo test cancelling_search_restores_the_full_selection_and_active_entry -- --nocapture`
  retained only `three` instead of the original `one` and `three` selection.
- Cause: the Search session saved only the active entry and cancellation selected
  that one entry, discarding the rest of the selection.
- Fix: retain and restore the selected set, active entry, and selection anchors.
- Green: the same regression passed in both searches; all-target tests passed
  (597 passed, 19 ignored); Clippy with warnings denied and formatting passed.
  All seven release-mode performance benchmarks passed. Grid/list p95 work
  remained below 0.72 ms, within the 8 ms budget.

## Round 3: `:refresh` leaves Recent and Trash

- Reproduction: display Recent or Trash, submit `:refresh` through app messages,
  inspect the requested location, and deliver its completion.
- Red: `cargo test refresh_command_reloads_the_displayed_recent_or_trash_location -- --nocapture`
  requested `Folder` while Recent was displayed.
- Cause: command dispatch called the folder refresh directly, unlike F5's
  location-aware refresh.
- Fix: dispatch `:refresh` through the same location-aware path as F5.
- Green: the regression passed for Recent and Trash; all-target tests passed
  (598 passed, 19 ignored); Clippy with warnings denied and formatting passed.

## Round 4: Search cancellation restores wrong files after a refresh

- Reproduction: select `bravo` and `omega`, search for `delta`, insert `alpha`,
  refresh through app messages, then press Escape. Also cover removal of `bravo`.
- Red: `cargo test cancelling_search_after_refresh_restores_surviving_selected_paths -- --nocapture`
  selected `alpha` and `delta` instead of `bravo` and `omega`.
- Cause: saved row indices referred to different entries after a folder refresh.
- Fix: retain original entry paths and resolve the selection and anchors against
  the current entries when cancelling. Entries that disappeared are not selected.
- Green: insertion and removal scenarios passed; all-target tests passed
  (599 passed, 19 ignored); Clippy with warnings denied and formatting passed.
  All seven release-mode benchmarks passed; grid/list p95 remained below 0.69 ms.

## Round 5: changing view settings leaves Recent and Trash

- Reproduction: display Recent or Trash, submit `:set view=list`, and deliver any
  resulting navigation completion through app messages.
- Red: `cargo test setting_list_view_keeps_the_displayed_recent_or_trash_location -- --nocapture`
  changed the displayed location from Recent to Folder.
- Cause: applying browse settings used the same folder-only refresh mistake in
  a separate command branch.
- Fix: refresh the displayed location after applying browse settings.
- Green: the regression passed for Recent and Trash; all-target tests passed
  (600 passed, 19 ignored); Clippy with warnings denied and formatting passed.

## Round 6: shell command completion leaves Recent and Trash

- Reproduction: run a harmless `!true` command and deliver its completion through
  app messages while Recent or Trash is displayed.
- Red: `cargo test shell_command_completion_preserves_the_displayed_recent_or_trash_location -- --nocapture`
  requested Folder instead of Recent after the command completed.
- Cause: shell completion refreshed the previous folder instead of the displayed
  location, independently of the built-in refresh and settings command paths.
- Fix: use the location-aware refresh when a shell command does not navigate.
- Green: the regression passed for Recent and Trash; all-target tests passed
  (601 passed, 19 ignored); Clippy with warnings denied and formatting passed.

## Round 7: Properties omits special permission bits

- Reproduction: open `:properties` through app messages for temporary files with
  setuid, setgid, and sticky bits, with and without the corresponding execute bit.
- Red: `cargo test properties_display_special_permission_bits -- --nocapture`
  displayed `rwxr-xr-x (4755)` instead of `rwsr-xr-x (4755)`.
- Cause: symbolic permission formatting considered only read/write/execute bits.
- Fix: display `s`/`S` and `t`/`T` for the special permission bits.
- Green: all six permission scenarios passed through the real Properties worker;
  all-target tests passed (602 passed, 19 ignored); Clippy with warnings denied
  and formatting passed.

## Round 8: partial permanent deletion leaves stale entries

- Reproduction: confirm permanent deletion after a failed Trash Transfer for two
  temporary files, one of which another process removes before confirmation.
- Red: `cargo test partial_permanent_delete_refreshes_entries_and_keeps_the_error -- --nocapture`
  left both removed files displayed after the real deletion worker completed.
- Cause: any permanent-delete failure suppressed refresh, even when other entries
  or part of a directory had already been removed.
- Fix: refresh after failed permanent deletion while retaining the failure prompt.
- Green: the regression passed through app messages and real filesystem work;
  all-target tests passed (603 passed, 19 ignored); Clippy with warnings denied
  and formatting passed. The existing failure test now expects a refresh.

## Round 9: queued metadata replaces newer details for the same file

- Reproduction: retain a completed details message for a three-byte file, change
  it to eight bytes, finish a refresh, then deliver the retained message.
- Red: `cargo test queued_entry_details_cannot_overwrite_newer_details_for_the_same_path -- --nocapture`
  changed the status bar from `8 B` back to `3 B`.
- Cause: metadata completion checked only the selected path. Cancelling a worker
  cannot retract a completion that is already queued.
- Fix: assign revisions to details requests and reject superseded completions.
- Green: the regression passed with real metadata workers; all-target tests
  passed (604 passed, 19 ignored); Clippy with warnings denied and formatting passed.

## Round 10: status details identify special files as regular files

- Reproduction: read status details for a real temporary Unix socket and FIFO.
- Red: `cargo test entry_details_identify_named_pipes_and_unix_sockets -- --nocapture`
  showed `-rw-------` for the socket instead of `srw-------`.
- Cause: file-type formatting distinguished only folders and symbolic links;
  every other kind used the regular-file marker.
- Fix: derive the marker from Unix mode bits, including sockets, FIFOs, and devices.
- Green: socket and FIFO regressions passed; all-target tests passed
  (605 passed, 19 ignored); Clippy with warnings denied and formatting passed.

## Round 11: monitoring stays attached to a replaced directory

- Reproduction: consume real monitoring events through app messages, move the
  current folder away, create a replacement at the same path, then modify it.
- Red: `cargo test location_monitoring_follows_a_replaced_current_folder -- --nocapture`
  twice timed out waiting for changes to `current/after.txt`; initial monitoring
  and the refresh after replacement had succeeded.
- Cause: inotify watches follow inodes, but the registry retained moved/deleted
  watches under their original paths and skipped registering replacements.
- Fix: retire watches on move-self, delete-self, and ignored events. Close moved
  watches explicitly, then allow refresh to register the replacement directory.
- Green: the native monitoring regression passed in under one second; all-target
  tests passed (606 passed, 19 ignored); Clippy with warnings denied and formatting
  passed.

## Round 12: refreshing Recent and Trash clears selection and scrolling

- Reproduction: select two nonadjacent entries, scroll, refresh through app
  messages, and deliver entries with a new row before the selected files.
- Red: `cargo test refreshing_recent_and_trash_preserves_selection_by_path_and_scroll -- --nocapture`
  returned an empty selection instead of `bravo` and `omega`.
- Cause: refreshing special locations used their opening requests, which always
  clear selection and reset scrolling.
- Fix: Navigation session distinguishes refresh from opening for Recent/Trash;
  refresh requests carry selected paths and preserve the scroll position.
- Green: Recent and Trash regression scenarios passed; all-target tests passed
  (607 passed, 19 ignored); Clippy and formatting passed. All seven release-mode
  benchmarks passed; grid/list p95 stayed below 0.77 ms (8 ms budget).

## Round 13: unchanged rename rewrites non-UTF-8 filenames

- Reproduction: open Rename and submit without editing, for both an ordinary name
  and a filename containing the raw byte `FF`.
- Red: `cargo test submitting_an_unchanged_rename_preserves_the_original_filename -- --nocapture`
  replaced the original raw byte with the Unicode replacement character.
- Cause: the editor's display string was sent back as a filesystem rename even
  though the user had not changed it. Ordinary unchanged names also hit a collision.
- Fix: dismiss an unchanged Rename through the existing cancellation path before
  scheduling filesystem work or recording Undo.
- Green: both filename cases preserve their exact names, return to the browser,
  and leave Undo empty; all-target tests passed (608 passed, 19 ignored), along with
  Clippy and formatting.

## Round 14: delayed shell completion reverses later navigation

- Reproduction: run `:true`, retain its real completion message, navigate to
  another folder, then deliver the retained completion through app messages.
- Red: `cargo test a_shell_command_without_cd_does_not_reverse_later_navigation -- --nocapture`
  returned the browser to the command's original folder.
- Cause: completion compared the shell's final directory only with the browser's
  current folder, confusing later user navigation with a shell directory change.
- Fix: execution suppresses a navigation request when the shell stayed in its
  starting directory. Explicit directory changes continue to navigate.
- Green: the regression preserves later navigation and verifies a subsequent
  real `:cd` still works; all-target tests passed (609 passed, 19 ignored), along
  with Clippy and formatting.
