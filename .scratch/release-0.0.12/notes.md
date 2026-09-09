Waddle 0.0.12 is a preview release improving keyboard focus, marquee selection, and file dragging.

- Tab and Shift+Tab now move between files and Sidebar. Location editing, bottom-bar prompts, and mouse interactions restore keyboard focus consistently.
- Marquee selection stays aligned with the visible files when resizing or shrinking content removes the scrollbar. Drag selection also works in Trash.
- Releasing a file drag over empty space or a toolbar widget clears the drag preview. Transfers finish once even when both the entry and window report the release.
- Cancelled or rejected X11 drags notify the destination so it can clear its hover state. Regression coverage includes Copy, Move, multi-file payloads, and recovery after native drag completion or failure.

Validation passed: 595 application tests, three scrollbar regressions, seven X11 checks on Xvfb, the rendered-layout regression, all seven performance benchmarks, strict Clippy, and the complete automated release gate.

This remains a preview with the transfer recovery and interoperability limitations documented in [0.0.10](https://github.com/PowerPenguini/Waddle/releases/tag/v0.0.10). The full manual interoperability matrix for 1.0 remains pending.

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
