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

## Round 8: counted viewport jumps discard their counts

- The local counted Browser key grammar ticket promises counts for supported
  viewport motions. The parser forwarded counts for H/L, but targeting ignored
  them.
- Red: `cargo test --locked counted_viewport_jumps_choose_rows_from_each_visible_edge`
  returned index 9 for 2H instead of index 12, even without a prior selection.
- Fix: offset H/L by the requested row count from the corresponding visible edge,
  clamping large counts inside the viewport. M retains the midpoint behavior.
  The built-in help now documents counted H/L explicitly.
- Green: both views pass with/without selection and counts 1, 2, and 10,000.
  The real keyboard/Iced regression also exercises 2H/2L and verifies they issue
  no scrolling commands. All-target parallel tests passed: 813 passed, 26 ignored.
  All three vendored scrollbar regressions, locked strict Clippy, formatting,
  and whitespace checks passed. Help remains within its tested 64-column limit.

## Round 9: line breaks in filenames hide compact status text

- Reproduction: select a real file whose name contains a newline, render the
  actual bottom bar with a headless wgpu renderer, and inspect layout geometry.
- Red: the filename status occupied 25 pixels instead of one 13-pixel text line,
  despite wrapping being disabled. The compact bar clipped the extra line.
- Cause: disabling wrapping does not suppress explicit line-break characters.
  Filenames, notices, and conflict prompts could supply those characters.
- Fix: escape control characters and Unicode line/paragraph separators only in
  compact status/conflict rendering. Keep model text, paths, and filenames intact.
  Ordinary text is borrowed without allocating a replacement string.
- Green: the explicit headless layout regression passes for newline, carriage
  return, vertical tab, form feed, NEL, and Unicode line/paragraph separators.
  It covers selected-file status, notices, and real Copy conflict prompts, while
  preserving both source names and existing destination contents. The existing
  status-carousel renderer test and both grid-layout renderer tests also pass.
- All-target parallel tests passed: 813 passed, 27 ignored. The new opt-in GPU
  regression was explicitly executed, rather than counted as a standard-suite
  pass. Locked strict Clippy, formatting, and whitespace checks passed.

## Round 10: nested folder Copy aborts the application

- Red: an actual App Copy of a temporary folder with 1,700 nested directories
  aborts its Tokio worker with a stack overflow. The original code also fails
  with only 64 nested directories. The regression runs in a subprocess so an
  abort fails the test instead of terminating the suite; core dumping is disabled.
- Live GDB stops in `journal::fingerprint::hash_tree`, with repeated recursive
  ancestor frames. Its 64 KiB file-read buffer is reserved at each level, even
  while traversing directories. The Copy itself has reached journal preparation.
- Fix: fingerprint the tree with an explicit pending stack. Reverse sorted
  children when pushing so traversal retains the existing depth-first ordering
  and byte stream for current and legacy journal digests. Files, symlinks,
  permission checks, and extended-attribute hashing retain their checks.
- Green: the real App regression copies 1,700 levels and completes keyboard Undo
  and Redo, checking copied contents and source preservation. All-target parallel
  tests pass: 814 passed, 27 ignored. Locked strict Clippy, formatting, whitespace,
  the development build, and isolated FileManager1 activation smoke test pass.

## Round 11: native changes disappear for directory aliases

- Red: register a real directory and a second path through a symlinked parent,
  wait for both registration notifications, then delete a real child. Only one
  path receives the deletion; the native regression times out waiting for both.
- Cause: inotify returns the same watch ID for both paths to the same inode.
  The previous ID-to-path map overwrote the first path without activating fallback.
- Fix: retain all requested paths for each watch ID and distribute native changes
  and removed child paths to each one. Overflow recovery rebinds all aliases;
  removing a path removes the shared kernel watch only when no aliases remain.
- Green: the real inotify regression checks deletion through both paths, quiet
  resynchronization, and continued native updates after dropping one alias.
  It passes ten consecutive runs. All-target parallel tests pass: 815 passed,
  27 ignored. Locked strict Clippy, formatting, and whitespace checks pass.

## Round 12: Cut remains pending after its parent moves externally

- Red: Cut a real file, navigate elsewhere, wait for native source monitoring,
  then rename the whole source folder externally. The App receives the native
  event but continues offering Paste from the nonexistent original child path.
- Cause: an inotify self-move/self-delete event has no child filename, and Cut
  reconciliation previously considered only exact named-child removal paths.
- Fix: include the vanished watched folder as a removal candidate, and consider
  captured Cut paths beneath that candidate. Confirm absence before clearing any
  item, preserving unrelated paths and paths that have reappeared.
- Green: the App/native inotify regression clears the stale Cut and verifies the
  externally moved contents. It passes ten consecutive runs. A second real-file
  regression retains both a sibling path and a recreated path after an ancestor
  move. All-target parallel tests pass: 817 passed, 27 ignored. Locked strict
  Clippy, formatting, and whitespace checks pass.

## Round 13: queue overflow leaves removed Cut items pending

- Red: extend the native inotify overflow fixture to Cut two real files and
  delete one. The shim replaces the named deletion with `IN_Q_OVERFLOW` and
  writes a marker proving that interception occurred. The App refreshes its
  file list but never clears the removed Cut item.
- Fix: overflow recovery includes each watched folder as a removal candidate.
  The existing ancestor-aware reconciliation confirms absent captured paths;
  a Cut item whose file still exists remains pending.
- Green: the injected overflow test covers both existing file-list recovery
  and partial Cut reconciliation, verifies the interception marker, and checks
  the remaining file's contents. Both scenarios pass ten consecutive runs.
  All-target parallel tests pass: 817 passed, 27 ignored. Locked strict Clippy,
  formatting, and whitespace checks pass.

## Round 14: line breaks expand single-line List cells

- Red: a headless wgpu layout of the actual List row renders a real filename
  containing a newline as a 28-pixel cell at icon size 24, exceeding one line.
  Disabling wrapping does not suppress explicit line breaks.
- Fix: escape control characters and Unicode line/paragraph separators at the
  List filename and type-label rendering boundaries, using the existing compact
  text helper (renamed to `single_line_text`). Preserve real paths and filenames.
- Green: the opt-in renderer regression explicitly passes at icon sizes 24,
  48, and 128 for LF, CR, VT, FF, NEL, Unicode separators, an extension containing
  LF, and a long extension. It checks cell geometry, displayed control characters,
  and original file contents/paths. Both existing grid-layout renderer tests and
  the compact-status renderer regression also pass.
- All-target parallel tests pass: 817 passed, 28 ignored. The new ignored GPU
  regression was explicitly executed. Locked strict Clippy, formatting, and
  whitespace checks pass.

## Round 15: a file replacing the Cut parent keeps missing children pending

- Red: extend the native App Cut-parent regression: after moving the source
  directory externally, create a regular file at its old path. The old child
  lookup returns `NotADirectory`; Cut continues offering that nonexistent child.
- Fix: treat both `NotFound` and `NotADirectory` as confirmed absence when
  reconciling paths affected by removal events. Other errors retain Cut state.
- Green: both parent-move scenarios pass ten consecutive native runs, and the
  regression verifies the replacement file and externally moved contents remain
  intact. All-target parallel tests pass: 817 passed, 28 ignored. Locked strict
  Clippy, formatting, and whitespace checks pass.

## Round 16: polling fallback never reconciles externally removed Cut items

- Red: extend the native-startup-failure fixture to Cut two real files and delete
  one externally. Polling refreshes the folder list but leaves both Cut paths
  pending, even though the removed source cannot be pasted.
- Fix: the centralized fallback policy requests a background Cut-source check.
  Probe captured paths on the existing background executor, coalesce overlapping
  polls, and compare the clipboard revision before applying a completion. Retain
  existing paths and other lookup errors; confirm absence before reconciliation.
- Green: the failure-injection fixture retains the existing Cut file and clears
  the deleted one. A second worker-level App regression verifies overlapping
  polls coalesce, delayed results cannot clear a newer Cut, and the next poll can
  still complete. Both regressions pass ten consecutive runs each.
- All-target parallel tests pass: 818 passed, 28 ignored. Locked strict Clippy,
  formatting, whitespace, the development build, and isolated FileManager1
  activation smoke test pass. A test-only unnecessary clone was corrected and
  the affected fixture rerun after Clippy identified it.

## Round 17: sidebar folder labels expand past one line

- Red: render an actual Favorite row for a real folder containing a newline.
  The label occupies 30 pixels inside its fixed-height row instead of one
  16-pixel line, clipping the name.
- Fix: escape control characters and Unicode line/paragraph separators before
  shortening sidebar labels. Keep the original Tree row label and path intact.
- Green: the explicit headless wgpu regression covers LF, CR, VT, FF, NEL, and
  Unicode line/paragraph separators, verifies label geometry and model labels,
  and confirms the real folder remains present. All four grid/sidebar renderer
  regressions pass. The text-bounds observer is shared between the List and
  sidebar tests instead of duplicated.
- All-target parallel tests pass: 818 passed, 29 ignored. The new opt-in GPU
  regression was explicitly executed. Locked strict Clippy, formatting, and
  whitespace checks pass.

## Round 18: filenames ending in a dot have blank type labels

- Red: real files named `trailing.` and `double..` return an empty type label.
  Their empty extensions also place them among typed files during Type sorting.
- Fix: classify an empty extension the same way as no extension for both the
  visible type label and its sort group.
- Green: the filesystem Type-sort regression checks both trailing-dot names
  display `File` and sort with extensionless files after known extensions.
  All-target parallel tests pass: 818 passed, 29 ignored. Locked strict Clippy,
  formatting, and whitespace checks pass.

## Round 19: long type labels paint over neighboring List columns

- Red: render real files with normal and 80-character extensions while keeping
  metadata identical, then compare pixels beyond the Type column. The long
  label changes 1,624 pixels in the neighboring metadata columns.
- Cause: assigning a text width does not clip the glyphs of an unbroken label;
  the text renderer receives the whole row's viewport.
- Fix: render the type label inside a clipped container with the existing column
  width and disable wrapping, keeping it on one line.
- Green: the pixel comparison reports zero differences beyond the Type column.
  All five grid/sidebar renderer regressions explicitly pass. All-target parallel
  tests pass: 818 passed, 30 ignored. Locked strict Clippy, formatting, and
  whitespace checks pass.
