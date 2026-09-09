# Transfer round 26 — preserve selected sources before dependent writes

Status: resolved

## Confirmed defects

A multi-source Copy could overwrite a later selected source before reading it. Selecting an incoming file and the existing destination file produced a duplicate containing the incoming bytes instead of the selected original. The same pattern occurred when an incoming directory merge overwrote a later selected child. The original TransferBatch regression failed on the copied contents (`overlap-red.log`).

Reordering alone was insufficient: a failed preservation copy still allowed the later overwrite. A permission-failure regression demonstrated loss of the selected original after the dependency failed (`dependency-failure-red.log`).

## Fix

The initial TransferBatch plan identifies roots whose destinations cover other selected sources and schedules the readers first. Parent paths are resolved without following the selected leaf, so the plan does not mistake a symlink's referent for the selected entry. Same-folder duplication uses an unused name and does not create a destructive dependency. Unrelated roots retain their original relative priority.

Dependency state survives conflict pauses in the batch. Replace refuses to overwrite a selected source whose preservation failed or was retained/skipped. Keep Both and Skip remain available because they do not overwrite it. Retry rebuilds the dependency plan and can complete after permissions are repaired.

Cycles and roots depending on them are reported before their writes execute; unrelated roots can still transfer. This prevents destructive execution but does not yet implement cyclic transfers through snapshots (see remaining scope).

## Validation

Tests run through TransferBatch on real temporary files without touching user data.

- Copy preserves a selected same-folder source before replacing its path.
- Copy and Move preserve a separately selected child before merging an incoming directory over its parent.
- An unreadable selected source blocks a dependent Replace; both roots remain retryable. After repairing permissions, Retry preserves the original and completes the incoming write.
- Keep Both still completes the unrelated incoming copy when preservation of the selected source failed, without overwriting that source.
- Cyclic Copy/Move source overwrites leave both originals intact, report actionable errors, and allow an unrelated root to complete.
- Existing conflict, progress, copy, move, hardlink and history regressions remain part of the full validation suite.

Red/green logs and the complete release-gate output are retained alongside this report.

## Remaining audit scope

The overall transfer goal remains active. Cyclic dependency execution still needs staged source snapshots; this commit detects and prevents destructive cycles but does not claim to support their execution. Other remaining leads include overlapping parent/child Move selections and path traversal aliases changed by earlier roots. Planning describes the initial paths, not arbitrary external remounts during the batch. Previously recorded queue persistence, abandoned staging and physical-device durability limitations also remain open. Pending icon changes are excluded from the commit. No release, push or installation was performed.

Final gate: 575 main tests passed, 18 ignored; two scrollbar tests and five real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke and diff checks passed. The main count includes one pending icon test outside this commit.
