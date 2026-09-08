# Transfer audit — round 7

Status: resolved

## Confirmed bugs

1. A directory conflict remembered only the destination identity. If the selected source directory was renamed away and replaced by a symlink while the conflict was open, Replace still followed the directory-merge branch. `read_dir` traversed the new symlink. Copy imported unrelated files, and Move removed them from the unrelated folder.
2. After a directory merge had planned its children, replacing either parent during a later child conflict redirected pending sibling operations. Skipping the blocked child still allowed later siblings to traverse the substituted source or destination parent.

Both failures were reproduced through the public TransferBatch interface using real temporary directories. The initial Move regression lost `unrelated.txt` from the unrelated folder (`red.log`). After fixing that entry point, the pending-sibling regression independently failed because `unrelated/b` disappeared (`parent-red.log`).

## Fix

Conflicts retain the source device/inode/type as well as the destination identity. Replace and Keep Both reject a substituted source before operating on it. Skip remains available because it does not modify the source or destination.

Directory merges record the identities of both participating directories. Before processing a planned child or source-directory cleanup, the batch verifies all recorded merge ancestors. A changed parent causes a reported failure with retry paths instead of reading, deleting or writing through the new path. Ancestor lookup uses the source path hierarchy, avoiding a scan of every merged directory per file.

## Regression coverage

- Source directory replaced by a symlink before resolving a directory conflict: Copy and Move preserve unrelated files, the saved original directory and existing destination contents.
- Source or destination parent replaced after a child conflict, followed by Skip: later siblings do not traverse the replacement (Copy and Move).
- Source file replaced during a conflict: Replace and Keep Both fail safely, while Skip remains available; no extra copy or overwrite occurs.
- Failures retain exact retry destinations and do not report incomplete roots as completed.
- Existing filesystem tests, including ordinary merges and permission/xattr changes on the same source inode, continue to pass.

These checks cover substitutions while operations are paused and between planned merge steps. They do not claim atomic protection against filesystem changes inside an individual syscall sequence.

## Validation

- Initial source substitution regression: failed before fix, passed after (`red.log`, `green.log`).
- Pending-sibling parent substitution regression: failed before fix, passed after (`parent-red.log`, `parent-green.log`).
- Filesystem regression group passed (`fs-tests.log`).
- Conflict-choice regression passed (`conflict-tests.log`).
- Full release gate and debug build: final results below.

Final validation: 507 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
