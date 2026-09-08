Waddle 0.0.9 is a preview release focused on safer transfers and recoverable Undo and Redo.

- Interrupted Copy, Move, Trash, and Restore history operations can resume after a process restart. Completed work is checkpointed, and recovery verifies file identity before continuing.
- Copy and replacement operations detect source changes before publishing the destination, including sparse files and directory trees.
- Copied files are synchronized before publication. Reported delayed storage errors stop replacement or cross-filesystem source removal while preserving existing data.
- Partially completed Redo remains recoverable when newer operations are recorded. Failed Trash batches preserve completed entries and retry safely.
- Trash receipts identify the actual moved file even when several entries share the same original path.

Validation includes regression tests for abrupt process exits, source changes, and injected storage errors, plus the automated release gate and package smoke checks.

This remains a preview. Ordinary active and queued transfers do not persist across process restarts, and temporary files abandoned before an operation checkpoint are not automatically cleaned up. Physical power-loss recovery and the complete cross-desktop interoperability matrix are not established by these tests.

The Linux archive and Flatpak bundle are built by GitHub Actions from the release tag. SHA-256 checksum files accompany both packages.
