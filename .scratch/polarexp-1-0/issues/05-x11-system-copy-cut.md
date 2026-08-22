# 05: X11 system Copy and Cut

Type: feature

**What to build:** Implement the shared multi-entry clipboard contract on X11, including selection ownership and Cut markers.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp.

**Status:** resolved

- [x] X11 publishes and reads every agreed MIME format.
- [x] Ownership changes clear stale pending state.
- [x] Large payload transfer follows the X11 incremental protocol.
- [x] Copy and Cut work with a compatible X11 application.

## Answer

PolarExp now selects a native clipboard adapter from the raw window handle. The X11 adapter
publishes the private generation payload plus GNOME, KDE, and URI-list formats, observes
selection ownership with XFixes, and implements both sides of the INCR protocol for payloads
larger than a normal X request.

The adapter-to-adapter compatibility test exchanged 5,000 paths as both Copy and Cut through a
real Xwayland server nested in headless Weston. Unit coverage also verifies the required empty
final INCR property. Nautilus and Dolphin interoperability remains the separate release gate in
ticket 07.
