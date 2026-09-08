# Icon and label resizing: running-window check

Date: 2026-09-08

Launched the release binary in a separate X11/XWayland window with temporary configuration, data, state, and cache directories. Fixtures included 140 text files, a folder, and a 256-pixel PNG thumbnail. The user's Waddle configuration was not changed.

Injected keyboard and wheel input into the running application and inspected window screenshots. Observed Ctrl+= increasing the displayed size to 56 and Ctrl+- returning it to 48. Ctrl+wheel changed the size in both directions. The system delivered two line units per synthetic wheel notch, resulting in 16-pixel changes for those notches. The :set command changed exact sizes and switched Grid/List views and system/Waddle sources.

Inspected Grid and List layouts at small and large sizes (24–128), including selected entries, long filenames, and the thumbnail. Labels grew with icons; inspected labels did not overlap their icons or adjacent rows. Long names remained truncated to the available space. No additional product changes were made during this check.

Evidence:
- grid-56.png: Ctrl+= result with system icons/fonts.
- grid-128.png: large Grid layout.
- list-128.png: large List layout.
- list-24.png: small List layout.
- waddle-96.png: bundled fonts and Waddle icons.

Limits: the desktop session was locked. These checks used synthetic input and X11 window captures; physical input, native Wayland, and precise touchpad gestures were not interactively verified. Early captures occasionally showed an earlier frame and were not treated as proof of the immediately preceding action. Automated tests separately cover precise wheel accumulation, keyboard routing, preference bounds, hit testing, and layout geometry.

## Sidebar fixture correction

The isolated XDG_CONFIG_HOME initially omitted user-dirs.dirs, so screenshots showed Home without Documents, Downloads, Music, Pictures, and Videos. The normal desktop configuration defines all five, and all five directories exist. Desktop points to Home and is intentionally omitted. Copied the real user-dirs.dirs into the temporary configuration for subsequent runs; the existing screenshots retain the original test setup. This was a test-fixture omission, not removal of the user’s folders or saved configuration.

Fresh capture after the fixture correction: sidebar-corrected-64.png. System icons/fonts, 64-pixel Grid icons, and Documents/Downloads/Music/Pictures/Videos visible. Captured from the running release build; the temporary preview window was closed afterward.
