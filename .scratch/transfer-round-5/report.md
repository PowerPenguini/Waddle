# Transfer audit — round 5

Status: resolved

## Confirmed bug

A Paste waiting for the native clipboard read chose its destination only after the read completed. Navigating from folder A to folder B during that delay redirected the Transfer into B, although Paste had been requested in A. Both imported Copy and Move used this path.

## Reproduction and fix

The regression creates the production clipboard Task while folder A is displayed, delays the external clipboard response with a oneshot, completes real navigation to folder B, then releases the response and executes the resulting Transfer through App message handling. It uses isolated temporary files and an isolated Transfer session history. Before the fix, the assertion that the file was pasted into A failed (`red.log`).

The clipboard completion message now carries the destination captured at request time. The response handler passes that destination to the Transfer session instead of reading the current folder again. `paste_clipboard` extracts the production asynchronous branch so the test can supply a delayed OS response without depending on a live desktop clipboard.

## Regression coverage

The same test runs Copy and Move. It verifies the resulting bytes in A, absence in B, source retention for Copy and removal for Move, and a completed Transfer session. Navigation and all filesystem operations run through the real application tasks; only the external response timing is controlled.

## Validation

- Before fix: regression failed as expected (`red.log`).
- After fix: Copy and Move scenarios passed (`green.log`).
- Full release gate and debug build: final results below.

This validates application handling of delayed clipboard responses. Native Wayland/X11 protocol behavior remains covered separately by the existing adapter tests.

Final validation: 502 tests passed, 8 ignored; 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1 smoke, desktop metadata, archive smoke and diff checks passed. The debug build passed; the running application was not restarted.
