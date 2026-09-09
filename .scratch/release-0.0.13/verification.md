# Waddle 0.0.13 validation

Validated on 2026-09-09 on Omarchy with Rust 1.98.1, in a Wayland desktop session.
The release commit includes this report; the `v0.0.13` tag identifies that commit.

- `env -u DISPLAY scripts/release-gate.sh`: passed. Includes 620 application tests
  (23 opt-in tests ignored), three scrollbar regressions, strict Clippy, formatting,
  locked release build, FileManager1 activation, desktop/AppStream validation,
  archive packaging, and archive launch smoke testing.
- Seven X11 checks passed on an isolated Xvfb display. Xvfb was extracted into
  release scratch space from the configured distribution mirror for these checks.
- The rendered marquee-selection regression passed with a headless graphics adapter.
- All eleven release-mode performance benchmarks passed. Trash confirmation for
  9,999 selected items opened in 9.6 ms; large selection refresh took 13.8 ms.
- Formatting and whitespace checks passed.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak from
the release tag. Publication waits for both jobs to succeed. The 1.0 manual
interoperability checklist remains pending; this release continues the preview series.
