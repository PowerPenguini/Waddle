# Waddle 1.0 daily Linux file manager

Status: ready-for-human

## Problem Statement

Waddle already covers fast navigation, Grid interaction, filename search, file operations,
commands, and native Wayland drag and drop. It is still not dependable enough to replace a
desktop file manager for daily work.

The current clipboard holds one private path and cannot exchange a complete Copy or Cut with
other applications. External changes can leave the displayed folder stale. Long Transfers have
no useful progress or cancellation. Conflict handling differs between Copy and Move. Undo does
not survive a restart, and Copy does not preserve all filesystem metadata. Standard mouse and
keyboard behaviors are incomplete, X11 support stops short of external drag and drop, and common
desktop locations and actions are missing.

The user wants Waddle to replace Nautilus on Linux while keeping its Vim-first advantage. The
application must support ordinary desktop interaction without making Vim users slower. File
operations must favor data safety, explicit partial results, and recovery over convenience.

## Solution

Build Waddle 1.0 as a local Linux file manager for Wayland and X11. Keep the standard desktop
controls visible, then expose faster equivalents through the Browser key grammar.

The first delivery adds an interoperable multi-entry system clipboard, Vim Cut and Trash
operators, safe conflict handling, and persistent Undo and Redo. Later milestones add live
directory monitoring, a single Transfer queue, complete Grid and List interaction, desktop
locations, Trash recovery, file properties, settings commands, keyboard access, and Linux
packaging.

Every mutation reports what succeeded and what failed. Waddle must not silently overwrite an
entry, discard metadata, claim an unsafe Undo, or leave a partial destination looking complete.

## User Stories

1. As a Linux user, I want Waddle to work on Wayland and X11, so that my file manager does not depend on one compositor.
2. As a desktop user, I want familiar mouse and keyboard controls, so that I can use Waddle without learning Vim first.
3. As a Vim user, I want the same file actions available through a compact Browser key grammar, so that routine navigation stays fast.
4. As a user with a multi-entry selection, I want Copy to include the complete selection, so that I do not have to copy files one at a time.
5. As a user with a multi-entry selection, I want Cut to include the complete selection, so that I can move a batch with one Paste.
6. As a Nautilus user, I want to paste files copied or cut in Nautilus into Waddle, so that switching file managers does not break my clipboard.
7. As a Dolphin user, I want to paste files copied or cut in Dolphin into Waddle, so that KDE clipboard conventions remain compatible.
8. As a Waddle user, I want Nautilus and Dolphin to understand Waddle Copy and Cut, so that the clipboard works in both directions.
9. As a user, I want a malformed or ambiguous external clipboard to fall back to Copy, so that Waddle never guesses that it may delete a source.
10. As a user, I want Waddle to reject unsupported remote clipboard URIs, so that a local file operation does not misinterpret a network resource.
11. As a user who cuts files, I want them hidden from the current Grid immediately, so that the pending operation feels like a Vim Cut.
12. As a user with a pending Cut, I want the bottom bar to show the item count and available actions, so that hidden entries are not mistaken for deleted data.
13. As a user with a pending Cut, I want Escape to cancel it and restore the entries, so that Cut remains reversible before Paste.
14. As a user whose clipboard ownership changes, I want pending Cut entries restored, so that Waddle does not keep hiding valid files.
15. As a user pasting a Cut into its source folder, I want a no-op, so that Waddle does not create a duplicate by mistake.
16. As a user copying into the source folder, I want a safe non-conflicting name, so that the original remains intact.
17. As a user whose batch Move partly fails, I want failed entries to remain in the Cut clipboard, so that I can retry only the unfinished work.
18. As a keyboard user, I want `y` to copy the current selection in one keystroke, so that Yank stays simple.
19. As a keyboard user, I want `dd` to Cut the active entry, so that file Cut follows a familiar Vim operator.
20. As a keyboard user, I want `d{motion}` to Cut an inclusive range, so that motions compose with file operations.
21. As a keyboard user, I want `x` to Cut the current selection, so that a selected batch can be moved without another prompt.
22. As a keyboard user, I want the black-hole register with `d` or `x` to move entries to Trash, so that Trash does not overwrite my clipboard.
23. As a Visual selection user, I want `y`, `d`, and `x` to act on the full selection, so that Visual operations remain consistent.
24. As a keyboard user, I want `p` to Paste into the displayed folder, so that the destination is predictable.
25. As a keyboard user, I want `gg`, `G`, and a count before `G`, so that I can jump to exact positions in a large folder.
26. As a keyboard user, I want `H`, `M`, and `L`, so that I can move within the visible part of a folder.
27. As a keyboard user, I want `Ctrl+D` and `Ctrl+U`, so that I can traverse large folders by half pages.
28. As a keyboard user, I want numeric counts before motions and Cut operators, so that repeated movement does not require repeated keystrokes.
29. As a user who switches between Grid and List, I want operator ranges to follow display order, so that the same command selects the same files in both views.
30. As a user entering a multi-key command, I want the bottom bar to show the pending sequence, so that I know what Waddle expects next.
31. As a deliberate keyboard user, I want pending sequences to wait without a timeout, so that typing speed does not change file operations.
32. As a keyboard user, I want Escape to cancel a pending sequence, so that I can abandon an operation without side effects.
33. As a keyboard user, I want `u` for Undo and `Ctrl+R` for Redo, so that recovery follows Vim conventions.
34. As a keyboard user, I want `Ctrl+O` for Back and `Ctrl+I` for Forward, so that navigation remains available after `u` becomes Undo.
35. As a sidebar user, I want Vim motions to traverse, expand, collapse, and open visible nodes, so that the sidebar does not require a mouse.
36. As a user on a technical sidebar root, I want file operators disabled, so that I cannot Cut or Trash Computer or Recent.
37. As a user facing a name collision, I want Replace, Skip, and Keep Both in the bottom bar, so that I can resolve the conflict without a dialog.
38. As a user resolving a batch, I want uppercase conflict keys to apply a choice to the remaining entries, so that repeated collisions stay manageable.
39. As a user merging two directories, I want a non-destructive merge, so that Replace never erases the existing directory tree.
40. As a user renaming or replacing an entry, I want an atomic no-clobber operation, so that another process cannot cause a silent overwrite.
41. As a user who made a mistake, I want persistent Undo for Rename, Move, Trash, New Folder, and Copy, so that recovery survives restarting Waddle.
42. As a user who undoes a Copy, I want Waddle to remove the result only when it is unchanged, so that later edits are protected.
43. As a user who undoes New Folder, I want Waddle to remove it only when it is empty, so that later content is protected.
44. As a user whose entry changed after a mutation, I want unsafe Undo refused with a clear reason, so that recovery does not overwrite newer work.
45. As a user who undid an operation, I want Redo until the next new mutation, so that I can restore the previous result.
46. As a user, I want permanent delete described as irreversible, so that the interface never promises impossible recovery.
47. As a user with old operation history, I want it bounded to 100 operations or 30 days, so that the journal remains relevant and finite.
48. As a user copying filesystem trees, I want timestamps, permissions, extended attributes, ACLs, symlinks, sparse layout, and hardlink relationships preserved when supported, so that Copy does not simplify my data.
49. As a user copying to a filesystem with weaker metadata support, I want an explicit report of what was lost, so that metadata loss is not silent.
50. As a user working beside a terminal or browser, I want the displayed folder to update when another process changes it, so that the view matches the filesystem.
51. As a user during live refresh, I want selection and scroll preserved by path, so that inserted or removed entries do not move my focus arbitrarily.
52. As a user entering a Rename, command, conflict response, or pending Cut, I want refresh to preserve the active session, so that external events do not discard my input.
53. As a user searching a folder, I want live changes reflected in current results, so that search does not show stale entries.
54. As a user whose current folder disappears, I want Waddle to navigate to the nearest existing ancestor and explain why, so that I do not remain in a broken location.
55. As a user, I want `F5`, a toolbar action, and `:refresh`, so that I can request a refresh even with monitoring enabled.
56. As a user copying a large tree, I want byte and entry progress, speed, and estimated time, so that I can judge how long the Transfer will take.
57. As a user with a long Transfer, I want Cancel, so that I can stop work that is no longer wanted.
58. As a user after a failed Transfer, I want Retry for failed entries, so that successful work is not repeated.
59. As a user after a partial Transfer, I want a detailed report and safe Undo for the successful part, so that partial results remain understandable.
60. As a user who cancels Copy, I want Waddle to remove only its private incomplete results, so that pre-existing files remain untouched.
61. As a user running several mutations, I want one ordered queue, so that conflicts and Undo remain predictable.
62. As a user diagnosing a failure, I want 30 days of local operation reports that I can copy, so that I can inspect or share exact errors.
63. As a standard desktop user, I want Ctrl-click, Shift-click, Ctrl+A, arrows, Shift-arrows, Space, Home, and End, so that selection works as expected.
64. As a user, I want separate single-click or double-click activation settings for files and folders, with files defaulting to double click and folders to single click, so that each entry type matches my desktop preference.
65. As a user, I want breadcrumbs with `Ctrl+L` access to editable paths, so that navigation is both discoverable and precise.
66. As a user, I want Grid and List views, so that image-heavy and metadata-heavy folders can use different layouts.
67. As a user, I want sorting by Name, Modified, Size, or Type in either direction, with Type ascending as the default and folders fixed before size-sorted files, so that I can find relevant entries quickly.
68. As a user, I want natural filename ordering across folders and files, with Type ordering folders before extension types and generic files last, so that names with numbers and mixed entry types sort predictably.
69. As a user, I want global view defaults with per-folder overrides, so that different folders can retain useful layouts.
70. As a user browsing images, I want asynchronously generated thumbnails with a bounded cache, so that Grid view is useful without blocking input.
71. As a user, I want Ctrl+H to toggle hidden entries in the folder and filename search, with visible hidden entries subtly muted, so that display and search use the same boundary.
72. As an X11 user, I want external drag and drop, so that file exchange matches the Wayland build.
73. As a user dragging through a long folder, I want edge autoscroll, so that off-screen destinations remain reachable.
74. As a user dragging over the sidebar, I want folders to expand after a short hover, so that nested destinations become reachable.
75. As a user dragging over a folder, I want entry after a longer hover with visible progress, so that deep navigation stays controlled.
76. As a user, I want Home and existing XDG user directories in the sidebar, so that common places need no manual Favorites.
77. As a user, I want Recent backed by the desktop's shared history with Clear and Disable actions, so that I control convenience and privacy.
78. As a user, I want reorderable Favorites with custom labels, so that the sidebar matches my workflow.
79. As a user with removable or mounted storage, I want normal mount, unmount, and eject actions, so that device handling stays in the file manager.
80. As a user, I want Trash as a location with Restore, permanent deletion, and Empty Trash, so that deleted files remain manageable.
81. As a user restoring an entry, I want destination conflicts handled in the bottom bar, so that Restore follows the same safety rules as other Transfers.
82. As a user, I want Properties with type, location, size, dates, permissions, and default application, so that I can inspect an entry without another tool.
83. As a user, I want to edit permissions allowed by my account, so that routine permission changes remain possible without root mode.
84. As a user, I want Open With and default-application selection, so that file activation uses the right application.
85. As a user, I want New Empty File and XDG Templates, so that I can create files from the current folder.
86. As a user, I want common display settings in the toolbar or menus, so that I do not need commands for routine choices.
87. As a power user, I want persistent `:set` and per-folder `:setlocal` with completion and validation, so that advanced settings remain fast and inspectable.
88. As a keyboard-only user, I want composite Tab stops, visible focus, standard activation, and focus restoration, so that every 1.0 action remains reachable.
89. As a user who needs stronger visual separation, I want a high-contrast palette that does not depend on color alone, so that state remains visible.
90. As a user who reduces motion, I want animations and high-frequency animation subscriptions disabled, so that Waddle respects the desktop preference.
91. As a command-line user, I want `waddle` to accept a path or local file URI, so that other applications can open a location directly.
92. As a user reopening Waddle, I want window geometry, the last folder, views, sorting, Favorites, and Undo history restored, so that the application resumes useful state.
93. As a user launching an explicit location, I want the command-line argument to override remembered state, so that external requests are honored.
94. As a Linux desktop user, I want a desktop entry, icons, AppStream metadata, Flatpak, and a regular binary archive, so that Waddle installs and launches normally.
95. As a user running `!`, I want the command to run directly with my permissions, so that Waddle keeps its current power-user command workflow.
96. As a user running a noisy command, I want output bounded while the process runs, so that one command cannot consume unbounded memory.
97. As a maintainer, I want manual interoperability tests with Nautilus and Dolphin on Wayland and X11, so that protocol compatibility is proven outside unit tests.
98. As a maintainer, I want a 10,000-entry folder test, so that Grid interaction, List view, search, thumbnails, and keyboard movement remain usable at scale.
99. As a maintainer, I want restart recovery, partial failure, clipboard loss, Trash, conflict, and cancellation tests, so that the highest-risk workflows gate 1.0.
100. As a maintainer, I want formatting, tests, warning-free Clippy, and release builds to pass, so that 1.0 has a repeatable engineering gate.

## Implementation Decisions

- Keep Waddle on Rust and Iced for 1.0. Do not add an Iced accessibility fork.
- Support local Linux files on Wayland and X11. Treat Windows, macOS, and remote URI backends as separate products or later work.
- Use the existing domain sessions as the main application boundaries. Extend the Browser key grammar, Transfer workflow, File operation session, Navigation session, Search session, Grid interaction, and Command session instead of moving their policy back into the UI adapter.
- Keep the application adapter responsible for messages, subscriptions, rendering, and orchestration. It should translate domain consequences into UI tasks without owning filesystem policy.
- Replace the private clipboard value with a multi-entry clipboard payload that contains paths, Copy or Move intent, and a generation identifier.
- Keep clipboard paste, internal drag, and native drag as initiators of the same Transfer concept.
- Extend the Wayland native transfer worker to own clipboard offers and selection lifecycle as well as drag and drop.
- Add a separate X11 native transfer adapter with multi-MIME selection support. X11 must also gain external drag and drop before 1.0.
- Publish `text/uri-list`, `x-special/gnome-copied-files`, and `application/x-kde-cutselection` in one clipboard offer.
- Prefer a valid GNOME copied-files payload because it contains paths and action together. Otherwise read the standard URI list and use the KDE Cut marker. Copy is the fallback for every ambiguous case.
- Keep clipboard payloads bounded by bytes and entry count. Accept only absolute local `file:` URIs without a foreign authority.
- Preserve symlink identity when parsing and transferring local paths. Do not route clipboard paths through a shell.
- Keep pending Cut as application state. Cut hides entries from the current Grid but leaves the filesystem unchanged until Paste performs a Move.
- Clear Cut only for the same clipboard generation. Restore hidden entries after cancellation or ownership loss.
- Let directory monitoring determine whether an external receiver actually moved Cut entries. Clipboard reads alone do not prove a successful Move.
- Make `y` a one-key Copy of the current selection. It does not start an operator and does not accept a motion.
- Make `d` the Cut operator, `dd` Cut the active entry, and `x` Cut the current selection.
- Use the `"_` prefix for Trash variants of `d` and `x`. Trash must not replace the clipboard.
- Compose the Cut operator with existing and new motions. Operator ranges are inclusive and follow current display order.
- Add counts, `gg`, `G`, counted `G`, viewport motions, and half-page motions. Keep named registers, macros, and dot repeat reserved outside 1.0.
- Show pending Browser key grammar sequences in the bottom bar without a timeout. Escape cancels the sequence. An invalid key resets it with a short explanation.
- Use `u` for Undo, `Ctrl+R` for Redo, `Ctrl+O` for Back, and `Ctrl+I` for Forward.
- Apply Vim traversal to the focused sidebar, but block file operators on technical roots.
- Keep conflict handling in the bottom bar. Lowercase choices apply once. Uppercase choices apply to the remaining batch.
- Support Replace, Skip, and Keep Both for file conflicts. Merge directory conflicts without deleting the existing directory tree.
- Implement Rename and Replace with atomic no-clobber filesystem operations where the platform permits them. Do not rely on a separate existence check before a destructive rename.
- Run one filesystem mutation at a time through an ordered queue. Reads and non-mutating background work may remain concurrent.
- Give every queued mutation a stable identity, cancellation state, progress, partial result, and operation report.
- Write incomplete copies to private temporary destinations. Publish completed results atomically where the filesystem permits it.
- On cancellation, remove only temporary or incomplete results created by that operation. Preserve every pre-existing entry.
- Preserve successful entries after partial failure. Keep failed Cut entries pending and make them available for Retry.
- Store Undo and Redo records for the last 100 operations or 30 days, whichever boundary comes first.
- Persist enough source, destination, identity, and result data to validate an inverse operation after restart.
- Cover Rename, Move, Trash, New Folder, and Copy. Permanent delete has no inverse.
- Validate current filesystem identity before Undo. Refuse an inverse operation that would overwrite later changes.
- Let Redo survive until the next new mutation.
- Preserve contents, timestamps, permissions, extended attributes, ACLs, symlinks, sparse-file layout, and hardlink relationships where supported.
- Record metadata that the destination filesystem cannot preserve and show it in the operation report.
- Add event-driven directory monitoring with debounce. Reconcile entries by path and preserve Grid interaction state.
- Keep File operation sessions, Command sessions, conflicts, Search sessions, and pending Cut alive during reconciliation.
- Navigate to the nearest existing ancestor when the current directory disappears.
- Add manual Refresh through F5, the toolbar, and `:refresh`.
- Show the active Transfer in the bottom bar. Expand the bottom area for queue history, progress, cancellation, retry, and details.
- Keep diagnostics local for 30 days and let the user copy a report.
- Bound shell output while the command runs. Keep direct shell execution enabled without a warning prompt, and describe its permissions accurately.
- Extend Grid interaction with standard multi-selection and navigation while preserving Visual selection and Vim motions.
- Add separate `file-click` and `folder-click` activation settings. Files default to double click and folders to single click.
- Add breadcrumbs and use `Ctrl+L` to enter an editable location.
- Add List view and keep Grid view. Both consume the same ordered entry model and operator-range rules.
- Support Name, Modified, Size, and Type sorting in either direction, with natural filename ordering across folders and files. Type orders Folder first, extension types next, and generic File last. Size keeps folders first and applies direction only to files.
- Store global view defaults plus optional per-directory overrides.
- Generate image thumbnails outside the UI thread and keep the cache bounded.
- Make Ctrl+H control hidden entries in both the folder and filename Search session.
- Keep 1.0 search limited to filenames.
- Add edge autoscroll and timed hover expansion or entry during drag and drop. Moving away cancels hover activation.
- Add Home, existing XDG user directories, Recent, Trash, mounted volumes, and user Favorites to the sidebar.
- Use the desktop's shared recent-file history. Provide Clear and Disable controls.
- Let Favorites use custom labels and drag reordering.
- Use normal GIO and Polkit flows for mount, unmount, and eject. File mutations never gain root privileges.
- Make Trash a browsable location with Restore, permanent deletion of selected entries, and Empty Trash.
- Route Restore conflicts through the same conflict handling used by other Transfers.
- Add Properties, permitted permission edits, Open With, default-application selection, New Empty File, and XDG Templates.
- Keep routine view settings visible in the toolbar or menus. Do not add a separate Preferences screen.
- Add persistent `:set` and per-directory `:setlocal`, including inspection, completion, validation, and override removal.
- Treat the toolbar, location control, sidebar, Grid or List, and bottom bar as composite Tab stops.
- Add visible focus, standard Enter and Space activation, focus trapping for active prompts, and focus restoration on close.
- Add high contrast, reduced motion, and reduced transparency. Do not make screen-reader semantics or Orca a 1.0 gate.
- Keep the interface in English and do not add translation infrastructure.
- Accept a local path or `file:` URI on the command line. An explicit location overrides remembered state.
- Open each 1.0 invocation in a new process and window. Do not add single-instance IPC.
- Persist window geometry, last folder, views, sorting, Favorites, and Undo history. Do not restore every previous window automatically.
- Ship a desktop entry using URI arguments, application icons, AppStream metadata, Flatpak packaging, and a regular binary archive.
- Deliver work in six ordered milestones: clipboard and Undo, live directory state, Transfer queue, complete browser interaction, desktop locations and file actions, then settings and release hardening.

## Testing Decisions

- Use one primary behavioral seam at the application boundary. Drive a user input or system event into the relevant domain session, let the application apply its consequences, then assert the visible state, requested operation, and bottom-bar result. Avoid tests that assert private fields or widget construction.
- Keep focused state-machine tests for the Browser key grammar, Transfer workflow, File operation session, Navigation session, Search session, Grid interaction, and Command session. These tests should cover behavior that can be expressed without a native display server.
- Reuse the existing Browser key grammar tests for mode precedence, pending operators, cancellation, and motion dispatch. Extend them with counts, `g` sequences, black-hole prefixes, Cut, Undo, and sidebar focus.
- Reuse the existing Transfer workflow tests for selected-entry collection, Copy/Paste consequences, drag activation, inbound targets, partial results, and clipboard selection after completion.
- Reuse the existing File operation session tests for prompt lifecycle, partial Trash failure, permanent-delete escalation, and completion consequences.
- Reuse the existing Navigation session tests for stale completion rejection, Back and Forward history, failed navigation, and selection restoration.
- Reuse the existing operation-lane tests for bounded concurrency and cancellation. Extend this seam to the mutation queue, progress, Retry, and ordered completion.
- Reuse the existing Search session tests for local matching, recursive restoration, cancellation, and truncated results. Add hidden-entry and live-reconciliation cases.
- Reuse the existing Grid interaction tests for movement, Visual selection, marquee geometry, drag thresholds, drop zones, and viewport behavior. Add standard selection, counts, List equivalence, autoscroll, and hover activation.
- Reuse temporary-directory filesystem tests for collisions, descendants, symlinks, partial batches, and incomplete-copy cleanup. Add atomic no-clobber races, metadata fidelity, sparse files, hardlinks, cancellation, and Undo validation after later changes.
- Test clipboard format encoding and parsing as adapter contract tests. Each backend must produce the same domain clipboard payload from the same external offers.
- Test malformed, oversized, mixed-generation, non-local, and contradictory clipboard data. Every ambiguous action must resolve to Copy or rejection, never Move.
- Run native integration tests on Wayland and X11 for clipboard ownership, ownership loss, multi-MIME offers, external drag and drop, and shutdown.
- Run manual interoperability tests for Copy and Cut in both directions with Nautilus and Dolphin. A unit test of MIME payloads does not replace this gate.
- Test directory monitoring with bursts, renames, deletion of the current directory, active Search sessions, pending Cut, active prompts, and selection preservation.
- Test the mutation queue with a large tree, cancellation at several points, Retry, partial success, temporary cleanup, restart recovery, and safe Undo.
- Test operation-history retention at both the 100-operation and 30-day boundaries.
- Test Grid and List with at least 10,000 entries. Cover selection, sorting, scrolling, search, thumbnail scheduling, and Vim motions.
- Test high-contrast color pairs, focus visibility, composite Tab order, Enter and Space activation, focus trapping, focus restoration, and reduced-motion behavior.
- Test command-line paths and local file URIs, remembered startup state, desktop entry arguments, Flatpak permissions, and regular binary startup.
- Require formatting, all unit and integration tests, Clippy with warnings denied, a release build, and diff checks before 1.0.
- A good test asserts a behavior a user or external application can observe. It should remain valid after internal modules are reorganized.

## Out of Scope

- Windows and macOS support.
- Remote GVfs locations such as SFTP and SMB in 1.0.
- System-indexed content search in 1.0.
- Filename filters for type, modification date, and size in 1.0.
- Bulk Rename in 1.0.
- PDF and video thumbnails in 1.0.
- Tabs, split view, and quick preview in 1.0.
- Named Vim registers, macros, and dot repeat in 1.0.
- Built-in archive creation, extraction, or browsing.
- Custom user actions beyond shell and terminal commands.
- A root file-operation mode.
- A separate Preferences screen.
- Single-instance IPC and automatic restoration of every prior window.
- Screen-reader semantics and Orca as a 1.0 release gate.
- Application telemetry.
- Interface translations.

## Further Notes

- This is a program-level 1.0 spec. Convert it into implementation tickets by milestone before starting broad code changes.
- The first implementation ticket should establish the shared clipboard payload and adapter contract. Wayland, X11, Browser key grammar, and Undo depend on that contract.
- Existing uncommitted work already contains the current domain sessions and native Wayland drag and drop. Preserve that work and build on the current checkout rather than historical Slint-era assumptions.
- The roadmap defines delivery order, not dates.
- The implementation tickets are complete or at their recorded human verification gates. The
  remaining interoperability, Flatpak, and desktop-session checks are listed in
  `docs/release-checklist.md`.
