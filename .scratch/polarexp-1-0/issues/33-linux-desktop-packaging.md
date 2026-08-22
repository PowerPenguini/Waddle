# 33: Linux desktop packaging

Type: feature

**What to build:** Install and launch PolarExp as a normal Linux desktop application through Flatpak and a regular binary archive.

**Blocked by:** 31: CLI locations and persisted startup state.

**Status:** ready-for-human

- [x] The desktop entry accepts URI arguments.
- [x] Icons and AppStream metadata validate.
- [x] Flatpak permissions support the agreed local file workflows.
- [x] The regular binary archive smoke test passes.
- [ ] The Flatpak bundle smoke test passes in an environment with Flatpak Builder.

## Comments

The regular x86_64 archive was built, unpacked, validated, checked for missing libraries, and
launched under Wayland on 2026-08-22. The AppStream metadata passes pedantic validation and the
desktop entry uses `%U`. The checked-in Flatpak manifest uses offline Cargo sources and is wired
into the package workflow. This host does not have `flatpak` or `flatpak-builder`, so the bundle
cannot be built locally. Flathub's manifest linter also correctly reports that the intentionally
broad `--filesystem=host` permission needs an exception, and that the future
`github.com/powerpenguini/polarexp` repository must exist before publication. Those are the only
remaining human/external packaging steps.
