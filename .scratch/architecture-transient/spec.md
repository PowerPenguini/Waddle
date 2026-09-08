# Deepen Transient presentation ownership

Requested: implement the Strong candidate from the architecture review of `0c236f6`.

Concentrate presentation precedence, replacement, dismissal, and associated Browser key grammar mode changes behind the Transient presentation module. Rendering, keyboard routing, and Iced focus must use the same resolved presentation. App executes presentation effects after session edits; Command session, File operation session, Open With, and Transfer session retain their domain work.

Preserve the visible priority: Transfer conflict, Open With, Command output, File operation prompt, expanded Transfer history, then standard browser content. Output temporarily hides bottom inputs and history without losing their text or state. The toolbar Location editor remains visible alongside output. Busy File operations remain protected from cancellation.

Validate through existing App interactions and real Iced input operations. Retain the existing lifecycle tests, add regressions for presentation replacement and restoration, and run the release gate and performance benchmarks.

Saved-state persistence and Command completion interpretation were rated Worth exploring and are outside this implementation.
