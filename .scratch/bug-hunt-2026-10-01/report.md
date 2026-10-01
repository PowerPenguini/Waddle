# Location monitoring and timestamp bug hunt

Two regression tests reproduced failures before the fixes:

- A change between a folder scan and native watch registration could leave
  displayed entries stale. Newly installed watches now invalidate that earlier
  scan. Resynchronizing unchanged watches emits no extra notification.
- Valid image modification times before 1970 prevented thumbnail generation.
  Fingerprints now retain `SystemTime` directly rather than requiring a positive
  duration since the Unix epoch.

All 795 application tests passed sequentially, with 26 opt-in tests ignored.
Strict Clippy passed. The new watch-registration test and the existing native
queue-overflow test each passed ten consecutive isolated repetitions.
Formatting, whitespace checks, the locked debug build, and FileManager1
activation smoke testing also passed.

The parallel suite again crashed inside GIO with an already-finalized-object
warning. The unresolved volume-monitor follow-up remains in
`../bug-hunt-2026-09-30-thumbnails/issues/01-parallel-gio-volume-monitor.md`.
