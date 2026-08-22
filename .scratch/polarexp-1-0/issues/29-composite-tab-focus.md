# 29: Composite Tab focus and keyboard-only operation

**What to build:** Make every 1.0 action reachable through a stable composite focus order.

**Blocked by:** 16: Standard Grid interaction and keyboard focus; 17: List view, sorting, and directory view overrides; 19: Breadcrumb navigation and click activation; 23: Desktop Places, Favorites, and mounted volumes; 25: Trash location, Restore, and Empty Trash.

**Status:** resolved

- [x] Toolbar, location, sidebar, Grid or List, and bottom bar are predictable Tab stops.
- [x] Enter and Space activate standard controls.
- [x] Active prompts and menus trap focus.
- [x] Closing a transient control restores prior focus visibly.

## Answer

Browser mode now owns a wrapping five-stop `Tab` and `Shift+Tab` order: toolbar, location,
sidebar, Grid or List, and bottom bar. Arrow keys and Vim motions move inside the focused
composite; Enter and Space activate its current control. Every region draws an explicit primary
focus border that is independent of hover and selection. Context menus have their own wrapping
keyboard cursor, consume keyboard input until they close, and support arrows, Home, End, Enter,
Space, Tab, Shift+Tab, and Escape. Existing inline prompts continue to own their text input and
confirmation keys. Transient controls do not replace the browser focus value, so closing them
reveals the same focus ring that was active before opening.
