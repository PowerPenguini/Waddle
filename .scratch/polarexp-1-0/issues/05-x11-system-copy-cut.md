# 05: X11 system Copy and Cut

**What to build:** Implement the shared multi-entry clipboard contract on X11, including selection ownership and Cut markers.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp.

**Status:** ready-for-agent

- [ ] X11 publishes and reads every agreed MIME format.
- [ ] Ownership changes clear stale pending state.
- [ ] Large payload transfer follows the X11 incremental protocol.
- [ ] Copy and Cut work with a compatible X11 application.
