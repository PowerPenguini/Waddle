# Transfer round 17 — permissions during and after copying

Status: resolved

## Confirmed defects

1. A file copied into a directory with a default POSIX ACL retained named-user access absent from its source. Source and destination both reported mode 0640, but the destination additionally granted the inherited named user read access. `cargo test copying_into_default_acl -- --nocapture` failed before the fix (`acl-red.log`) and passed after it (`acl-green.log`). Copying xattrs only assigned source attributes, so an absent source ACL never removed the inherited destination ACL. The copier now removes inherited access/default ACLs absent from the source, preserves explicit source ACLs, and leaves unrelated filesystem-assigned attributes alone. Unsupported or absent ACL removal is harmless; other failures remain reported through the existing metadata-warning mechanism.
2. Private input was temporarily copied into staging files/directories created with default permissions, before source permissions were applied. The regression observed group/other read bits on an incomplete copy of a 0600 file (`private-red.log`). Staging files are now created with mode 0600 and directories with 0700. ACL reconciliation runs before final source permissions are opened, so changing the group mask does not briefly enable an unwanted inherited named-user entry.

## Validation

The existing TransferBatch interface is the test boundary; actual temporary files and Linux POSIX ACL xattrs are used without invoking setfacl or changing user files. Tests cover:

- A regular copy with matching Unix mode bits but an unwanted inherited ACL.
- Sixteen combinations of file/directory, Copy/cross-device Move, fresh destination/Replace, and ordinary/default-ACL target parents. Progress callbacks inspect the unpublished entry while data is still being written, then verify final content and private permissions.
- Eight combinations of Copy/cross-device Move, fresh destination/Replace, and absent/explicit source ACLs. Checks include access ACLs, directory defaults, final mode/content, source retention/removal, and default ACL inheritance for newly created children.

An initial matrix expectation incorrectly treated directory-on-directory Replace as complete replacement. Waddle defines that case as Merge, which retains the existing destination directory. The corrected replacement fixture uses a file conflict to exercise whole-entry replacement; no Merge policy was changed.

Full release gate passed: 549 main tests, 13 ignored; 2 scrollbar tests; 5 real-X11 tests; formatting, strict Clippy, release build, FileManager1, desktop/AppStream validation, and archive smoke. The worktree includes the user's pending icon redesign (one additional main test and one opt-in GPU test); this round's commit stages only filesystem code, its three regression tests, and this report/logs.

## Scope and remaining work

These fixes apply to the shared copy engine, including staging for replacement, cross-filesystem moves, and history replay. The explicit matrix exercises TransferBatch; the full suite also checks journal operations. Symlinks continue to use no-follow metadata calls, and existing hardlink, sparse-file, cancellation, and metadata-preservation regressions pass.

This is progress toward the transfer audit, not proof that every transfer bug is eliminated. Persistent resumption of ordinary queued Transfers and collection of abandoned pre-checkpoint staging files remain open capabilities recorded in round 16. ACL failures on filesystems that reject metadata changes still surface as warnings; this round establishes successful ACL reconciliation on supporting filesystems, not a guarantee of exact permissions on every target filesystem. No release, push, or installation was performed in this round.
