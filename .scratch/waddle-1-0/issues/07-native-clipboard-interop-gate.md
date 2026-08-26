# 07: Native clipboard interoperability gate

Type: verification

**What to build:** Prove the complete clipboard contract against Nautilus and Dolphin on Wayland and X11.

**Blocked by:** 04: Wayland Cut lifecycle and pending Cut UI; 05: X11 system Copy and Cut.

**Status:** ready-for-human

- [ ] Copy passes in both directions with Nautilus and Dolphin.
- [ ] Cut passes in both directions with Nautilus and Dolphin.
- [x] Ownership loss, cancellation, and application shutdown are covered by adapter tests.
- [x] Exact automated commands and the manual matrix are recorded.

## Answer

The Wayland and X11 adapters publish and read the complete multi-format offer under one verified
generation. Unit and real-X11 tests cover multi-entry Copy/Cut, ownership generations,
cancellation, shutdown, large incremental payloads, malformed or contradictory markers, and
source replacement. Run `WADDLE_X11_TEST=1 cargo test x11 -- --test-threads=1` for the native X11
protocol evidence. The remaining Nautilus and Dolphin rows are recorded in
`docs/release-checklist.md` and require human Wayland and X11 desktop sessions.
