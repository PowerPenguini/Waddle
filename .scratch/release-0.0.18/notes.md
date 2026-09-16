Waddle 0.0.18 is a preview release with fixes for file operations, Undo and Redo, and navigation.

- Preserve New File timestamps so later Copy and Move operations can redo.
- Preserve later permission and extended-attribute edits when undoing file or folder creation.
- Refuse to rename or permanently delete an item replaced after its prompt opened, including queued operations.
- Refuse to create files or folders inside a replaced parent directory, including symlink targets.
- Keep delayed shell directory changes from overriding newer navigation while retaining valid changes after refreshes.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. SHA-256 checksum files accompany both packages. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
