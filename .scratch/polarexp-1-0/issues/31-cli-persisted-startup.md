# 31: CLI locations and persisted startup state

**What to build:** Open explicit local locations and restore useful application state across independent process launches.

**Blocked by:** 11: Persistent Undo and Redo for Rename and New Folder; 17: List view, sorting, and directory view overrides; 23: Desktop Places, Favorites, and mounted volumes; 28: Persistent `:set`, `:setlocal`, and common controls.

**Status:** ready-for-agent

- [ ] A local path or file URI opens the requested location.
- [ ] Explicit input overrides remembered state.
- [ ] Geometry, last folder, views, sorting, Favorites, and Undo history persist.
- [ ] Each invocation remains an independent process and window.
