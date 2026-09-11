Waddle 0.0.16 is a preview release containing all current application changes since 0.0.15.

- Open `open-with` using the `ow` keyboard sequence or the `:ow` command alias.
- Choose applications with `j`/`k` or arrow keys and Enter. `Custom app...` is the final row in the application list; application rows no longer launch on mouse clicks.
- Enter an application name, desktop ID, or executable path for a custom application. Paths with spaces are supported; `~/` expands to your home directory and relative executable paths resolve beside the selected entry.
- Escape returns from custom input to the list without losing the input. Backspace stays inside `open-with`, while empty `:`, `!`, and `/` inputs keep their existing cancellation behavior.
- Align the application columns and use monospace text throughout the bottom bar.
- Dim hidden folders and their icons in the sidebar, restoring full visibility when selected, focused, or targeted for a drop, and when reduced transparency is enabled.

Validation covers the chooser layout, keyboard grammar, browser input and focus, hidden-entry opacity, strict Clippy, and package smoke checks. The full release gate and manual interoperability matrix were not rerun for this preview.

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
