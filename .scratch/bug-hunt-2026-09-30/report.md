# Bug hunt — 2026-09-30

Baseline: `efee6f4` (Waddle 0.0.25), fetched and fast-forwarded from
`origin/main` before investigation. The starting working tree was clean.

## Findings

1. **Broken symbolic links cannot be revealed.** `launch::location` and
   `launch::show_items` require target metadata, so an existing dangling link
   is rejected as missing. Inspect the link itself to validate existence;
   continue following directory links when opening a folder. Regression covers
   ordinary launch paths and ShowItems paths/file URIs.
2. **Recent uses insertion order instead of visit times.** Reversing XBEL URI
   order does not place the most recently visited entry first, particularly
   when an existing bookmark is revisited. Sort by visit time, falling back to
   modification time when visit time is absent, with deterministic path ties.
   Regression covers out-of-order timestamps and revisiting an old bookmark.
3. **Startup settings share a temporary filename.** Concurrent windows can
   truncate or rename each other's `startup.json.tmp`. A stale symlink at that
   filename also directs settings writes into an unrelated file. Use a unique
   temporary file in the destination directory and atomically persist it.
   Regressions cover preserving another window's pending temporary file and
   preserving a stale symlink's target, alongside existing geometry and
   non-UTF-8 directory round-trip coverage.

## Verification

Before the fixes, `cargo test --locked --all-targets` reproduced all four new
regressions: **785 passed, 4 failed, 26 ignored**. No existing test failed.

After the fixes, `cargo test --locked --all-targets` passed: **789 passed,
0 failed, 26 ignored**. `cargo fmt --all -- --check` passed.

After replacing an unnecessary clone in a test assertion, the affected launch
suite passed again: `cargo test --locked launch::tests` — **7 passed**.

Additional checks passed:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --locked`
- `scripts/smoke-file-manager-service.sh target/debug/waddle`
- Desktop entry validation and AppStream metadata validation
- `git diff --check`

Verification used Rust 1.98.1 and the repository's configured development
profile. Missing native dependencies were supplied from a temporary local
prefix. This workspace has no X11 or Wayland display; interactive desktop and
ignored GPU tests were not exercised.
