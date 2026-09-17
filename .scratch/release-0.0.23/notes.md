Waddle 0.0.23 is a preview release with history recovery and Trash safety fixes.

- Recover Rename, New File, and New Folder Undo or Redo after interrupted journal saves.
- Preserve replacement files when queued Trash requests run or failed and cancelled requests are retried.
- Preserve newer Trash entries and their metadata when an interrupted restore resumes.
- Keep newer navigation feedback when an older volume command finishes.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. Both packages include SHA-256 checksum files. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
