# Live byte progress for Transfers

Reported: copying one 6.1 GiB file displayed `0 B`, `0 B/s`, and `ETA --` until the file completed. Requested coverage across Transfer kinds.

## Cause and implementation

`TransferBatch` previously credited an entire source root only after all its work completed. The underlying filesystem copy never published intermediate bytes. The existing UI speed and ETA calculations therefore had no useful input during a large-file copy.

The shared copy implementation now reports cumulative logical bytes after each completed block of at most 1 MiB. It retains `io::copy`, private staging and atomic publication, sparse extents, hardlinks, symlinks, and metadata preservation. Sparse-copy fallback uses a high-water mark so recopying an extent cannot count the same bytes twice.

Byte callbacks pass through ordinary Copy, cross-filesystem Move, Replace, Keep Both, and directory merges. The Transfer batch accumulates progress across roots and retains it when a conflict pauses the batch. Clipboard, internal drag, incoming native drag, and Retry all use this batch. Restore uses its mapped Move path and receives the same byte updates. Existing queue snapshots feed the UI's bytes, speed, and ETA without a separate UI reporting path.

Same-filesystem Move publishes logical bytes after the atomic rename; merged directories publish updates between child moves. Trash uses GIO's metadata operation, which has no intermediate byte-copy callback: it reports completed entries and their logical size after each successful operation. Trash now measures directory contents in its worker when execution starts, instead of treating absent listing metadata as zero. Skipped filesystem entries and failed Trash operations no longer inflate transferred bytes.

## Regression evidence

- `copy-red.log`: a real 4 MiB copy only produced zero and final-byte updates before the fix.
- `merge-red.log`: merging a directory on the same filesystem did not report completed child moves.
- `trash-size-red.log`: a queued directory with no listing-size metadata incorrectly reported zero bytes.
- `skip-red.log`: a skipped conflict was incorrectly credited as transferred bytes.
- `trash-failure-red.log`: failed Trash operations incorrectly increased transferred bytes.

Additional filesystem tests exercise Copy and cross-filesystem mapped Move (also used by Restore), with ordinary destinations, Replace, and Keep Both. Fixtures use distinct filesystems in the system temporary directory and `/dev/shm`; successful destination contents and source retention/removal are checked. Sparse files, hardlinks, and symlinks have monotonic logical-byte coverage. Existing tests retain cancellation, Retry, Restore receipt cleanup, queue serialization, collision handling, and metadata preservation coverage.

Tests use isolated temporary files. No user transfer was interrupted and no live pendrive data was modified. This changes byte reporting, not the existing cancellation granularity.

## Validation

- Full release gate: 476 application tests passed, eight opt-in tests excluded from the ordinary suite; two vendored scrollbar checks and five real-X11 adapter checks passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and archive smoke tests passed.
- `benchmarks.log`: all seven release-mode interaction benchmarks passed their budgets.
