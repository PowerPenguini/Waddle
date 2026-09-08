# Recover Undo and Redo interrupted between effects and persistence

Status: ready-for-agent
Type: bug
Priority: P2

`src/journal/store.rs` invokes filesystem effects before saving their updated state. Abrupt process termination in that interval leaves the journal describing the pre-effect filesystem. Reproduced for Copy Undo, Move Undo, Copy Redo, and Move Redo. Reopening and retrying fails on the now-missing old path or the already-existing new path.

Evidence: `../history-matrix.log`, tests matching `audit_*recovers_after_process_exit`. The isolated child exits with code 86 without unwinding, and the parent verifies the completed physical effect before testing recovery.

Acceptance: persist an intent/progress record before effects and reconcile conservatively after restart; never overwrite or remove unrelated externally changed paths. Test both directions, partial batches, and restart boundaries around publication, source cleanup, and journal commit. Power-loss/fsync durability remains a separate validation scope; these reproductions cover process termination only.
