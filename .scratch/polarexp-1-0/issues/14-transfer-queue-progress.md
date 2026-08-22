# 14: Ordered Transfer queue with progress

**What to build:** Run mutations through one queue and show useful progress for each active Transfer.

**Blocked by:** 09: Atomic Rename and Replace; 10: Metadata-faithful Copy.

**Status:** ready-for-agent

- [ ] Only one filesystem mutation runs at a time.
- [ ] Progress includes entries, bytes, speed, and estimated time.
- [ ] The bottom bar summarizes the active Transfer.
- [ ] An expanded view shows queued and completed work.
