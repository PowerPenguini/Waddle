# Copy Undo rejects a folder after child Rename Undo

Status: ready-for-agent
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
