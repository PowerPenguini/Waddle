# 24: Shared desktop Recent location

**What to build:** Browse the desktop's shared recent-file history without creating a separate PolarExp activity database.

**Blocked by:** 23: Desktop Places, Favorites, and mounted volumes.

**Status:** resolved

- [x] Recent reads the shared desktop history.
- [x] Missing entries do not break the location.
- [x] Clear History and Disable are available.
- [x] Disabled Recent stops presenting activity in PolarExp.

## Answer

Recent reads the desktop's `recently-used.xbel` through GLib's XBEL implementation and exposes
only existing local files. Missing files and non-local URIs are skipped. `:recent clear` replaces
the shared history with a valid empty XBEL document; `:recent disable` removes the location and
persists the privacy choice without deleting the shared history. `:recent enable` restores it.
The virtual location is read-only for PolarExp file operators and returns to the last real folder
when left.
