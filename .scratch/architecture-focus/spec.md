# Deepen Browser focus transition ownership

Implement the Strong candidate selected from the focus architecture review of `f8fd88e`.

The Browser focus module owns the browser return surface, Location editing ownership,
focus traversal, Sidebar tree entry preparation, and Iced focus effects. Accepted
interactions use its transition interface rather than writing a presentation field.
Transient presentation retains precedence and decides which temporary editor is visible.

Preserve Tab versus activation, hidden-Sidebar traversal, contextual keyboard traps,
text input selection, and the browser return surface behind temporary presentations.
File-action availability and special-view navigation semantics remain outside this change.

Use existing App event and Iced widget-operation test seams. Regression tests must cover
file clicks after Location editing, Location cancellation, incomplete operators across
focus transitions, and obsolete asynchronous Location observations. Existing marquee,
Ctrl+A, Location editing, transient restoration, and Rename selection tests remain.

Concurrent external-drag changes in the working directory are separate work and must
not be included in this implementation commit.
