# Waddle 0.0.22 validation

Validated on 2026-09-17 in a separate checkout containing the committed fixes.
The v0.0.22 tag identifies the release commit containing this report.
Uncommitted search changes in the main checkout are excluded.

- The automated release gate passed: 738 application tests, 24 opt-in tests ignored,
  three scrollbar regressions, strict Clippy, formatting, locked release build,
  FileManager1 activation, desktop and AppStream validation, archive packaging,
  and archive launch smoke on the local Wayland desktop.
- Seven X11 checks passed on an isolated Xvfb display.
- The rendered marquee-selection regression passed with a headless adapter.
- Regression coverage includes persistence collisions with unrelated files,
  directories, and links; diagnostics across multiple windows and save failures;
  and unmount completion during newer navigation and commands.

Publication waits for both GitHub Packages jobs to succeed at the release tag.
The CI archive is smoke-tested locally before publication and installation.
Both the archive and Flatpak bundle receive SHA-256 checksum files.

The 1.0 manual interoperability checklist remains pending. This release
continues the preview series.
