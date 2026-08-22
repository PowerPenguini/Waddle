# 32: Bounded command output and local diagnostics

Type: feature

**What to build:** Keep shell execution direct while bounding live output and integrating failures with local diagnostic history.

**Blocked by:** 15: Transfer Cancel, Retry, partial results, and history.

**Status:** resolved

- [x] Output remains bounded while a command is running.
- [x] Truncation is explicit and keeps the useful tail or summary.
- [x] Command failures produce copyable local reports.
- [x] Documentation states that shell commands use the user's permissions and are not sandboxed.

## Answer

The streaming readers cap each live stream, preserve a 4 KiB tail, and insert an explicit
truncation marker before the command completes. Non-zero exits, launch failures, and rejected
terminal-screen commands now enter a local 30-day, 100-record diagnostic history. `:diagnostics`
shows that history and the bottom bar copies the current command report. The README now states
plainly that Bash is unsandboxed and runs with the current user's filesystem permissions.
