Waddle 0.0.13 is a preview release with 31 regression-tested bug fixes and improved sidebar disk ordering.

- Search cancellation restores the full selection and stays anchored to the same files after refresh. Context-menu Rename also keeps its original target.
- Recent and Trash remain open across refreshes, view changes, and shell-command completion. Selection and scroll position survive refreshes.
- Older shell-command and Properties results cannot overwrite newer output. Selecting another file no longer cancels an explicit Properties request.
- Folder monitoring recovers after directory replacement and notification overflow, falls back to polling when native monitoring cannot start, and delivers updates during continuous writes.
- Large selection refreshes and Cut reconciliation use indexed path lookups. Delete confirmation for 9,999 selected Trash items fell from 5.94 seconds to about 10–13 milliseconds in local release measurements.
- File operations respect deselection, unchanged Rename preserves non-UTF-8 filenames, and partial permanent-delete failures refresh the browser. Properties correctly reports special permission bits and directory applications for folder symlinks.
- Disks are grouped at the top of the sidebar.

Validation passed: 620 application tests, three scrollbar regressions, seven X11 checks on Xvfb, the rendered-layout regression, all eleven performance benchmarks, strict Clippy, and the automated release gate.

This continues the preview series. The manual interoperability matrix for 1.0 remains pending; see the [release checklist](https://github.com/PowerPenguini/Waddle/blob/v0.0.13/docs/release-checklist.md).

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
