# 08: Bottom-bar Transfer conflict workflow

**What to build:** Resolve file and directory conflicts through the bottom bar without a modal dialog.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp.

**Status:** resolved

- [x] Replace, Skip, and Keep Both work for one conflict.
- [x] Uppercase choices apply to the remaining batch.
- [x] Directory conflicts merge without deleting the existing tree.
- [x] Escape cancels unresolved work without touching pending entries.

## Answer

Transfers now pause before touching a conflicting source and expose the decision in the bottom
bar. Lowercase `r`, `s`, and `k` resolve one collision; uppercase applies the same policy to the
remaining batch. Replace on two real directories queues their children through the same conflict
engine, so the existing directory tree is merged rather than deleted. Escape reports untouched
roots as retained, which also keeps a pending Cut available for retry.
