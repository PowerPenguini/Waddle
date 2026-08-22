# 13: Live directory reconciliation

**What to build:** Keep displayed entries current while preserving active sessions, selection, scroll, and pending Cut.

**Blocked by:** 04: Wayland Cut lifecycle and pending Cut UI.

**Status:** ready-for-agent

- [ ] External changes update the current folder after debounce.
- [ ] Selection and scroll reconcile by path.
- [ ] Search, Rename, commands, conflicts, and pending Cut survive refresh.
- [ ] A deleted current folder navigates to the nearest existing ancestor.
- [ ] F5, toolbar Refresh, and `:refresh` work explicitly.
