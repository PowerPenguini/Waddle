# Waddle code audit — 2026-09-08

Reviewed the working tree including the five previously fixed issues. Nine additional behavioral bugs were reproduced with compiled Rust probes in an isolated source copy and fixed in the working tree. Ten regression tests now cover these fixes, including concurrent journal writers. The probes assert the intended behavior and fail on the audited implementation; the original failure log is retained as before-fix evidence. Adapted regression tests are now part of the normal test suite.

The scan examined Transfers, Undo/Redo, Trash/Restore, clipboard state, Search sessions, navigation, thumbnails, shell commands, persistence, Sidebar places, location monitoring, and FileManager1 process handling. This is a targeted source and behavioral audit, not a guarantee that every path is bug-free. No new defect is claimed for an area merely because it was inspected.

## Fixed during the scan: duplicate Desktop

Your `XDG_DESKTOP_DIR` is `$HOME/`. Waddle previously created both Home and Desktop and highlighted both because their paths matched the current location. The requested fix now omits Desktop when its path or canonical path matches Home. Separate Desktop locations remain visible.

Changed [places.rs](/home/powerpenguini/Projects/Waddle/src/app/places.rs:88). Three isolated configuration probes passed: Desktop = Home, Desktop = a distinct directory, and Desktop = a symlink to Home. No desktop configuration was changed.

## 1. [P1] Undo of a merged Restore removes pre-existing destination files

Location: [transfer_queue.rs](/home/powerpenguini/Projects/Waddle/src/app/transfer_queue.rs:94), [effects.rs](/home/powerpenguini/Projects/Waddle/src/journal/effects.rs:102).

Restore a trashed `folder/restored.txt` into an existing `folder/preexisting.txt`, choose Replace to merge, then Undo. The Restore journal entry fingerprints the entire merged destination, discarding `TransferReceipt.replaced_existing`. Undo then moves that whole folder to Trash, including `preexisting.txt`, and reports `Undid Restore`.

The reproduction used temporary directories and a dedicated temporary Trash location. Undo returned success while the pre-existing file disappeared from its original location. It remains in Trash; the demonstrated damage is removal of unrelated destination content, not permanent deletion.

Fixed: Restore records whether a receipt replaced an existing destination and refuses its inverse. Older Restore records without this information are conservatively refused too. The merge regression verifies that unrelated content remains in place.

Probe: `undo_merged_restore_keeps_preexisting_destination_files`.

## 2. [P2] Two windows overwrite each other's persistent Undo history

Location: [store.rs](/home/powerpenguini/Projects/Waddle/src/journal/store.rs:79).

Open two Journal instances on the same path, record a New File operation in each, then reopen the journal. Only the second operation remains. Each window writes its stale in-memory snapshot without a shared lock or reload. FileManager1 launches separate window processes, so this is a supported usage pattern.

Fixed: a stable sidecar file lock covers reload, filesystem effects, and atomic journal replacement. Every record, Undo and Redo reloads current shared state while holding the lock. Sequential and concurrent two-window tests verify retained entries and a shared cursor. The temporary file is protected by the same lock.

Probe: `two_windows_keep_both_recorded_operations` (expected two entries, observed one).

## 3. [P2] A partial Copy Undo cannot be retried

Location: [effects.rs](/home/powerpenguini/Projects/Waddle/src/journal/effects.rs:211), [effects.rs](/home/powerpenguini/Projects/Waddle/src/journal/effects.rs:270).

Copy multiple entries, make one destination parent unwritable, then Undo. Undo deletes other destinations before hitting the permission error. Copy rollback does nothing and the journal cursor stays unchanged. After restoring permissions, another Undo fails because it first fingerprints a destination that the previous attempt already deleted. Redo cannot recover it because the cursor was never advanced.

Fixed for the reproduced failure: persist which Transfer entries have already been undone, including when a later entry fails. Retrying Undo skips completed entries. The regression reopens the journal before retrying, completes Undo, then verifies Redo. This is per-entry recovery, not a crash-atomic filesystem transaction; interruption inside a recursive removal remains a limitation.

Probe: `copy_undo_can_recover_after_partial_removal_failure`.

## 4. [P2] Redo falsely refuses an unchanged directory after cross-filesystem Undo

Location: [effects.rs](/home/powerpenguini/Projects/Waddle/src/journal/effects.rs:224), [fingerprint.rs](/home/powerpenguini/Projects/Waddle/src/journal/fingerprint.rs:56).

Move a directory from the filesystem holding your Projects folder to `/dev/shm`, Undo, then Redo. Undo succeeds, but Redo reports that the directory changed. Fingerprints contain filesystem-dependent directory sizes; after copying the directory back, the source fingerprint is still the one captured on the destination filesystem.

The first probe using two tmpfs locations did not reproduce this; the probe with the Projects filesystem and tmpfs did. This distinction is retained to avoid claiming that every cross-filesystem move fails.

Fixed: successful Move Undo refreshes the source fingerprint on its actual filesystem. The round-trip regression uses the checkout filesystem and `/dev/shm`.

Probe: `cross_filesystem_move_can_redo_after_undo`.

## 5. [P2] Paste after a partial Cut still flattens failed nested files

Location: [transfer.rs](/home/powerpenguini/Projects/Waddle/src/transfer.rs:642).

The previous fix corrected the Transfer queue's Retry button. Clipboard reconciliation still builds its pending paths from `report.failures` and `report.retained`, ignoring destination mappings and overlapping parent/child failures. After a folder merge partially fails, pressing Paste again moves `source/folder/b` to `destination/b` instead of `destination/folder/b`.

The reproduction moved the first child successfully, induced a permission failure for the second child and parent cleanup, restored permissions, and pressed Paste again. The failed child appeared at the wrong level.

Fixed: retained Cut state keeps original selected roots containing failed or retained children. Repeated Paste therefore preserves the folder structure, including when pasting into a different destination. The regression now completes the second Paste and verifies both the nested contents and source removal.

Probe: `pasting_again_after_partial_cut_preserves_folder_structure`.

## 6. [P2] Enter during recursive-search loading opens an unrelated entry

Location: [search.rs](/home/powerpenguini/Projects/Waddle/src/app/search.rs:125).

Select an unrelated entry, start `//needle`, then press Enter before results arrive. The recursive Search session still displays the original selection. `submit` does not check `loading`, takes that old entry, and the application opens it. The 160 ms search debounce makes this possible even on a fast disk.

Fixed: a new recursive query clears the stale display and selection. Enter keeps the Search session and pending work active while loading, at both the Search session and application integration layers.

Probe: `enter_while_recursive_search_is_loading_does_not_submit_old_selection`.

## 7. [P2] A FIFO named like an image blocks a thumbnail worker indefinitely

Location: [thumbnail.rs](/home/powerpenguini/Projects/Waddle/src/app/thumbnail.rs:150).

Create `pipe.png` with `mkfifo` and browse the folder in Grid view. The thumbnail queue checks extension and metadata, but not regular-file type. `ImageReader::open` blocks waiting for a writer. This bypasses the special-file protection added to Copy and journal fingerprinting.

The probe timed out waiting for the decoder, then deliberately opened the FIFO for writing to release the temporary worker. The directly verified behavior is the blocked thumbnail worker; runtime shutdown consequences were not separately exercised.

Fixed: the queue rejects non-regular files. Decoding opens nonblocking and checks the opened file descriptor is regular before reading. The regression also replaces a queued regular image with a FIFO to exercise the inspection/open race.

Probe: `fifo_named_png_does_not_block_thumbnail_decoder`.

## 8. [P2] Background shell children keep Command sessions running

Location: [shell.rs](/home/powerpenguini/Projects/Waddle/src/app/shell.rs:188).

Execute `!sleep 1 &`. The shell exits, but Waddle waits for its stdout/stderr reader threads to reach EOF. The background child inherited those pipe descriptors, so completion takes the full second. A long-lived child can keep the Command session and shared mutation lane occupied indefinitely.

Fixed: pipe readers use nonblocking reads and receive shell completion. They drain buffered output with a bounded deadline and close the pipes instead of waiting for descendants. Output produced by background children after the Command session completes is no longer captured.

Probe: `background_command_completes_when_shell_exits`.

## 9. [P2] FileManager1 leaves closed windows as zombie processes

Location: [file_manager_service.rs](/home/powerpenguini/Projects/Waddle/src/file_manager_service.rs:57).

The persistent service spawns window processes and drops `Child` without waiting. A temporary executable that exits immediately remained in `/proc` with `State: Z`. The probe reaped its child explicitly afterward.

Fixed: an asynchronous waiter reaps each launched window process. The regression verifies that the exited process is not left as a zombie.

Probe: `service_reaps_closed_windows`.

## Evidence and reproducibility

- [Diagnostic probe patch](reproduction-tests.patch): appended test modules only; apply to a disposable copy of the audited working tree.
- [Probe results](reproduction-results.log): nine intended-behavior assertions failed and reproduced the nine findings above.
- [Desktop configuration probes](sidebar-validation.log): all three passed.
- Isolated source copy: `/tmp/waddle-deep-audit-6fw2lqlz`.
- The cross-filesystem probe assumes the Projects filesystem differs from tmpfs. Permission probes require a non-root user. The Restore probe requires `GIO_USE_VFS=local` and `XDG_DATA_HOME` under a dedicated `/home/powerpenguini/.cache/waddle-deep-audit-*` directory, on the same filesystem as its temporary source. Never aim the reproduction at real files.

All nine reproduced findings have fixes in the working tree, with the per-entry recovery limitation noted above. The requested Desktop visibility change is implemented. Earlier Transfer fixes were preserved.

Validation: 411 normal tests passed; formatting and strict Clippy passed. The full release gate passed, including the release build, real X11 adapter tests, FileManager1 activation, desktop metadata, and packaged archive smoke tests. All seven release performance benchmarks also passed. See `release-gate.log` and `benchmarks.log` for final validation output. Rust 1.98.1, Cargo 1.98.1, rustfmt and Clippy are installed under `~/.cargo/bin`.
