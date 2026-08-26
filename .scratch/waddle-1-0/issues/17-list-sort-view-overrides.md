# 17: List view, sorting, and directory view overrides

Type: feature

**What to build:** Add List view and predictable sorting shared with Grid, including persistent per-directory overrides.

**Blocked by:** 16: Standard Grid interaction and keyboard focus.

**Status:** resolved

- [x] Grid and List use the same ordered entry model.
- [x] Name, Modified, Size, and Type sort in both directions.
- [x] Natural ordering and folders-first are configurable.
- [x] Global defaults and per-directory overrides persist.

## Answer

Grid and List now render the same directory entry vector, with List virtualizing only its visible
rows. Directory reads apply natural name ordering or Modified, Size, and Type keys, in either
direction, while folders-first remains independent of direction. View options are serialized as a
global default plus sparse per-directory overrides and restored on startup.
