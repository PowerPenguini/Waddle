# 20: Asynchronous image thumbnails

**What to build:** Show useful image thumbnails without blocking Grid interaction or growing cache state without a limit.

**Blocked by:** 17: List view, sorting, and directory view overrides.

**Status:** resolved

- [x] Thumbnail work runs outside the UI thread.
- [x] Visible entries receive priority over off-screen entries.
- [x] Cache size and invalidation are bounded and deterministic.
- [x] A 10,000-entry folder remains interactive.

## Answer

Grid requests thumbnails only for its virtualized visible range. Each image is decoded and scaled
to at most 96 by 96 pixels on Tokio's blocking worker pool, then returned to the UI as an RGBA
handle. A 128-entry LRU cache includes successful and failed decodes, keys invalidation by file
length and modification time, suppresses duplicate in-flight work, and drops stale completions.
No thumbnail task or widget is created for the remaining off-screen entries in a large folder.
