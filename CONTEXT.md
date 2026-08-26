# Waddle

Waddle is a graphical filesystem explorer built around keyboard navigation, selection, search, commands, and file operations.

## Language

**Search session**:
An active lookup over explorer entries that retains enough context to submit a match or restore the prior selection when cancelled.
_Avoid_: Search mode, filter state

**Transfer**:
A Copy or Move request for one or more filesystem entries to their destination paths. Clipboard paste, internal drag, native drag, and restoring Trash entries are ways to initiate the same Transfer.
_Avoid_: Drag operation, file move

**Transfer session**:
The queued and active Transfers together with clipboard ownership, progress, conflict choices, completion feedback, and retained history.
_Avoid_: Transfer workflow, transfer queue state

**Command session**:
An active `!` or `:` prompt together with the command result Waddle presents or applies.
_Avoid_: Command mode, shell state

**Grid interaction**:
The file-grid selection together with the pointer, context target, scroll, marquee, hover, and viewport geometry that determine how the user changes it.
_Avoid_: Selection state, grid math

**Sidebar tree**:
The hierarchical Computer, places, mounted drives, and expanded folders together with the focused row that the user can navigate or choose as a Transfer destination.
_Avoid_: Explorer state, folder tree

**Browser key grammar**:
The modal meaning of browser key sequences, including Vim motions and pending operators, before they act on another Waddle concept.
_Avoid_: Shortcut handler, key map

**File operation session**:
An active rename, New Folder, Trash, or permanent-delete interaction that retains its input, entries, and failure context until completion or cancellation.
_Avoid_: Prompt state, delete dialog

**Transient presentation**:
The temporary bottom-bar presentation of a Command session, File operation session, Open With choice, or expanded Transfer session history, including restoration of the underlying browser status when it closes.
_Avoid_: Bottom-bar mode, modal state

**Navigation session**:
The current folder, displayed entries, browsing history, and any pending folder transition whose result Waddle may accept.
_Avoid_: Navigation state, pending navigation

**Location monitoring**:
Observation of filesystem locations whose changes can affect displayed entries, pending Cut entries, or expanded navigation, including fallback when notifications are unavailable.
_Avoid_: Directory watch, file watcher
