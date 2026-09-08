# Deepen Navigation session and Transfer session lifecycle ownership

Requested: implement both Strong candidates from the architecture review of `1d21eb2`.

## Navigation session

Concentrate request replacement, cancellation, deferred refresh, displayed-location decisions, and associated Sidebar tree loads behind the Navigation session interface. App executes task and presentation effects. Search session, Grid interaction, and Location monitoring retain their existing responsibilities.

Preserve first-Back cancellation, subsequent history navigation, selection/scroll behavior, stale-completion rejection, Recent/Trash return, and deferred refresh after filesystem changes.

## Transfer session

Concentrate operation identity, Retry, foreground lifetime, conflict continuation, and completion behind the production Transfer session interface. Keep its queue as private implementation. Operations continues to provide shared worker execution and mutation lanes; existing native clipboard and drag adapter seams remain intact.

Tests should exercise the actual submission-to-completion task path. Preserve Undo preparation, Trash metadata cleanup, ordered Transfers, retry validation, and Restore's intentional exclusion from Transfer history.

## Validation

Keep existing App regressions, add lifecycle coverage at the domain interfaces, run the full release gate and Sidebar performance benchmarks, and retain evidence alongside the implementation report.

The saved-state candidate was rated Worth exploring and is outside this requested implementation.
