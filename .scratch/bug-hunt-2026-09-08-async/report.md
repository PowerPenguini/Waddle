# Background work and folder-refresh bug hunt

Base: `2b0e07b` on `main`. Date: 2026-09-08.

Two additional defects were reproduced and fixed through separate red/green cycles. The scan examined background-operation lifetime, folder loading, refresh notifications, sort changes, and cancellation.

## Dropping an asynchronous task releases a still-running worker's queue slot

`Operations::schedule` held its semaphore permit and foreground activity marker in the asynchronous waiter. Dropping that waiter does not stop a Tokio blocking worker, but it released both guards. The operation appeared inactive and the shared mutation/command queue could start its next job while the first worker was still running.

The regression starts a real blocking worker, waits for its start signal, aborts its asynchronous waiter, and checks both foreground activity and whether a subsequent command enters before the worker is released. The before-fix run failed because the running mutation was reported inactive. The fixture always releases the worker before assertions so a failure cannot strand it.

The worker now owns both guards until its closure returns, including when the asynchronous waiter is dropped.

Test: `app::operations::tests::hunt_dropped_task_keeps_running_mutation_serial_and_foreground`.

## Refresh requests disappear during a folder scan

If a scan has read a folder and a new entry appears before its result is delivered, a Refresh request or filesystem notification is discarded while loading. The old snapshot then becomes the final listing. The minimized regression observed only `before.txt` instead of `after.txt` and `before.txt`.

The application now coalesces these requests into one pending refresh associated with the affected folder. Once navigation settles, it rescans if that folder is still displayed. Navigation to another folder discards the old request; explicit cancellation clears it. Sort changes during loading also receive the required follow-up scan.

Tests:

- `app::tests::navigation::hunt_refresh_during_a_folder_scan_does_not_lose_new_entries` covers manual Refresh and filesystem notifications.
- `app::tests::navigation::sort_change_during_a_folder_scan_reaches_the_displayed_entries` checks actual final entry order.
- `app::tests::navigation::deferred_refresh_cannot_undo_navigation_or_cancellation` checks navigation and cancellation boundaries.

These tests use temporary directories and execute the resulting Iced task outputs through the application message handler. The test-only `iced_runtime` dependency exposes that task runner and reuses the version already in the lockfile. Non-message UI effects are not simulated; these are application integration tests, not an interactive rendering check.

## Validation

- `worker-red.log` and `refresh-red.log` record failing reproductions before their respective fixes.
- `worker-green.log`, `refresh-green.log`, and `navigation-green.log` record passing checks.
- Full `release-gate.log`: 441 application tests passed; 7 performance benchmarks ignored by the normal test suite.
- Two vendored scrollbar tests and five real-X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- No user files, desktop settings, or real Trash contents were modified by the fixtures.
