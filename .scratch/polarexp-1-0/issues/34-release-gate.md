# 34: PolarExp 1.0 integration and release gate

**What to build:** Prove that all complete 1.0 slices work together on Wayland and X11 before publishing a release.

**Blocked by:** 07: Native clipboard interoperability gate; 12: Persistent Undo and Redo for Move, Trash, and Copy; 15: Transfer Cancel, Retry, partial results, and history; 18: Hidden entries across browsing and search; 20: Asynchronous image thumbnails; 21: Drag autoscroll and hover navigation; 22: External X11 drag and drop; 24: Shared desktop Recent location; 25: Trash location, Restore, and Empty Trash; 26: Properties, permissions, and Open With; 27: New Empty File and XDG Templates; 29: Composite Tab focus and keyboard-only operation; 30: High contrast and reduced motion; 32: Bounded command output and local diagnostics; 33: Linux desktop packaging.

**Status:** ready-for-agent

- [ ] Wayland and X11 interoperability matrices pass.
- [ ] Restart recovery, cancellation, conflicts, Trash, and clipboard loss pass.
- [ ] A 10,000-entry folder remains usable.
- [ ] Keyboard-only, high-contrast, reduced-motion, and packaging checks pass.
- [ ] Formatting, tests, warning-free Clippy, release build, and diff checks pass.
