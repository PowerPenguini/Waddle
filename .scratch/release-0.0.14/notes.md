Waddle 0.0.14 is a preview release containing ten regression-tested bug fixes since 0.0.13.

- Undo and Redo refresh the browser after partial failures. Completed operations preserve Recent and Trash views and respect navigation already in progress.
- Earlier Rename and file-action results cannot replace newer editors or command output.
- File-action confirmations and Properties failures remain visible through automatic refreshes until the next interaction.
- Cancelling recursive search or submitting it without matches refreshes the original folder, showing files added or removed during the search.

Validation passed: 630 application tests, three scrollbar regressions, seven X11 checks on Xvfb, the rendered-layout regression, all eleven performance benchmarks, strict Clippy, and the automated release gate.

This continues the preview series. The manual interoperability matrix for 1.0 remains pending; see the [release checklist](https://github.com/PowerPenguini/Waddle/blob/v0.0.14/docs/release-checklist.md).

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
