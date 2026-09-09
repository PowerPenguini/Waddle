# Waddle 0.0.12 validation

Validated on 2026-09-09 on Omarchy with Rust 1.98.1, in a Wayland desktop session.
The release commit includes this report; the `v0.0.12` tag identifies that commit.

- `DISPLAY=:99 scripts/release-gate.sh`: passed. Includes 595 application tests
  (19 opt-in tests ignored), three scrollbar regressions, strict Clippy, formatting,
  locked release build, FileManager1 activation, seven X11 checks, desktop/AppStream
  validation, archive packaging, and archive launch smoke testing.
- `cargo test --locked marquee_selects_the_tiles_inside_the_rendered_rectangle -- --ignored --nocapture`:
  passed using the real browser widget layout and a headless graphics adapter.
- `DISPLAY=:99 scripts/benchmark-performance.sh`: all seven benchmarks passed.
- `git diff --check` and staged whitespace checks: passed.

The inherited X11 display `:0` refused connections. A temporary XWayland server
connected but could not satisfy the test's pointer-position precondition under
the desktop compositor. All seven X11 checks passed on an isolated Xvfb `:99`.
This is protocol-test evidence, not completion of the manual desktop matrix.

The first rendered-layout run used stale compiled widget output and reproduced
the old selection offset. `cargo clean -p iced_widget` forced that dependency to
rebuild; the same test then passed without source changes. The complete release
gate was rerun successfully afterward.

The GitHub Packages workflow builds and smoke-tests the archive and Flatpak from
the release tag. Publication waits for both jobs to succeed. The 1.0 manual
interoperability checklist remains pending; this release continues the existing
preview series.
