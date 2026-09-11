# Waddle 0.0.16 validation

Validated on 2026-09-11 on the local Linux Wayland desktop.
The release includes all pending application changes since 0.0.15, including
hidden Sidebar tree folder/icon dimming and the complete open-with update.
The `v0.0.16` tag identifies the release commit.

Local checks passed:

- Formatting, whitespace, desktop-entry and AppStream validation.
- 12 focused open-with tests, including the headless rendered layout, custom
  executable paths, custom input retention, and the command alias.
- 22 Browser key grammar tests.
- 30 input integration tests, including `ow`, Backspace behavior, and context menus.
- 10 Browser focus tests.
- Hidden-entry opacity and accessibility behavior test.
- Strict Clippy across all targets.
- Locked release build, archive packaging, and archive launch smoke test.

The user's instruction to avoid full release testing remains in effect. The full
release gate, broad test suite, benchmarks, X11 checks, and 1.0 manual
interoperability matrix were not rerun for this preview.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak from
the release tag. Publication waits for both jobs to succeed, then includes both
CI packages and their SHA-256 checksum files.
