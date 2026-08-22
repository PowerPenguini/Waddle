# 28: Persistent `:set`, `:setlocal`, and common controls

**What to build:** Expose common settings in the UI and advanced persistent settings through Command sessions.

**Blocked by:** 17: List view, sorting, and directory view overrides; 18: Hidden entries across browsing and search; 19: Breadcrumb navigation and click activation.

**Status:** ready-for-agent

- [ ] Common view settings remain visible in the toolbar or menus.
- [ ] `:set` inspects and changes global values with completion and validation.
- [ ] `:setlocal` stores and removes per-directory overrides.
- [ ] Invalid settings leave prior values unchanged and explain the error.
