# Transfer audit — source consistency, post-effect reads, interrupted history

Status: ready-for-agent

Audited commit: `8009455` (after the two fixes following release 0.0.8).
Date: 2026-09-09. Scope: diagnosis only; no production behavior changed.

## Confirmed findings

| Priority | Defect | Reproduced consequence |
| --- | --- | --- |
| P1 | A failed fingerprint read after Copy/Move Redo leaves the physical effect marked undone | Retry refuses the existing destination (Copy) or missing source (Move). Undo can cross this invisible partial operation and remove an unrelated older New File entry. |
| P1 | Ordinary Copy does not validate source consistency before publishing | A 4 MiB source rewritten after the first chunk yields a successful copy containing 1 MiB of the old bytes and 3 MiB of the new bytes, with zero failures or warnings. Also reproduced with Replace and a sparse source. |
| P2 | Undo/Redo has no durable record of effects in progress | Abrupt process exit after the filesystem effect but before history persistence leaves Copy/Move Undo and Redo unable to resume after reopening. File contents remain at the expected post-effect location in these fixtures; this is confirmed history/recovery loss, not evidence of missing file contents. |

Details, causes, and acceptance criteria are in `issues/`.

## Reproduction and controls

Tests exercise the existing public TransferBatch and Journal boundaries using isolated real temporary files. Copy tests use the progress callback to schedule a writer deterministically between chunks. No production fault hooks were added.

History tests spawn a separate test process. `open_fault.c` interposes `open`/`open64` via LD_PRELOAD only in that child and for one exact temporary pathname, armed by a one-use marker. It either returns EACCES at the post-effect fingerprint read or calls `_exit(86)` there. Copy Undo has no post-unlink fingerprint read, so its exit is injected when opening the journal's temporary output for writing. The parent checks that the fault fired, checks the physical file contents and locations, reopens the real persisted Journal, and attempts recovery.

This verifies abrupt process termination without destructors. It does not simulate power loss, disk-cache loss, kernel failure, or a real disconnected device. The read-failure case verifies handling of a transient filesystem error; it does not claim an observed permission failure on the user's drive.

### Commands

Run from the repository root with Cargo on PATH:

```sh
cargo test fs::tests::audit_ -- --include-ignored --nocapture --test-threads=1
.scratch/transfer-audit-14/run-history-probes.sh
```

Both commands intentionally return failure while the defects remain. `copy_probes.rs` and `history_probes.rs` are included only from the existing test modules. The 11 failing behavioral assertions are marked ignored for explicit diagnostic execution; there is also an ignored subprocess helper and an ignored control requiring the fault-injection runner. They are not fixes or successful regression protection yet.

- Copy matrix: 3 new failing cases (ordinary, Replace, sparse), 2 new passing controls (cross-device Move with and without Replace). The broader name filter also ran 5 existing passing tests.
- History matrix: 8 failing cases (2 post-read-error retry failures, 2 unrelated older-history mutations, 4 abrupt-exit recovery failures), 1 passing behavioral control (same-filesystem Move Undo recovers after the transient fingerprint read error), and 1 helper no-op in the parent.
- Repeated both complete matrices with identical verdicts: `copy-matrix.log`, `copy-repeat.log`, `history-matrix.log`, `history-repeat.log`.
- Standard suite with diagnostics excluded: 525 passed, 21 ignored (8 pre-existing + 13 diagnostic tests/helper/control). Strict Clippy, formatting, and diff checks passed. See `baseline-tests.log` and `clippy.log`.

The passing controls narrow the diagnosis: cross-device Move already validates its source snapshot, and Move Undo marks the completed effect before its post-effect fingerprint read. Ordinary Copy and Redo do not have the corresponding protections. The reproductions and direct state-transition evidence made speculative bisection or broad instrumentation unnecessary.

## Recommended order

1. Record enough post-effect state to recover from fingerprint failures and prevent Undo crossing the affected Redo. Do not simply ignore missing sources or existing destinations; they may be external changes.
2. Validate Copy source consistency before publication, including Replace and sparse paths; preserve the old destination on refusal.
3. Add durable in-progress history with conservative restart reconciliation. Saving more often after effects still leaves a crash window between each effect and its save.

No release, installation, or production fixes were made during this audit.
