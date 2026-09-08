# Chevron rendering at fractional display scales

The reported sort and navigation chevrons had uneven edges at 125% display
scaling. A headless Iced regression renders the production toolbar buttons and
sort headers at six positions per scale. The header label is empty to measure
only the chevron's stroke coverage.

Two causes were reproduced independently:

- Fixed toolbar button bounds stretched the requested 16px SVG to the button's
  dimensions. Before containment, moving Back changed stroke brightness by
  11.2% at 125%. Centering the icon in a container preserves its requested size.
- The 14px sort canvas becomes 17.5 physical pixels at 125%. Iced rasterizes
  with rounded dimensions and snaps the drawn bounds, dropping different edge
  pixels as the position changes. The production header changed stroke
  brightness by 19.7%. A 16px canvas gives even physical dimensions at the
  tested quarter-step scales. The SVG path and stroke were subsequently reduced
  slightly at the user's request, keeping this canvas size.

Validation:

- The regression fails when either fix is independently removed.
- Final smaller icons pass the OpenGL renderer check at 100%, 125%, 150%, 175%,
  and 200%, including both sort directions and Back, Forward, and Parent.
- The release gate passed: 461 regular tests, 2 vendor scrollbar tests, 5 X11
  checks, Clippy, release build, service and package smoke checks, metadata.
- After the final SVG size adjustment, the renderer regression, all 7 release
  benchmarks, and the locked release build passed again.

The GPU regression is ignored by default because it requires an adapter. Its
explicit command is documented in README.md. Other scale factors are outside
this regression's coverage.
