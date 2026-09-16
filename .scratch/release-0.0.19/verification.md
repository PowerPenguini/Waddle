# Waddle 0.0.19 validation

Validated on 2026-09-16 on the local Linux Wayland desktop.
The `v0.0.19` tag identifies the release commit containing this report.

- `env -u DISPLAY scripts/release-gate.sh` passed, including 694 application
  tests with 24 opt-in tests ignored, three scrollbar regressions, strict
  Clippy, formatting, the locked release build, FileManager1 activation,
  desktop/AppStream validation, archive packaging, and archive launch smoke.
- Seven X11 checks passed on an isolated Xvfb display.
- The rendered marquee-selection regression passed with a headless adapter.
- The final regression verified that pending refreshes and filesystem
  notifications preserve active Location edits and their submission behavior.
  Explicit Parent navigation still updates Location.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak
from this tag. Publication waits for both jobs to succeed and includes both
packages with SHA-256 checksum files. Installation uses that CI-built archive.

The 1.0 manual interoperability checklist remains pending. This release
continues the preview series.
