# 11: Persistent Undo and Redo for Rename and New Folder

Type: feature

**What to build:** Persist a bounded operation journal and use it to Undo and Redo Rename and New Folder after restart.

**Blocked by:** 02: Counted Browser key grammar and Vim motions; 09: Atomic Rename and Replace.

**Status:** resolved

- [x] The journal retains 100 operations or 30 days.
- [x] `u` and `Ctrl+R` execute safe Undo and Redo.
- [x] New Folder Undo requires the directory to remain empty.
- [x] Later filesystem changes cause a clear refusal instead of overwrite.

## Answer

The versioned JSON operation journal is written atomically under the XDG state directory and is
reloaded at startup. It truncates redo history on a new mutation and prunes by both the 100-entry
and 30-day limits. Rename Undo/Redo validates the recorded result before using an atomic
no-clobber rename. New Folder Undo additionally requires an empty directory. Unsafe inverse
operations are refused with a bottom-bar explanation.
