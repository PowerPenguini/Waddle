# 06: Vim Cut, Paste, and black-hole Trash

Type: feature

**What to build:** Make the Browser key grammar expose single-key Yank, motion-based Cut, Paste, and Trash through the black-hole register.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp; 02: Counted Browser key grammar and Vim motions.

**Status:** resolved

- [x] `y` copies the current selection without accepting a motion.
- [x] `dd`, `d{motion}`, and `x` Cut the expected entries.
- [x] `"_` variants move entries to Trash without changing the clipboard.
- [x] Visual operations act on the complete Visual selection.
- [x] Technical sidebar roots reject file operators.

## Answer

The Browser key grammar now separates one-key Yank, Cut operators, and the black-hole Trash register. Counts compose before and after `d`; ranges follow current display order. `x` and Visual `d` or `x` use the complete selection. `"_dd`, `"_d{motion}`, and `"_x` reuse the existing Trash confirmation without replacing the clipboard. A focused sidebar rejects file operators instead of applying them to a stale Grid selection.
