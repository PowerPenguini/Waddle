# 25: Trash location, Restore, and Empty Trash

**What to build:** Make Trash a complete location with safe Restore and explicit permanent deletion.

**Blocked by:** 08: Bottom-bar Transfer conflict workflow; 12: Persistent Undo and Redo for Move, Trash, and Copy; 23: Desktop Places, Favorites, and mounted volumes.

**Status:** resolved

- [x] Trash lists entries and original locations.
- [x] Restore reuses normal conflict handling and Undo.
- [x] Permanent deletion of selected entries requires confirmation.
- [x] Empty Trash requires confirmation and reports partial failure.

## Answer

Trash is a read-only virtual sidebar location backed by GIO's aggregate `trash:///` view and the
freedesktop `Trash/files` and `Trash/info` pairs, including mounted-volume Trash roots. Entries use
their original names and show the decoded original path in the bottom bar. Restore maps every
physical Trash name to its exact original destination, reuses the
existing Replace, Skip, Keep Both, directory-merge, and uppercase batch choices, removes consumed
metadata, and records restart-safe Undo/Redo. The Trash context menu exposes Restore, selected
permanent deletion, and Empty Trash. Both destructive actions require the existing bottom-bar
confirmation, say that deletion cannot be undone, and retain a detailed partial-failure report.
