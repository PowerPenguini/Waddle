# Transfer audit — round 10

Status: resolved

## Confirmed bug

The completed-copy hardlink cache checked inode identity, size, modification time, permissions and extended attributes, but not content changes hidden by a restored modification timestamp. A file edited to the same length could still be reused by Retry. In the reproduced cross-filesystem Move, the pending source contained `original`, but its destination received `modified` from the edited earlier copy and the pending source was removed (`red.log`).

The regression separately edits the pending source and the completed destination, preserving size and modification time, for both Copy and Move.

## Fix

Cached links retain source and destination change times. Matching identity and metadata, including unchanged change times, retain the fast path. A changed or unavailable change time requires a byte comparison before reuse. Equal contents preserve valid hardlinks even after unlink/link operations changed change times; differing contents fall back to copying the pending source.

Comparison opens regular files without following a substituted final symlink or blocking on a FIFO. It checks open-file identity, size and change time before and after reading. It polls cancellation between bounded read chunks. Read errors disable reuse; cancellation propagates as cancellation. Verification does not add to copied-byte progress.

New cached change-time fields default to absent when reading older journal contexts; absence takes the comparison path. Publishing a staged file or creating another hardlink updates the stored destination change time after Waddle's own operation.

## Regression coverage

- Retry preserves the current source bytes after same-size, same-mtime edits to either side (Copy and cross-filesystem Move).
- Modified completed copies remain intact and are not linked to the new result.
- Cancellation during comparison leaves both sides unchanged, publishes no pending destination, and a later Retry still preserves the valid hardlink relationship.
- Existing hardlink, sparse file, permission/xattr, Transfer/Restore Retry and persisted Undo/Redo tests remain green.

Tests use isolated temporary files. The checks cover the reproduced edits between attempts; they do not make the entire copy/link syscall sequence atomic against concurrent filesystem writers.

## Validation

- Before fix: byte comparison regression failed with `modified` where `original` was expected (`red.log`).
- After fix: all four edit/action cases passed (`green.log`).
- Hardlink regression group: 15 passed (`hardlink-tests.log`).
- Full release gate and debug build: final results below.

Final validation: 516 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
