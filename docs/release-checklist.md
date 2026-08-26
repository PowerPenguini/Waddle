# Waddle 1.0 release checklist

Run `scripts/release-gate.sh` first. Record the commit SHA, distribution versions, desktop session,
and application versions beside every manual result. A failure keeps the release blocked.

## Interoperability matrix

Repeat every row on native Wayland and native X11 sessions. Use at least one filename containing a
space and one two-file selection.

| Flow | Nautilus | Dolphin |
| --- | --- | --- |
| Copy into Waddle | Pending | Pending |
| Copy out of Waddle | Pending | Pending |
| Cut into Waddle | Pending | Pending |
| Cut out of Waddle | Pending | Pending |
| Drag into Waddle as Copy | Pending | Pending |
| Drag out of Waddle as Copy | Pending | Pending |
| Drag into Waddle as Move | Pending | Pending |
| Drag out of Waddle as Move | Pending | Pending |

Confirm the preview, target highlight, cancellation, error feedback, ownership loss, source
shutdown, and partial-Cut remainder in the same sessions.

## Integrated workflows

- [ ] Large Transfer shows progress and supports Cancel, Retry, a partial failure, and Undo after a
  restart.
- [ ] Keep Both, Replace, Skip, and apply-to-remaining conflict choices preserve unrelated files.
- [ ] Trash, Restore, Empty Trash, permanent-delete fallback, and Undo after restart pass.
- [ ] Clipboard loss restores a pending Cut; an external partial Move leaves only failed entries.
- [ ] A real 10,000-entry directory remains responsive while sorting, searching, navigating, and
  scrolling thumbnails.
- [ ] Tab and Shift+Tab traverse all composite stops; prompts trap and restore focus; Enter, Space,
  arrows, and Vim motions work without leaking into text input.
- [ ] High contrast has visible selection, focus, drop, and error states; reduced motion has no
  spinner rotation or animated bottom-bar expansion.

### Local smoke evidence, 2026-08-22–23

The release binary was exercised in a Sway Wayland session and in an isolated Weston 15 XWayland
session. Grid and List layouts, hidden entries, Vim and standard-key selection, Tab focus, Copy,
Paste, Undo, Keep Both, Skip, high contrast, clipboard ownership loss, and external Cut
reconciliation behaved as expected. Two screenshots taken 300 ms apart while a command was active
with reduced motion enabled differed by zero pixels.

The visual pass found and fixed three feedback bugs: an active Transfer conflict now shows its
`r`/`s`/`k` choices instead of stale progress, a retained Retry no longer hides the current status,
and clipboard ownership loss remains visible through the following refresh. The isolated XWayland
run proved pending Cut plus external rename reconciliation without a desktop clipboard manager. It
does not replace the native X11, Nautilus/Dolphin, or Flatpak rows above.

## Packages

- [ ] Build the Flatpak with `flatpak-builder`, then run `scripts/smoke-flatpak.sh Waddle.flatpak`.
- [ ] Install and launch the Flatpak on Wayland and X11; repeat clipboard and drag-and-drop smoke
  rows from the matrix.
- [ ] Download the CI-produced regular archive on a clean supported Linux system and run
  `scripts/smoke-package.sh ARCHIVE`.
- [ ] Publish the archive and Flatpak only after every item above passes.
