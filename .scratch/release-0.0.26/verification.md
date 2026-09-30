# Waddle 0.0.26 validation

Validated on 2026-09-30. Release tag `v0.0.26` points to
`67edb9110afed3d4a873afdc4a14f22956c7b65e`.

- Automated release gate passed: 789 application tests, 26 opt-in tests ignored,
  three scrollbar regression tests, strict Clippy, formatting, locked release
  build, FileManager1 activation, desktop and AppStream validation, and archive
  smoke testing.
- GitHub Packages archive and Flatpak jobs both passed at the release tag:
  https://github.com/PowerPenguini/Waddle/actions/runs/36733186259
- The CI archive passed local metadata, icon, shared-library, and FileManager1
  activation checks. The Flatpak bundle passed its CI install and inspection.
- Release assets include the CI archive, Flatpak bundle, and their SHA-256 files.
  Both checksum files verified locally. GitHub's uploaded SHA-256 digests match
  all four local files.

No display was attached, so GUI launch and real-X11 checks were skipped. The
1.0 manual interoperability checklist remains pending. This is a preview
release, continuing the existing 0.0.x series.
