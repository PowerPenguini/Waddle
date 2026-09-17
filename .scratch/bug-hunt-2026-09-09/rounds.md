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

## Round 67: New File and New Folder Undo reject a restored cross-device Move (2026-09-14)

- Reproduction: create an item, record its creation, Move it across filesystems,
  Undo Move, restart the journal, then Undo creation.
- Red: `cargo test new_item_undo_survives_a_cross_device_move_round_trip -- --nocapture`
  refused the restored file as a different item.
- Cause: cross-device Move Undo recreates the item with a new identity. The
  journal updated dependent Rename identities but not creation records.
- Fix: update New File and New Folder identities for paths verified as
  recreated by a journal operation, persisting the updates in checkpoints.
- Green: the regression passes for files and folders across distinct devices
  and restart. Existing replacement-preservation regressions remain green.
  All-target tests passed with 671 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 68: Transfer Redo rejects a file recreated by New File Redo (2026-09-16)

- Reproduction: create a file, Copy or Move it, Undo both operations, Redo New
  File, restart the journal, then Redo the Transfer.
- Red: `cargo test redo_transfer_accepts_a_redone_new_file_after_restart -- --nocapture`
  refused the recreated file as changed.
- Cause: New File Redo assigned a fresh modification time. The later Transfer
  retained the original timestamp in its source fingerprint.
- Fix: restore the recorded timestamp when redoing New File. Keep the Transfer
  verification unchanged and refresh the recreated file's identity as before.
- Green: Copy and Move pass through Redo, restart, and another Undo cycle.
  Timestamp coverage includes fractional seconds before and after the Unix
  epoch. All-target tests passed with 673 tests and 24 opt-in tests ignored.
  Clippy, formatting, and whitespace checks passed.

## Round 69: creation Undo deletes later permission and attribute edits (2026-09-16)

- Reproduction: record New File or New Folder, change its permissions or add a
  user attribute, restart the journal, and Undo creation.
- Red: `cargo test creation_undo_preserves_later_metadata_edits_after_restart -- --nocapture`
  deleted the edited item.
- Cause: creation records checked identity and basic metadata but had no
  persisted permission or attribute check.
- Fix: capture permissions and an attribute digest with new creation records,
  verify them before removal, and refresh them when Redo creates the item.
  Older records retain their previous checks and acquire the new guard on Redo.
- Green: the regression preserves files and folders after permission and
  attribute edits, both initially and after Redo, including restart. Legacy
  compatibility and cross-device recreation regressions still pass.
  All-target tests passed with 674 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 70: permanent-delete fallback removes a replacement item (2026-09-16)

- Reproduction: open the permanent-delete confirmation after a failed Trash
  Transfer, replace the file at its path, then confirm deletion.
- Red: `cargo test permanent_delete_fallback_preserves_a_replacement_after_confirmation_opens -- --nocapture`
  deleted the replacement file.
- Cause: the fallback retained only FileEntry paths and deleted whatever
  occupied each path when the worker ran.
- Fix: capture device/inode identity when the confirmation opens, carry it into
  the worker, and refuse deletion if the item differs or cannot be verified.
- Green: regressions preserve a file replaced while the prompt is open and a
  folder replaced after confirmation while work is queued. Original and
  replacement contents survive, and the error remains visible. Existing
  partial-deletion behavior still passes. All-target tests passed with 676
  tests and 24 opt-in tests ignored. Clippy, formatting, and whitespace checks
  passed.

## Round 71: Rename acts on a replacement after its editor opens (2026-09-16)

- Reproduction: open Rename, replace the source at the same path, then submit
  the new name. Also replace the source after submission while work is queued.
- Red: `cargo test rename_preserves_replacements_after_the_editor_opens -- --nocapture`
  moved the replacement to the requested name.
- Cause: the editor and its worker retained a path without the source identity
  from when the editor opened.
- Fix: capture device/inode identity when opening Rename, carry it through
  submission, and verify it in the worker before renaming.
- Green: file and folder replacements survive both timing cases, originals
  remain intact, errors remain in the editor, and no Undo record is added.
  All-target tests passed with 677 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 72: creation writes into a replaced parent folder (2026-09-16)

- Reproduction: open New File or New Folder, replace the parent directory at
  the same path, then submit. Also replace it after submission while work is
  queued.
- Red: `cargo test creation_preserves_a_replaced_parent_after_the_prompt_opens -- --nocapture`
  created the item in the replacement folder.
- Cause: the prompt retained no parent identity, and the worker used whichever
  directory occupied the navigation path when creation ran.
- Fix: capture parent device/inode identity when opening the prompt and verify
  it in the worker. Follow directory symlinks when identifying the parent,
  matching where creation actually writes.
- Green: both creation operations refuse replaced parents in both timing
  cases and add no Undo record. Symlink coverage verifies that valid targets
  support creation and Undo while replaced targets remain untouched.
  All-target tests passed with 679 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 73: delayed shell directory changes override newer navigation (2026-09-16)

- Reproduction: queue the result of a shell command that changes directory,
  navigate to the parent, then deliver the command result.
- Red: `cargo test queued_shell_directory_changes_do_not_override_newer_navigation -- --nocapture`
  replaced the user's chosen folder with the old command's destination.
- Cause: command completions applied their directory without checking whether
  a newer Navigation session request had superseded that choice.
- Fix: capture the navigation revision at command submission and apply the
  directory only if it still matches. Refreshes do not advance this revision.
- Green: settled and pending navigation, navigating away and back, and refresh
  cases pass. Fresh shell directory changes still work. All-target tests passed
  with 680 tests and 24 opt-in tests ignored. Clippy, formatting, and whitespace
  checks passed.

## Round 74: creation Redo changes recorded permissions (2026-09-16)

- Reproduction: record a private file or folder, Undo creation, reopen the
  journal, then Redo under a process with different creation defaults.
- Red: `cargo test creation_redo_restores_recorded_permissions_after_restart -- --nocapture`
  recreated a file recorded as 0600 with permissions 0644.
- Cause: creation Redo ignored the stored mode and used the current umask.
- Fix: create using the recorded permission bits, then restore those bits
  exactly so a more restrictive current umask cannot remove intended access.
  Legacy records without metadata retain their previous defaults.
- Green: private and group-readable files and folders retain their modes with
  both 022 and 077 umasks in isolated test processes. Reopened journals and
  repeated Undo/Redo cycles pass. All-target tests passed with 681 tests and
  24 opt-in tests ignored. Clippy, formatting, and whitespace checks passed.

## Round 75: creation Redo inherits changed parent ACLs (2026-09-16)

- Reproduction: create a file or folder, Undo creation, change the parent's
  default ACL to grant a different user access, reopen the journal, and Redo.
- Red: `cargo test creation_redo_preserves_access_control_when_parent_defaults_change -- --nocapture`
  added an inherited named-user ACL to an item originally recorded without one.
- Cause: the journal retained an attribute digest for verification but no ACL
  values for replay. Restoring permission bits could enable newly inherited users.
- Fix: retain access and default ACL snapshots in creation metadata. Create
  with group and other access masked, restore the saved ACLs or remove inherited
  ACLs when none were recorded, then restore the recorded permission bits.
  Optional snapshots preserve compatibility with older journal records.
- Green: files and folders preserve their ACLs across changed parent defaults,
  journal restart, and repeated Undo/Redo. Older records remain usable. Injected
  ACL set, unsupported, and removal failures report errors and keep inherited
  users masked. All-target tests passed with 684 tests and 24 opt-in tests
  ignored. Clippy, formatting, and whitespace checks passed.

## Round 76: failed creation Redo cannot retry after metadata errors (2026-09-16)

- Reproduction: inject an ACL restoration failure during New File or New
  Folder Redo, remove the fault, reopen the journal, and retry Redo.
- Red: `cargo test failed_creation_acl_restore_can_retry_without_exposing_inherited_users -- --nocapture`
  refused the retry because the incomplete item still occupied its destination.
- Cause: creation returned metadata errors without removing its empty result.
- Fix: retain an incomplete-creation guard until metadata capture succeeds.
  On failure, remove an empty result only while its device/inode identity still
  matches. Preserve replacement items and files or folders with added content.
- Green: retries restore the original permissions and ACLs after set, unsupported,
  and removal failures, including a journal restart and another Undo. Injected
  replacements and external content survive cleanup. All-target tests passed
  with 685 tests and 24 opt-in tests ignored. Clippy, formatting, and whitespace
  checks passed.

## Round 77: delayed shell directory changes replace a newer Search session (2026-09-16)

- Reproduction: queue a completed shell command that changes directory, start
  a new Search session, then deliver the old command result.
- Red: `cargo test queued_shell_directory_changes_preserve_a_newer_search_session -- --nocapture`
  navigated to the old command's target and closed the newer recursive search.
- Cause: command completion checked folder navigation revisions, but starting
  a Search session did not change that revision.
- Fix: capture Search session identity when submitting a shell command and
  require that identity to match before applying its directory change. Refreshes
  retain session identity, and completion still refreshes filesystem results.
- Green: local and recursive searches retain their query and selected match.
  Fresh shell directory changes still work after refreshing an existing search.
  All-target tests passed with 686 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 78: queued permission changes modify replacement targets (2026-09-16)

- Reproduction: submit a chmod command, replace a target before its worker
  executes, then deliver the resulting app messages.
- Red: `cargo test queued_permission_changes_preserve_replaced_targets -- --nocapture`
  changed the replacement from 0700 to 0755.
- Cause: permission work retained only paths, so it modified whichever items
  occupied those paths when the queued worker ran.
- Fix: capture target device/inode identity at submission, following symlinks
  as chmod does. Verify identity before each permission change and retain
  initial inspection errors rather than accepting targets that appear later.
- Green: replacement files and folders retain their permissions; unchanged
  targets in the same command still succeed and failures remain visible.
  Selected symlink coverage checks valid, replaced, and initially missing
  targets. All-target tests passed with 688 tests and 24 opt-in tests ignored.
  Clippy, formatting, and whitespace checks passed.

## Round 79: user shell variables override reported command status (2026-09-16)

- Reproduction: submit `readonly status=0; false` through the command prompt.
  Also check a successful command after setting a nonzero readonly status.
- Red: `cargo test shell_exit_status_is_not_overridden_by_a_user_status_variable -- --nocapture`
  reported exit 0 for the failing command and added a readonly-variable error.
- Cause: the shell wrapper assigned the command's exit code to a user-visible
  variable named status. A readonly variable prevented that assignment.
- Fix: expand the numeric exit code directly into the wrapper's final builtin
  commands, preserving it across directory reporting without assigning status.
- Green: failing and successful commands retain their exit codes and output.
  Both command prefixes retain their directory behavior, and user exit traps
  still run. All-target tests passed with 689 tests and 24 opt-in tests ignored.
  Clippy, formatting, and whitespace checks passed.

## Round 80: shell output drops stdout printed by exit traps (2026-09-16)

- Reproduction: run a command with an EXIT trap that prints to stdout and
  stderr, after the command prints its normal output and changes directory.
- Red: `cargo test shell_output_preserves_stdout_from_exit_traps -- --nocapture`
  retained normal output and trap stderr but discarded the trap's stdout.
- Cause: extracting the shell directory marker truncated all stdout after
  that marker, including output printed by the exit trap.
- Fix: remove only the marker, path, and terminating delimiter from stdout.
- Green: both command prefixes retain normal output, trap stdout, trap stderr,
  the failing command's exit code, and their intended directory behavior.
  All-target tests passed with 690 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 81: refresh immediately hides silent shell results (2026-09-16)

- Reproduction: submit a silent shell command and finish its completion and
  subsequent directory refresh through app messages.
- Red: `cargo test silent_shell_results_survive_refresh_until_the_next_input -- --nocapture`
  displayed the folder summary instead of the command's exit status.
- Cause: silent command results were ordinary browser status text, which the
  completion's own refresh immediately replaced.
- Fix: retain completion status as a status notice until the next user input.
- Green: silent success, failure, and directory-changing commands keep their
  exit status through refreshes and clear it on the next input. Directory
  changes still apply and silent commands do not open an output panel.
  All-target tests passed with 691 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 82: changing shell directory retargets selected paths (2026-09-16)

- Reproduction: select a file, run `cd ../other; cat $selected`, and place an
  unrelated file with the same name in the other directory.
- Red: `cargo test selected_shell_paths_still_refer_to_the_selection_after_cd -- --nocapture`
  printed the unrelated file's contents instead of the selected file's contents.
- Cause: selected entries beneath the starting directory were passed to Bash
  as relative paths, whose meaning changed after cd.
- Fix: pass absolute selected paths and resolve relative inputs against the
  command's starting directory. Update the command help to describe this behavior.
- Green: both command prefixes read the selected file after cd, including a
  filename with spaces. Existing argument quoting and option-prefix checks
  pass with absolute paths. All-target tests passed with 692 tests and
  24 opt-in tests ignored. Clippy, formatting, and whitespace checks passed.

## Round 83: unchanged Location text can open a different non-UTF-8 folder (2026-09-16)

- Reproduction: open a folder whose name contains invalid UTF-8, focus Location,
  and submit it unchanged. Create a valid UTF-8 folder with the same displayed
  replacement character to distinguish the paths.
- Red: `cargo test submitting_an_unchanged_location_preserves_non_utf8_path_bytes -- --nocapture`
  entered the valid UTF-8 twin instead of preserving the original folder.
- Cause: Location submission rebuilt a filesystem path from lossy display text.
- Fix: track whether Location text was edited. Preserve the Navigation session's
  original path when unedited, and interpret explicit input as before. Reset
  the edit flag whenever navigation or focus resets the Location text.
- Green: unchanged submission retains the original path bytes and entries;
  explicitly entering the identical-looking UTF-8 path still opens its folder.
  All-target tests passed with 693 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 84: folder refresh replaces active Location edits (2026-09-16)

- Reproduction: edit Location while a folder refresh is pending, or trigger a
  filesystem notification after typing a relative destination.
- Red: `directory_refresh_preserves_location_edits_and_their_submission`
  showed the current folder path replacing the typed destination.
- Cause: every navigation completion reset Location text and its edited flag,
  including refreshes of the current folder.
- Fix: preserve an active Location edit when a refresh commits.
- Green: refreshes update the file list while retaining the text and its
  submission behavior. Explicit Parent navigation still updates Location.
  All-target tests passed with 694 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 85: shell comments trigger selected-placeholder validation (2026-09-16)

- Reproduction: submit `printf result # $selected is optional` with no selection
  through each command prefix.
- Red: `shell_comments_do_not_require_selected_entries` produced no command
  output. The placeholder scanner rejected the comment before Bash could run.
- Cause: the scanner interpreted placeholders and quotes inside shell comments.
- Fix: recognize an unquoted comment at the start of a shell word and copy it
  through the newline without interpreting its contents. Track word boundaries
  through quoting, escapes, operators, and line continuations.
- Green: both prefixes execute comments without requiring a selection. Further
  checks cover quoted placeholders and apostrophes in comments, multiline input,
  and literal hashes beside selected paths, including escaped newlines.
  All-target tests passed with 696 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 86: noisy command output hides standard error (2026-09-16)

- Reproduction: submit a command that prints 140,000 characters to standard
  output, writes an error to standard error, and exits unsuccessfully.
- Red: `noisy_shell_commands_keep_standard_error_visible` lost the error text
  in both command prefixes because combined-output truncation removed stderr.
- Cause: stdout consumed the display allowance before stderr was appended.
- Fix: allocate the display allowance between both streams before combining
  them. Either stream can use the other's unused space. Include truncation
  notices and the stderr label in the limit, and preserve UTF-8 boundaries.
- Green: errors remain visible after noisy stdout, fitting streams retain all
  their text, and large UTF-8 or stderr-only output stays within 128 KiB.
  All-target tests passed with 698 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 87: shell redirection captures Waddle's directory report (2026-09-16)

- Reproduction: redirect shell stdout with `exec >log.txt`, change directory,
  print a log message and an error, then exit unsuccessfully.
- Red: `shell_stdout_redirection_preserves_logs_and_directory_changes` found
  Waddle's NUL-delimited directory report appended to the user's log.
- Cause: the Bash wrapper sent its directory report through user stdout, so
  persistent redirection captured it and prevented navigation from receiving it.
- Fix: use a separate inherited Unix-stream descriptor for the directory report.
  Its reader has the existing bounded-output and completion behavior. The parent
  closes its sender after spawning, and ordinary script descriptors stay free.
- Green: redirected logs contain only user output, standard error and exit codes
  remain intact, and colon commands still follow directory changes. Further
  checks cover script descriptors 3 and 4, large output before and during EXIT
  traps, and directory names containing a newline and non-UTF-8 bytes.
  All-target tests passed with 700 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 88: shell navigation trusts a stale PWD variable (2026-09-16)

- Reproduction: enter a folder, rename it from within the shell command, and
  finish the command. Bash's PWD variable still names the old location.
- Red: `shell_directory_changes_follow_a_renamed_working_directory` left Waddle
  in its original folder instead of following the shell to the renamed folder.
- Cause: the directory report copied PWD rather than querying the actual cwd.
- Fix: report `builtin pwd -P` through the dedicated channel, preserving the
  command's original exit status. Remove exactly the builtin's final newline
  and accept only an absolute path.
- Green: renamed directories are followed correctly. Changed, unset, and readonly
  PWD variables do not affect reporting; non-UTF-8 bytes and trailing newlines
  in directory names remain intact. Both command prefixes retain their behavior.
  All-target tests passed with 702 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 89: shell function arguments retarget selected paths (2026-09-16)

- Reproduction: define a function that reads `$selected`, then call it with an
  unrelated file as its argument while another file is selected in Waddle.
- Red: `shell_functions_keep_selected_paths_separate_from_function_arguments`
  read the unrelated file's contents through both command prefixes.
- Cause: placeholders expanded to Bash's positional arguments, which change
  inside functions and after set or shift commands.
- Fix: capture the selected paths in a readonly Bash array before evaluating
  the command and expand placeholders from that array.
- Green: function arguments retain their normal meaning while placeholders
  retain the original selected paths. Additional checks cover set, shift,
  subshells, multiple selections, spaces, and non-UTF-8 filenames.
  All-target tests passed with 704 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 90: replacement during chmod changes the wrong item (2026-09-16)

- Reproduction: use an isolated child and a filesystem-call shim to replace the
  target immediately before chmod, after the queued command's identity check.
- Red: `permission_changes_preserve_items_replaced_during_chmod` changed the
  replacement to 755 instead of leaving its original 700 permissions intact.
- Cause: checking the pathname and later passing it to chmod leaves a race in
  which the pathname can resolve to a different inode.
- Fix: open the target with O_PATH, verify the opened inode, and apply permissions
  through its owned procfs descriptor path. The handle follows selected symlink
  targets and does not require read access or open device/FIFO contents.
- Green: replacement files, folders, and symlink targets retain their permissions
  while the original opened item receives the requested change. Existing queued
  replacement guards pass, as do mode-000 files/folders and FIFOs without peers.
  All-target tests passed with 706 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 91: explicitly retyped non-UTF-8 names are ignored (2026-09-17)

- Reproduction: open Rename for a non-UTF-8 filename, edit its text, then enter
  the valid UTF-8 spelling shown in the editor and submit.
- Red: `explicitly_retyping_a_lossy_filename_renames_its_original_bytes` left
  the original byte sequence untouched instead of applying the requested name.
- Cause: the unchanged-name check compared only lossy display strings, making
  distinct non-UTF-8 and UTF-8 filenames appear identical.
- Fix: track whether Rename text was edited. Untouched text preserves the
  original filename; edited text must match its actual UTF-8 name to be a no-op.
- Green: explicit renames work for files and folders and survive Undo/Redo.
  Existing destination names produce a visible error and preserve both items.
  Untouched names and retyped identical valid names remain no-ops without history.
  All-target tests passed with 708 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 92: tabs send built-in commands to Bash (2026-09-17)

- Reproduction: submit favorite, recent, volume, or chmod with a tab between
  the command name and its arguments through the colon prompt.
- Red: `built_in_commands_accept_tabs_before_their_arguments` showed a
  tab-separated favorite command entering shell execution instead of its handler.
- Cause: those four built-ins recognized only a literal space after the name,
  unlike other commands that already used whitespace-aware tokenization.
- Fix: use the existing command-and-arguments parser for their exact command names.
- Green: spaces, tabs, and mixed separators dispatch to the built-in handlers.
  Tab-separated chmod applies to selected files and quoted explicit paths while
  preserving unrelated files. Bang commands and similarly named external
  commands retain shell dispatch.
  All-target tests passed with 710 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 93: failed Recent preference saves change the active setting (2026-09-17)

- Reproduction: block the Recent preferences temporary file in an isolated
  configuration directory, then try to disable or enable Recent through the app.
- Red: `failed_recent_preference_saves_preserve_the_previous_behavior` could
  no longer open Recent after a failed disable, despite unchanged saved settings.
- Cause: enable and disable changed the in-memory flag before attempting the save.
- Fix: write the proposed preferences first and commit the active preferences
  only after the save succeeds.
- Green: failed enable and disable preserve the prior behavior and report the
  write error. Removing the filesystem obstruction allows a successful retry,
  and a newly opened app reads the new saved preference.
  All-target tests passed with 711 tests and 24 opt-in tests ignored. Clippy,
  formatting, and whitespace checks passed.

## Round 94: queued volume errors overwrite newer command feedback (2026-09-17)

- Reproduction: queue an invalid volume command result, complete a newer command,
  then deliver the older result through the app message loop.
- Red: `queued_volume_errors_preserve_newer_command_feedback` showed the obsolete
  volume error replacing the newer command's status.
- Fix: associate volume results with their Command session output revision and
  display feedback only when that revision is still current. Successful volume
  operations still refresh the Sidebar tree's volume list.
- Green: delayed errors preserve newer feedback, and current errors remain visible.
  All-target tests, strict Clippy, formatting, and whitespace checks passed.

## Round 95: delayed sidebar mounts override newer navigation (2026-09-17)

- Reproduction: activate an unmounted Sidebar volume, navigate elsewhere, then
  deliver the desktop's mount result. Also complete a mount without a known
  path, navigate elsewhere, and let the path lookup deadline expire.
- Red: `delayed_volume_mounts_preserve_newer_navigation` reopened the mounted
  folder after Parent navigation. A second regression showed an abandoned
  path lookup replacing newer feedback with its timeout error.
- Fix: capture the Navigation session revision when starting the mount and
  retain it while waiting for the path. Obsolete completions and path lookups
  release the Sidebar loading state without changing navigation or feedback.
- Green: settled navigation, pending navigation, and returning to the original
  folder all supersede the mount's automatic opening. Refreshes still allow
  opening the mounted folder, and a current missing-path timeout remains visible.
  All-target tests passed with 714 tests and 24 opt-in tests ignored. Strict
  Clippy, formatting, and whitespace checks passed.

## Round 96: an earlier mount wins over the latest Sidebar volume choice (2026-09-17)

- Reproduction: activate two unmounted volumes and deliver their mount results
  in either order through app messages.
- Red: `the_latest_sidebar_volume_choice_wins_regardless_of_mount_order` opened
  the earlier volume while the user's later choice was still mounting.
- Cause: activating an unmounted volume did not advance the Navigation session,
  so both mounts shared a revision and the first completion could win.
- Fix: start a MountVolume navigation transition at activation. This supersedes
  prior navigation work and advances the revision before the mount starts.
- Green: the later volume opens in either completion order, and Back returns
  directly to the original folder. A queued real folder-read result is also
  superseded when the user chooses an unmounted volume.
  All-target tests passed with 716 tests and 24 opt-in tests ignored after
  excluding an unrelated, concurrently added diagnostic test from this round.
  The shared checkout's complete suite also passed with that diagnostic included.
  Strict Clippy, formatting, and whitespace checks passed.

## Round 97: unmount completion discards navigation away from the volume (2026-09-17)

- Reproduction: begin unmounting the displayed volume, submit Location navigation
  to a folder outside it, then deliver the unmount completion before the folder opens.
- Red: `unmount_completion_preserves_navigation_away_from_the_volume` showed the
  unmount replacing the newer destination with the home folder.
- Cause: unmount fallback considered only the displayed folder, ignoring pending
  navigation and whether the displayed location was a collection.
- Fix: obtain the pending target directory, or the displayed directory when no
  navigation is pending. Redirect only when that target lies within the volume.
  Recent and Trash do not have a target directory for this decision.
- Green: navigation away from the volume reaches its intended destination;
  navigation into the unmounted volume still falls back to a safe folder.
  Pending and open Recent and Trash views survive the completion.
  The shared checkout's all-target suite passed with 735 tests and 24 opt-in
  tests ignored, including the separate search work in progress. Strict Clippy,
  formatting, and whitespace checks passed. That search work is excluded from
  this round's commit.

## Round 98: delayed unmount feedback overwrites newer actions (2026-09-17)

- Reproduction: request a Sidebar unmount, complete newer navigation or a
  command, then deliver the desktop's old unmount result.
- Red: `delayed_unmount_feedback_preserves_newer_navigation_and_commands` showed
  a delayed device-busy error replacing the newer folder's status.
- Fix: capture Navigation and Command session revisions with the unmount request
  and display completion feedback only while both revisions remain current.
  Volume refresh and navigation out of an unavailable folder still run.
- Green: obsolete success and failure feedback preserve newer navigation and
  command status; current results remain visible. Success still refreshes the
  Sidebar volume list, and failures clear the busy state so unmount can be retried.
  All-target tests passed with 736 tests and 24 opt-in tests ignored in the shared
  checkout. Strict Clippy, formatting, and whitespace checks passed. Separate
  search changes remain outside this commit.

## Round 99: Recent preference saves overwrite pre-existing temporary entries (2026-09-17)

- Reproduction: place a symlink, hardlink, file, or directory at recent.json.tmp
  in an isolated configuration directory, then disable Recent through the app.
- Red: `recent_preference_saves_preserve_preexisting_temporary_entries` showed
  the symlink's unrelated target overwritten with the preferences JSON.
- Cause: saving used fs::write on a fixed temporary path, following links and
  truncating any existing file before renaming it over the preferences file.
- Fix: create a fresh NamedTempFile in the preferences directory, write and sync
  it, then atomically persist it. Promote the existing tempfile dependency to
  application use; the lockfile and packaged dependency sources are unchanged.
- Green: all four collision types are preserved and the saved preference survives
  reopening the app. The save-failure regression now obstructs the final target
  instead of the old temporary name; active settings remain unchanged on failure,
  owned temporary files are removed, and retry persists the new setting.
  Locked all-target tests passed with 737 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 100: Favorites edits overwrite pre-existing temporary entries (2026-09-17)

- Reproduction: occupy favorites.json.tmp with a symlink, hardlink, file, or
  directory, then add, reorder, and remove Favorites through app messages in
  an isolated configuration directory.
- Red: `favorite_edits_preserve_preexisting_temporary_entries` showed an unrelated
  symlink target overwritten with Favorites JSON during Add.
- Cause: the locked save still wrote through a fixed temporary path, following
  links and truncating existing files before atomic replacement.
- Fix: write and sync an exclusively created NamedTempFile in the configuration
  directory, then persist it atomically. Keep the existing stable lock around
  read, modification, and replacement so concurrent windows retain each other's edits.
- Green: all collision types remain intact; Add, drag reordering, Remove, and
  reopening preserve the requested Favorites. The storage-failure regression
  now removes directory write permission, confirming unchanged in-memory and
  saved state after failed Add, Remove, and Reorder, followed by successful retry.
  All eight Favorites checks passed, including simultaneous-window writes.
  Locked all-target tests passed with 738 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 101: command diagnostics overwrite pre-existing temporary entries (2026-09-17)

- Reproduction: occupy the diagnostics temporary path with a symlink, hardlink,
  file, or directory, then run a failing shell command through app messages.
- Red: `command_failure_history_preserves_preexisting_temporary_entries` showed
  the symlink's unrelated target replaced with diagnostics JSON.
- Cause: the history saver wrote through a fixed temporary path before rename,
  following links and truncating files that it did not create.
- Fix: write and sync an exclusively created NamedTempFile in the state directory,
  then atomically persist it as the diagnostics history.
- Green: all four collision types remain intact. The command failure is persisted
  and visible through :diagnostics. Existing retention and reporting checks pass.
  Locked all-target tests passed with 739 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 102: one window erases another window's command diagnostics (2026-09-17)

- Reproduction: open two app windows against one diagnostics file, run a failing
  command in each, and request :diagnostics from both windows.
- Red: `command_failures_from_multiple_windows_remain_in_shared_diagnostics`
  showed the second window removing the first failure from the saved history.
  An added storage-failure check also exposed loss of previously displayed
  shared records from the report's fallback cache.
- Cause: each save replaced the shared file with one window's cached list,
  and reports did not reload or cache records written by other windows.
- Fix: lock a stable sidecar around read, append, prune, and atomic replacement.
  Track unsaved records separately so retries append them once without merging
  duplicate cached records. Reload shared history for reports and retain it as
  the fallback cache. Keep the existing time and record-count limits.
- Green: interleaved window writes and reports retain both failures. Blocked saves
  keep pending failures visible; retry preserves another window's intervening
  write. Repeated identical commands remain separate records, and a newly opened
  window reports every saved occurrence exactly once. Test-default diagnostic
  paths are unique per app instance so unrelated tests do not share history.
  Locked all-target tests passed with 740 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 103: delayed volume commands overwrite newer navigation feedback (2026-09-17)

- Reproduction: run a real failing :volume command, retain its completion,
  start navigation through app messages, then deliver the queued result.
- Red: the regression replaced the pending folder's Opening status with
  "unknown volume action: invalid-action". It was initially named
  queued_volume_command_errors_preserve_newer_folder_navigation.
- Cause: volume commands guarded feedback against newer commands but did not
  retain the Navigation session revision, unlike sidebar unmounts.
- Fix: capture the navigation revision on submission and require both revisions
  to match before displaying feedback. Successful results still refresh volumes.
- Green: queued_volume_command_feedback_preserves_newer_folder_navigation
  covers errors and supplied desktop-success results during pending and settled
  navigation, leaving and returning to the original folder, ordinary refresh,
  and no intervening navigation. Current feedback remains visible, obsolete
  feedback preserves status, and success refreshes the Sidebar volume list.
  The existing newer-command regression also passed. Locked all-target tests
  passed with 741 tests and 24 opt-in tests ignored in the shared checkout.
  Strict Clippy, formatting, and whitespace checks passed.

## Round 104: Rename Undo/Redo loses recovery after journal save failures (2026-09-17)

- Reproduction: make an existing journal directory unwritable, then Undo or Redo
  a rename while its file directory remains writable. A second regression injects
  a journal fsync failure only after the rename has completed, then reopens history.
- Red: rename_history_refuses_changes_without_a_durable_checkpoint showed Undo
  moving the file without saving intent. After adding that checkpoint,
  rename_history_recovers_when_saving_after_the_rename_fails showed retry failing
  because it still looked for the pre-rename source.
- Cause: Rename had no durable intent before its filesystem effect and no recovery
  path for an effect completed before the final journal save.
- Fix: verify the source and destination, persist intent with the source identity,
  then rename. When retrying recorded intent, recognize completion only if the
  source is absent and the destination retains the recorded identity and required
  fingerprint. Upgrade legacy records with identity before saving intent.
- Green: Undo and Redo refuse mutation when checkpoint storage is unavailable,
  recover after post-rename save failure for files and populated directories,
  reject identical-content replacement destinations, and recover after process
  interruption between intent commit and rename. Reopening and reversing the
  recovered operation works. Seven focused Rename history checks passed.
  Locked all-target tests passed with 744 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 105: creation Undo deletes items without recoverable journal progress (2026-09-17)

- Reproduction: make the journal directory unwritable and Undo New File or New
  Folder. Separately inject fsync failure only after the item has been deleted,
  then reopen the journal and retry Undo.
- Red: undo_creation_preserves_the_item_when_history_cannot_be_saved showed an
  item deleted without saved intent. After adding intent checkpoints,
  undo_creation_recovers_when_saving_after_deletion_fails showed retry failing
  because the deleted item could no longer be verified.
- Cause: creation Undo performed deletion before recording intent and treated
  an absent item as an error even while recovering a saved Undo operation.
- Fix: verify the original item, save intent with its identity, then delete it.
  A retry with recorded Undo intent accepts absence as completed deletion.
  Existing paths still require the normal identity and metadata checks.
- Green: both creation types preserve their items on checkpoint failure, recover
  after final-save failure or interruption before deletion, and retain Undo/Redo
  across reopening. Identical-metadata replacements survive recovery attempts.
  Missing items without recorded intent remain errors and do not enable Redo.
  All four new regressions passed. Locked all-target tests passed with 748 tests
  and 24 opt-in tests ignored in the shared checkout. Strict Clippy, formatting,
  and whitespace checks passed.

## Round 106: creation Redo strands published items after journal save failures (2026-09-17)

- Reproduction: Undo a creation, inject fsync failure after Redo publishes its
  replacement item, reopen history, and retry Redo.
- Red: redo_creation_recovers_when_saving_after_publication_fails recreated the
  item, but every retry failed because the destination already existed.
- Cause: creation Redo published directly at the destination without recording
  the new identity before publication or retaining a recoverable preparation.
- Fix: exclusively create a temporary sibling, restore its metadata, and save
  the preparation path and identity before an atomic non-overwriting rename.
  Failed uncommitted checkpoints clean up only owned empty preparations; saved
  preparations survive failures. Recovery verifies identity and metadata at the
  preparation or final path. Completed recovery updates dependent Rename history.
  Optional preparation paths preserve raw Unix bytes and retain older records.
- Green: recovery covers file and folder creation, uncommitted checkpoint errors,
  committed directory-sync errors, process interruption before publication, and
  final-save failure after publication. Unrelated destinations and replacement
  preparations survive failed recovery; successful retry leaves no preparation.
  Reopened Undo/Redo and dependent Rename remain usable. Existing ACL restoration
  and cleanup regressions still pass; the cleanup fault now targets the actual
  preparation path and verifies preservation of foreign data there.
  Fifteen creation-related checks passed. Locked all-target tests passed with
  750 tests and 24 opt-in tests ignored in the shared checkout. Strict Clippy,
  formatting, and whitespace checks passed.

## Round 107: retried Trash recovery deletes metadata for a reused slot (2026-09-17)

- Reproduction: restore an item through history while its Trash metadata directory
  is unwritable, then populate the vacated Trash slot with another item and new
  recovery metadata before reopening and retrying the history operation.
- Red: retrying_trash_restore_preserves_a_reused_trash_slot reported success and
  removed the newer item's metadata. A control also caught an overly strict first
  fix that blocked completion even when no metadata remained to remove.
- Cause: a restore_pending receipt verified the already-restored original but
  unconditionally deleted its old metadata path without checking the Trash slot.
- Fix: when metadata still exists, require the physical Trash slot to be absent
  before removing it. If metadata is already gone, finish without touching a
  newer occupant. This matches normal Restore's source-absence guard.
- Green: the regression covers Undo Trash and Redo Restore with a replacement
  file, populated folder, or dangling symlink. Both the newer contents and its
  recovery metadata survive refusal. Cleanup can finish after the newer item is
  handled, and missing metadata permits completion while preserving an occupant.
  Locked all-target tests passed with 751 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 108: queued Trash moves replacement source items (2026-09-17)

- Reproduction: submit Trash through app messages, replace a selected source
  before running the queued task, then drain the real worker in isolated XDG
  directories on the home filesystem so GIO Trash is available.
- Red: queued_trash_preserves_replaced_source_items showed the replacement file
  moved to Trash instead of preserving it and reporting a changed source.
- Cause: a Trash batch retained paths and listing metadata but no source identity;
  the worker accepted whichever entry occupied a path when execution began.
- Fix: capture each source's device and inode at batch creation using symlink
  metadata, then verify them immediately before invoking desktop Trash. Inspection
  failures and changed entries become individual failures; other entries continue.
- Green: the app regression covers replaced files, populated directories, and
  symlinks pointing to the same target. Originals retained elsewhere and new
  occupants remain intact. An unchanged inode edited while queued is still
  trashed with its updated contents. The existing adapter progress test now uses
  real temporary source files; progress, cancellation, and retry checks pass.
  Locked all-target tests passed with 752 tests and 24 opt-in tests ignored in
  the shared checkout. Strict Clippy, formatting, and whitespace checks passed.

## Round 109: Trash Retry adopts replacement source identities (2026-09-17)

- Reproduction: fail or cancel a Trash request, replace one selected source,
  and invoke Retry through app messages in an isolated real GIO Trash fixture.
- Red: trash_retry_keeps_original_source_identities showed Retry moving the
  replacement that the initial failed attempt had correctly rejected.
- Cause: the queue rebuilt a Trash batch from FileEntry paths on every Retry,
  capturing current identities instead of retaining the original request's ones.
- Fix: the worker returns a retry batch containing the original source snapshots
  for failed and cancelled entries. The queue retains and runs that batch instead
  of recapturing identities. Successful entries are excluded from later retries.
- Green: failed and cancelled requests preserve replacements through repeated
  retries, continue processing unchanged selected entries, and succeed when the
  original source returns to its path. The fixture verifies actual Trash contents
  and that the replacement remains intact. Locked all-target tests passed with
  753 tests and 24 opt-in tests ignored in the shared checkout. Strict Clippy,
  formatting, and whitespace checks passed.

## Round 110: permanent-delete fallback adopts a replacement Trash source (2026-09-17)

- Reproduction: request Trash through App messages, replace a selected source before
  the worker runs, then confirm the permanent-delete fallback. Also fail real GIO
  Trash using a read-only parent, replace the source after the worker finishes but
  before its completion reaches the UI, and confirm the resulting prompt.
- Red: `cargo test --locked trash_fallback_preserves_sources_replaced_before_confirmation -- --nocapture`
  failed with `Fallback deleted a replacement file, timing=queued`.
- Cause: the worker retained the original identity for Trash and Retry, but its
  failure result discarded that identity. Opening the fallback prompt captured
  whichever item currently occupied the original path.
- Fix: Trash failures carry the source identity captured when the request was
  queued. The Transfer session preserves it, and permanent-delete confirmation
  uses it without inspecting the path again to choose a new target.
- Green: the regression passes for files, populated folders, and symlinks replaced
  at either timing. Unchanged originals remain deletable after confirmation, and
  symlink targets remain intact. Tests use real files and GIO in an isolated child.
- Validation: all-target tests passed (754 passed, 24 ignored); strict Clippy,
  formatting, and whitespace checks passed. Existing uncommitted search changes
  were excluded from this commit.
