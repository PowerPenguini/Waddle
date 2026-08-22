# 06: Vim Cut, Paste, and black-hole Trash

**What to build:** Make the Browser key grammar expose single-key Yank, motion-based Cut, Paste, and Trash through the black-hole register.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp; 02: Counted Browser key grammar and Vim motions.

**Status:** ready-for-agent

- [ ] `y` copies the current selection without accepting a motion.
- [ ] `dd`, `d{motion}`, and `x` Cut the expected entries.
- [ ] `"_` variants move entries to Trash without changing the clipboard.
- [ ] Visual operations act on the complete Visual selection.
- [ ] Technical sidebar roots reject file operators.
