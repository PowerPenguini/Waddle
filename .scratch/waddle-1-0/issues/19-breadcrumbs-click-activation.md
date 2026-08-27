# 19: Breadcrumb navigation and click activation

Type: feature

**What to build:** Add discoverable breadcrumbs, editable locations, and configurable click activation.

**Blocked by:** 16: Standard Grid interaction and keyboard focus.

**Status:** resolved

- [x] Breadcrumb segments navigate to their folders.
- [x] Ctrl+L enters an editable path and Escape restores breadcrumbs.
- [x] Folders default to single-click activation and files default to double-click activation.
- [x] File and folder click activation can be configured independently without breaking selection.

## Answer

The toolbar now shows one navigable button per path ancestor and swaps that breadcrumb row for the
location editor only while Location mode is active. Ctrl+L focuses the editor; Escape restores the
current canonical path and breadcrumb presentation. `folder-click=single|double` and
`file-click=single|double` select each behavior globally. Folders default to single click and files
to double click. Ctrl-click and Shift-click always remain selection gestures in either mode.
