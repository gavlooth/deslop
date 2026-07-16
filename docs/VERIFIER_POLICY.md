# Verifier policy

The verifier is the only authority that can turn a proposal into a committed source change. A green parser or
test command alone is insufficient.

## Safety lattice

| Class | Release behavior |
|---|---|
| `SafeAuto` | Automatic only with a Ready M7 plan, exact bytes, complete selected evidence, exact graph delta, and durable rollback. |
| `AnalyzerConfirmed` | Review required; analyzer evidence must be current and conflict-free. |
| `SafeWithPrecondition` | Review required until every named precondition is proven on the pinned snapshot. |
| `RiskySuggest` | Review plus pre-change characterization and all selected checks. |
| `LlmOnly` | Proposal context only; model output has no semantic or write authority. |
| `NeverAuto` | Report-only and rejected by proposal/apply boundaries. |

## Planning checks

Impact selection starts from work-order resources and graph coverage. Complete dependency evidence may select a
bounded check set. Incomplete coverage widens to the project-wide build/lint/type/test fallback; it never narrows.
Compiler, LSP, and adapter observations are exact-artifact facts. Disagreement is `Conflict` and blocks.

Risky changes require characterization captured and approved on the exact pre-change snapshot before patch
authorship. A post-hoc test written to fit a patch is not characterization authority. Coverage and mutation results
remain typed evidence, including unknowns and surviving mutants.

## Runtime policy

Every command records program/arguments, working directory, timeout, environment policy, filesystem/network
policy, exit status, and bounded output evidence. Timeout, crash, truncation, unavailable sandbox, unavailable
tool, or policy mismatch is a structured failure. The server-owned runtime must not inherit an unrestricted shell
or silently enable network access.

## Commit protocol

Before commit, re-read exact target bytes, recheck preconditions, and reject stale handles/spans. Apply patches to
the journaled transaction, format if selected, reparse/reanalyze, and compare the actual graph delta with the
recipe contract. A mismatch rejects, rolls back, and records a recipe counterexample. Only then fsync and mark the
journal committed.

M10 limitation: whole-project finding-proposal batching exceeded the measured 15-minute dogfood ceiling and is
unshipped. Use bounded `index`, `triage`, `explain`, `plan`, `propose_patch`, and `verify` operations. This does not
weaken verifier policy; it reduces the query surface.

The detailed capability table remains in [M7_CAPABILITY_MATRIX.md](M7_CAPABILITY_MATRIX.md).
