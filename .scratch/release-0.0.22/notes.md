Waddle 0.0.22 is a preview release with persistence and volume-navigation fixes.

- Preserve unrelated files, directories, and links when saving Recent preferences, Favorites, and command diagnostics.
- Keep command diagnostics from multiple windows, show new shared records, and retain pending records when a save fails.
- Preserve pending folder destinations and Recent or Trash views when a volume unmount completes.
- Keep newer command and navigation feedback when an older unmount result arrives.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. Both packages include SHA-256 checksum files. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
