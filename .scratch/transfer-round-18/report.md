# Transfer round 18 — extended metadata in history verification

Status: resolved

## Confirmed defects and fixes

1. Copy Undo deleted a result after an external edit to its `user.comment` attribute. The content and mtime were unchanged, so the old tree fingerprint accepted the edit as unchanged. The public Journal regression failed before the fix (`metadata-red.log`) and passed after it (`metadata-green.log`). Tree fingerprints now retain a separate, optional digest of extended attributes, including ACLs. Names, values, and relative paths are length-framed and attributes are sorted by the existing filesystem reader. Attribute-read errors prevent unverifiable replay; unsupported attributes are treated as absent.
2. Resuming Copy Undo after a permission-related interruption could delete a newly annotated directory. The removal plan stored directory identity and membership, but no directory attributes. The regression failed with the first fix in place (`directory-red.log`). New removal plans now capture and verify directory attributes too (`directory-green.log`), before removing any remaining entries. File cleanup fingerprints also protect attribute edits.

## Compatibility and permission repair

The original content digest format is unchanged. Attribute digests and directory-attribute fields are optional in persisted records. Legacy fingerprints continue using their original checks, rather than being compared to a new digest they never recorded. A reopen/Undo/reopen/Redo regression covers legacy Copy and Move records.

Partial cleanup intentionally permits permission repairs as before: its fingerprints omit mode bits and POSIX access/default ACLs, but still verify other attributes. Ordinary untouched history entries include ACLs, so a named-user access change with the same 0640 mode is detected. This distinction keeps interrupted cleanup recoverable after chmod/ACL repair without dropping annotations. Old records cannot retrospectively protect attributes that were never captured.

## Coverage

Tests use the established Journal boundary, real temporary files, actual Linux xattrs, and persisted/reopened journals. No user files or desktop Trash are modified.

- The original edited-file Copy Undo reproduction.
- A failed and restarted Copy Undo with a new directory annotation; data remains present when replay is refused.
- Sixteen Copy/Move × Undo/Redo × changed file annotation/removed annotation/new directory annotation/ACL cases. File content, Unix mode, and mtime stay unchanged in the fixture.
- Legacy Copy and Move history replay after removing the newly introduced optional JSON fields.
- All four Trash/Restore × Undo/Redo cases after metadata edits. The Trash service is replaced by an isolated fixture that only renames files inside its temporary directory.

The full release gate covers existing hardlink, sparse-file, permission-repair, process-interruption, cancellation, and transfer regressions in addition to the new tests. It also runs strict Clippy, formatting, release build, FileManager1 activation, real-X11 tests, desktop/AppStream checks, and archive smoke. Final gate output is retained in `release-gate.log`.

The worktree still includes the user's pending icon redesign. This round commits only filesystem attribute-reader exposure, journal verification/removal changes, their tests, and this audit's artifacts.

## Remaining scope

This closes two reproduced verification omissions, not the whole transfer audit. Ordinary Transfer queue persistence and collection of abandoned pre-checkpoint staging files remain open as recorded in round 16. Legacy history retains its older, weaker metadata checks; non-Linux builds and physical power-loss behavior were not tested. No release, push, or installation was performed.

Final gate: 554 main tests passed, 13 ignored; 2 scrollbar tests and 5 real-X11 tests passed. The main count includes one pending icon regression outside this commit. All automated release checks passed.
