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

## Round 15: folder symlinks use the wrong application association

- Reproduction: use a temporary desktop application registry in an isolated test
  process, then run Open With and default-app commands for a folder and a symlink
  to it named `folder.txt`.
- Red: `cargo test folder_symlinks_use_directory_applications_and_default_associations -- --nocapture`
  offered the fixture file manager for the folder but not its symlink.
- Cause: application lookup and default-app changes classified the symlink itself
  with a filename guess instead of recognizing its directory target.
- Fix: follow the link for type classification while keeping its original path
  for launching the chosen application.
- Green: both targets offer the directory application and update inode/directory
  without assigning that application to text/plain. All-target tests passed
  (610 passed, 19 ignored); Clippy and formatting passed.

## Round 16: Copy acts on a deselected file

- Reproduction: select two files through pointer messages, Ctrl-click the second
  to deselect it, and copy. Then deselect the remaining file and copy again.
- Red: `cargo test copy_respects_ctrl_click_deselection_including_an_empty_selection -- --nocapture`
  copied `deselected.txt` instead of the remaining selected file, `keep.txt`.
- Cause: the shared file-operation selection collector used the active row when
  fewer than two files were selected, even if that row was deselected.
- Fix: collect the selected set for every selection count, including zero.
- Green: Copy uses the remaining selection and leaves the clipboard untouched
  when no files are selected. All-target tests passed (611 passed, 19 ignored),
  along with Clippy and formatting. All seven release performance benchmarks
  passed; grid/list p95 work remained below 0.77 ms against the 8 ms budget.

## Round 17: queued Properties replaces newer command output

- Reproduction: retain the completion of a real Properties read, submit `:help`
  or Properties for another file, then deliver the retained result through app
  messages. Also exercise an unsuccessful read of a missing file.
- Red: `cargo test queued_properties_results_cannot_replace_newer_command_output -- --nocapture`
  replaced the help screen with Properties for `old.txt`.
- Cause: Properties completions had no identity linking them to the Command
  session output they were requested for.
- Fix: advance the output revision when output is replaced or dismissed, capture
  it on Properties requests, and ignore obsolete results and errors.
- Green: newer help and Properties output and status survive both successful
  and unsuccessful stale reads. All-target tests passed (612 passed, 19 ignored),
  along with Clippy and formatting.

## Round 18: refreshing resets the active file and selection anchors

- Reproduction: select `bravo` and `omega`, insert `alpha`, then refresh and
  Shift-click `delta`. Repeat with the active `omega` row deselected by Ctrl-click.
- Red: `cargo test folder_refresh_preserves_the_active_file_and_shift_selection_anchor -- --nocapture`
  changed the active file from `omega` to `bravo` during the refresh.
- Cause: installing refreshed entries retained the selected set but reset the
  active row and anchors to its first member.
- Fix: map the previous selection and anchors through entry paths. When a refresh
  preserves the selected set, retain its surviving active row and anchors;
  requests for a different selection still install the requested selection.
- Green: the active row and subsequent Shift-click range survive insertions and
  deselection. Recent and Trash refresh assertions also pass. All-target tests
  passed (613 passed, 19 ignored), along with Clippy and formatting. All seven
  release performance benchmarks passed; grid/list p95 remained below 0.77 ms.

## Round 19: selecting another file cancels explicit Properties

- Reproduction: submit Properties for a temporary file, retain the pending task,
  and select another file through pointer messages before running the tasks.
- Red: `cargo test selecting_another_file_does_not_cancel_an_explicit_properties_request -- --nocapture`
  showed no Properties and reported "Properties request was replaced".
- Cause: explicit Properties and passive selection details shared a cancellation
  group. Scheduling selection details cancelled the user's Properties request.
- Fix: run Properties outside the selection-details cancellation group. The
  Command session output revision continues to reject superseded results.
- Green: the original Properties target is displayed after selection changes;
  stale-result coverage also passes. All-target tests passed (614 passed,
  19 ignored), along with Clippy and formatting.

## Round 20: Properties shows text applications for a folder symlink

- Reproduction: extend the isolated desktop-registry regression to read Properties
  after setting the directory default application for a folder and its symlink
  named `folder.txt`.
- Red: `cargo test folder_symlinks_use_directory_applications_and_default_associations -- --nocapture`
  reported text/plain and a text editor for the symlink, disagreeing with Open With.
- Cause: Properties guessed the content type from the link name without checking
  whether its target was a directory.
- Fix: recognize directory targets for MIME and application lookup while retaining
  the symlink's own metadata for the rest of Properties.
- Green: both targets show inode/directory and the configured folder application;
  the link still reports its symbolic-link type and own size. All-target tests
  passed (614 passed, 19 ignored), along with Clippy and formatting.

## Round 21: refreshing a large selection freezes the app

- Reproduction: deliver a refreshed 10,000-entry folder through app messages with
  every entry selected, timing only the app-thread completion work.
- Red: `cargo test --release benchmark_large_selection_refresh_work -- --ignored --nocapture`
  took 13.66 seconds, exceeding the new 100 ms completion budget.
- Cause: selection membership and the path remapping added in round 18 each
  repeatedly searched the full entry list, producing quadratic work.
- Fix: index selected paths and refreshed entry positions with hash collections,
  keeping selected rows in display order and preserving first-match behavior.
- Green: the same completion took 15–16 ms and retained all 10,000 selections.
  All-target tests passed (614 passed, 20 ignored), Clippy and formatting passed,
  and all eight release performance benchmarks passed. The added ignored test
  runs automatically in the existing performance benchmark script.

## Round 22: a large pending Cut freezes selection and refresh

- Reproduction: press `x` with 10,000 entries selected, then deliver a refreshed
  listing through app messages while those entries remain pending Cut.
- Red: `cargo test --release benchmark_large_cut_and_refresh_work -- --ignored --nocapture`
  measured 6.46 seconds for Cut and 6.38 seconds for refresh, exceeding the
  100 ms app-thread budget for each operation.
- Cause: both hiding Cut entries immediately and filtering a refreshed listing
  compared every entry against the entire pending Cut path list.
- Fix: share indexed path filtering between the initial Cut and refresh paths.
- Green: Cut took 12–15 ms and refresh 7–8 ms, retaining all 10,000 pending paths
  and keeping them hidden. All-target tests passed (614 passed, 21 ignored),
  along with Clippy and formatting. All nine release performance benchmarks
  passed, including the new Cut and refresh regression.

## Round 23: a removal notification batch freezes pending Cut reconciliation

- Reproduction: Cut 10,000 real temporary files, remove them, recreate one path,
  and deliver the batched removal notification through app messages.
- Red: `cargo test --release benchmark_large_cut_removal_batch_work -- --ignored --nocapture`
  blocked the app thread for 6.09 seconds against a 100 ms budget.
- Cause: each pending Cut path searched the full list of reported removals.
- Fix: index reported paths before reconciling the clipboard, retaining the
  filesystem check that protects paths recreated since the notification.
- Green: reconciliation took 23–25 ms and kept the recreated file pending Cut.
  All-target tests passed (614 passed, 22 ignored), along with Clippy and
  formatting. All ten release performance benchmarks passed.

## Round 24: refining Search after refresh uses the wrong starting file

- Reproduction: start Search from `delta.txt`, match `omega.txt`, insert
  `alpha.txt`, refresh, and refine the query through app messages.
- Red: `cargo test refining_search_after_refresh_keeps_its_starting_file_by_path -- --nocapture`
  jumped back to `delta.txt` instead of continuing to match `omega.txt`.
- Cause: the Search session retained its original row number, which referred to
  a different file after refreshed entries were inserted before it.
- Fix: retain the starting file's path and resolve its current row on query edits.
- Green: query refinement stays anchored to the original starting file after
  refresh. All-target tests passed (615 passed, 22 ignored), along with Clippy
  and formatting.

## Round 25: queued shell output replaces a newer command presentation

- Reproduction: run a real shell command that creates a file, retain its completed
  result, open `:help`, and then deliver the retained result through app messages.
  Cover both printed output and an unsuccessful command with no output.
- Red: `cargo test queued_shell_results_preserve_newer_help_while_refreshing_changed_files -- --nocapture`
  replaced help with the older shell command's output.
- Cause: shell completions lacked the output revision already used by Properties.
- Fix: tag command completions with their output revision. Resolve obsolete
  completions without changing the current presentation, while retaining their
  filesystem refresh and navigation effects and existing diagnostic recording.
- Green: help survives both cases and the created file appears after refresh.
  All-target tests passed (616 passed, 22 ignored), along with Clippy and formatting.

## Round 26: continuous writes starve filesystem notifications

- Reproduction: confirm the app's native watch is installed, then write a file
  every 10 ms while awaiting an event through the monitoring subscription.
- Red: `cargo test location_monitoring_refreshes_while_a_file_is_continuously_written -- --nocapture`
  received no event during the three-second window of sustained writes.
- Cause: every write reset the trailing debounce timestamp, so the monitor waited
  indefinitely for a quiet period before notifying the browser.
- Fix: retain the first change time and flush batches once they reach 500 ms,
  while keeping the existing 120 ms quiet-period debounce for short bursts.
- Green: an event arrives while writes continue and the app refresh displays the
  busy file. Existing burst-debounce coverage also passes. All-target tests passed
  (617 passed, 22 ignored), along with Clippy and formatting.

## Round 27: failed native monitor startup disables live refresh

- Reproduction: use an isolated subprocess with an OS-boundary fault shim that
  makes inotify initialization return EMFILE. Create files and deliver periodic
  polling messages, repeating after the first refresh.
- Red: `cargo test failed_native_monitor_startup_keeps_polling_after_each_refresh -- --nocapture`
  failed to display `first.txt` after native monitoring initialization failed.
- Cause: inotify initialization happened after successful source construction;
  its worker silently exited on failure. The app also disabled fallback polling
  when no native monitor existed.
- Fix: initialize the descriptor before reporting a successful source, transfer
  its ownership safely to the worker, and poll the location and expanded folders
  when monitoring is unavailable.
- Green: both successive external file creations appear through polling.
  All-target tests passed (618 passed, 22 ignored), along with Clippy and formatting.

## Round 28: context-menu Rename targets a different file after refresh

- Reproduction: open the context menu on `delta.txt`, insert an earlier-sorting
  file, refresh, and perform Rename through app messages. Also remove the menu's
  target before refresh and verify that Rename does nothing.
- Red: `cargo test context_rename_keeps_its_file_target_across_refresh -- --nocapture`
  renamed `bravo.txt` instead of `delta.txt`.
- Cause: the context menu retained an obsolete row number even though the grid's
  active selection was already restored by path.
- Fix: remap the context target by path when refreshed entries are installed;
  close it when the target disappears or navigation replaces the interaction.
- Green: Rename affects only the original target after insertion, and a removed
  target cannot cause another file to be renamed. All-target tests passed
  (619 passed, 22 ignored), along with Clippy and formatting. All ten release
  performance benchmarks passed; grid/list p95 remained below 0.78 ms.

## Round 29: queue overflow leaves displayed entries stale

- Reproduction: an isolated OS-boundary fixture replaces a new file's native
  notifications with IN_Q_OVERFLOW, then the app consumes the monitoring stream.
- Red: `cargo test native_queue_overflow_rescans_the_displayed_folder -- --nocapture`
  confirmed injection but did not discover the new file within three seconds.
- Cause: overflow records use watch descriptor -1, so the directory lookup ignored
  them. This contract is documented in [inotify(7)](https://man7.org/linux/man-pages/man7/inotify.7.html).
- Fix: mark every watched directory for rescan when overflow is reported, keeping
  the existing notification batching and refresh behavior.
- Green: the browser discovers the file without receiving its individual native
  notifications. All-target tests passed (620 passed, 22 ignored), along with
  Clippy and formatting.

## Round 30: overflow leaves monitoring attached to a replaced folder

- Reproduction: extend the native overflow fixture to hide a folder's move event,
  replace that folder, await its rescan, then create another file in the replacement.
- Red: `cargo test native_queue_overflow_rescans_the_displayed_folder -- --nocapture`
  displayed the replacement's initial contents but missed `future.txt` afterward.
- Cause: rescanning repaired the displayed entries, but the native watch remained
  attached to the moved folder's inode because its invalidation event was lost.
- Fix: remove and re-register watched paths when overflow is reported, preserving
  their pending rescans. Registration failures use the existing polling fallback.
- Green: both the replacement's contents and subsequent file creation appear.
  All-target tests passed (620 passed, 22 ignored), along with Clippy, formatting,
  and whitespace checks.

## Round 31: large Trash selections freeze Delete confirmation

- Reproduction: load 10,000 Trash entries, select all through the keyboard,
  deselect one through a Ctrl-click, then measure opening Delete confirmation.
- Red: `cargo test --release benchmark_large_trash_selection_opens_delete_confirmation_promptly -- --ignored --nocapture`
  took 5.94 seconds on the UI thread, exceeding the 250 ms budget.
- Cause: collecting selected Trash receipts linearly searched the entire selected
  path list for every Trash entry, producing quadratic work.
- Fix: use a path set for selection membership while retaining Trash entry order.
- Green: the same confirmation selected 9,999 items and opened in 12.6 ms
  (9.6 ms in the full benchmark run). All-target tests passed (620 passed,
  23 ignored), all eleven release benchmarks passed, and Clippy, formatting,
  and whitespace checks passed.

## Round 32: partial Undo leaves deleted entries visible (2026-09-11)

- Reproduction: record two real file copies in the journal, make one destination
  parent read-only, then invoke Undo through the keyboard. Undo removes the copy
  in the displayed folder before failing to remove the other copy.
- Red: `cargo test partial_undo_refreshes_removed_entries_and_retains_the_failure -- --nocapture`
  confirmed the partial filesystem change but still displayed the removed file.
- Cause: the app's journal completion handler returned no refresh on failure,
  even though Undo/Redo can have partial effects.
- Fix: refresh the displayed location and expanded Sidebar tree folders after
  journal failures. Present the error as a notice so refresh status cannot hide it.
- Green: the removed file disappears from the browser while the permission error
  stays visible and the blocked copy remains intact. All-target tests passed
  (621 passed, 23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 33: queued Undo completion replaces Recent or Trash (2026-09-11)

- Reproduction: run a real New File Undo, retain its completion message, and
  switch the displayed location to Recent or Trash before delivering that message.
- Red: `cargo test queued_undo_completion_preserves_a_newer_recent_or_trash_location -- --nocapture`
  found that the completion requested a Folder refresh instead of Recent.
- Cause: successful journal completion always refreshed the current filesystem
  folder, even when a newer navigation had switched to a virtual location.
- Fix: refresh the displayed Recent or Trash location when appropriate; retain
  the existing selection behavior when a folder is displayed.
- Green: both Recent and Trash remain displayed through the queued completion
  and subsequent refresh. All-target tests passed (622 passed, 23 ignored),
  along with Clippy, formatting, and whitespace checks.

## Round 34: queued Undo cancels a newer pending navigation (2026-09-11)

- Reproduction: complete a real New File Undo but retain its UI result, submit
  another folder through Location, then deliver the Undo result before that
  folder's navigation task finishes.
- Red: `cargo test queued_undo_completion_does_not_cancel_a_newer_folder_navigation -- --nocapture`
  stayed in the original folder instead of opening the requested destination.
- Cause: the journal refresh replaced the pending Navigation session request and
  cancelled its work, even though the user's navigation was newer than Undo.
- Fix: defer the journal refresh while a navigation request is pending, using
  the existing Navigation session refresh coalescing behavior.
- Green: the requested folder opens and displays its destination file. All-target
  tests passed (623 passed, 23 ignored), along with Clippy, formatting, and
  whitespace checks.
