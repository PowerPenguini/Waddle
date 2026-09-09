# Transfer round 21 — metadata warnings in Undo/Redo

Status: resolved

## Confirmed defect

History replay discarded metadata warnings returned by the shared copy engine. When a target rejected a user attribute, Copy Redo completed the file content correctly but returned only `Redid Copy`, concealing the lost annotation. The Journal regression failed before the fix (`warning-red.log`) and passed afterward (`warning-green.log`).

## Fix

JournalTransfer now retains warnings alongside its hardlink context. Each warning identifies the destination path. They are checkpointed before publication, restored after restart, and included in the final history effect's status. The existing application integration already displays that status. The success verb is finalized before warnings are appended, so Restore's label no longer risks replacing the warning text.

Rejected, unrecorded checkpoints roll back newly prepared warnings with the discarded copy and hardlink state. Completed operations consume their warnings and reset the context, so later successful Undo/Redo does not repeat stale messages. The persisted field defaults to an empty list for older journals.

The content transfer remains successful when only metadata cannot be preserved; the warning describes the limitation rather than incorrectly making the completed transfer retryable as a content failure.

## Validation

Tests use the public Journal interface and actual temporary files. The existing syscall fixture now interposes lsetxattr only for `user.comment` below one isolated target directory, returning ENOTSUP. A separate marker proves injection was reached; no system configuration, real desktop Trash, or user files are modified.

- Ordinary Copy Redo reports the rejected attribute and preserves file contents.
- A process exits after checkpoint publication but before the copied file is published. Reopening and resuming retains the metadata warning from the checkpoint.
- A subsequent successful Undo/Redo does not repeat old warnings and successfully preserves the source annotation.
- Cross-device Move Undo, Move Redo, Trash Undo, and Restore Redo all report the warning with the correct operation label and destination path. Source cleanup and final contents are checked. Restore Redo models a remounted destination with a private symlink-backed location, as in round 20; the fixture's native Trash action preserves inode identity on its own filesystem.

Full automated release-gate output is retained in `release-gate.log`. Pending icon changes are not staged with this round.

## Remaining scope

This fixes the reproduced reporting omission in history replay. It does not claim exact metadata preservation on filesystems that reject it, persistent ordinary Transfer queues, or automatic collection of all abandoned copies. The larger transfer audit remains open. No release, push, or installation was performed.

Final gate: 562 main tests passed, 15 ignored (including the explicitly invoked new metadata-fault helper); 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1, desktop/AppStream validation, archive smoke and diff checks passed. The main count includes one pending icon regression outside this commit.
