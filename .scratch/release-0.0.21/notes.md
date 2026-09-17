Waddle 0.0.21 is a preview release focused on Recent, file transfers, Undo/Redo, and navigation.

- Use file operations in Recent and recursive search results with consistent keyboard, context-menu, and execution rules.
- Preserve the displayed collection and pending Cut entries after refreshes, clipboard changes, and transfers. Replacing a Cut selection restores the previous entries.
- Keep Recent and Trash up to date when operations finish during a scan, and detect Recent files restored outside Waddle.
- Prevent older scans from restoring cleared or disabled Recent history, without replacing a newer folder choice.
- Keep Undo/Redo available across views. Added integration coverage exercises real Trash/Restore, multi-folder conflicts, partial transfers, cancellation, Retry, and replacement-file protection.
- Reject drops onto collection backgrounds instead of writing to a hidden previous folder.
- Preserve newer navigation when mounts finish late, including multiple sidebar volume choices completed out of order.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. Both packages include SHA-256 checksum files. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
