# Trash keyboard deletion reports a focused sidebar

Status: resolved

## Reproduction

The application input boundary reproduced the screenshot's exact status after clicking a
Trash entry and pressing `"_dd`:
`File operators are unavailable in the focused sidebar`.
The regression explicitly confirmed that browser focus was Entries.

`cargo test trash_keyboard_delete_opens_confirmation_for_selected_items -- --nocapture`
failed before the fix with that status instead of a permanent-delete confirmation.

Two subsequent red/green tests cover Ctrl+A followed by Delete after sidebar focus,
and misleading feedback for ordinary Cut keys (`d` and `x`) in Trash.

## Diagnosis and fix

Ranked hypotheses were an over-broad location restriction, stale sidebar focus, and
incorrect deletion dispatch. The location restriction was confirmed: the Browser key
grammar only enabled file operators for a focused ordinary folder. Trash reused the
sidebar rejection message. Separately, Ctrl+A selected entries without transferring
focus, and deletion dispatch only attempted a Trash Transfer.

The input context now distinguishes permission to delete from Trash from permission
to operate on an ordinary folder. Delete and black-hole deletion use the existing
File operation session confirmation for the selected Trash receipts. Ordinary Cut
keys remain non-destructive and explain that Delete performs permanent deletion.
Ctrl+A transfers browser focus to the entries it selects. Sidebar-focused Delete
remains inactive.

## Validation

- Three regression tests first failed and then passed at the application input boundary.
- Full `cargo test --all-targets`: 578 passed, 18 ignored, no failures.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo fmt`: applied.
- Tests use synthetic Trash entries and never confirm deletion of user files.

This fixes repository code; the previously installed 0.0.10 release is unchanged.
Native interactive validation of this patch has not been performed.
