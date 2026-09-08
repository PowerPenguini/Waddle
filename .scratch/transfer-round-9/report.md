# Transfer audit — round 9

Status: resolved

## Confirmed bug

Journal Undo/Redo reused a hardlink context within one invocation but recreated it for every retry. After a partial Copy Redo or cross-filesystem Move Undo, repairing permissions and reopening the journal allowed completion but silently split the selected hardlinks into independent files. The original regression failed on differing final inode values (`red.log`).

## Fix

Transfer, Trash and Restore history actions now retain a serializable JournalTransfer context while incomplete. Journal's existing save-on-error path preserves that context. It is cleared after the action fully succeeds, before the cursor advances.

The hardlink map serializes as a sequence because JSON object keys cannot hold device/inode tuples. Cached destination paths use the existing byte-preserving path serializer. New action fields default to an empty context for older journal files.

Retaining the context also requires checking previously completed Move Undo results before reusing them. A follow-up regression during implementation edited a restored file while preserving its size and modification timestamp. The first implementation accepted it (`changed-red.log`); the final implementation verifies the recorded recursive content fingerprint and refuses to continue with altered data (`changed-green.log`). Copy Redo and Trash Undo already perform equivalent completed-result verification.

## Regression coverage

- Partial Copy Redo and cross-filesystem Move Undo preserve hardlinks after permission repair and journal reopen.
- The history retry uses a non-UTF-8 filename, covering serialized cached paths.
- Move Undo refuses an externally edited completed file even when its size and timestamp match; pending original data remains intact.
- Trash Undo preserves hardlinks after physical restoration succeeds but Trash metadata cleanup fails, followed by permission repair and journal reopen.
- Older Transfer history without the new context field still supports Undo and Redo.

All filesystem effects use isolated temporary directories. The Trash fixture does not use desktop Trash.

## Validation

- Primary regression failed before the fix (`red.log`) and passed after (`green.log`).
- Completed-result integrity regression failed during implementation and passed with content verification (`changed-red.log`, `changed-green.log`).
- Journal tests: 37 passed (`journal-tests.log`).
- Full release gate and debug build: final results below.

The context is saved when an operation returns an error. This round does not make journal mutation durable against power loss or a process crash before that save.

Final validation: 514 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
