# 22: External X11 drag and drop

Type: feature

**What to build:** Bring native external drag and drop to X11 with the same Transfer behavior and feedback as Wayland.

**Blocked by:** 05: X11 system Copy and Cut.

**Status:** ready-for-human

- [x] X11 sends and accepts local multi-entry URI payloads.
- [x] Copy and Move negotiation follows the shared Transfer rules.
- [x] Preview, target highlighting, cancellation, and errors match Wayland.
- [x] A real-X11 cross-window protocol test passes.
- [ ] Manual compatible-application tests pass with Nautilus and Dolphin.

## Answer

PolarExp now owns `XdndSelection`, advertises multi-entry `text/uri-list`, discovers Xdnd-aware
targets under the pointer, negotiates Copy or Move, serves selection requests, and waits for
`XdndFinished`. It uses the same 64 px SVG preview renderer and shared Transfer workflow as the
Wayland adapter. Incoming Winit file events are merged into one drop and external sources safely
default to Copy when their action is unavailable; PolarExp-to-PolarExp Move is carried by a private
X11 action marker after selection ownership is verified.

`POLAREXP_X11_TEST=1 cargo test x11 -- --test-threads=1` exercises two real X11 windows and verifies
Move negotiation plus a two-entry URI payload containing a path with a space. The remaining
Nautilus and Dolphin matrix requires a human desktop session.
