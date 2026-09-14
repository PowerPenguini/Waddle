# Copy Undo rejects a folder after child Rename Undo

Status: resolved
Type: bug
Blocked by: none

Copy a folder containing `nested/file.txt`, record the Copy, rename the copied
file to `nested/renamed.txt`, and record the Rename. Undo Rename, then Undo Copy.
The second Undo refuses the copied folder as changed even though the child has
its original name and contents.

## Evidence

A temporary nested-folder variant of
`copy_rename_history_rebinds_only_journal_recreated_files` failed at Copy Undo
with `Refused operation: .../destination or its contents changed after it was
recorded`. The retained test covers a copied file instead, so this folder case
still needs its own regression and fix.

Investigate directory metadata changed by the journal's child Rename. Preserve
the refusal when a user changes actual contents or replaces a file externally.

## Answer

Round 65 adds a persisted digest that omits directory timestamps and storage
size while retaining entry names, file contents and timestamps, permissions,
and attributes. New records use this digest, so restoring the child's original
name allows Copy Undo. Older records retain their complete metadata checks.

The journal regression covers Copy, child Rename, both Undo operations, restart,
both Redo operations, and another Undo cycle. Separate regressions retain
content, file-timestamp, and directory-permission protection and verify legacy
folder records. All 667 tests passed, with 24 opt-in tests ignored.
