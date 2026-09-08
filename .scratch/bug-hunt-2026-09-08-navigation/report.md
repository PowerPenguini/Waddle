# Sidebar and Back navigation bug hunt

Base: `60bc67a` on `main`. Date: 2026-09-08.

This pass reproduced three user-visible navigation failures, each with a failing test before its fix. Tests use the existing App and Navigation session interfaces. Directory scans run through actual Iced task streams against temporary folders; Recent and Trash contents are supplied through their normal completion messages without reading or modifying desktop Trash.

## Clicking the previous folder cannot leave Recent or Trash

The Navigation session retains its last folder path while displaying Recent or Trash. Sidebar activation compared only that path and skipped navigation when it matched the selected row. Opening Recent from Home and clicking Home could therefore leave Recent displayed.

The shortcut now also requires that a folder is actually displayed. The regression covers both Recent and Trash, activates the previous folder's Sidebar row, runs its task to completion, and verifies that the folder and its file are displayed.

Test: `app::tests::navigation::sidebar_returns_from_recent_and_trash_to_the_previous_folder`.

## A pending folder request overrides a newer Sidebar choice

While a different folder was loading, clicking the currently displayed folder hit the same shortcut and left the pending request active. Its later result opened the other folder despite the user's more recent choice.

The shortcut now also requires no pending navigation. Selecting the current folder during a scan goes through normal navigation cancellation and request replacement. The regression explicitly delivers a late completion for the superseded request and verifies that the chosen folder remains displayed, its entries are loaded, and no spurious history entry appears.

Test: `app::tests::navigation::sidebar_current_folder_supersedes_a_pending_navigation`.

## Back is disabled in Recent and Trash without folder history

On first launch, entering Recent or Trash left the toolbar Back button disabled because its availability checked only folder history. The Back transition already supports returning from either location to the retained folder.

Back availability now accounts for Recent and Trash. The regression checks both locations with empty history, verifies the return destination, and checks that returning disables Back again without adding a history entry.

Test: `app::navigation::tests::back_is_available_from_recent_and_trash_without_folder_history`.

## Validation

- Each `*-red.log` records a failing reproduction before the corresponding implementation change; matching green logs record passing tests.
- `release-gate.log`: 455 application tests passed; seven performance benchmarks were intentionally ignored in the normal suite.
- Two vendored scrollbar tests and five real-X11 adapter tests passed.
- Formatting, strict Clippy, locked release build, FileManager1 activation, desktop metadata validation, and packaged archive smoke tests passed.
- `benchmarks.log`: all seven release-mode performance benchmarks passed their budgets.
- This pass did not perform manual desktop interaction or change user settings.
