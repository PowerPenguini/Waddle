# 17: List view, sorting, and directory view overrides

Type: feature

**What to build:** Add List view and predictable sorting shared with Grid, including persistent per-directory overrides.

**Blocked by:** 16: Standard Grid interaction and keyboard focus.

**Status:** resolved

- [x] Grid and List use the same ordered entry model.
- [x] Name, Modified, Size, and Type sort in both directions.
- [x] New sessions default to Type ascending.
- [x] Natural ordering applies across folders and files.
- [x] Type sorting orders Folder first, extension types next, and generic File last.
- [x] Size sorting keeps folders first and orders files by size.
- [x] Global defaults and per-directory overrides persist.

## Answer

Grid and List now render the same directory entry vector, with List virtualizing only its visible
rows. New sessions start with Type ascending. Directory reads apply natural name ordering or Modified, Size, and Type keys, in either
direction across folders and files. Type uses the same visible labels as the List column and orders
Folder first, extension types next, and generic File last. Size keeps folders first in both
directions, then orders only files by size. View options are serialized as a
global default plus sparse per-directory overrides and restored on startup.
