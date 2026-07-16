# Undo and recovery

Deslop writes through an atomic transaction with a `deslop.undo-manifest/1` journal. Backups are part of write
authority, not a convenience flag for production automation.

## Normal apply

1. Recover or reject any incomplete earlier transaction.
2. Verify the work order, revision guards, source/read-set bytes, policy, and selected checks.
3. Write and fsync the journal containing original bytes, intended replacements, digests, and transaction state.
4. Replace files atomically, format/reanalyze, and validate the graph delta.
5. On success, fsync and mark the journal committed. On any error, restore exact original bytes and validate them.

`--no-backup` is not suitable for unattended release use. Controlled canaries remain explicitly opt-in.

## Recovery after interruption

Before another write, run the library recovery path (`recover_incomplete_transactions(root, ".deslop/undo")`) or
the corresponding CLI startup path. Recovery is deterministic:

- a prepared/partially written journal restores original bytes;
- a completely committed journal remains available for explicit undo;
- corrupt, missing, or drifted artifacts fail closed and require operator inspection;
- recovery validates exact digests and reruns declared rollback checks.

Do not delete, reorder, or hand-edit journal entries to force progress.

## Explicit undo

`deslop undo` restores a committed transaction only when current replacement bytes still match the recorded
replacement digest. Later user edits cause a drift rejection; Deslop will not overwrite them. Successful undo
restores exact original bytes and retains auditable result metadata.

## Operational evidence

For a failure report preserve the work-order/transaction IDs, revision, manifest, command evidence, graph-delta
result, recovery/undo result, and `jj status`. A rollback failure is a release-blocking integrity defect, not a
warning to suppress.
