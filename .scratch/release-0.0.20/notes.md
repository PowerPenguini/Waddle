Waddle 0.0.20 is a preview release with ten bug fixes since 0.0.19.

- Keep shell selections stable inside functions and after positional-argument changes, and ignore selection placeholders in comments.
- Keep shell errors visible when output is truncated and preserve directory changes when stdout is redirected or the working folder is renamed.
- Protect replacement files from permission changes and support unreadable items and named pipes.
- Honor explicit Rename edits to filenames containing invalid UTF-8 bytes.
- Accept tabs before built-in command arguments.
- Preserve the active Recent setting when saving preferences fails.
- Keep delayed volume results from overwriting newer command feedback.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. SHA-256 checksum files accompany both packages. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
