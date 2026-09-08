# Transfer audit — round 6

Status: resolved

## Confirmed bug

An asynchronous clipboard response unconditionally replaced the Transfer session clipboard. If the user started Paste, then performed a new Copy or Cut before the response arrived, the earlier response overwrote that newer selection. Completing an imported Move could then clear the imported clipboard, also losing the newer Cut state.

The application regression delays the external response, executes a newer Copy/Cut through App behavior, then executes the earlier Paste and its real filesystem Transfer. Before the fix it failed because the clipboard contained `external.txt` instead of `newer.txt` (`red.log`).

## Fix

Each asynchronous Paste captures a typed local clipboard revision as well as its destination. If the local revision changed before the response arrived, the earlier Paste creates a request independent of the current clipboard. It still executes the originally requested Transfer, but does not replace the newer clipboard, change local Copy ownership, or carry a generation that could clear the newer Cut when it completes.

Unchanged reads retain the normal import behavior, including pending Cut tracking. Both paths share the same request construction rules for empty imports and same-folder Move. The revision uses the existing monotonically advancing local generation counter, so a new Cut followed by cancellation is distinguishable from the original empty clipboard.

## Regression coverage

- Imported Copy and Move, each followed by a newer Copy and Cut (four cases).
- Original Paste completes with correct bytes and source retention/removal.
- Newer clipboard paths, action and generation remain unchanged after completion.
- The next Paste request uses the newer selection.
- New Cut followed by cancellation leaves the clipboard empty while an older Paste still completes.
- The previous destination-after-navigation regression still passes.

Tests use the App and Transfer session boundaries and isolated real filesystem data. Only external clipboard response timing is controlled.

## Validation

- Focused regression failed before the fix (`red.log`) and passed after it (`green.log`).
- Delayed-response regression group: 4 tests passed (`delayed-tests.log`).
- Full release gate and debug build: final results below.

Final validation: 504 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. Debug build passed. The running application was not restarted.
