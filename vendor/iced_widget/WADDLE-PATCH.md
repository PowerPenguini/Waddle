# Waddle scrollbar patch

Source: crates.io `iced_widget` 0.14.2, MIT licensed (see LICENSE).

Iced 0.14.2 hardcodes a 2 px minimum scroller length and exposes no setting
for it. Waddle uses this local Cargo patch so builds consistently use a
32 px minimum on both axes, including the Sidebar and command output.

Only `src/scrollable.rs` differs from the upstream source:
- Clamp the minimum thumb length to the available track.
- Map scrolling onto the remaining travel distance after the larger thumb.
- Keep dragging finite when a track is shorter than the minimum.
- Test large content, endpoints, both axes and short tracks.

Remove this override when an upstream release provides a configurable minimum
and equivalent geometry. Do not edit the Cargo registry cache.
