# PolarExp roadmap

PolarExp is intended to become a daily Linux file manager for Wayland and X11. It keeps a
standard desktop interface, but gives keyboard users a faster Vim-style grammar. Data safety
takes priority over adding more views or integrations.

This document records the agreed product scope. It is not a release schedule.

## Current baseline

PolarExp already has:

- folder navigation, history, a location field, and a lazy folder tree;
- grid selection, Visual selection, marquee selection, and basic Vim motions;
- rename, New Folder, Trash, and permanent-delete fallback;
- local and recursive filename search;
- internal file drag and drop;
- native Wayland drag and drop between PolarExp and compatible applications;
- a shared internal and Wayland drag preview;
- a private single-file Copy/Paste implementation;
- shell commands, application commands, and terminal launch;
- bounded background work with stale-result rejection.

The current Wayland drag and drop code remains the base for native clipboard work. X11 still
needs external drag and drop and a native clipboard adapter.

## Product rules

- Support Linux on Wayland and X11. Windows and macOS are outside the planned scope.
- Keep ordinary mouse and keyboard behavior available. Vim commands are an additional, faster
  path through the same operations.
- Never hide partial failure, metadata loss, overwrite risk, or an unsafe Undo.
- Keep conflicts, command sequences, and short prompts in the bottom bar instead of modal
  dialogs.
- Run one filesystem mutation at a time. Reads, scans, thumbnails, and other non-mutating work
  may run concurrently.
- Do not escalate file operations to root. Mount operations may use the normal GIO and Polkit
  flow supplied by the desktop.
- Keep all history and diagnostics local. PolarExp has no telemetry.
- Keep the interface in English. Translation support is not planned.
- Keep `!` shell execution available without a warning prompt. Documentation must state plainly
  that these commands run with the user's permissions and are not sandboxed.

## Milestone 1: system clipboard, Vim operators, and Undo

### System clipboard

- [x] Replace the private single-path clipboard with a multi-entry system CLIPBOARD model.
- [ ] Support Copy and Cut between PolarExp, Nautilus, and Dolphin in both directions.
- [x] Implement the clipboard on both Wayland and X11.
- [x] Publish `text/uri-list`, `x-special/gnome-copied-files`, and
  `application/x-kde-cutselection` together.
- [x] Treat a missing, malformed, or contradictory Cut marker as Copy, never Move.
- [ ] Read all MIME payloads from one clipboard offer and generation.
- [x] Accept only absolute local `file:` URIs until remote locations are implemented.
- [x] Apply byte and entry-count limits before accepting clipboard data.
- [ ] Revalidate sources, permissions, descendants, and the destination immediately before
  Paste.
- [x] Clear Cut only if the clipboard still contains the same generation.

### Cut behavior

- [x] Hide pending Cut entries from the grid immediately without changing the filesystem.
- [x] Show a persistent bottom-bar status such as `Cut: 4 items, p paste, Esc cancel`.
- [x] Restore hidden entries when Cut is cancelled or PolarExp loses clipboard ownership.
- [ ] Let filesystem monitoring confirm when another application actually moves the entries.
- [x] Treat Cut followed by Paste into the same directory as a no-op.
- [x] Let Copy into the same directory create a non-conflicting copy name.
- [x] After a partial Move, keep only the failed entries in the Cut clipboard.

### Vim grammar

- [x] Make `y` copy the current selection. Yank is a single-key action and does not accept a
  motion.
- [x] Make `dd` cut the active entry.
- [x] Make `d{motion}` cut the inclusive range between the active entry and the motion target.
- [x] Make `x` cut the current selection.
- [x] Make `"_dd`, `"_d{motion}`, and `"_x` move entries to Trash without replacing the system
  clipboard.
- [x] In Visual selection, make `y`, `d`, and `x` act on the complete selection.
- [x] Keep `p` as Paste into the current directory.
- [x] Add `gg`, `G`, `{count}G`, `H`, `M`, `L`, `Ctrl+D`, and `Ctrl+U`.
- [x] Support counts before motions and operators. Examples include `3j`, `4l`, `3dd`, `d3j`,
  `3dj`, and `5G`.
- [x] Interpret operator ranges in current display order so Grid and List behave identically.
- [x] Show incomplete sequences such as `3`, `d`, `g`, and `"_` in the bottom bar.
- [x] Do not time out an incomplete sequence. `Esc` cancels it, and an invalid key resets it with
  a short message.
- [x] Use `u` for Undo, `Ctrl+R` for Redo, `Ctrl+O` for Back, and `Ctrl+I` for Forward.
- [x] Give the focused sidebar its own Vim behavior: `j` and `k` move through visible nodes, `h`
  collapses or moves to the parent, `l` expands or opens, and `gg` and `G` jump to the ends.
- [x] Disable file operators on technical sidebar roots such as Computer and Recent.
- [x] Limit 1.0 to the system clipboard and the `"_` black-hole register. Named registers, macros,
  and `.` repeat remain reserved.

### Conflict handling

- [x] Present file conflicts in the bottom bar.
- [x] Use `r`, `s`, and `k` for Replace, Skip, and Keep Both on one conflict.
- [x] Use `R`, `S`, and `K` to apply the corresponding rule to the remaining batch.
- [x] Merge conflicting directories without deleting the existing directory tree.
- [x] Route conflicts inside a directory merge through the same bottom-bar flow.
- [x] Use an atomic no-clobber implementation for Rename and Replace.

### Persistent Undo and Redo

- [x] Store the last 100 operations or the last 30 days of history, whichever limit is reached
  first.
- [x] Cover Rename, Move, Trash, New Folder, and Copy.
- [x] Never claim that permanent delete can be undone.
- [x] Remove a copied result during Undo only if it is still the unchanged result of that Copy.
- [x] Remove a newly created directory during Undo only if it is still empty.
- [x] Refuse Undo when later filesystem changes make the inverse operation unsafe.
- [x] Keep Redo available until the next new mutation.
- [x] Execute safe Undo immediately and report the result in the bottom bar.
- [x] Persist the operation report needed for recovery after restarting PolarExp.

### Copy fidelity

- [x] Preserve file contents, timestamps, permissions, extended attributes, ACLs, symlinks, and
  sparse-file layout where the filesystem permits it.
- [x] Preserve hardlink relationships inside a copied tree where possible.
- [x] Report unsupported or lost metadata instead of silently dropping it.

## Milestone 2: live directory state

- [ ] Monitor the current directory and other displayed locations for external changes.
- [x] Debounce bursts without delaying normal interactive updates.
- [x] Preserve selection and scroll position by path rather than by list index.
- [x] Keep Rename, command input, conflict handling, and pending Cut active during refresh.
- [x] Refresh active local and recursive search results.
- [x] Keep pending Cut entries hidden until Cut ends.
- [x] If the current directory disappears, navigate to the nearest existing ancestor and report
  what happened.
- [x] Add manual Refresh through `F5`, a toolbar action, and `:refresh`.

## Milestone 3: transfer queue

- [x] Run filesystem mutations through one ordered queue.
- [x] Show the active operation in the bottom bar.
- [x] Expand the bottom area to show queue history, bytes, entry counts, speed, estimated time,
  Cancel, Retry, and error details.
- [x] Keep successful items after a partial operation and retain failed items for Retry.
- [x] Let users Undo the successful part of a partial operation when it is still safe.
- [x] Write copies to private temporary destinations and reveal them atomically where the
  filesystem supports it.
- [x] On Cancel, remove only incomplete results created by the cancelled operation.
- [x] Never alter files that existed before the operation started.
- [x] Keep operation reports and diagnostics for 30 days and allow copying a report.
- [x] Bound shell output while the command runs, not only after it exits.

## Milestone 4: complete browser interaction

### Selection and activation

- [x] Add `Ctrl+click`, `Shift+click`, `Ctrl+A`, arrow navigation, `Shift+arrows`, Space, Home,
  and End.
- [x] Keep Visual selection and Vim motions as equivalent keyboard paths.
- [x] Support configurable single-click or double-click activation, with double-click as the
  default.
- [x] Add breadcrumbs and switch to editable location input with `Ctrl+L`.

### Views and sorting

- [x] Add List view alongside Grid.
- [x] Sort by Name, Modified, Size, or Type in either direction.
- [x] Use natural filename ordering.
- [x] Make folders-first configurable.
- [x] Store global view defaults and optional per-directory overrides.
- [x] Add image thumbnails with asynchronous generation and a bounded cache.

### Hidden files and search

- [x] Toggle hidden files with `Ctrl+H`.
- [x] Include hidden entries in local and recursive filename search only while hidden files are
  visible.
- [ ] Keep 1.0 filename-only search. Full-text search and structured filters belong to 1.1.

### Drag and drop

- [ ] Add external X11 drag and drop.
- [x] Autoscroll the grid and sidebar while dragging near an edge.
- [x] Expand sidebar folders after a short hover.
- [x] Enter a folder after a longer hover and show progress toward activation.
- [x] Let moving the pointer away cancel hover activation.

## Milestone 5: locations and desktop file actions

### Sidebar locations

- [x] Add Home.
- [x] Show Desktop, Documents, Downloads, Music, Pictures, and Videos when their XDG directories
  exist. Do not create missing directories.
- [x] Add Recent using the desktop's shared recent-file history.
- [x] Let users clear or disable Recent.
- [x] Add user Favorites with custom labels and drag reordering.
- [x] Keep mounted volumes and add their normal mount, unmount, and eject actions.

### Trash

- [x] Add Trash as a sidebar location.
- [x] Restore selected entries to their original paths.
- [x] Resolve Restore conflicts through the bottom bar.
- [x] Permanently delete selected Trash entries after confirmation.
- [x] Empty the whole Trash after confirmation.
- [x] Keep Trash Undo available without opening the Trash view.

### File actions

- [x] Add Properties with MIME type, location, size, dates, permissions, and default application.
- [x] Let users edit permissions that their current account is allowed to change.
- [x] Add Open With and default-application selection.
- [x] Add New Empty File.
- [x] Create files from the user's XDG Templates directory.
- [x] Keep ACL and extended-attribute editing outside 1.0.

## Milestone 6: settings, accessibility, and release

### Settings without a Preferences screen

- [x] Keep common controls such as Grid/List, sorting, and hidden files discoverable in the
  toolbar or menus.
- [x] Use `:set` for advanced settings and `:setlocal` for per-directory overrides.
- [x] Make `:set` show current values and `:set all` show every option with a short description.
- [x] Add completion and validation for option names and values.
- [x] Persist settings by default.
- [x] Let `:setlocal option&` remove a directory override.
- [ ] Include settings for view, sorting, folders-first, hidden files, click activation, contrast,
  reduced motion, and startup behavior.

### Keyboard access and display preferences

- [x] Treat the toolbar, location control, sidebar, grid, and bottom bar as composite Tab stops.
- [x] Keep arrow navigation inside the sidebar and grid.
- [x] Add visible focus rings independent of hover and selection.
- [x] Trap focus inside active prompts and menus, then restore the previous focus on close.
- [x] Support Enter and Space activation where standard desktop controls expect them.
- [x] Add a high-contrast palette with opaque surfaces and non-color state indicators.
- [x] Respect reduced-motion and reduced-transparency preferences.
- [x] Stop the 16 ms animation subscription when reduced motion is active.
- [ ] Do not make screen-reader support a 1.0 release gate while stock Iced lacks an accessibility
  tree.

### Startup and desktop integration

- [x] Accept `polarexp [PATH|file://URI]`.
- [x] Open each invocation in a new process and window for 1.0.
- [x] Let an explicit CLI location override remembered startup state.
- [x] Remember window size and position, the last directory, view overrides, sorting, Favorites,
  and Undo history.
- [x] Do not restore every previous window automatically.
- [ ] Add a `.desktop` file with `%U`, application icons, and AppStream metadata.
- [ ] Publish a Flatpak and a regular binary archive.

### Release gate

Do not publish 1.0 until all of the following pass:

- [ ] Copy and Cut in both directions with Nautilus and Dolphin.
- [ ] Clipboard, drag and drop, navigation, and file operations on Wayland and X11.
- [ ] A large transfer with progress, Cancel, Retry, partial failure, and Undo after restart.
- [ ] A directory with at least 10,000 entries.
- [ ] Manual conflict, Trash, Restore, clipboard-loss, and pending-Cut tests.
- [ ] Keyboard-only navigation, focus restoration, high contrast, and reduced motion.
- [ ] Flatpak and regular binary smoke tests.
- [ ] Formatting, unit tests, integration tests, Clippy with warnings denied, and a release build.

An Orca test and screen-reader semantics are not part of the 1.0 gate.

## After 1.0

The following work may start after the local 1.0 file manager is stable:

- GVfs network locations such as SFTP and SMB;
- system-indexed content search;
- filename search filters for type, modified date, and size;
- Bulk Rename with numbering, find and replace, prefixes, suffixes, and conflict preview;
- PDF and video thumbnails;
- tabs, split view, and quick preview;
- named Vim registers and carefully designed repeat behavior.

## Not planned

- Windows or macOS support;
- built-in archive creation, extraction, or browsing;
- custom user actions beyond shell and terminal commands;
- a root file-operation mode;
- application telemetry;
- interface translations.
