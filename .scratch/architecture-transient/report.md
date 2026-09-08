# Transient presentation deepening

Base: `0c236f6` on `main`. Implements the Strong candidate selected from the architecture review.

## Ownership

`src/app/transient.rs` now owns coupled presentation edits through a scoped session interface and resolves the visible content, input target, Browser key grammar mode, prompt interaction, and expansion height. Existing sessions still own filesystem work and their data.

The App adapter in `transient_integration.rs` applies presentation effects after edits and at the end of message handling. Animation and browser-status synchronization is private to this adapter. Rendering, keyboard routing, bottom actions, and input focus consume the same resolved presentation. Action-specific focus and selection operations run after restored focus.

Deleting this module would now scatter precedence, replacement, dismissal, and restoration policy across its callers. This adds depth and locality beyond the previous kind-and-height descriptor. No speculative storage or execution adapters were added.

## Behavior and regressions

- Escape dismisses visible Command output before a hidden File operation prompt. The prompt retains its typed input. The regression failed before the policy change (`dismissal-red.log`).
- Copying visible output cannot confirm a hidden permanent-delete prompt.
- Dismissing output reveals the existing Search session, Command session, Rename input, or expanded Transfer history. Restored bottom inputs receive real Iced focus; the adapter regression initially failed (`focus-red.log`).
- Output leaves the visible toolbar Location editor in control of Ctrl+A and Backspace. Review caught a mode-projection regression, reproduced in `location-red.log` and fixed before validation.
- Rename retains initial filename selection after its focus operations. This compatibility test passed before explicit ordering was added; it is not claimed as a reproduced bug.
- Replacing one transient with another retains browser status until the final presentation closes. Synchronization does not restart animation on each frame.

Two descriptor-constructor tests were replaced by App transition coverage. Seven new App tests cover the interactions above, including real Iced text-input state for focus and selection. Existing prompt, busy-operation, immediate Trash, Open With, Transfer, and navigation regressions remain in the suite.

## Validation

- `transient-tests.log`: all seven new interaction tests pass.
- `release-gate.log`: 467 application tests passed, eight opt-in tests ignored; two vendored scrollbar checks and five real-X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and archive smoke tests passed.
- `benchmarks.log`: all seven release-mode performance benchmarks passed their budgets.

Tests use synthetic entries or the existing isolated fixtures. Iced operation checks verify focus and selection without claiming manual desktop interaction.
