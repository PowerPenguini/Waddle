Waddle 0.0.11 is a preview release fixing keyboard deletion in Trash.

- Delete and black-hole deletion shortcuts now open permanent-delete confirmation for the selected Trash items.
- Ctrl+A transfers focus to the selected files, so the next file action does not target the Sidebar.
- Cut shortcuts in Trash explain how to delete permanently instead of incorrectly reporting Sidebar focus.

Three application-input regression tests reproduced these failures before the fix. Validation includes 578 application tests, scrollbar regressions, real-X11 adapter tests, strict Clippy, and the automated release gate.

This remains a preview with the transfer recovery and interoperability limitations documented in [0.0.10](https://github.com/PowerPenguini/Waddle/releases/tag/v0.0.10). No broader transfer-safety guarantee is introduced by this patch.

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
