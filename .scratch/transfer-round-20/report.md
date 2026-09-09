# Transfer round 20 — prepared-copy ownership after checkpoint errors

Status: resolved

## Confirmed defect

A failed history checkpoint left a complete unpublished copy that was absent from the persisted journal. After repairing journal-directory permissions and reopening history, Retry prepared a second copy and left the first behind. The original Journal regression failed with one unexpected target entry (`checkpoint-red.log`) and passed after the fix (`checkpoint-green.log`).

The copy engine previously returned immediately from every checkpoint error without knowing whether ownership of the prepared copy had been recorded. The history callback also retained in-memory publication state even when the checkpoint was rejected.

## Fix

Checkpoint failures now distinguish an unrecorded intent from an intent whose journal replacement succeeded but whose directory synchronization failed. This is an internal runtime distinction; the persisted journal schema is unchanged.

- Before journal replacement, failed checkpoints clear the publication/restoration intent, clean their prepared copy, and restore the earlier hardlink context. A same-filesystem Move never deletes its original source as cleanup.
- After journal replacement, a directory-open/fsync failure carries an explicit committed outcome. The prepared copy, publication intent, hardlink context, and running direction remain recoverable. A durability error must not cause deletion of data referenced by the committed checkpoint.
- This applies to Copy/Move history replay and the shared checkpoint path used by Trash Undo and Restore Redo.

## Validation

Tests use the existing public Journal boundary and temporary files:

1. Four Copy/Move × Undo/Redo cases with an unwritable journal directory. Cross-device Move exercises the prepared-copy path; source/destination entry counts are unchanged after failure, reopening and retry succeed, and no extra staging entries remain.
2. Isolated child-process fsync EIO cases before and after journal replacement. The existing syscall shim now supports exact-directory matching in addition to descendant-file matching. Each test verifies the injected failure was reached. Before replacement no prepared copy remains; after replacement exactly one remains, opposite-direction history is refused, and Retry publishes that same inode. Hardlinks remain intact, and a subsequent Undo succeeds.
3. Trash Undo and Restore Redo with a cross-device restoration target and failed checkpoint. Trash contents and receipt metadata survive the failure, staging is removed, and retry restores the file. Native Trash is replaced only by a private fixture. For Restore Redo, the original location initially lives on the Trash filesystem (preserving native Trash inode identity), then a symlink-backed location is replaced by a local directory to model a remounted destination before Redo. An earlier invalid fixture tried to model native Trash as a cross-device copy; production correctly refused that inode change, so the fixture was corrected without weakening the identity check.

Full release gate passed: 560 main tests, 14 ignored; 2 scrollbar tests; 5 real-X11 tests; formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke, and diff checks. The main suite includes one pending icon regression outside this commit. Tests and logs are retained here; the existing fault shim change is included in this round's commit.

## Remaining audit scope

This closes a reproduced orphaning path during reported checkpoint failures. It does not implement persistent ordinary Transfer queues or collection of copies abandoned by an abrupt exit before any checkpoint exists. Cleanup still depends on the target filesystem accepting removal; no power-loss guarantee or automatic collection of every abandoned temporary entry is claimed. The icon redesign remains separate and uncommitted. No release, push, or installation was performed.
