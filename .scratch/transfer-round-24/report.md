# Transfer round 24 — ACL policy survives optional metadata failures

Status: resolved

## Confirmed defects

An error reading an optional user attribute caused the shared metadata copier to return before reconciling inherited ACLs. The later chmod applied the source group mask to the destination's inherited named-user entries, granting access that the source never granted. The original TransferBatch regression failed because the copied file retained the destination parent's named-user ACL (`acl-red.log`).

Actual ACL read/write/removal failures were also downgraded to optional metadata warnings. This could publish the wrong access policy and allow cross-filesystem Move to remove the original. Running the new fault regression against HEAD before the fix confirmed that a failed ACL read still produced a successful completed transfer (`acl-faults-red.log`).

## Fix

Access-control copying is independent of optional attribute enumeration and copying. The source access/default ACLs are queried by name and applied before chmod. If the source has no ACL, inherited ACLs are removed. A failed optional annotation read no longer prevents either operation. The existing attribute value reader is shared by full enumeration and direct ACL queries.

Genuine ACL read/write/removal errors now propagate through the copy operation. The existing staging cleanup handles them before publication, so Move retains the source and Retry can try again after the failure is repaired. Files, directories and symlinks all propagate this result. Optional metadata errors remain warnings.

A destination rejecting ACL support with ENOTSUP still permits the content transfer with a warning. The copier first clears any inherited ACL instead of retaining an unrelated destination policy. Absent/unsupported source ACLs continue to be treated as absent, and the existing non-Linux fallback is unchanged.

## Validation

Regression tests exercise TransferBatch with actual files on distinct /tmp and /dev/shm devices. The existing LD_PRELOAD fixture confines every fault to child processes and isolated temporary paths. It now supports selecting an attribute name and set-error code, and simulates a destination refusing inherited-ACL removal. Marker removal proves each injected failure was reached.

- Eight Copy/Move × file/directory × absent/explicit-source-ACL cases inject an optional annotation read failure. Contents, warnings, source retention/removal and exact resulting access/default ACL values are checked, including nested files.
- Sixteen Copy/Move × file/directory × ACL read/write/removal/unsupported cases check the error boundary. Genuine failures leave the source present, the destination unpublished, and no abandoned staging entries; an unfaulted retry succeeds. Unsupported destination ACLs leave no inherited named-user grant and report a warning.
- Existing ACL, symlink, hardlink, metadata-warning, staging, journal and transfer recovery tests remain in the full validation suite.

Red/green logs, the focused matrix and full automated release-gate output are retained in this directory.

## Remaining audit scope

This round protects against inherited ACL exposure during metadata failures. It does not promise identical ownership or access policy on filesystems that do not support source ACLs; those limitations are reported. The broader transfer audit remains active, including previously recorded durability and abandoned-staging limitations. Tests use syscall faults, not physical device removal or power loss. Pending icon changes are excluded from this commit. No release, push or installation was performed.

Final gate: 568 main tests passed, 18 ignored; two scrollbar tests and five real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke and diff checks passed. The main test count includes one pending icon test outside this commit.
