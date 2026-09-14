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

## Round 35: queued Rename cancels a newer pending navigation (2026-09-11)

- Reproduction: complete an inline Rename's real filesystem work, retain its UI
  result, submit another folder through Location, then deliver the Rename result
  before the new navigation task finishes.
- Red: `cargo test queued_rename_completion_does_not_cancel_a_newer_folder_navigation -- --nocapture`
  renamed the file successfully but stayed in the original folder.
- Cause: File operation session completion replaced the newer navigation with
  its own refresh. Rename permits this navigation while its result is queued.
- Fix: defer the file-list refresh while navigation is pending, retaining the
  Sidebar tree refresh and completed operation's journal record.
- Green: the new folder opens and displays its file; Undo restores the original
  filename and contents. All-target tests passed (624 passed, 23 ignored), along
  with Clippy, formatting, and whitespace checks.

## Round 36: old Rename results overwrite a newer editor (2026-09-11)

- Reproduction: complete a Rename's filesystem work but retain its result, open
  Rename for another file from the context menu, type a new name, then deliver the
  older result. Exercise both successful and failed first operations.
- Red: `cargo test queued_rename_results_do_not_replace_a_newer_rename_editor -- --nocapture`
  showed that the older successful result closed the newer Rename editor.
- Cause: file-operation completions had no interaction identity, so an old result
  updated the current session and could close its editor or assign it an old error.
- Fix: carry the originating interaction revision through work and completion.
  Superseded completions retain filesystem refresh and journal effects while
  preserving the current editor, selection, error, and busy state.
- Green: both cases preserve the new input without an old error; submitting it
  renames the second file, and both successful operations remain undoable.
  All-target tests passed (625 passed, 23 ignored), along with Clippy, formatting,
  and whitespace checks.

## Round 37: old permission failures overwrite newer command output (2026-09-11)

- Reproduction: run a real permission change, retain its result, open Help, and
  then deliver the result. Include a missing second target to cause partial failure.
- Red: `cargo test queued_permission_results_preserve_newer_command_output -- --nocapture`
  replaced Help with the older "File action failed" report.
- Cause: permission and Open With completions shared an unversioned metadata
  result message, so they could replace output from a newer Command session.
- Fix: tag these results with their originating command output revision and only
  present matching results. Keep file-detail refreshes after completed operations.
- Green: Help survives successful and partially failed permission changes; the
  requested permissions are applied and file contents remain intact. All-target
  tests passed (626 passed, 23 ignored), along with Clippy, formatting, and
  whitespace checks.

## Round 38: permission success feedback vanishes during refresh (2026-09-11)

- Reproduction: change two files' permissions through `:chmod`, consume completion
  and detail-refresh tasks, and inspect the browser's visible status.
- Red: `cargo test permission_success_feedback_survives_details_refresh_until_next_input -- --nocapture`
  showed only the selected file's details instead of the operation confirmation.
- Cause: the completion set a status message, then immediately scheduled details,
  whose status update replaced that message before the next frame.
- Fix: use the existing neutral status notice for successful metadata actions so
  details can refresh without erasing the confirmation.
- Green: the success message survives refreshed details; the next click dismisses
  it and reveals the new permissions. Both files have the requested mode.
  All-target tests passed (627 passed, 23 ignored), along with Clippy, formatting,
  and whitespace checks.

## Round 39: automatic refresh hides a Properties failure (2026-09-11)

- Reproduction: request Properties for a file, remove it before inspection, then
  deliver its directory-removal notification after the inspection fails.
- Red: `cargo test properties_failure_survives_an_automatic_directory_refresh -- --nocapture`
  replaced the Properties error with the surviving file's ordinary details.
- Cause: Properties failures used transient status text, which routine folder and
  selection-detail refreshes overwrite.
- Fix: retain the error in the existing failure notice, dismissed by the next input.
- Green: the removed entry disappears while its inspection failure remains visible;
  the next click reveals the surviving file's refreshed details. All-target tests
  passed (628 passed, 23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 40: cancelling recursive search restores stale files (2026-09-11)

- Reproduction: start recursive search, delete one file and create another, deliver
  the directory notification, and let search update. Then press Escape.
- Red: `cargo test cancelling_recursive_search_refreshes_changes_made_during_the_search -- --nocapture`
  correctly updated search results, but Escape restored the deleted file and hid
  the newly created one.
- Cause: cancellation restored the original folder snapshot and only refreshed
  selected-file details; directory changes consumed during search were lost.
- Fix: refresh the restored location after recursive cancellation, preserving the
  restored selection through the existing location refresh behavior.
- Green: Escape displays the current files and retains the originally selected
  surviving file. All-target tests passed (629 passed, 23 ignored), along with
  Clippy, formatting, and whitespace checks.

## Round 41: submitting a recursive search without matches restores stale files (2026-09-11)

- Reproduction: finish a recursive search with no matches, replace a file while
  search remains open, deliver its directory notification, then submit with Enter.
- Red: `cargo test submitting_an_empty_recursive_search_restores_current_folder_contents -- --nocapture`
  closed search but restored the deleted file instead of the newly created one.
- Cause: submitting without a selected recursive match restored the saved folder
  snapshot and returned without refreshing it.
- Fix: refresh the restored location when recursive submission has no entry to open.
- Green: Enter closes search and displays the current file. All-target tests
  passed (630 passed, 23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 42: opening a recursive search match restores stale files (2026-09-11)

- Reproduction: find a file recursively, replace another file in the original
  folder, consume its directory notification in search, then open the match.
- Red: `cargo test opening_a_recursive_search_match_restores_current_folder_contents -- --nocapture`
  opened the correct file but restored the deleted file and hid the new one.
  A subprocess with isolated desktop associations records the real application
  launch without changing the user's defaults or launching their editor.
- Cause: submission restored the search snapshot and launched the selected file
  without refreshing the original location.
- Fix: batch opening the file with refreshing the restored location.
- Green: the default application receives the matched file, search closes, and
  the browser displays the current folder contents. All-target tests passed
  (631 passed, 23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 43: recursive search refresh resets selected matches (2026-09-11)

- Reproduction: recursively search for three files, select two matches with
  clicks, add an earlier-sorting match, and deliver its directory notification.
- Red: `cargo test recursive_search_refresh_preserves_selected_matches_by_path -- --nocapture`
  replaced the two selected matches with the newly added first result.
- Cause: rerunning an unchanged recursive query cleared selection and always
  selected the first result, just as when editing the query.
- Fix: retain selection by path for an unchanged query, including the active file
  and selection anchors. Preserve that snapshot while a refresh is pending and
  discard it when the query changes; remap surviving paths on completion.
- Green: the selected matches and active file survive insertion ahead of them,
  and a subsequent Shift-click uses the original anchor. All-target tests passed
  (632 passed, 23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 44: recursive search menus offer actions that do nothing (2026-09-11)

- Reproduction: run recursive search and right-click a result. The menu offers
  New Folder, New Empty File, Rename, and Move to Trash, but their handlers reject
  mutations during recursive search.
- Red: `cargo test recursive_search_menus_offer_only_available_actions -- --nocapture`
  returned all six folder actions instead of the two supported result actions.
- Cause: menu construction considered the displayed location but not the active
  recursive search, unlike the mutation guard.
- Fix: offer Properties and Open With for recursive results and no background
  mutation actions.
- Green: the menu contains the supported actions and selecting Properties opens
  real file properties. All-target tests passed (633 passed, 23 ignored), along
  with Clippy, formatting, and whitespace checks.

## Round 45: Open With hides an active recursive search after cancellation (2026-09-11)

- Reproduction: open Open With from a recursive-search result, then press Escape.
  The chooser closes, but the search stays active with its input hidden in Browser
  mode, preventing the usual Escape exit from search.
- Red: `cargo test cancelling_open_with_restores_the_recursive_search_input -- --nocapture`
  returned Browser mode instead of restoring the Search input.
- Cause: Open With replaced the underlying browser mode, even though transient
  presentation already resolves the chooser as an overlay.
- Fix: retain the underlying mode when opening or submitting the chooser, and
  do not clear that mode when Escape dismisses the Open With overlay.
- Green: the first Escape restores the query and search results; the second exits
  search and restores the full folder. Existing Open With checks still pass.
  All-target tests passed (634 passed, 23 ignored), along with Clippy, formatting,
  and whitespace checks.

## Round 46: Recent menus offer unsupported mutation actions (2026-09-11)

- Reproduction: open Recent and right-click a file. New Folder, New Empty File,
  Rename, and Move to Trash appear, but their handlers reject the Recent view.
- Red: `cargo test recent_entry_menu_offers_only_available_actions -- --nocapture`
  returned the six folder actions instead of Properties and Open With.
- Cause: entry menus treated Recent as an ordinary folder while the mutation
  guard requires an actual folder to be displayed.
- Fix: use the existing menu for views without mutation actions in Recent too.
- Green: the menu contains supported actions, Properties inspects the real file,
  and Recent stays open. All-target tests passed (635 passed, 23 ignored), along
  with Clippy, formatting, and whitespace checks.

## Round 47: Trash refresh erases permanent-deletion confirmation (2026-09-11)

- Reproduction: confirm Empty Trash, let the deletion worker finish, and consume
  its resulting Trash refresh.
- Red: `cargo test empty_trash_confirmation_survives_the_resulting_refresh -- --nocapture`
  deleted the file and its metadata but replaced the success result with the
  ordinary empty-Trash status.
- Cause: file-operation completion wrote the deletion result into transient
  status text, which navigation completion immediately overwrites.
- Fix: retain the completion status as a notice, using the existing next-input
  dismissal behavior.
- Green: real deletion and refresh finish while the confirmation remains visible;
  the next click dismisses it. The test uses an isolated physical Trash fixture.
  All-target tests passed (636 passed, 23 ignored), along with Clippy, formatting,
  and whitespace checks.

## Round 48: already-removed Trash metadata causes a false deletion failure (2026-09-11)

- Reproduction: open permanent-delete confirmation for a Trash item, remove its
  metadata externally, then confirm deletion while the item itself still exists.
- Red: `cargo test trash_deletion_succeeds_when_metadata_disappears_after_confirmation_opens -- --nocapture`
  deleted the item but reported zero deleted and one failed.
- Cause: metadata cleanup treated NotFound as a failure even though both the item
  and its metadata were gone after the operation.
- Fix: count already-absent metadata as successful cleanup after deleting the item;
  other metadata errors retain their existing failure handling.
- Green: the real item is deleted, the status reports one success, no error report
  opens, and Trash refreshes to empty. All-target tests passed (637 passed,
  23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 49: an externally deleted Trash item leaves orphaned metadata (2026-09-11)

- Reproduction: open permanent-delete confirmation, remove the item externally
  while leaving its metadata, then confirm.
- Red: `cargo test trash_deletion_cleans_metadata_when_the_item_disappears_before_confirmation -- --nocapture`
  left the item's metadata behind after the failed attempt to inspect it.
- Cause: item-deletion errors stopped cleanup even when the item was already gone.
- Fix: after an item-deletion error, check the item with symlink metadata. Continue
  metadata cleanup only if absence is confirmed; retain errors for existing or
  inaccessible items.
- Green: metadata is removed, deletion reports success without an error report,
  and Trash is empty. All-target tests passed (638 passed, 23 ignored), along with
  Clippy, formatting, and whitespace checks.

## Round 50: queued Restore completion reopens Trash after navigation (2026-09-11)

- Reproduction: complete a real Restore worker while retaining its result, use
  the Back button to return from Trash to the previous folder, then deliver the
  queued result through the app.
- Red: `cargo test queued_restore_completion_does_not_reopen_trash_after_back_navigation -- --nocapture`
  reopened Trash even though Back had already finished displaying the folder.
- Cause: Restore completion requested opening Trash instead of refreshing the
  location currently displayed.
- Fix: route Restore's refresh through the existing current-location refresh.
- Green: the restored file remains visible in the folder chosen with Back, and
  Restore removes its Trash metadata. All-target tests passed (639 passed,
  23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 51: queued Copy completion cancels newer Parent navigation (2026-09-11)

- Reproduction: finish a real Copy worker while retaining its completion, start
  Parent navigation, then deliver the queued Copy result before navigation settles.
- Red: `cargo test queued_copy_completion_preserves_pending_parent_navigation -- --nocapture`
  cancelled Parent navigation and left the browser in the Copy destination.
- Cause: Transfer completion started a folder refresh without checking whether
  newer navigation was pending.
- Fix: use the existing deferred-refresh behavior before requesting the Transfer's
  entry refresh.
- Green: Parent navigation completes, the parent lists the destination, and both
  source and copied file remain intact. All-target tests passed (640 passed,
  23 ignored), along with Clippy, formatting, and whitespace checks.

## Round 52: Copy completion leaves a newly opened Trash view (2026-09-11)

- Reproduction: complete a real Copy worker while retaining its result, open Trash,
  then deliver the queued completion.
- Red: `cargo test queued_copy_completion_preserves_the_newly_opened_trash_view -- --nocapture`
  replaced the already-open Trash view with the previous folder.
- Cause: Transfer entry refreshes always requested an ordinary folder listing,
  even when the displayed location was Recent or Trash.
- Fix: refresh the displayed virtual location when no folder is displayed, while
  retaining normal folder selection and pending-navigation behavior.
- Green: Trash stays open and empty while the copied file and source remain
  intact. All-target tests passed (641 passed, 23 ignored), along with Clippy,
  formatting, and whitespace checks.

## Round 53: virtual-location refresh dismisses a partial Copy failure (2026-09-11)

- Reproduction: prepare a two-file Copy, revoke access to one source after
  preflight, let the other file copy, then open Trash before delivering the result.
- Red: `cargo test partial_copy_failure_remains_visible_when_trash_refreshes -- --nocapture`
  lost the partial-copy error during the resulting Trash refresh.
- Cause: refreshing a virtual location called the action-blocking policy, which
  dismisses nonbusy prompts, including the failure just presented.
- Fix: allow the listing to refresh behind the prompt without dismissing it;
  active foreground work and pending navigation still suppress this refresh.
- Green: the error identifies the unreadable source, Trash remains open and
  refreshed, and the failed source remains intact. The permissions regression ran
  as a non-root user. All-target tests passed (642 passed, 23 ignored), along with
  Clippy, formatting, and whitespace checks.

## Round 54: Restore result disappears during the resulting refresh (2026-09-14)

- Reproduction: restore a real file through `ContextRestore`, drain the app's
  completion and refresh tasks, and inspect the browser status.
- Red: `cargo test restore_confirmation_survives_the_resulting_trash_refresh -- --nocapture`
  showed `0 items  •  Trash` instead of the Restore counts, despite restoring the
  file and removing its Trash metadata successfully.
- Cause: Transfer completion stored the counts in the temporary status, which
  navigation overwrote during refresh.
- Fix: retain completion status as a neutral notice until user interaction.
  Existing danger notices, including Undo failures, retain precedence.
- Green: the regression passed, including dismissal on the next click. Clippy,
  formatting, and whitespace checks passed. The full suite had 651 passes,
  24 ignored tests, and one separate failure in the unchanged `:help` width
  check. That failure also reproduces in isolation and is the next round.

## Round 55: Open With help exceeds the narrow output panel (2026-09-14)

- Reproduction: the existing `help_is_interpreted_inside_the_session` regression
  failed both in the full suite and when run alone.
- Red: `cargo test help_is_interpreted_inside_the_session -- --nocapture`
  failed its 64-character help-line limit.
- Cause: release 0.0.16 expanded the Open With description onto one overlong
  line. The help-section separators and width limit were unchanged.
- Fix: put the accepted application identifiers on an indented continuation
  line, retaining the command syntax and all supported argument forms.
- Green: the existing regression passed. All-target tests passed with 652 tests
  and 24 opt-in tests ignored. Clippy, formatting, and whitespace checks passed.
  This also completes the full-suite validation of round 54.

## Round 56: queued Copy completion replaces recursive-search results (2026-09-14)

- Reproduction: finish a real Copy worker but retain its completion message,
  start recursive search, add another matching file, then deliver the completion.
- Red: `cargo test queued_copy_completion_refreshes_the_active_recursive_search -- --nocapture`
  replaced the nested matches with the root folder's directory and nonmatching
  copied file.
- Cause: the Transfer entry refresh bypassed the active recursive Search session
  and requested an ordinary folder listing.
- Fix: route completion through the existing current-location refresh when a
  recursive Search session is active, preserving pending-navigation precedence.
- Green: the same regression retains the original nested match and discovers
  the new one while preserving the copied file and source. All-target tests
  passed with 653 tests and 24 opt-in tests ignored. Clippy, formatting, and
  whitespace checks passed.

## Round 57: queued Undo completion replaces recursive-search results (2026-09-14)

- Reproduction: undo a real file creation, retain the completion message, start
  recursive search, add another matching file, then deliver Undo's completion.
- Red: `cargo test queued_undo_completion_refreshes_the_active_recursive_search -- --nocapture`
  replaced the nested matches with their parent directory from the root listing.
- Cause: successful journal completion requested an ordinary folder refresh,
  bypassing the recursive Search session started after the worker finished.
- Fix: use the existing current-location refresh for recursive search in the
  shared Undo/Redo completion handler. Pending navigation retains precedence.
- Green: the regression retains the original match and discovers the new one;
  the undone file remains absent. All-target tests passed with 654 tests and
  24 opt-in tests ignored. Clippy, formatting, and whitespace checks passed.

## Round 58: stale Trash confirmation deletes a replacement item (2026-09-14)

- Reproduction: open permanent-delete confirmation for a real Trash item, move
  that item elsewhere, replace its Trash path and metadata, then confirm.
- Red: `cargo test trash_delete_confirmation_does_not_delete_a_replacement_item -- --nocapture`
  deleted the replacement item through the confirmation for the original item.
- Cause: deletion retained a path but no identity for the listed Trash item.
- Fix: retain the device and inode from the metadata already read during Trash
  listing. Before deletion, reject a different or unidentifiable existing item
  and preserve its metadata. Existing missing-item cleanup still works.
- Green: the regression preserves the replacement file, its metadata, and the
  recovered original, and reports one failed deletion. All-target tests passed
  with 655 tests and 24 opt-in tests ignored. Clippy, formatting, and whitespace
  checks passed. All eleven performance benchmarks passed; opening confirmation
  for 9,999 selected Trash items took 9.3 ms.

## Round 59: queued Restore moves a replacement Trash item (2026-09-14)

- Reproduction: queue Restore for a listed Trash item, replace its file and
  metadata before the worker starts, then process the queued work.
- Red: `cargo test queued_restore_does_not_move_a_replacement_trash_item -- --nocapture`
  moved the replacement to the original item's old location and removed the
  replacement's Trash metadata.
- Cause: Restore mapped source and destination paths into a Transfer batch
  without checking the identity retained by the Trash listing.
- Fix: the Restore worker checks each source against the listed Trash root's
  device and inode before execution, including resumed work. Failures use the
  existing per-source reporting; Skip remains available. Ordinary Transfers
  retain their existing execution path.
- Green: the replacement, its metadata, and the recovered original remain
  intact, and Restore reports one failure. Updated physical Restore fixtures
  supply their real identities. All-target tests passed with 656 tests and
  24 opt-in tests ignored, including conflict, retry, and partial Restore tests.
  Clippy, formatting, whitespace checks, and all eleven benchmarks passed.

## Round 60: Undo and Redo results disappear during refresh (2026-09-14)

- Reproduction: undo and redo a real New File action through keyboard input,
  drain each operation's refresh tasks, and inspect the browser status.
- Red: `cargo test undo_and_redo_results_survive_the_resulting_folder_refresh -- --nocapture`
  displayed the folder's item count instead of `Undid New File` after Undo.
- Cause: journal completion stored its result in the temporary status, which
  the ensuing navigation refresh overwrote.
- Fix: retain the result as a neutral notice until user interaction. Existing
  danger notices retain precedence.
- Green: the regression verifies both Undo and Redo, the file and listing
  changes, and dismissal on the next click. All-target tests passed with
  657 tests and 24 opt-in tests ignored. Clippy, formatting, and whitespace
  checks passed.

## Round 61: queued Rename completion replaces a newer recursive search (2026-09-14)

- Reproduction: finish a real Rename worker while retaining its result, navigate
  to Parent, start recursive search, add a new match, then deliver Rename's result.
- Red: `cargo test queued_rename_completion_refreshes_a_newer_recursive_search -- --nocapture`
  replaced the nested matches with the parent folder's directory listing.
  After an interrupted build caused a linker error, rebuilding Waddle's own
  development artifacts allowed the behavioral failure to reproduce.
- Cause: file-operation completion requested an ordinary folder refresh without
  checking the newer recursive Search session.
- Fix: refresh the active recursive search while retaining pending-navigation
  precedence and Sidebar tree invalidation.
- Green: the regression retains the original match, discovers the new match,
  stays at Parent, and verifies Undo restores the renamed file. All-target tests
  passed with 658 tests and 24 opt-in tests ignored. Clippy, formatting, and
  whitespace checks passed.

## Round 62: New File Undo deletes a replacement with matching metadata (2026-09-14)

- Reproduction: record New File, restart the journal, replace the file while
  retaining its size and modification time, then Undo. Repeat after Redo.
- Red: `cargo test new_file_undo_preserves_replacements_with_matching_metadata_after_restart -- --nocapture`
  removed the replacement file.
- Cause: New File history retained a metadata fingerprint but no device/inode
  identity, so another file with matching metadata passed verification.
- Fix: save identity with new New File records, verify it before Undo, and
  update it when Redo creates the file again. Older records retain their prior
  metadata checks and acquire identity on Redo.
- Green: replacements and retained originals survive Undo, including after
  restart and Redo. A compatibility test verifies older records still load and
  support Undo/Redo. All-target tests passed with 660 tests and 24 opt-in tests
  ignored. Clippy, formatting, and whitespace checks passed.

## Round 63: Rename history moves a replacement with matching metadata (2026-09-14)

- Reproduction: record Rename, restart the journal, replace its source while
  retaining size and modification time, then Undo or Redo.
- Red: `cargo test rename_history_preserves_replacement_files_with_matching_metadata_after_restart -- --nocapture`
  moved the replacement file.
- Cause: Rename records checked metadata without checking device/inode identity.
- Fix: persist and verify identity for Rename, retaining compatibility with
  older records. When another journal operation recreates a missing path,
  update dependent Rename identities in its checkpoints. This preserves Copy
  Redo followed by Rename Redo without accepting external replacements.
- Green: regressions verify Undo and Redo after restart, older records, and
  Copy Redo with a deliberately different inode. External replacement after
  Copy Redo remains refused. The existing partial-Redo recovery test also passes.
  All-target tests passed with 663 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 64: Rename Redo rejects an item recreated by New File or New Folder (2026-09-14)

- Reproduction: create an item, record its creation, rename it, then Undo both
  operations. Redo creation, restart the journal, and Redo Rename.
- Red: `cargo test new_item_then_rename_can_be_redone_after_restart -- --nocapture`
  refused Rename because the recreated item had different metadata. The test
  fixes the original modification time to make the difference deterministic.
- Cause: creation refreshed its own fingerprint, but the dependent Rename
  retained the original fingerprint. Identity rebinding required that old
  fingerprint to match, so it could not repair this sequence.
- Fix: refresh dependent Rename metadata as well as identity after successful
  New File or New Folder Redo. Only paths missing before that journal operation
  and actually recreated by it qualify.
- Green: the regression passes for both files and folders across restart and
  a further Undo cycle. All-target tests passed with 664 tests and 24 opt-in
  tests ignored. Clippy, formatting, and whitespace checks passed.
- Follow-up: the separate copied-folder/child-Rename refusal is recorded in
  `.scratch/journal-folder-undo/issues/01-copy-undo-after-child-rename.md`.

## Round 65: Copy Undo rejects a folder after child Rename Undo (2026-09-14)

- Reproduction: Copy a folder with a nested file, rename the copied child, Undo
  Rename, restart the journal, and Undo Copy.
- Red: `cargo test copy_folder_then_rename_child_supports_undo_and_redo_after_restart -- --nocapture`
  refused Copy Undo because the copied tree appeared changed.
- Cause: tree fingerprints included directory timestamps and storage size,
  which child operations can change even after Undo restores every entry.
- Fix: persist a second digest that excludes those directory fields. New
  records verify this digest and attributes; older records retain their
  original checks. Both digests share the same filesystem traversal.
- Green: the regression covers restart, both Redo operations, and another Undo
  cycle. Separate tests verify that same-size content changes with retained
  timestamps, file timestamp edits, and directory permissions still block
  Copy Undo. Legacy folder records retain Undo/Redo and metadata checks.
  All-target tests passed with 667 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 66: folder Rename history rejects undone child changes (2026-09-14)

- Reproduction: Rename a folder, create a child, Undo New File, restart the
  journal, then Undo Rename.
- Red: `cargo test folder_rename_history_survives_an_undone_child_creation -- --nocapture`
  rejected the same folder because its metadata changed during child creation
  and removal.
- Cause: Rename required its original directory timestamp and storage size
  even when the saved device/inode identity still matched.
- Fix: verify identity for folder Rename while retaining metadata checks for
  files and older records without identity. Renaming the same folder preserves
  its current contents.
- A second failing regression, `copied_folder_rename_can_be_redone_after_undoing_child_changes`,
  exposed the same metadata dependency when Copy Redo recreates the folder.
  Update the dependent Rename identity for a verified journal-created folder
  even when its directory metadata differs.
- Green: regressions cover complete Undo/Redo cycles and restart. A separate
  test verifies Undo and Redo refuse replacement folders and symlinks, keeping
  the originals and replacement contents. All-target tests passed with 670
  tests and 24 opt-in tests ignored. Clippy, formatting, and whitespace checks
  passed.
