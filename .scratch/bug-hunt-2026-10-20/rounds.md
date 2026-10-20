# Bug hunting, 2026-10-20

Baseline: `b94e40a`, Waddle 0.0.26. Preserve existing untracked scratch artifacts.
Each round reproduces a failure, validates a focused fix, then merges into and
pushes `main`. No release or installation is included.

## Round 1: small images inflate the thumbnail cache

- Reproduction: decode a real 1x1 red BMP through the thumbnail request, decoder,
  and cache. The regression expected a 1x1 buffer but received 256x256.
- Red: `cargo test --locked small_images_are_not_upscaled_when_creating_thumbnails`
  failed with actual dimensions `(256, 256)` instead of `(1, 1)`.
- Cause: `DynamicImage::thumbnail` enlarges as well as shrinks to its bounds.
  Waddle always supplied the maximum thumbnail dimensions, inflating a one-pixel
  image from 4 bytes to 262,144 bytes before caching and uploading it.
- Fix: cap both resize bounds at the corresponding source dimension.
- Green: all 11 thumbnail tests passed, including original BMP pixels and six
  PNG aspect-ratio cases covering small, large, wide, tall, and one-pixel edges.
  All-target tests passed sequentially: 797 passed, 26 ignored. Locked strict
  Clippy, formatting, and whitespace checks passed.
- The earlier parallel GIO crash report remains unresolved. Sequential tests do
  not establish that it is fixed.

## Round 2: local file URI spellings are rejected

- Reproduction: launch and import `file://localhost/...` through the real launch,
  URI-list, and GNOME Cut parsers. Both original regressions returned errors.
- Red: `cargo test --locked localhost_file_uris` failed in both launch and
  clipboard parsing before filesystem access.
- Cause: both parsers required a literal `file:///` prefix instead of parsing
  the URI's local authority. That also excluded `file:/...` launch paths.
- Fix: share GLib filename decoding with explicit absolute-path and local-host
  validation. Preserve percent escapes and non-UTF-8 filename bytes. Reject
  embedded NUL before the FFI call, which a malformed-input test showed panicked.
- Green: 802 all-target tests passed sequentially, 26 ignored. Tests cover
  localhost, mixed-case localhost, authority-free absolute URIs, URI spelling
  deduplication, GNOME Cut, Unicode and raw filename bytes, remote hosts,
  malformed escapes, and embedded NUL. Locked strict Clippy, formatting, and
  whitespace checks passed.

## Round 3: unavailable watch paths silently disable recovery

- Reproduction: request native watches for an existing folder and a missing
  folder, as happens when a Recent bookmark's whole parent is removed.
- Red: `cargo test --locked unavailable_watch_paths_request_polling_until_native_monitoring_recovers`
  timed out waiting for the polling-fallback signal.
- Cause: the safe watch filter discards unavailable directories before inotify
  registration. Only inotify errors activated fallback, so filtered paths were
  silently treated as monitored.
- Fix: report incomplete registration whenever fewer unique paths are watched
  than requested. Keep the existing no-automount filtering.
- Green: the native test detects fallback, creates the missing folder, and
  verifies native changes arrive from both folders. It passed ten consecutive
  isolated repetitions. An application regression removes a Recent bookmark's
  parent, consumes the real fallback event, restores the parent and file, then
  verifies periodic polling rediscovers the entry in Recent.
- All-target tests passed sequentially: 804 passed, 26 ignored. Locked strict
  Clippy, formatting, whitespace checks, the locked debug build, and isolated
  FileManager1 activation smoke testing passed.

## Round 4: volume monitoring crosses threads and private contexts

- The historical core mentioned in the September follow-up is unavailable on
  this host. Resource inspection showed available memory; no new crash was
  needed to reproduce the current ownership violation.
- Reproduction: an isolated native-call trace exercises Sidebar discovery,
  mount-by-ID, unmount-by-ID, and a named volume command from separate callers.
  Missing synthetic IDs/names prevent actual device mutations.
- Red: `cargo test --locked native_volume_queries_share_one_thread_and_the_default_context`
  detected volume discovery inside a private thread-default GLib context.
- Cause: the application and its workers independently acquired GVolumeMonitor,
  including under private MainContexts. Installed GIO documentation requires
  volume monitoring on the main thread with the global default context.
- Fix: one persistent desktop thread owns the default context, the monitor, and
  its objects. Queries return plain data through channels. Async volume actions
  run on that context while the owner continues processing queries and events.
- Green: the native trace observes one owner and no private context. A separate
  regression keeps a non-Send async operation pending, verifies queries still
  complete, then verifies the operation resumes on its original owner.
- The complete parallel all-target suite passed 21 consecutive runs, each with
  806 passed and 26 ignored. Locked strict Clippy, formatting, whitespace checks,
  a locked debug build, and isolated FileManager1 activation smoke testing passed.
- This fixes the reproduced ownership violation. Without the old core, the
  historical segmentation fault cannot be conclusively attributed to it.

## Round 5: viewport jumps select offscreen files and move the viewport

- Reproduction: issue H/M/L against scrolled grids and lists at small, default,
  and large icon sizes. The first regression selected a row above the viewport.
  A real-keyboard/Iced-operation regression also detected an unwanted scroll.
- Red: the geometry regression returned index 6 instead of 9 for H. The input
  regression found a scroll-to-top command after an otherwise valid target.
  With no selection, the keyboard path instead chose the first file.
- Cause: viewport motions used the renderer's overscan range, always calculated
  with grid tile height. The generic motion caller then aligned the selected
  target at the top. The no-selection path bypassed viewport targeting entirely.
- Fix: calculate actual visible entry bounds using the current view's row height,
  excluding overscan and rows with only their spacing visible. Resolve viewport
  jumps even without an existing selection. Cancel pending scroll motion and
  keep the viewport in place when H/M/L changes the active file.
- Green: tests cover both views, three icon sizes, full and partial rows, spacing
  at the top edge, empty selection, and real keyboard/Iced scroll operations.
  Two older expectations were corrected because they asserted overscan targets.
  All-target parallel tests passed: 809 passed, 26 ignored. Locked strict Clippy,
  formatting, and whitespace checks passed.

## Round 6: half-page movement uses overscan and changes grid columns

- Reproduction: move up/down by half a page in four-row and five-row viewports,
  in both views at three icon sizes, with counts 1, 2, and 10,000.
- Red: `cargo test --locked half_page_motions_use_visible_rows_and_preserve_grid_columns`
  moved a four-row grid from index 32 to 41 instead of 38.
- Cause: half-page distance came from the rendering range, including overscan,
  and divided the number of entries rather than rows. Odd row counts could
  therefore change columns. List mode also used tile height for the distance.
- Fix: derive the row count from the actual viewport, halve it with rounding up,
  then move by whole rows and apply the numeric count with saturating arithmetic.
- Green: both directions, both view types, three icon sizes, odd/even page sizes,
  and normal/extreme counts pass. An older overscan-based expectation was updated.
  All-target parallel tests passed: 810 passed, 26 ignored. Locked strict Clippy,
  formatting, and whitespace checks passed.

## Round 7: camera and mirrored image orientation is ignored

- Reproduction: encode small images with real EXIF orientation tags. A JPEG
  marked for 90-degree rotation kept its stored landscape dimensions; a PNG
  marked for horizontal reflection kept its unreflected pixel order.
- Red: the thumbnail suite reproduced both failures, with 11 existing tests
  passing and the two new regressions failing.
- Cause: ImageReader::decode returns stored pixels; Waddle did not read and apply
  the decoder's orientation metadata.
- Fix: retain the decoder long enough to read orientation, preserve the previous
  decode allocation-limit accounting, and apply the transformation after resizing
  so rotation does not require copying the full-resolution image.
- Green: exact pixel/dimension checks pass for all eight EXIF orientations and
  a camera-style rotated JPEG. The complete parallel all-target suite passed:
  812 passed, 26 ignored. All 13 thumbnail tests passed again after a test-only
  Clippy correction. Locked strict Clippy, formatting, and whitespace checks pass.
