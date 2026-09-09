# Transfer round 19 — safe operation-journal writes

Status: resolved

## Confirmed defects

1. Recording a Transfer followed a symlink at the fixed `.json.tmp` journal path, truncated an unrelated file, and published the link as the journal. A pre-existing hardlink or regular file could also be overwritten; a directory at that name disabled history saving. The public Journal regression reproduced the overwrite before the fix (`temp-red.log`). Saving now reserves a file with `create_new`, skips occupied names, and uses a bounded sequence of alternate candidates. Existing files, directories, and links are never opened for writing as temporary state. The four-case regression preserves them and successfully reopens/undoes the new Transfer (`temp-green.log`).
2. New history files were created with default permissions (0644 on this machine), exposing recorded paths and metadata when the containing directory was accessible. The privacy regression failed before the fix (`privacy-red.log`). New pending journal files are created with mode 0600; atomic replacement carries these permissions to the published journal (`privacy-green.log`).

## Write failure and cleanup

The pending file has an ownership guard. A failed write/flush/rename drops it and attempts to remove only the created inode; successful rename consumes its temporary path. Cleanup checks device/inode identity before unlinking so a substituted entry is not deliberately removed. As with ordinary path operations, this is not an adversarial race-proof directory capability system.

An injected fsync EIO in an isolated child verifies that the previous committed journal stays byte-identical, the incomplete pending file is removed, and recording/Undo works after the fault ends (`sync-error.log`). The parent asserts the fault was actually reached. The existing sidecar lock, reload behavior, file fsync, atomic rename and directory fsync remain in place. A directory-fsync failure after rename is still an uncertain durability outcome; it must not be treated as proof that the rename never happened.

## Validation and scope

The regressions use the established Journal interface and isolated temporary files. Only the fsync error is injected, using the existing syscall shim in a child process. No user history, desktop Trash, or removable drive is modified.

The full release gate is recorded in `release-gate.log`. The worktree also contains the user's pending icon redesign; this round stages only journal storage code, its tests and round artifacts.

The audit is still open. Inspection also identified the boundary between prepared copy data and a failed publication checkpoint as a next investigation target; this round does not claim to implement durable ownership or garbage collection for all prepared Transfer staging. Ordinary queue restart persistence and pre-checkpoint abandoned copies remain open as documented in round 16. No release, push or installation was performed.

Final validation: 557 main tests passed, 14 ignored (including the explicitly invoked new fault-injection helper); 2 scrollbar tests and 5 real-X11 tests passed. Formatting, strict Clippy, release build, FileManager1, desktop/AppStream validation and archive smoke passed. The main count includes one pending icon regression outside this commit.
