# Browser focus regions

Tab / Shift+Tab traverse files and Sidebar only; with a hidden Sidebar they stay in files.
Ctrl+W h/l move horizontally between these surfaces; j/k do not move into toolbars.
Toolbar buttons keep their pointer actions. Location remains editable by clicking or Ctrl+L.
Bottom-bar prompts receive keyboard interaction automatically when activated and restore
browser interaction on dismissal, preserving the previous browser surface.

This supersedes the five-region traversal retained in architecture-focus and issue
waddle-1-0/issues/29-composite-tab-focus.md, following the user's explicit product decision.

Validation uses the existing App message/key event and real Iced widget-operation seams:
all bottom editors, prompt capture, command completion, dismissal/restoration, hidden Sidebar,
Location release, toolbar clicks, command output, and permanent-delete confirmation.
