# 21: Drag autoscroll and hover navigation

Type: feature

**What to build:** Reach off-screen and nested destinations while dragging without accidental folder entry.

**Blocked by:** 16: Standard Grid interaction and keyboard focus.

**Status:** resolved

- [x] Grid and sidebar autoscroll near their edges.
- [x] Sidebar folders expand after a short hover.
- [x] Folder entry uses a longer hover with visible progress.
- [x] Moving away cancels pending hover activation.

## Answer

An active internal or inbound drag now drives 60 Hz edge scrolling for the surface under the
pointer. Sidebar targets expand after 350 ms; Grid, List, and sidebar folder targets enter after
1.1 seconds, with a thin progress strip rendered on the target. Changing targets or leaving a
valid folder cancels and resets the timer. Internal drags snapshot their source entries when the
drag threshold is crossed, so hover navigation cannot replace the transfer source set.
