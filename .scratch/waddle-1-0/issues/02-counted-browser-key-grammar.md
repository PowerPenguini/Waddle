# 02: Counted Browser key grammar and Vim motions

Type: feature

**What to build:** Add counted navigation, `gg`, `G`, viewport motions, pending-sequence feedback, and equivalent sidebar traversal.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] Counts compose with supported motions and `G`.
- [x] Pending sequences stay visible until completion or Escape.
- [x] Invalid input resets the sequence with a short explanation.
- [x] Grid and sidebar navigation remain deterministic.

## Answer

Implemented counted Browser key grammar for directional, row, absolute, viewport, and half-page motions. `gg`, `G`, and counted `G` use display positions; counts also compose around the pending delete operator. The bottom bar now shows count and `g` sequences without a timeout, Escape cancels them, and invalid keys explain the reset. Grid motion uses current viewport geometry, while a focused sidebar traverses visible nodes with the same count and end-jump rules.
