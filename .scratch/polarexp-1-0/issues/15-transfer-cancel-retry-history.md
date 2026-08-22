# 15: Transfer Cancel, Retry, partial results, and history

**What to build:** Let users stop, retry, inspect, and recover partial Transfers without damaging pre-existing entries.

**Blocked by:** 12: Persistent Undo and Redo for Move, Trash, and Copy; 14: Ordered Transfer queue with progress.

**Status:** resolved

- [x] Cancel removes only private incomplete results.
- [x] Retry includes failed entries without repeating successes.
- [x] Partial success remains available for safe Undo.
- [x] Reports remain local, copyable, and retained for 30 days.

## Answer

Cancel is an atomic flag observed between transfer roots; already completed results stay in the
report and untouched roots are retained. Copy roots are built under private hidden staging names
and revealed with a no-clobber rename, so cancellation or a content error cannot expose a partial
destination. Retry is constructed only from failed and retained source paths. Successful receipts
still enter the persistent Undo journal after a partial result. Transfer reports are stored under
XDG state for 30 days and the expanded bar can copy them as text. Shell stdout and stderr readers
now bound memory while the process is running, retaining a small tail for the final PWD marker.
