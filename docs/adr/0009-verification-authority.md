# ADR 0009: Revision-pinned verification authority

Status: Accepted (2026-07-16)

## Context

M5 recipes already carry preconditions, impact cones, expected graph deltas, validation plans, and reverse edits.
M6 turns each candidate into one immutable work order and schedules it against declared resources. The previous
verifier, however, combined local parse guards, caller-selected shell commands, coverage/mutation adapters, and
per-file renames. It did not provide one authority boundary for selecting checks, preserving provider disagreement,
pinning characterization evidence, constraining execution, comparing graph deltas after formatting, or recovering a
multi-file commit after process interruption.

A green command is evidence, not equivalence proof. Tree-sitter syntax, adapter facts, compiler artifacts, language-
server artifacts, and runtime checks retain different authority. No stable ordering or provider rank may erase a
semantic conflict.

## Decision

`deslop-verify` owns the following strict M7 layers:

1. `deslop.verifier-plan/1` binds one exact shared work order and project snapshot to a dependency-closed check set,
   adapter/compiler/language-server precondition decisions, execution policy, and residual uncertainty. Complete
   impact coverage selects checks by intersecting work-order resources. Incomplete/truncated/unknown coverage selects
   every project build, lint, type, and test fallback.
2. Provider observations are artifact- and snapshot-bound. Adapter, compiler, and language-server conclusions are
   retained independently. Current accepted Proven and Disproven evidence yields Conflict and blocks; no provider
   precedence chooses a winner. Syntax and runtime labels cannot masquerade as these semantic providers.
3. `deslop.pre-change-characterization/1` binds captured behavior, test artifact, approval, work order, and snapshot.
   Risky characterization must be captured and approved before patch authorship. A test generated only after the
   rewrite cannot authorize the rewrite.
4. Selected parse/format/build/lint/type/targeted-test/coverage/characterization/differential/mutation/graph-delta
   checks return typed evidence. Every selected check must have current matching evidence. SafeAuto can cross the
   automatic boundary only with a ready plan and all selected checks passing. Every weaker class remains review-only
   or rejected and carries explicit uncertainty.
5. Commands run only through an explicit time/output/file/filesystem/environment/network policy. The production
   command runtime uses an external namespace sandbox, clears the environment, denies network, limits time/output and
   post-command file counts/sizes, and fails closed when the requested sandbox cannot be established. Generic shell
   execution cannot implement host-level allowlists, so such policies reject instead of degrading silently.
6. Verification stages exact candidate bytes in an isolated workspace, rechecks revisions/preconditions, compares an
   authoritative actual graph delta to the recipe delta, formats, checks protected bytes/files, and repeats graph
   reanalysis. The exact formatted bytes are the bytes considered for commit. Live graph reanalysis follows commit.
7. Source commit uses `deslop.undo-manifest/1`. Original bytes and digests are fsynced before a transaction enters
   Committing. Same-directory replacement files are fsynced before ordered renames; the journal is fsynced at each
   state transition. Ordinary failures restore every original byte immediately. A process interruption leaves a
   Prepared/Committing journal that startup recovery deterministically restores. Explicit undo rejects subsequent
   source drift.
8. A format/graph/differential counterexample immediately appends `deslop.recipe-demotion/1` to the project-local
   negative-memory journal. A demoted recipe cannot execute automatically until an explicit authority/reason
   supersession is appended. Records are content-addressed; corruption and orphan supersessions fail closed.

M7 does not make client-supplied M6 observations authoritative and does not infer graph deltas from test success.
`GraphDeltaOracle` is a required production boundary: a caller without an authoritative reanalysis implementation
cannot complete the transaction.

## Consequences

- Check selection narrows only under complete impact authority; missing facts increase work.
- Compiler and language-server disagreement becomes visible and blocking.
- Risky characterization has a temporal and revision identity instead of being an unpinned test file.
- Sandboxing unavailable on a host is an explicit policy failure, not permission to run unsandboxed.
- A verified staged transaction either installs its exact considered bytes with durable undo or restores exact prior
  bytes. Recovery is required after an injected hard-crash boundary and is deterministic from the journal.
- Recipe counterexamples stop subsequent automatic use immediately and remain auditable after supersession.

## Rejected alternatives

- Selecting only tests named near the changed file when dependency coverage is incomplete.
- Letting compiler rank overwrite a conflicting language-server conclusion.
- Treating post-rewrite generated tests as proof of prior intended behavior.
- Running commands with the caller's inherited environment/network when sandbox setup fails.
- Per-file backups and rename loops without a transaction state journal.
- Logging a counterexample while leaving the recipe automatically enabled.
