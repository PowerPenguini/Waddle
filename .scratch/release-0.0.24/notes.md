Waddle 0.0.24 is a preview release with fixes for replaced files during transfers.

- Keep replacement files, folders, and symlinks safe when confirming permanent deletion after a Trash failure.
- Reject replaced sources in queued Copy and Move requests while allowing content edits to the original files.
- Preserve original source identities across retries of failed, cancelled, and partially merged transfers. Restoring the original source allows Retry to finish.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. Both packages include SHA-256 checksum files. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
