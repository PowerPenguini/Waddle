# PolarExp

PolarExp is a graphical filesystem explorer built around keyboard navigation, selection, search, commands, and file operations.

## Language

**Search session**:
An active lookup over explorer entries that retains enough context to submit a match or restore the prior selection when cancelled.
_Avoid_: Search mode, filter state

**Transfer**:
A Copy or Move request for one or more filesystem entries to a destination. Clipboard paste, internal drag, and native drag are ways to initiate the same Transfer.
_Avoid_: Drag operation, file move

**Command session**:
An active `!` or `:` prompt together with the command result PolarExp presents or applies.
_Avoid_: Command mode, shell state

**Grid interaction**:
The file-grid selection together with the pointer, scroll, marquee, hover, and viewport geometry that determine how the user changes it.
_Avoid_: Selection state, grid math

**Browser key grammar**:
The modal meaning of browser key sequences, including Vim motions and pending operators, before they act on another PolarExp concept.
_Avoid_: Shortcut handler, key map

**File operation session**:
An active rename, New Folder, Trash, or permanent-delete interaction that retains its input, entries, and failure context until completion or cancellation.
_Avoid_: Prompt state, delete dialog

**Navigation session**:
The current folder, displayed entries, browsing history, and any pending folder transition whose result PolarExp may accept.
_Avoid_: Navigation state, pending navigation
