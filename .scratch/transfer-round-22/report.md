# Transfer round 22 — hardlinks after incomplete metadata preservation

Status: resolved

## Confirmed defect

Copying two hardlinked source files to a destination that rejected a user attribute created two independent destination files. The content transfer succeeded and reported the metadata warning, but silently lost the hardlink relationship. The original TransferBatch regression failed with different destination inode numbers; see `hardlink-red.log`.

The hardlink cache compared both source and destination metadata against the destination snapshot. A legitimately missing destination attribute therefore invalidated an otherwise reusable copy. This was a cache eligibility failure, not a failed hard_link syscall or an invalid publication path.

## Fix

CopiedLink now records the source's metadata separately from the destination's metadata. Each side must still match its own snapshot before the completed copy can be reused. Device/inode, file type, size and destination identity checks remain in place. The ctime fast path and cancellable byte comparison still protect against same-size, same-mtime content edits.

Source metadata is serialized with the copy context, so Journal retry can retain the relationship after reopening. Old journal entries without the new snapshot retain the previous conservative eligibility checks. An unreadable attribute snapshot still disables reuse; no read error is treated as proof that metadata is unchanged.

## Validation

The regression uses real files through TransferBatch, with the existing child-process syscall fixture rejecting `user.comment` only below an isolated destination. A marker confirms the fault was reached.

- Copy and cross-filesystem Move preserve the relationship after a conflict, cancellation and Retry when neither side was edited.
- External attribute edits on either the pending source or completed destination prevent reuse. The matrix covers both transfer kinds and all three edit states on distinct /tmp and /dev/shm devices.
- The metadata warning is present in the report for the attempt that copied the first file.
- Copy Redo fails on the second destination after completing the first copy with a metadata warning. A new process reopens the journal and completes Redo, preserving both the hardlink relationship and the warning.
- Existing hardlink tests cover source permissions and attributes changed during conflicts, same-size/same-mtime byte edits, cancellation during verification, merged transfers, cross-device history and Trash restoration.

Focused suite: 17 passed, one subprocess helper ignored by the ordinary harness and explicitly invoked by its parent test. Full validation is recorded in `release-gate.log`.

## Remaining audit scope

This round fixes reuse after metadata writes fail while attributes remain readable. A filesystem rejecting attribute enumeration altogether remains a lead for the next round; the cache currently declines reuse when its attribute snapshot cannot be read. No claim is made that every filesystem preserves hardlinks or all metadata.

The broader audit remains open, including previously recorded queue persistence and abandoned-staging limitations. Pending file-icon changes are excluded from this commit. No release, push or installation was performed.

Final gate: 564 main tests passed, 16 ignored; two scrollbar tests and five real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke and diff checks passed. The main test count includes one pending icon test outside this commit.
