# 16: Standard Grid interaction and keyboard focus

**What to build:** Add conventional desktop selection and navigation without weakening Visual selection or Vim motions.

**Blocked by:** 02: Counted Browser key grammar and Vim motions.

**Status:** resolved

- [x] Ctrl-click and Shift-click update the expected selection.
- [x] Ctrl+A, arrows, Shift-arrows, Space, Home, and End work.
- [x] Active and selected entries remain distinct and visible.
- [x] Large Grid interaction stays responsive.

## Answer

The Grid now keeps an active cursor, a selection anchor, and the selected set as separate state.
Ctrl-click toggles one item, Shift-click selects the anchored display range, and conventional
keyboard movement composes with Shift without entering Visual mode. Ctrl+A and Space operate on
the same set. These operations remain index/range based and the view still virtualizes rows, so
they do not add per-entry widgets outside the visible window. Mouse activation now defaults to a
double click; a single click only updates selection.
