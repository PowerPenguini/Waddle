# 12: Persistent Undo and Redo for Move, Trash, and Copy

Type: feature

**What to build:** Extend restart-safe recovery to Move, Trash, and Copy without deleting later user work.

**Blocked by:** 06: Vim Cut, Paste, and black-hole Trash; 10: Metadata-faithful Copy; 11: Persistent Undo and Redo for Rename and New Folder.

**Status:** resolved

- [x] Move and Trash restore only verified entries.
- [x] Copy Undo removes only unchanged Copy results.
- [x] Redo remains available until the next new mutation.
- [x] Permanent delete is never journaled as reversible.

## Answer

Successful transfer roots now carry exact source/result receipts into the persistent journal.
Move, Copy, and Trash entries store a deterministic fingerprint of the complete tree, including
file content and symlink targets. Undo validates every affected path before changing anything;
Copy results are removed only when the complete result tree is unchanged. Trash also records the
actual freedesktop Trash data and `.trashinfo` paths before offering recovery. Redo is persisted
and is truncated only by a newly recorded mutation. Permanent Delete has no journal path.
