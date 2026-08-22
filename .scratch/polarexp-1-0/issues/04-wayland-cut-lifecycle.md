# 04: Wayland Cut lifecycle and pending Cut UI

**What to build:** Add system Cut on Wayland with hidden pending entries, cancellation, ownership-loss recovery, and safe action fallback.

**Blocked by:** 03: Wayland system Copy and Paste.

**Status:** resolved

- [x] Cut hides entries without changing the filesystem.
- [x] The bottom bar shows the pending item count and actions.
- [x] Escape and ownership loss restore hidden entries.
- [x] Ambiguous action markers resolve to Copy, never Move.
- [x] Partial Move retains only failed entries in pending Cut.

## Answer

Cut now stores a Move payload and immediately removes its paths from the displayed Navigation session without touching the filesystem. The bottom bar retains the item count and `p` or Escape actions. Escape clears the matching native selection and refreshes the folder; the Wayland worker also reports selection cancellation with its generation so stale ownership events cannot clear a newer Cut. Paste into the source folder is a no-op, and partial Move completion rewrites the pending payload to contain only failed sources. Malformed external action markers fall back to Copy.
