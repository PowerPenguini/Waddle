# Transfer audit — round 12

Status: resolved

## Confirmed bugs

The journal cursor advances only after an entire Undo/Redo succeeds. After a partial Redo, Undo still selected the older entry at cursor minus one; after a partial Undo, Redo selected the newer entry at cursor. This let an opposite-direction command operate on unrelated history while a Transfer was still incomplete.

The first public Journal regression reproduced Undo crossing a partial Copy Redo into an earlier New File action (`red.log`). The second reproduced Redo recreating an unrelated newer file while Copy Undo remained incomplete (`undo-red.log`). Both fixtures reopen the persisted journal before the opposite-direction command.

## Fix

Actions expose whether their stored state represents partial effects: mixed completed/pending Transfer items, an active directory removal plan, or a physical Trash restoration pending metadata cleanup. Undo and Redo check the adjacent cursor boundary before selecting an action.

An opposite-direction command now returns an explicit instruction to retry the incomplete operation, without changing filesystem contents or cursor state. Continuing in the original direction remains allowed. Once it succeeds, ordinary navigation through history resumes. No journal schema change is needed.

## Regression coverage

- Partial Redo cannot make Undo remove a file from an older operation.
- Partial Undo cannot make Redo recreate a file from a newer operation.
- Both cases remain protected after reopening the journal and return to normal Undo/Redo after permission repair and successful retry.
- A single partially deleted directory remains protected even though it is only one Transfer item.
- Trash Undo awaiting metadata cleanup cannot be bypassed by Redo.

All effects use isolated temporary filesystem and Trash fixtures.

## Validation

- Both direction regressions failed before their fixes (`red.log`, `undo-red.log`).
- Both passed after their fixes (`green.log`, `undo-green.log`).
- Journal tests: 40 passed (`journal-tests.log`).
- Full release gate and debug build: final results below.

Next audit area: recording new operations while earlier history still contains partial effects.

Final validation: 521 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
