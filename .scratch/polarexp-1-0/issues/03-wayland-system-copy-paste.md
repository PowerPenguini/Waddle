# 03: Wayland system Copy and Paste

**What to build:** Exchange multi-entry Copy payloads between PolarExp and other Wayland applications through the system clipboard.

**Blocked by:** 01: Multi-entry clipboard Transfer inside PolarExp.

**Status:** resolved

- [x] One offer publishes all agreed clipboard MIME formats.
- [x] Paste accepts bounded absolute local file URIs.
- [x] Malformed, oversized, mixed-generation, and remote payloads are rejected safely.
- [x] Copy works in both directions with a compatible Wayland application.

## Answer

Added a shared clipboard-format module and a `ClipboardAdapter` seam used by the Wayland worker. One selection source publishes PolarExp, GNOME, URI-list, and KDE Cut formats. Selection reads prefer a payload that carries paths and action together, remain bounded to 4 MiB and 10,000 entries, reject foreign authorities, and fail if ownership changes during an asynchronous read. PolarExp routes imported payloads back through the existing Transfer workflow. Contract tests cover compatible producer and consumer data; a nested Weston smoke run confirmed worker startup and shutdown. Nautilus and Dolphin evidence remains in ticket 07, which is the explicit interoperability gate.
