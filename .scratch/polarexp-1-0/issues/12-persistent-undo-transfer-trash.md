# 12: Persistent Undo and Redo for Move, Trash, and Copy

**What to build:** Extend restart-safe recovery to Move, Trash, and Copy without deleting later user work.

**Blocked by:** 06: Vim Cut, Paste, and black-hole Trash; 10: Metadata-faithful Copy; 11: Persistent Undo and Redo for Rename and New Folder.

**Status:** ready-for-agent

- [ ] Move and Trash restore only verified entries.
- [ ] Copy Undo removes only unchanged Copy results.
- [ ] Redo remains available until the next new mutation.
- [ ] Permanent delete is never journaled as reversible.
