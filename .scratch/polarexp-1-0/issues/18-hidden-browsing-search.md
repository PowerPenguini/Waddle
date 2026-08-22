# 18: Hidden entries across browsing and search

Type: feature

**What to build:** Make one hidden-entry preference control normal browsing, recursive browsing, and filename Search sessions.

**Blocked by:** 17: List view, sorting, and directory view overrides.

**Status:** resolved

- [x] Ctrl+H updates Grid and List without losing valid context.
- [x] Search includes hidden entries only while they are visible.
- [x] Sidebar traversal follows the same preference.
- [x] The setting persists globally and per directory where configured.

## Answer

Ctrl+H flips the current directory's persisted hidden-entry preference and reconciles the live
view by path, retaining selection and scroll where possible. The same option now controls normal
directory reads, recursive filename search, and sidebar child traversal, so hidden folders do not
leak through a secondary browser surface.
