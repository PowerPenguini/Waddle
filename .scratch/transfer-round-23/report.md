# Transfer round 23 — distinguish absent attributes from unreadable metadata

Status: resolved

## Confirmed defects

1. A filesystem returning ENOTSUP for initial attribute enumeration caused the copy cache to reject valid hardlinks. Copy completed with matching contents but different destination inode numbers. The TransferBatch regression failed before the fix (`enumeration-red.log`) and passed afterward (`enumeration-green.log`).
2. Journal fingerprinting treated every Unsupported error from the attribute reader as an empty attribute set. When a copied file originally had no attributes, an external annotation added after copying could be discarded by Undo if enumeration succeeded but reading the annotation returned ENOTSUP. The new Journal regression proved that Undo actually deleted the edited copy (`attribute-value-red.log`).

## Fix

The Linux attribute reader now interprets ENOTSUP from the initial llistxattr size query as an empty attribute set. That denotes an unsupported facility, not an unknown attribute value. EIO, EACCES, errors after successful enumeration, and errors reading listed values remain failures.

This lets the hardlink cache compare known empty metadata normally. It also lets copying from a filesystem without ACL support remove ACLs inherited from the destination, preserving the source's access policy.

Linux Journal fingerprinting now propagates remaining attribute-reader errors. It cannot turn a failed read of known metadata into proof that the file has no attributes. The existing non-Linux fallback remains unchanged.

## Validation

Tests use real files and the existing isolated subprocess syscall fixture. The added llistxattr fault is restricted to paths below one temporary directory; lgetxattr faults target one exact temporary file and the user.comment attribute. Marker removal proves each injection was reached. No real user files, desktop Trash, system settings or removable media are changed.

- TransferBatch: Copy and cross-filesystem Move, faults at source and destination, ENOTSUP/EIO/EACCES (12 cases). Contents and hardlink identity are checked. Real read errors disable reuse instead of being treated as matching empty metadata.
- Unsupported enumeration cases record actual transfer receipts, reopen the Journal for Undo, and reopen it again for Redo. Both directions preserve the hardlinks.
- A source without attribute support does not acquire a destination parent's named-user ACL. The parent process checks resulting ACLs without fault injection.
- Journal Undo: ENOTSUP/EIO/EACCES reading an attribute that was successfully listed all protect the externally annotated copy and report the fingerprint error. A later unmodified process still refuses Undo because the external edit remains.
- The existing test suite continues to cover hardlink retry after conflict/restart, incomplete metadata writes, external edits, timestamp-preserving content changes, ACL handling and transfer history recovery.

Focused outputs and the full automated release gate are retained alongside this report.

## Remaining scope

The broader transfer audit remains open. These tests model syscall failures and supported hardlinks on actual temporary filesystems; they do not assert that every target supports hardlinks, nor simulate physical device removal or power loss. Previously recorded queue-persistence and abandoned-staging limitations remain outside this fix. Pending icon changes are excluded from the commit. No release, push or installation was performed.

Final gate: 566 main tests passed, 17 ignored; two scrollbar tests and five real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 activation, desktop/AppStream validation, archive smoke and diff checks passed. The main count includes one pending icon test outside this commit.
