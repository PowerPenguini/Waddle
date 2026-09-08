# Navigation and Transfer lifecycle deepening

Base: `1d21eb2` on `main`. Requested implementation of both Strong architecture candidates; the saved-state candidate is excluded.

## Navigation session

The Navigation session now owns request replacement and first-Back cancellation, deferred refresh, the distinction between a retained folder path and the displayed location, and the Sidebar tree load associated with a request.

Its start result describes the new request, superseded request, reusable Sidebar tree load, and interaction cleanup. Its completion result contains only the accepted request's Sidebar tree load and whether a deferred refresh must run. App executes these effects and launches Iced tasks. App no longer stores `pending_tree_navigation` or `pending_refresh`, pairs Sidebar tree loads with completions, decides whether the first Back cancels, or repeats the current-folder shortcut policy.

Folder, Recent, and Trash task execution now share the same adapter entry point. Search session and Grid interaction retain their responsibilities; shared worker execution remains in Operations.

Tests preserve the existing App regressions for Sidebar activation, navigation races, refresh during a scan, selection, and scroll. Additional Navigation session tests cover:

- First Back cancels a pending request; the next Back uses history. This initially failed at the domain interface because the rule lived only in App.
- Sidebar tree loads and deferred refresh remain associated with the request that owns them; stale and duplicate completions cannot consume either.
- Only an idle, displayed folder can reuse its existing Sidebar tree children.
- Parent at the filesystem root and Forward without history still cancel an older pending request. Independent review caught this refactor regression; it was reproduced and corrected before the final validation.

## Transfer session

The queue is now private implementation under `src/app/transfer_session/queue.rs`. App can use the Transfer session's outcome, snapshot, and history types, but cannot bypass it to enqueue or resume private work.

Each Restore owns its foreground activity through submission, waiting in the queue, worker execution, conflict handling, Retry, and completion. The independent `restore_activity` field and `retry_is_restore` query are removed. The private queue constructs Retry with the same lifecycle guarantees as original submission, and the completion path retains activity while preparing the result.

This fixes a reproduced bug: submitting two Restores replaced the one session-level activity marker; completing the first cleared it while the second was still pending. New production-path tests verify both Restores stay covered and that submitted work retains activity after the session itself is dropped.

Test-only enqueue routes were removed. Transfer session tests and the App conflict test now run the real Iced task path for scheduling and continuation. Existing coverage still verifies retry validation, conflict cancellation, queue order, Trash metadata cleanup, and Undo preparation. Restore remains intentionally excluded from Transfer history. Native clipboard and drag adapter seams and Operations' worker-lane implementation are unchanged.

## Evidence

- `navigation-red.log` / `navigation-green.log`: moving first-Back cancellation to the Navigation session, plus navigation regression coverage.
- `navigation-cancel-red.log` / `navigation-cancel-green.log`: unavailable navigation still cancels superseded requests.
- `transfer-red.log` / `transfer-green.log`: two queued Restores retain foreground activity.
- `transfer-tests.log`: production-path Transfer regression checks.
- `release-gate.log`: 461 application tests passed, with seven benchmarks intentionally ignored in the normal suite; two vendored scrollbar tests and five real-X11 adapter tests also passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- `benchmarks.log`: all seven release-mode performance benchmarks passed their budgets.

All Restore fixtures use private temporary paths. No manual desktop interaction or live user Trash verification is claimed.
