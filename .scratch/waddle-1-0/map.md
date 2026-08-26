# Waddle 1.0 feature map

The implementation is split into six product slices plus the final integration gate:

- System clipboard and Vim operations: 01-12.
- Live directory state and the ordered Transfer queue: 13-15.
- Standard browsing and views: 16-18.
- Navigation, thumbnails, drag and drop: 19-22.
- Desktop locations and file actions: 23-27.
- Settings, accessibility, startup, diagnostics, and packaging: 28-33.
- Cross-cutting 1.0 release verification: 34.

Ticket 07 is the human interoperability gate for clipboard adapters. Ticket 22 retains a human
compatible-application gate for X11 drag and drop. Ticket 33 retains the Flatpak Builder gate.
Ticket 34 combines those results with the automated release gate and the manual desktop checklist.

All implementation dependencies are recorded in each ticket's `Blocked by:` line. The source of
truth for remaining human verification is `docs/release-checklist.md`.
