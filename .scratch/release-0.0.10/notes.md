Waddle 0.0.10 is a preview release focused on transfer correctness and refined file icons.

- Preserve selected sources before another Copy or Move overwrites their paths. Failed preservation blocks destructive replacement, while Keep Both remains available and Retry can recover.
- Create independent same-folder duplicates through directory aliases, including when Replace-all or Skip-all is active for other conflicts.
- Preserve hardlinks when destinations cannot retain all metadata, including Retry and reopened Undo/Redo history.
- Protect source access-control policy from inherited destination ACLs and optional metadata failures. Genuine ACL failures stop publication and cross-filesystem source removal.
- Refuse destructive Undo when metadata cannot be verified, retain metadata warnings through history replay, and protect journal checkpoints and temporary files.
- Use flatter, more delicate file-type icons. Folder icons continue to use the theme accent, including drag previews.

Validation includes 575 application tests, scrollbar regressions, real-X11 adapter tests, icon rendering at multiple scales, performance benchmarks, the automated release gate, and package smoke checks.

This remains a preview. Cyclic source-overwrite dependencies are detected and stopped; executing those cycles through staged source snapshots remains unfinished. Active and queued transfers are not persisted across restarts, and copies abandoned before a checkpoint are not automatically collected. Physical power-loss recovery and the complete cross-desktop interoperability matrix remain unverified.

The Linux archive and Flatpak bundle are built by GitHub Actions from this release tag. SHA-256 checksum files accompany both packages.
