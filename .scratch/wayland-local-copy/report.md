# Local Copy must not require a Wayland file offer

Reported: Copy files inside Waddle's Downloads folder, navigate to a pendrive, and Paste reports that Wayland has no candidate. The matching error in the code is `the Wayland clipboard has no file offer`.

## Cause and fix

App already pastes pending Cut entries directly from the Transfer session. Copy instead always requested the native clipboard whenever an adapter was installed, even though the complete local selection was available. An absent native offer therefore blocked an internal Copy before any destination filesystem work started.

The Transfer session now remembers when Copy originated locally. Its clipboard-read decision uses the stored selection until a clipboard-ownership-loss event supersedes it. Cut and imported clipboard content reset that preference. Ownership loss is handled independently of drag-and-drop adapter availability, and leaves pending Cut entries intact.

This avoids depending on a native round trip for Waddle's own Copy. It does not claim to establish why the user's compositor had no offer available.

## Tests

- `copied_downloads_paste_without_a_wayland_file_offer`: the clipboard adapter accepts publication but returns the production Wayland missing-offer error on reads. Through the real Transfer task, copy from an isolated Downloads folder to an isolated destination, verify destination contents, and verify the source survives. Failed before the fix with the missing-offer error; passed afterward.
- `external_clipboard_replaces_waddles_copy_after_ownership_loss`: after another clipboard owner replaces Copy, read its file selection. A subsequent missing/non-file offer must surface its error instead of pasting stale files. Failed before ownership invalidation was added; passed afterward.
- Existing `clipboard_ownership_loss_keeps_the_internal_cut_pending` now exercises the production native-event entry point.

The deterministic clipboard adapter simulates the unavailable Wayland offer; filesystem transfers use real temporary files. No live pendrive contents were modified and no manual compositor reproduction is claimed.

## Validation evidence

- `copy-red.log`, `copy-green.log`: original regression.
- `ownership-red.log`, `session-green.log`: ownership regression and Transfer session coverage.
- `release-gate.log`: full automated checks, including 469 application tests, two vendored scrollbar checks, five real-X11 adapter checks, strict Clippy, locked release build, desktop metadata and packaging smoke tests. Eight opt-in application tests are excluded from the normal suite.
