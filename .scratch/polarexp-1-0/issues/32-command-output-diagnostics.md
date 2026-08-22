# 32: Bounded command output and local diagnostics

**What to build:** Keep shell execution direct while bounding live output and integrating failures with local diagnostic history.

**Blocked by:** 15: Transfer Cancel, Retry, partial results, and history.

**Status:** ready-for-agent

- [ ] Output remains bounded while a command is running.
- [ ] Truncation is explicit and keeps the useful tail or summary.
- [ ] Command failures produce copyable local reports.
- [ ] Documentation states that shell commands use the user's permissions and are not sandboxed.
