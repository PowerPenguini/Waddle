# 10: Metadata-faithful Copy

**What to build:** Preserve supported filesystem metadata during Copy and report every unsupported loss.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp.

**Status:** ready-for-agent

- [ ] Copy preserves timestamps, permissions, xattr, ACL, and symlinks where supported.
- [ ] Sparse layout and internal hardlink relationships survive where possible.
- [ ] Unsupported metadata appears in the Transfer report.
- [ ] Partial metadata support never hides a content-copy failure.
