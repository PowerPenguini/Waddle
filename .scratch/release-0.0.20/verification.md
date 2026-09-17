# Waddle 0.0.20 validation

Validated on 2026-09-17 on the local Linux Wayland desktop.
The `v0.0.20` tag identifies the release commit containing this report.

- `env -u DISPLAY scripts/release-gate.sh` passed, including 712 application
  tests with 24 opt-in tests ignored, three scrollbar regressions, strict
  Clippy, formatting, the locked release build, FileManager1 activation,
  desktop/AppStream validation, archive packaging, and archive launch smoke.
- Seven X11 checks passed on an isolated Xvfb display.
- The rendered marquee-selection regression passed with a headless adapter.
- Ten fixes since 0.0.19 have behavioral regression coverage. The final fix
  verifies that queued volume errors preserve newer command feedback while
  current volume errors remain visible.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak
from this tag. Publication waits for both jobs to succeed and includes both
packages with SHA-256 checksum files. Installation uses that CI-built archive.

The 1.0 manual interoperability checklist remains pending. This release
continues the preview series.
