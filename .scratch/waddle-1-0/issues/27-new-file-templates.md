# 27: New Empty File and XDG Templates

Type: feature

**What to build:** Create empty files and template-based files through the normal File operation session.

**Blocked by:** 11: Persistent Undo and Redo for Rename and New Folder.

**Status:** resolved

- [x] New Empty File validates the name and handles conflicts in the bottom bar.
- [x] Existing XDG Templates are available without creating the directory.
- [x] Template copying preserves the source template.
- [x] Successful creation supports restart-safe Undo.

## Answer

The normal context menu now offers New Empty File and one New from action per regular file in the
existing XDG Templates directory. Missing Templates directories are ignored and never created.
Both paths use the existing inline bottom-bar name prompt and atomic no-overwrite creation;
validation and name conflicts stay in that prompt. Template creation copies metadata through the
normal safe copy path without changing the source. Empty files have their own persistent journal
action, while template results reuse persistent Copy Undo/Redo.
