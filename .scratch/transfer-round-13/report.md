# Transfer audit — round 13

Status: resolved

## Confirmed defects

1. Recording a new action after a partial Redo truncated the partially applied entry. Its completed filesystem effects survived without the history required to resume or undo them. The Journal regression failed with `incomplete transfer was lost: Nothing to undo` (`history-red.log`).
2. Trash Redo (also Restore Undo) attempted to roll back completed Trash entries on a later failure. It ignored failed rollback moves and removed their recovery metadata anyway. The Journal regression failed with `failed rollback must not orphan the first Trash entry` (`trash-red.log`).

## Changes

- Retain an already started Redo below newly recorded actions, with a persisted `redo_pending` marker. Newer actions can be undone first; Redo then resumes the retained action without advancing past the newer history. Undo cannot cross it while it remains incomplete.
- Persist each successful Trash receipt with a `trash_pending` marker. A later failure preserves the physical file and its metadata. Retrying checks completed files for changes, skips them, and continues the remaining entries. Opposite-direction history remains blocked until completion.
- Both markers default to false when reading older history. No migration is required.
- Move wrappers used only by the removed rollback and by tests are now test-only.

## Regression coverage

The tests exercise the public Journal boundary, persisted history, and real temporary files. Copy and Move tests cover a partial Redo, a newer rename of the completed destination, another new action, reopening, undoing those newer actions, another failed retry, reopening again, successful retry, and subsequent ordinary Undo/Redo.

Trash Redo and Restore Undo tests force a second-entry desktop Trash failure plus a permission change preventing rollback of the first entry. They check retained metadata, a newer action, reopening, refusal after editing a completed Trash file, opposite-direction guards, retry, and recovery of both originals.

Only the external desktop Trash service is substituted by a thread-local test backend. It moves real files and writes recovery metadata in the fixture; no test touches the user's desktop Trash. This deterministically exercises the Journal failure path without claiming a real GIO interoperability test.

## Validation

Both reproductions failed before their corresponding fixes and passed afterward. The expanded Journal suite passes all 42 tests. Full release-gate results are recorded in `release-gate.log`.

Final gate: 523 main tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream metadata validation, archive launch smoke, and diff checks passed.

Remaining audit areas are unchanged: interrupted-process durability between effects and journal persistence, source edits during ordinary Copy, and failures when reading fingerprints after an effect. This round does not claim those are fixed. The installed 0.0.8 release is unchanged.
