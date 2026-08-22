# 04: Wayland Cut lifecycle and pending Cut UI

**What to build:** Add system Cut on Wayland with hidden pending entries, cancellation, ownership-loss recovery, and safe action fallback.

**Blocked by:** 03: Wayland system Copy and Paste.

**Status:** ready-for-agent

- [ ] Cut hides entries without changing the filesystem.
- [ ] The bottom bar shows the pending item count and actions.
- [ ] Escape and ownership loss restore hidden entries.
- [ ] Ambiguous action markers resolve to Copy, never Move.
- [ ] Partial Move retains only failed entries in pending Cut.
