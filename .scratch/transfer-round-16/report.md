# Transfer round 16 — Trash recovery and delayed storage failures

Status: resolved

## Confirmed and fixed

1. Trash/Restore Undo and Redo could lose their recoverable state when the process exited after the physical move. The new regression reproduced this before the fix (`trash-red.log`). Restoring now uses the existing prepared-publication checkpoint and cleanup plan. Trashing records the source identity before invoking GIO; after interruption it locates and verifies the matching Trash receipt. Per-item checkpoints and the existing running-direction marker prevent crossing incomplete history. The regression exercises all four directions in isolated child processes (`trash-green.log`).
2. The copy engine published buffered file data without checking delayed write errors. A deterministic fsync fault fixture showed success despite a simulated ENOSPC condition (`storage-red.log`). Regular files are now synchronized while still staged, after their metadata is applied. Failure aborts before publication and flows through owned-staging cleanup. Eight combinations pass: ENOSPC/EIO, Copy/cross-device Move, with/without Replace. Source data and existing destinations are preserved and no staging entry remains (`storage-green.log`). This synchronization is required work and can add latency on slow media.
3. Trash receipt lookup could select an older entry for the same original pathname based on metadata timestamps. A real GIO subprocess with an isolated XDG data directory returned the older file in the fixture (`receipt-red.log`). Receipt discovery now also checks the moved source's device, inode and file type. Recovery additionally checks the saved fingerprint. The native GIO regression passes (`receipt-green.log`).

The Trash test substitutes only the desktop service during forced interruption. A separate receipt test uses real GIO in a temporary directory on the home filesystem, with XDG_DATA_HOME redirected into that same fixture. No user's actual desktop Trash or removable drive is used. The storage tests interpose fsync only on file descriptors below the exact temporary destination directory, and assert that the injected failure was reached.

## Other audited areas and limits

- Queue state is explicitly volatile: `Queue::open` reconstructs completed display history and starts with no active or pending work; `Queue::save` writes only completed HistoryEntry values. Durable resumption of brand-new queued Transfers remains an unimplemented capability. These changes do not add a persistent queue.
- Reported Copy/Move failures now include delayed storage failures and safely clean their owned staging paths; the eight storage cases assert that cleanup. Abrupt process termination before a publication intent exists can still leave an unpublished temporary file. Automatic collection of such files needs a durable ownership registry and is not implemented here; scanning names and deleting them would not establish ownership.
- EIO and ENOSPC are simulated at the filesystem call boundary. Actual power loss, loss of device caches, and physical drive disconnection are not simulated. The tests establish handling of reported storage errors, not a general power-loss guarantee.

This audit therefore closes the three reproduced defects above, not every restart/durability limitation. No release or installation was performed; installed 0.0.8 is unchanged.

## Validation

Original failing probes and green results are retained alongside this report. The regressions run with the ordinary test suite; only their subprocess entry points are ignored for direct invocation. Final validation is recorded in `release-gate.log`.

Final gate: 545 main tests passed, 12 ignored (8 pre-existing plus 4 explicitly invoked subprocess helpers); 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke, and diff checks passed.
