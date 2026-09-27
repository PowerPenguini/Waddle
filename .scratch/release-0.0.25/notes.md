Waddle 0.0.25 is a preview release with rectangular keyboard selection, scrolling status text, and transfer improvements.

- Use Ctrl+V for rectangular selection, move with Vim keys or arrows, and switch to linear selection with v. Paste files with p; text fields retain Ctrl+V paste.
- Overflowing status text scrolls at 32 pixels per second with a two-second pause. Short text stays still, and reduced motion disables scrolling.
- Browse during Undo and Redo without losing later navigation or selection.
- Show preparation and Undo-recording phases explicitly, and use item counts for transfers where byte throughput is misleading.
- Restrict recursive search to folders and correct the view-toggle icon.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag and include SHA-256 checksum files. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
