Waddle 0.0.26 is a preview release with fixes for symbolic links, Recent, and startup settings.

- Reveal broken symbolic links when launching Waddle with a local path or using FileManager1 ShowItems. Opening valid directory links still works.
- Sort Recent by visit time so revisiting an old bookmark moves it to the top.
- Save startup settings with unique temporary files. Concurrent windows no longer share a staging filename, and stale temporary symlinks cannot redirect writes into unrelated files.

Four new regression tests cover these bugs. The 1.0 manual interoperability checklist remains pending; this release continues the preview series.
