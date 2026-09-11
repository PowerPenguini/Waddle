Waddle 0.0.15 is a preview release containing twelve regression-tested bug fixes since 0.0.14.

- Recursive search preserves selections during refresh, restores search input after closing Open With, and refreshes the original folder when opening a file match.
- Recent and recursive-search menus hide mutation actions that are unavailable in those views.
- Trash deletion results and partial-copy errors remain visible through automatic refreshes. Deletion cleans up correctly when an item or its metadata disappears during confirmation.
- Completed Copy and Restore operations preserve the current view and respect navigation already in progress.

Validation passed: 642 application tests, three scrollbar regressions, seven X11 checks on Xvfb, the rendered-layout regression, all eleven performance benchmarks, strict Clippy, and the automated release gate.

This continues the preview series. The manual interoperability matrix for 1.0 remains pending; see the [release checklist](https://github.com/PowerPenguini/Waddle/blob/v0.0.15/docs/release-checklist.md).

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
