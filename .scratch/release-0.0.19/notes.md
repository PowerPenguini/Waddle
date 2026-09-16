Waddle 0.0.19 is a preview release with fixes for Undo and Redo, shell commands, and Location editing.

- Restore recorded permissions and ACLs when redoing file or folder creation, and allow retries after metadata errors.
- Refuse queued permission changes when the selected file, folder, or symlink target has been replaced.
- Keep delayed shell commands from overriding a newer Search session.
- Preserve shell exit codes, output from exit traps, and completion messages.
- Keep $selected paths anchored to the selected files when a shell command changes directory.
- Preserve unchanged Location paths containing non-UTF-8 names and retain typed Location edits during refreshes.

The Linux archive and Flatpak bundle are built and smoke-tested by GitHub Actions from the release tag. SHA-256 checksum files accompany both packages. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
