# 20: Asynchronous image thumbnails

**What to build:** Show useful image thumbnails without blocking Grid interaction or growing cache state without a limit.

**Blocked by:** 17: List view, sorting, and directory view overrides.

**Status:** ready-for-agent

- [ ] Thumbnail work runs outside the UI thread.
- [ ] Visible entries receive priority over off-screen entries.
- [ ] Cache size and invalidation are bounded and deterministic.
- [ ] A 10,000-entry folder remains interactive.
