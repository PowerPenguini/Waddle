# 13: Live directory reconciliation

Type: feature

**What to build:** Keep displayed entries current while preserving active sessions, selection, scroll, and pending Cut.

**Blocked by:** 04: Wayland Cut lifecycle and pending Cut UI.

**Status:** resolved

- [x] External changes update the current folder after debounce.
- [x] Selection and scroll reconcile by path.
- [x] Search, Rename, commands, conflicts, and pending Cut survive refresh.
- [x] A deleted current folder navigates to the nearest existing ancestor.
- [x] F5, toolbar Refresh, and `:refresh` work explicitly.

## Answer

A dedicated inotify worker watches the canonical current directory and emits one refresh after a
120 ms burst debounce. Refresh captures selection by path and distinguishes reconciliation from
navigation, so it retains scroll and does not close bottom-bar sessions or pending Cut. Recursive
search is rerun against the live tree, including a low-frequency fallback poll for nested changes.
If the watched directory disappears, Waddle opens the closest existing ancestor and reports it.
F5, the toolbar icon, and `:refresh` all use this same path.
