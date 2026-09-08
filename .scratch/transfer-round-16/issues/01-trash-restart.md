# Preserve Trash and Restore history through process interruption

Status: resolved
Type: bug
Priority: P1

Physical moves preceded persistent progress, leaving missing source paths on retry. Reproduced with an isolated child exit after Trash service completion; the full test covers Trash Undo/Redo and Restore Undo/Redo. Fixed with per-item checkpoints, prepared-publication recovery for restoration, and verified receipt recovery for pending Trash. See `../report.md` and `../trash_probes.rs`.
