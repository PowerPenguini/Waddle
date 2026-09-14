Waddle 0.0.17 is a preview release with fixes for operation history, queued work, and feedback.

- Keep recursive-search results current when queued Copy, Rename, or Undo completes.
- Keep Transfer, Undo, and Redo results visible through automatic refreshes.
- Preserve replacement Trash entries when a queued Restore or stale deletion confirmation runs.
- Preserve replacement files and folders when replaying operation history.
- Keep folder Copy and Rename history usable after undone child operations.
- Update dependent history when journal operations recreate items, including cross-filesystem Move Undo.
- Wrap Open With help to fit the narrow output panel.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. SHA-256 checksum files accompany both packages. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
