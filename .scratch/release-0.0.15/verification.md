# Waddle 0.0.15 validation

Validated on 2026-09-11 on Omarchy with Rust 1.98.1, in a Wayland desktop session.
The release commit includes this report; the `v0.0.15` tag identifies that commit.

- `env -u DISPLAY scripts/release-gate.sh`: passed. Includes 642 application tests
  (23 opt-in tests ignored), three scrollbar regressions, strict Clippy, formatting,
  locked release build, FileManager1 activation, desktop/AppStream validation,
  archive packaging, and archive launch smoke testing.
- Seven X11 checks passed on an isolated Xvfb display, using the distribution
  Xvfb binary retained in release scratch space from an earlier release.
- The rendered marquee-selection regression passed with a headless graphics adapter.
- All eleven release-mode performance benchmarks passed. Delete confirmation for
  9,999 selected Trash items opened in 9.6 ms.
- Formatting and whitespace checks passed.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak from
the release tag. Publication waits for both jobs to succeed. The 1.0 manual
interoperability checklist remains pending; this release continues the preview series.
