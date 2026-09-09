# Wrong-file marquee selection after resizing

## Reproduction and cause

Scroll a three-row grid in a short window, then enlarge the window until all
entries fit. Drag a marquee across the first two rows. Before the fix, the real
widget-layout reproduction selected indices `{8, 9, 15, 16}` while the rectangle
intersected tiles `{1, 2, 8, 9}`. Repeated runs reproduced the same one-row offset.

The vendored Iced scrollable suppressed viewport notifications whenever content
no longer overflowed. Its rendered offset clamped to zero, but Grid interaction
retained the previous scroll offset. Removing that early return keeps the
reported viewport and pointer hit testing aligned. Existing viewport comparison
still suppresses repeated notifications.

## Verification

- `cargo test --locked marquee_selects_the_tiles_inside_the_rendered_rectangle -- --ignored --nocapture`
  failed before the fix and passes afterward. This exercises the real browser
  widget tree, scrolling, resize, viewport messages, and marquee selection.
- `cargo test --locked -p iced_widget --lib`: all four tests pass. The added
  regression covers both viewport enlargement and content shrinkage, including
  suppression of duplicate notifications; it failed before the fix.
- `cargo test --locked --all-targets`: 590 passed, 19 ignored. The GPU-dependent
  layout regression is among the ignored tests and was run explicitly above.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- Formatting of the new test and `git diff --check`: passed. Repository-wide
  `cargo fmt --all -- --check` flags unrelated changes in
  `src/app/tests/focus.rs`; those changes were preserved.

The fix changes `vendor/iced_widget/src/scrollable.rs`; the browser regression
lives in `src/app/tests/grid_layout.rs`, registered in `src/app/view.rs`.

## Follow-up: marquee did not start in Trash

The mouse-press handler required `mutations_allowed()`, which excludes virtual
locations such as Trash. Selection does not change files, so its guard now keeps
the existing busy/loading/prompt restrictions without requiring a regular folder.

`cargo test --locked trash_marquee_selects_entries_in_grid_and_list_views -- --nocapture`
failed before this change because the marquee never started. It now passes for
both Grid and List views, checking multiple selected entries, Browser focus, and
selection persistence after mouse release. `cargo test --locked --all-targets`
passes with 595 tests and 19 ignored; repository formatting and diff checks pass.
