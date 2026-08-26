# 01: Multi-entry clipboard Transfer inside Waddle

Type: feature

**What to build:** Copy the complete current selection into one generation-aware clipboard payload and Paste it as one Transfer inside Waddle.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] Copy stores every selected path once in display order.
- [x] A single active entry still copies and pastes as before.
- [x] Paste creates one Copy Transfer for the complete payload.
- [x] Completion selects the pasted results and leaves Copy reusable.
- [x] Behavior is covered at the Transfer workflow and application seams.

## Answer

Implemented a shared `ClipboardPayload` containing ordered paths, Copy intent, and a generation identifier. Waddle now copies the complete visual selection into one reusable payload, pastes it as a single Transfer, and restores every successfully pasted result as the refreshed selection. Workflow, navigation, and application-seam tests cover multi-entry order, generation replacement, single-entry compatibility, reusable Copy, and multi-result selection.
