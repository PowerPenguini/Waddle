# 09: Atomic Rename and Replace

Type: feature

**What to build:** Make Rename and Replace resist concurrent destination creation without silent overwrite.

**Blocked by:** 08: Bottom-bar Transfer conflict workflow.

**Status:** resolved

- [x] Rename and Replace use an atomic no-clobber operation where supported.
- [x] A concurrent collision returns to the conflict workflow.
- [x] Symlink identity and destination safety remain intact.
- [x] Race-focused filesystem tests demonstrate the behavior.

## Answer

Linux Rename uses `renameat2(RENAME_NOREPLACE)`. Replace stages Copy operations and uses
`RENAME_EXCHANGE`; Move uses the exchange directly when both paths share a filesystem. The
displaced destination is compared with the inode, device, and file kind observed by the conflict
prompt. If it changed, PolarExp exchanges the paths back and reopens the conflict instead of
silently replacing the newcomer. All checks use `symlink_metadata`, so a link itself is moved or
replaced rather than its target.
