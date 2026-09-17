# Waddle 0.0.21 validation

Validated on 2026-09-17 on the local Linux Wayland desktop.
The `v0.0.21` tag identifies the release commit containing this report.

- `env -u DISPLAY scripts/release-gate.sh` passed: 731 application tests,
  24 opt-in tests ignored, three scrollbar regressions, strict Clippy,
  formatting, locked release build, FileManager1 activation, desktop and
  AppStream validation, archive packaging, and archive launch smoke.
- Seven X11 checks passed on an isolated Xvfb display.
- The rendered marquee-selection regression passed with a headless adapter.
- Recent integration tests exercised real GIO Trash/Restore in isolated XDG
  directories, Undo/Redo, replacement-file protection, multi-folder conflicts,
  partial transfer cancellation and retry, filesystem monitoring, stale reads,
  and clearing/disabling Recent during navigation.
- Both sidebar mount-navigation fixes since v0.0.20 are included.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak
from this tag. Publication waits for both jobs to succeed and includes both
packages with SHA-256 checksum files.

The 1.0 manual interoperability checklist remains pending. This release
continues the preview series. This release task does not install the package.
