# Follow-up Transfer audit

Date: 2026-09-08
Revision: 0cf4dea
Scope: diagnosis and fixes for remaining Transfer defects after the previous four fixes.

## Fix outcome

The five reproductions below are now permanent tests and pass. Three additional tests cover changed extended attributes, nested Skip with exact retry targets across filesystems, and failed mapped cross-device Move Replace.

### Replace publication and cleanup

Replace checks removal permissions throughout the old directory tree before exchange. The staged-copy path checks again after copying, before exchange. Existing protected directories therefore fail before anything is removed, preserving the incoming source, all old contents, and retry information.

After publication, destructive cleanup is committed: it cannot be rolled back by renaming a partially deleted old tree. If that cleanup fails, the incoming destination is retained and the operation returns a warning naming the remaining old data. Same-filesystem Move first moves the old destination out of the source path into a private backup name; failure of that rename is still reversible because no cleanup has begun. This prevents Retry from treating old destination data as incoming source data.

The permission preflight is not a lock against concurrent filesystem changes. Late cleanup failures deliberately use the completed-with-warning behavior above. The deterministic tests cover preflight rejection with complete preservation; the late cleanup warning policy was reviewed in code, without adding timing-dependent tests.

### Skip and hardlink metadata

Skip retains only the conflicting entry for retry and removes pending ancestor-directory cleanup that would conflict with deliberately retained descendants. Siblings continue transferring, including in nested merges and mapped cross-device Move used by Restore.

Hardlink reuse now verifies permissions and full extended-attribute values (including ACL attributes) on both the source and the cached destination. An unreadable or changed metadata snapshot disables reuse, so a fresh copy receives current source metadata. Unchanged hardlinks still survive merged cross-device Move, even though unlinking source siblings changes ctime. If source metadata changes between copies, older published copies keep their original snapshot rather than being silently modified through a shared inode.

### Validation

`fix-red.log`: all five original regressions failed before changes. `initial-green.log`: those five pass. `xattr-red.log`: changed extended attributes reproduced separately before the additional fix. `fix-green.log`: 74 filesystem tests pass before the final two additional scenarios.

The full release gate passed with 491 tests passed and 8 ignored, 2 scrollbar tests, strict Clippy, a release build, FileManager1 activation, 5 real X11 tests, desktop metadata checks, and archive smoke tests (`release-gate.log`). All tests use temporary files. No live Transfer was interrupted.

All 7 performance benchmarks passed (`benchmarks.log`). The debug executable was also rebuilt successfully (`debug-build.log`).

The original audit below describes the pre-fix revision; its saved patch should only be applied there.

Three findings confirmed by five failing filesystem tests in two consecutive runs. Production code is unchanged. Tests are saved in `reproduction-tests.patch`; `red.log` and `red-repeat.log` record the results. Fixtures use only temporary directories.

## P1 — Failed Replace restores a partially deleted old destination

Locations: `src/fs/mutation.rs:209` and `src/fs/mutation.rs:295`.

Reproduction: replace a destination directory with an incoming regular file. The old destination contains two subdirectories, each containing a file. Make the last subdirectory in directory iteration order nonwritable. Cleanup deletes the first child, then fails on the protected child. The error path exchanges the old directory back, but its first child's data is already gone. The incoming source is preserved and the operation reports failure.

Both Copy and same-filesystem Move reproduce this. It extends the existing rollback tests, which protected the sole old child and therefore never exercised partial deletion. The same staging cleanup is also used by cross-device Move Replace, but that variant was not separately exercised here.

Tests: `audit_failed_copy_replace_preserves_all_old_children` and `audit_failed_move_replace_preserves_all_old_children`.

Expected: an operation rolled back as failed retains the complete old destination. A recursive deletion cannot be undone by renaming the partially deleted directory. The fix needs an explicit publication/cleanup contract with recoverable old data, rather than assuming cleanup failure leaves the old tree intact.

The fixture asserts it is running as a non-root user and chooses the protected child from actual directory order; it does not rely on lexical order.

## P2 — Skip on one merged child skips unrelated siblings

Location: `src/fs/transfer_batch.rs:414`.

Reproduction: source folder contains `a` and `b`; destination folder contains only `a`. Choose Replace for the folder merge, then Skip for the conflict on `a`, without applying the decision to remaining conflicts. Destination `a` stays intact, but `b` is never transferred. Both Copy and Move reproduce this.

Cause: the Skip branch drains every pending item with the same root index, rather than retaining only the conflicting entry/subtree. It unintentionally gives a single-child decision the scope of the entire top-level folder. For Move, the eventual cleanup of ancestor directories also needs to account for intentionally retained descendants without reporting a spurious failure.

Tests: `audit_skip_copy_child_still_transfers_sibling` and `audit_skip_move_child_still_transfers_sibling`.

## P2 — Cached hardlinks reuse stale access permissions

Locations: `src/fs/tree_copy.rs:22` and `src/fs/tree_copy.rs:120`.

Reproduction: merge a source folder with hardlinked names `a` and `b`, initially mode 0644. After `a` is copied, pause at the destination conflict on `b`. Change source permissions to 0600, then choose Replace. Destination `b` is created with mode 0644, despite the current source being 0600. Copy reports no failure.

Cause: the recently added batch-wide hardlink cache checks identity, size and mtime, but not permission/metadata changes. chmod leaves mtime unchanged. The hardlink shortcut returns before metadata preservation, so it reuses the first destination inode's older, more permissive mode.

Test: `audit_hardlink_copy_uses_current_source_permissions_after_conflict`.
Observed mode: decimal 420 (0644), expected decimal 384 (0600).

The fix must reconcile metadata changes when reusing hardlinks. ctime-only invalidation needs care: removing a source hardlink during Move also updates ctime on remaining siblings.

## Reproduction

From the audited revision, with an unchanged `src/fs/tests.rs`:

```sh
git apply .scratch/transfer-followup-audit/reproduction-tests.patch
PATH=/home/powerpenguini/.cargo/bin:$PATH cargo test fs::tests::audit_
git apply -R .scratch/transfer-followup-audit/reproduction-tests.patch
```

Expected current result: five failures, no passes. The patch was checked with `git apply --check` after restoring the original test file. These are diagnostic reproductions, not implemented fixes.

After removing the diagnostic patch, all 68 existing filesystem tests passed (`baseline.log`). No full release gate was needed for this diagnosis-only scan; no running application was restarted.
