# 09: Atomic Rename and Replace

**What to build:** Make Rename and Replace resist concurrent destination creation without silent overwrite.

**Blocked by:** 08: Bottom-bar Transfer conflict workflow.

**Status:** ready-for-agent

- [ ] Rename and Replace use an atomic no-clobber operation where supported.
- [ ] A concurrent collision returns to the conflict workflow.
- [ ] Symlink identity and destination safety remain intact.
- [ ] Race-focused filesystem tests demonstrate the behavior.
