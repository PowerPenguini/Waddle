# Transfer audit — round 11

Status: resolved

## Confirmed bug

Concurrent copies in the same process could choose the same temporary path because staging-name allocation restarted at nonce zero and only checked existence before copying. One operation could remove or publish the other operation's staging entry. A deterministic two-thread TransferBatch regression held one copy after its first write and reproduced a `No such file or directory` failure in the other operation (`red.log`).

## Fix

Staging names use a process-wide atomic sequence, so concurrent operations receive distinct candidates before either creates an entry. Existing paths are still skipped.

Cleanup for a failure during copying now belongs to the copy engine, which knows whether this attempt successfully created its root. Failed exclusive creation does not authorize deletion of an entry that already exists. The publication and replacement paths still clean copies they successfully created and preserve their existing backup recovery behavior.

Staging stays beside the final destination, preserving the existing behavior for read-only directories. No permissions are changed on unrelated pre-existing paths.

## Regression coverage

- Two overlapping transfers into one directory complete with exact bytes and no staging leftovers: Copy and cross-filesystem Move.
- Move source entries are removed only after successful transfer; Copy source entries remain.
- Pre-existing files, directories and symlinks using a staging-like name survive a failed copy.
- Existing read-only-source, read-only-descendant cleanup, Replace recovery, cancellation, maximum-length filename and hardlink tests pass.

The two-thread test controls the schedule through the existing progress callbacks and bounded channels. All filesystem data is isolated in temporary directories.

## Validation

- Concurrent-copy regression failed before the fix (`red.log`).
- Final implementation passes both Copy and Move scenarios (`green.log`).
- Filesystem tests: 86 passed (`fs-tests.log`).
- Full release gate and debug build: final results below.

Final validation: 518 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
