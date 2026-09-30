# Thumbnail bug hunt

Two reproduced bugs fixed:

- Replacing or editing an image while preserving its size and modification time
  kept its old thumbnail. The cache now also checks device, inode, and change time.
- Replacing an image with a folder left its thumbnail visible. Visible folders
  now reach cache invalidation, and paths that stop being regular files clear
  cached and pending thumbnails.

The initial targeted run reproduced both failures. Four regression tests cover
replacement, in-place edits, cache invalidation, and the grid refresh caller.
The grid test runs in a subprocess to isolate GIO's desktop state.

All 793 tests passed in sequential and parallel runs, with 26 opt-in tests
ignored. Strict Clippy, formatting, and whitespace checks passed.
The locked debug build and FileManager1 activation smoke check also passed.

Earlier parallel runs hit intermittent monitor-test and GIO failures. The
monitor overflow test passed ten isolated repetitions. The GIO backtrace and
remaining uncertainty are recorded in `issues/01-parallel-gio-volume-monitor.md`.
