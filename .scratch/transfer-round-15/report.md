# Transfer round 15 — source consistency and recoverable Copy/Move history

Status: resolved

## Fixes

1. Ordinary Copy and Replace Copy capture and verify the source tree before publishing the prepared destination. Verification checks device/inode/type, size, mtime and ctime throughout the captured tree. A changed source refuses publication and retains the original destination and source. This covers the dense and sparse mixed-version reproductions, same-size edits with restored mtime, and a directory changed during Copy.
2. Copy/Move history persists a publication intent before renaming the result into place. It contains the prepared pathname, identity and fingerprint, and (for Move) a source cleanup plan. A failed post-effect fingerprint read leaves this intent recoverable. Recovery requires the recorded inode and verified contents; a foreign destination with identical bytes and mtime is refused. An in-progress direction marker prevents crossing into unrelated history even between the final per-item checkpoint and cursor advancement.
3. Copy Undo persists its removal plan before deleting files. Recovery reconciles already absent entries, then verifies retained identities, file contents, and exact directory membership before further deletion. Move recovery uses the same mechanism after interrupted cross-device source cleanup. A recreated same-filesystem Move source is refused rather than deleted.

Journal checkpoints are atomically replaced after flushing the file; the containing directory is flushed too. Active history is protected against age/count pruning. New fields have serde defaults so prior history can still be read. These changes do not make old, already stranded history reconstructible retroactively.

## Evidence and tests

The previous audit's failing reproductions were rerun before implementation (`copy-red.log`, `history-red.log`). After the changes, the original Copy matrix and history fault matrix passed (`copy-green.log`, `history-green.log`).

The formerly failing reproductions are now ordinary regression tests. The only newly ignored test is the subprocess entry point, explicitly run by the parent tests. Faults are confined to child test processes and temporary files; the tests automatically compile their small C interposition library with `cc`. No user's Trash, mounted drive contents, or live application state is modified.

Additional checks cover:

- A destination substituted after interruption with identical contents and restored mtime is refused, and restoring the owned inode makes recovery possible.
- Cross-device Move interrupted after a source child was unlinked resumes after reopening, preserves hardlinks, and refuses cleanup while an unrelated new child exists.
- Interruption after the write-ahead journal rename but before visible result publication recovers Copy and Move correctly.
- The Copy Undo exit hook waits until the physical deletion happened, so the new pre-effect checkpoint does not accidentally move that test's fault earlier.
- Copy detects size/mtime-preserving edits and additions to a directory during copying.
- Existing partial Undo/Redo, history branching, non-UTF8 paths, hardlinks, replacement refusal, and concurrent-window journal tests continue to pass.

## Scope and limits

These fixes address the three demonstrated defects and process-interruption recovery for Copy/Move Undo/Redo. Tests terminate child processes without unwinding; they do not simulate power loss or filesystem/device failure. A persistent queue for brand-new Transfers, crash recovery of Trash/Restore, and removal of temporary copies abandoned before their publication intent was persisted are separate scopes. Source validation detects mutations during the observed copy interval; it is not a filesystem snapshot or a lock excluding concurrent writers indefinitely.

No release or installation was performed. Installed 0.0.8 remains unchanged. Final release-gate results are in `release-gate.log`.

Final validation: 542 main tests passed, 9 ignored (8 pre-existing plus the child-process helper); 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream metadata, archive smoke, and diff checks passed.
