# 30: High contrast and reduced motion

**What to build:** Respect visual accessibility preferences without adding a screen-reader requirement to 1.0.

**Blocked by:** 28: Persistent `:set`, `:setlocal`, and common controls.

**Status:** resolved

- [x] High contrast uses opaque surfaces and non-color state indicators.
- [x] Focus, selection, drop target, and errors meet agreed contrast checks.
- [x] Reduced motion removes transitions and the 16 ms animation subscription.
- [x] System preference and explicit override both work.

## Answer

High contrast uses a black and white base with bright semantic colors, opaque sidebar and text
surfaces, and distinct one-pixel selection versus three-pixel drop-target outlines. Reduced
transparency independently makes translucent chrome opaque. Reduced motion bypasses bottom-bar
interpolation, freezes spinners, and removes the 16 ms subscription; drag dwell keeps only a
100 ms functional timer. The `auto` values follow GNOME's HighContrast theme and
`enable-animations`, while `:set` true/false overrides persist.
