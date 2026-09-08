# Transfer audit — round 8

Status: resolved

## Confirmed bug

Retry rebuilt a TransferBatch using only pending source/destination paths. It discarded the map connecting source hardlinks to files already copied by the earlier attempt. After the first selected hardlink completed and a later one was cancelled at a conflict, Retry created an independent file for the later name. Contents matched, but inode identity differed and the hardlink relationship was lost.

The Queue regression creates two selected hardlinks, transfers the first, cancels at the second conflict, removes the conflicting destination and invokes Queue Retry. Before the fix the final inode comparison failed (`red.log`). It covers Copy and cross-filesystem Move using isolated temporary directories on distinct devices.

## Fix

The filesystem report retains the completed-copy link context. It produces an opaque TransferRetry plan containing exact pending paths and that context. Transfer and Restore retries use the plan to construct the next batch, validating paths at retry time and reusing the existing metadata/identity checks before linking to a completed copy.

The Queue does not inspect or reconstruct hardlink bookkeeping. Ordinary newly created batches still start with an empty context.

## Regression coverage

- Copy and cross-filesystem Move preserve hardlinks across cancellation and Retry.
- Restore Retry preserves hardlinks to files already restored across filesystems.
- Editing either the pending source or the completed copy prevents reuse of that stale copy; both versions retain their expected bytes and separate inodes.
- Existing exact nested retry destinations, partial Restore cleanup, cancellation and progress tests remain green.

The retained plan belongs to the live Transfer session, matching existing Retry lifetime. This round does not add queue persistence across application restarts or change partial Undo/Redo retry bookkeeping in the Journal.

## Validation

- Before fix: focused Queue regression failed on inode inequality (`red.log`).
- After fix: Copy and Move cases passed (`green.log`).
- Retry regression group: 10 tests passed (`retry-tests.log`).
- Full release gate and debug build: final results below.

Final validation: 510 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
