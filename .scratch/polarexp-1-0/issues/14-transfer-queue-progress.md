# 14: Ordered Transfer queue with progress

Type: feature

**What to build:** Run mutations through one queue and show useful progress for each active Transfer.

**Blocked by:** 09: Atomic Rename and Replace; 10: Metadata-faithful Copy.

**Status:** resolved

- [x] Only one filesystem mutation runs at a time.
- [x] Progress includes entries, bytes, speed, and estimated time.
- [x] The bottom bar summarizes the active Transfer.
- [x] An expanded view shows queued and completed work.

## Answer

Transfers now enter an explicit FIFO queue on top of the existing single mutation lane. Each
active job has an atomic progress tracker with root-entry and byte totals; the UI derives speed
and ETA from the elapsed time and refreshes the compact bottom-bar line every 100 ms. The History
control expands that same area with the active snapshot, queued count, and newest completed
reports.
