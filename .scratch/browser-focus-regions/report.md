# Browser focus regions — validation

Status: resolved

## Result

Only files and Sidebar are browser traversal destinations. A hidden Sidebar leaves files
as the sole destination. Toolbar actions remain clickable; Location is edited directly
or with Ctrl+L. Bottom editors and confirmations retain their keyboard ownership and
restore interaction with the prior browser surface when dismissed.

Removed obsolete toolbar/bottom cursors, activation routing, and focus-only decoration.
Updated keyboard help and the release checklist to match the requested behavior.

## Evidence

The new Tab regression first failed: Tab from files returned BottomBar instead of Sidebar.
It passes after removing the three browser destinations.

- Full test suite: 587 passed, 18 ignored, no failures.
- All seven bottom editors checked from both files and Sidebar through App key/message
  events and actual Iced widget focus operations. Tab, Shift+Tab, arrows, Space, Delete,
  Ctrl+A and Ctrl+W sequences do not leak into browser selection or navigation.
- Command completion, mouse refocus, cancellation, permanent-delete confirmation capture,
  hidden Sidebar traversal, Location release, transient restoration, and Rename selection pass.
- Replaced the mouse-refocus task-count assertion with an actual widget-focus assertion.
- Strict Clippy, formatting, locked release build, and whitespace checks pass.

Validation ran on an isolated checkout of ce9b097 plus these changes, excluding concurrent
external-drag/grid-layout work. This is automated App/widget verification, not a manual
desktop interaction test. No release was published or installed.
