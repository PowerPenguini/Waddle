# Transfer round 25 — same-folder duplication and remembered conflict choices

Status: resolved

## Confirmed defects

1. Copy into the source's own parent through a symlink alias paused for a conflict instead of creating a duplicate. The same-folder check compared parent path spelling, so the existing automatic Keep Both behavior was bypassed. The TransferBatch regression failed on the unexpected conflict (`alias-red.log`).
2. A remembered Replace-all or Skip-all choice took precedence over same-folder duplication. In a multi-source Copy, Replace-all could replace an original with a copy of itself, change its inode and produce no second entry; Skip-all could omit the duplication. The regression failed because the original inode was replaced (`remaining-red.log`).

## Fix

Copy compares parent directory device/inode identities when their path spellings differ. This recognizes aliases without mistaking different-parent hardlinks for the same folder. Automatic same-folder duplication now takes priority over a remembered conflict decision. The remembered decision remains intact for subsequent genuine conflicts.

## Validation

Tests exercise TransferBatch and the public Journal API using temporary files.

- Files and directories copied into a parent reached through a symlink or a path containing `..` create independent duplicates without prompting. Original contents and inode are preserved. Reopened Journal Undo removes only the duplicate; reopened Redo recreates it. Editing the duplicate afterward does not alter the original.
- Replace-all and Skip-all matrices include a real conflict, a same-folder Copy through an alias, and another real conflict. The original remains unchanged, its duplication produces a separate non-replacement receipt, and the last conflict still obeys the user's remembered decision.
- Hardlinked entries in different parent directories still present a genuine conflict for both Copy and Move. Choosing Skip preserves the entries without manufacturing duplicates.

The initial red/green logs and the final automated release-gate log are retained here. The gate caught an unused matrix selector in the test fixture; the fixture was corrected to actually exercise the `..` spelling and the complete gate was rerun.

## Remaining audit scope

The broader transfer audit remains active. This round covers static directory aliases and conflict-choice ordering, not remounts during an operation or physical power loss. Overlapping source selections whose earlier destination is a later source remain an audit lead. Previously recorded durability and abandoned-staging limitations remain open. Pending file-icon changes are excluded from this commit. No release, push or installation was performed.

Final gate: 571 main tests passed, 18 ignored; two scrollbar tests and five real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke and diff checks passed. The main count includes one pending icon test outside this commit.
