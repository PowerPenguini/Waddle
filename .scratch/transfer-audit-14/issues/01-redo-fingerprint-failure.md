# Preserve completed Redo effects when fingerprinting fails

Status: ready-for-agent
Type: bug
Priority: P1

`src/journal/effects.rs:329` reads the destination fingerprint before setting `item.undone = false`. A successful Copy or Move followed by a transient read error is saved as still undone. Retry fails, and partial-effect guards do not recognize it. In both reproduced variants, Undo deleted an unrelated older New File entry while the new destination remained.

Evidence: `../history-matrix.log`, tests matching `audit_*redo_recovers_after_fingerprint_read_error` and `audit_*fingerprint_error_protects_older_history`.

Acceptance: transient post-effect read failures remain recoverable after reopening, completed data is preserved, unrelated history cannot be crossed, and ordinary Undo/Redo works after recovery. Include a genuinely modified destination to ensure recovery does not trust an arbitrary existing file.
