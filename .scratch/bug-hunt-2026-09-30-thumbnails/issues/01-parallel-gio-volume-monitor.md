# Intermittent GIO crash in parallel application tests

Status: needs-triage
Type: bug

On 2026-09-30, two parallel test runs crashed with SIGSEGV. One excluded the new
thumbnail grid test. Core PID 185030 showed `g_type_check_instance` and
`g_signal_handlers_disconnect_matched` during `tree::volume_roots`, called by
`App::new`. Another test thread was in `g_volume_monitor_get` at the same time.

This suggests concurrent GIO volume discovery or teardown, but the root cause
is unconfirmed. No core was extracted. The full suite subsequently passed both
sequentially and in parallel. This has not been reproduced in the GUI.

## Comments

Do not treat a single passing parallel run as proof that this race is fixed.
