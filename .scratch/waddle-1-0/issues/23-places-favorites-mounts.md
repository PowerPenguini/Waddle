# 23: Desktop Places, Favorites, and mounted volumes

Type: feature

**What to build:** Put common local places, user Favorites, and normal volume actions in the sidebar.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] Home and existing XDG user directories appear without creating missing folders.
- [x] Favorites support custom labels and drag reordering.
- [x] Mount, unmount, and eject use normal desktop authorization.
- [x] File mutations never gain root privileges.

## Answer

The sidebar now starts with Home and each existing XDG user directory; it never creates missing
locations. `:favorite add [LABEL]`, `:favorite remove INDEX`, and `:favorite list` manage a
persistent ordered list, while dragging one Favorite row onto another saves the new order. The
`:volume mount|unmount|eject NAME` commands run GIO's normal asynchronous desktop operation with
a session-scoped mount authorization object. Waddle does not elevate file mutations or invoke a
root helper.
