# 28: Persistent `:set`, `:setlocal`, and common controls

Type: feature

**What to build:** Expose common settings in the UI and advanced persistent settings through Command sessions.

**Blocked by:** 17: List view, sorting, and directory view overrides; 18: Hidden entries across browsing and search; 19: Breadcrumb navigation and click activation.

**Status:** resolved

- [x] Common view settings remain visible in the toolbar or menus.
- [x] `:set` inspects and changes global values with completion and validation.
- [x] `:setlocal` stores and removes per-directory overrides.
- [x] Invalid settings leave prior values unchanged and explain the error.

## Answer

The toolbar retains direct view, sort, direction, folders-first, hidden-entry, and click controls.
`:set` reads or atomically changes their persistent global values, while `:setlocal` writes the
current directory override and `option&` restores a field from the global value. `:set all` opens
the documented option list in the bottom bar. Tab completes unambiguous setting names, and parser
or persistence errors preserve the previous settings and report the rejected value.
