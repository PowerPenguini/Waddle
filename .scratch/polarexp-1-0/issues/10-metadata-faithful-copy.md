# 10: Metadata-faithful Copy

Type: feature

**What to build:** Preserve supported filesystem metadata during Copy and report every unsupported loss.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp.

**Status:** resolved

- [x] Copy preserves timestamps, permissions, xattr, ACL, and symlinks where supported.
- [x] Sparse layout and internal hardlink relationships survive where possible.
- [x] Unsupported metadata appears in the Transfer report.
- [x] Partial metadata support never hides a content-copy failure.

## Answer

Copy now has one tree-wide context. It reproduces symlinks without following them, tracks source
device/inode pairs to rebuild internal hardlinks, copies data extents with `SEEK_DATA` and
`SEEK_HOLE`, and then applies permissions, Linux xattrs (including POSIX ACL xattrs), and access
and modification timestamps. Unsupported metadata is returned as a warning attached to the
completed Transfer; a content-copy error still removes the incomplete root and is reported as a
failure.
