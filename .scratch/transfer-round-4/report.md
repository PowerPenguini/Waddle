# Transfer audit — round 4

Status: resolved

## Confirmed bug

After Copy Redo completed one entry and failed on another, it attempted a best-effort recursive rollback. If cleanup failed (for example because the copied directory contained a read-only subdirectory), the completed destination remained. The next Redo required every destination to be absent and refused its own result, including after reopening the journal. Rollback errors were ignored.

The regression uses the public Journal interface and real files in an isolated temporary directory. It performs Copy, partial Undo plus permission repair, Redo with a non-writable second destination, permission repair, journal reopen and Redo retry. Before the fix it failed with `Refused operation: .../target-one/copy now exists`.

## Fix

Redo retains completed entries instead of rolling them back after a later error, matching the existing partial Undo behavior. It verifies retained destinations against recorded fingerprints, verifies pending sources and absent destinations, and executes only pending entries. Existing save-on-error persistence records progress. Successful completion advances the journal cursor as before.

## Regression coverage

- The original read-only-directory failure and recovery after reopening the journal.
- Copy and Move retries after repairing a later destination's permissions.
- Completed destination inode remains unchanged during retry.
- An external edit to a completed destination blocks retry and remains intact; pending entries are untouched.
- Full Undo and Redo still work after completing a partial Redo.

## Validation

- Before fix: focused regression failed as expected (`red.log`).
- After fix: focused regression passed (`green.log`).
- Journal tests: 33 passed (`journal-tests.log`).
- Full release gate and debug build: see logs and final result below.

This change covers retries after a returned operation error. It does not add persistence during a process crash or power loss, nor change older journals left with inconsistent state by the previous rollback implementation.

Final validation: 501 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Strict Clippy, formatting, release build, FileManager1 smoke, desktop metadata, packaged application smoke and diff checks passed. Debug build also passed. The running application was not restarted.
