# Browser focus transition ownership

Status: resolved

Base: `f8fd88e` (0.0.11). Implements the selected Strong architecture candidate.

## Ownership

`src/app/focus.rs` owns Browser focus state, browser traversal, Sidebar tree entry
preparation, Location editor ownership, and Iced focus/query/select/unfocus operations.
Presentation now consumes focus without owning or mutating it. Accepted pointer and
keyboard interactions share the transition rules. Incomplete Browser key grammar
sequences are cancelled when the browser surface changes.

The browser return surface remains separate from text-input ownership. Transient
presentation still chooses the visible editor; restored focus runs before an action's
initial text selection. Location yields to a deliberately activated bottom editor.
Asynchronous Location observations carry a generation, so obsolete replies cannot
replace a newer browser or editor interaction. No hypothetical backend interface was added.

Deleting this module would redistribute synchronization, Sidebar preparation, mode
cleanup, callback freshness, and widget-operation ordering among the former callers.
The change adds depth rather than only relocating the old Presentation setter.

## Test-first regressions

Each of these App input scenarios was run red before its corresponding fix:

1. Location editing → click file → Ctrl+A selected one file rather than both.
2. Location editing → Escape changed the mode but left the real Iced input focused.
3. Pending `d` → click Sidebar tree → `j` cut file entries instead of navigating the tree.
4. Location probe → new Command session → delayed reply replaced Command with Location.
5. Location editing → New Folder → cancel left Ctrl+A targeting an unfocused Location editor.

New tests live in `src/app/tests/focus.rs`. They use application messages and real Iced
text-input state, including executing widget operations and receiving probe results.
Existing marquee, Trash Ctrl+A, hidden-Sidebar traversal, context trapping, Location,
transient restoration, and initial Rename selection tests remain and pass.

## Validation

An isolated checkout containing only these changes passed 583 application tests
(18 opt-in tests ignored), two scrollbar regressions, formatting, strict Clippy,
locked release compilation, FileManager1 activation, desktop/AppStream validation,
and archive launch smoke checks.

The initial release-gate attempt could not connect to the inherited X11 display `:0`.
The five X11 checks were rerun successfully on a temporary dedicated XWayland `:97`;
remaining package checks passed there too. This is not a manual desktop workflow audit.
All seven release-mode performance benchmarks passed their budgets. The raw initial gate,
successful X11 rerun, and benchmark results are retained alongside this report.

Concurrent external-drag and grid-layout changes in the shared checkout were excluded
from this validation and implementation commit. Action-availability centralization
(the Worth exploring candidate), special-view Location semantics, and marquee admission
policy are outside this implementation. No release or installation was performed.
