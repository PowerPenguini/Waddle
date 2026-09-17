# Waddle 0.0.24 validation

Validated on 2026-09-17 in a separate checkout containing the committed fixes.
The v0.0.24 tag identifies the release commit containing this report.
Uncommitted search changes in the main checkout are excluded.

- The automated release gate passed: 755 application tests, 24 opt-in tests ignored,
  three scrollbar regressions, strict Clippy, formatting, locked release build,
  FileManager1 activation, desktop and AppStream validation, archive packaging,
  and archive launch smoke on the local Wayland desktop.
- Seven X11 checks passed on an isolated Xvfb display.
- The rendered marquee-selection regression passed with a headless adapter.
- Regression coverage includes replaced files, populated folders, and symlinks
  during Trash permanent-delete fallback, queued Copy and Move, and repeated
  retries of failed, cancelled, and partially merged transfers.

Publication waits for both GitHub Packages jobs to succeed at the release tag.
The CI archive is smoke-tested locally before publication.
Both the archive and Flatpak bundle receive SHA-256 checksum files.

The 1.0 manual interoperability checklist remains pending. This release
continues the preview series.
