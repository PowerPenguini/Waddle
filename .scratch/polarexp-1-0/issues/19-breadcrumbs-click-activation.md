# 19: Breadcrumb navigation and click activation

**What to build:** Add discoverable breadcrumbs, editable locations, and configurable click activation.

**Blocked by:** 16: Standard Grid interaction and keyboard focus.

**Status:** resolved

- [x] Breadcrumb segments navigate to their folders.
- [x] Ctrl+L enters an editable path and Escape restores breadcrumbs.
- [x] Double-click is the default activation behavior.
- [x] Single-click activation can be enabled without breaking selection.

## Answer

The toolbar now shows one navigable button per path ancestor and swaps that breadcrumb row for the
location editor only while Location mode is active. Ctrl+L focuses the editor; Escape restores the
current canonical path and breadcrumb presentation. Double-click remains the persisted default,
while the toolbar can enable single-click activation globally. Ctrl-click and Shift-click always
remain selection gestures in either mode.
