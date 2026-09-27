# Waddle 0.0.25 validation

Validated on 2026-09-27. The release includes all current local source changes,
as requested, including Visual Block selection and a 32 px/sec status carousel.

- Automated release gate passed: 785 application tests, 26 opt-in tests ignored,
  three scrollbar regression tests, strict Clippy, formatting, locked release
  build, FileManager1 activation, desktop and AppStream validation, and local
  archive launch smoke testing.
- Seven real-X11 clipboard and drag checks passed on an isolated Xwayland display.
- Headless rectangular-selection and status-carousel rendering checks passed.
- GitHub archive and Flatpak jobs must both pass at the release tag before
  publication. The CI archive is smoke-tested locally and both assets receive
  SHA-256 checksum files.

The 1.0 manual interoperability checklist remains pending. This is a preview
release, continuing the existing 0.0.x series.
