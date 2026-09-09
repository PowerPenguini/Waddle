# Bug hunting, 2026-09-09

Each round uses a failing behavioral regression before its fix, followed by a
commit and push to `main`. The user approved filesystem operations, journal
Undo/Redo, and app messages as test boundaries. Existing scratch artifacts are
outside this work.

## Round 1: queued recursive search results

- Reproduction: finish a real recursive search worker, retain its output message,
  change the query, then deliver the retained message through `App::update`.
- Red: `cargo test recursive_search_ignores_a_completed_result_queued_before_the_query_changed -- --nocapture`
  failed because the old result marked the new search finished.
- Cause: worker cancellation cannot retract an output already queued for the UI;
  search completions lacked request identity.
- Fix: Search sessions assign revisions to requests and ignore obsolete results.
- Green: the same regression passed; `cargo test --all-targets --quiet` passed
  (596 passed, 19 ignored); Clippy with warnings denied and formatting passed.
