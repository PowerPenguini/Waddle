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
