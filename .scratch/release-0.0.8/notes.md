Waddle 0.0.8 is a preview release focused on file-transfer reliability and desktop integration.

- Copy, Move, Trash, and Restore now report live transfer statistics and handle cancellation, retries, and conflicts more reliably.
- Partial Undo and Redo operations can resume after a restart without losing completed work or crossing incomplete history. Hardlinks are preserved across transfers and retries.
- Safer replacement and temporary-file handling protects unrelated data, including during concurrent transfers and changes to source or destination paths.
- Copy and Paste within Waddle works without a native Wayland clipboard offer. Delayed clipboard reads retain the original destination and preserve newer clipboard changes.
- Marquee selection correctly focuses files. Moving files to Trash no longer asks for confirmation.
- System and bundled fonts, configurable icons, icon and label zoom, clearer SVG arrows, and refined sidebar icons improve desktop appearance.
- Additional fixes cover navigation, Favorites persistence, and asynchronous folder refreshes.

This remains a preview: the complete cross-desktop interoperability and Flatpak interaction matrix for 1.0 is still pending.

The Linux archive and Flatpak bundle are built by GitHub Actions from the release tag. SHA-256 checksum files accompany both packages.
