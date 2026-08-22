# 15: Transfer Cancel, Retry, partial results, and history

**What to build:** Let users stop, retry, inspect, and recover partial Transfers without damaging pre-existing entries.

**Blocked by:** 12: Persistent Undo and Redo for Move, Trash, and Copy; 14: Ordered Transfer queue with progress.

**Status:** ready-for-agent

- [ ] Cancel removes only private incomplete results.
- [ ] Retry includes failed entries without repeating successes.
- [ ] Partial success remains available for safe Undo.
- [ ] Reports remain local, copyable, and retained for 30 days.
