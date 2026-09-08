# Refuse publication of a copy assembled from changing source data

Status: ready-for-agent
Type: bug
Priority: P1

`src/fs/tree_copy.rs:414` copies in chunks, and `src/fs/mutation.rs:118` publishes ordinary Copy without validating the source after reading it. Replace similarly snapshots the source only for Move. Rewriting the source after 1 MiB creates a 4 MiB result mixing 1 MiB of A with 3 MiB of B, reported as success without warnings. Reproduced for ordinary Copy, Replace Copy, and a sparse source. The Replace path overwrites the existing destination with that mixed result.

Evidence: `../copy-matrix.log`. Cross-device Move with and without Replace correctly refuses the same scheduled edit and preserves the source and previous destination.

Acceptance: a changing source cannot be silently published as a successful mixed copy. Refuse conservatively with retry information; keep source data and any previous destination intact. Cover ordinary, Replace, sparse, directory/merge paths, and changes that preserve size or timestamps when designing the consistency guarantee.
