# 31: CLI locations and persisted startup state

**What to build:** Open explicit local locations and restore useful application state across independent process launches.

**Blocked by:** 11: Persistent Undo and Redo for Rename and New Folder; 17: List view, sorting, and directory view overrides; 23: Desktop Places, Favorites, and mounted volumes; 28: Persistent `:set`, `:setlocal`, and common controls.

**Status:** resolved

- [x] A local path or file URI opens the requested location.
- [x] Explicit input overrides remembered state.
- [x] Geometry, last folder, views, sorting, Favorites, and Undo history persist.
- [x] Each invocation remains an independent process and window.

## Comments

CLI location resolution, remembered last folder, window size and X11 position are implemented.
Views, sorting, Favorites, and Undo persist in their owning modules. Window size, X11 position, and
the last usable folder persist in startup state. An explicit CLI path or local file URI wins over
the remembered location, and a file URI opens its containing folder.
