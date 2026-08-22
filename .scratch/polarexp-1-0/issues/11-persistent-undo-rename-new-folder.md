# 11: Persistent Undo and Redo for Rename and New Folder

**What to build:** Persist a bounded operation journal and use it to Undo and Redo Rename and New Folder after restart.

**Blocked by:** 02: Counted Browser key grammar and Vim motions; 09: Atomic Rename and Replace.

**Status:** ready-for-agent

- [ ] The journal retains 100 operations or 30 days.
- [ ] `u` and `Ctrl+R` execute safe Undo and Redo.
- [ ] New Folder Undo requires the directory to remain empty.
- [ ] Later filesystem changes cause a clear refusal instead of overwrite.
