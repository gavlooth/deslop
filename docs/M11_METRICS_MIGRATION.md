# M11 metrics migration: deslop.metrics/6 to /7

`deslop.metrics/7` preserves every `/6` intrinsic measurement, heuristic-burden field,
evidence-only readability disposition, and no-gating restriction. It adds transparent evidence;
it does not restore a health/readability score or authorize rewrites.

## New region fields

- `role`: `behavioral`, `container`, `file`, or `other` for peer grouping and aggregation.
- `complexity.structural_mass`: `cyclomatic * sqrt(nloc)`.
- `complexity.complexity_mass`: `ln(1 + structural_mass)`.
- `expressivity.ast_edge_entropy`: normalized Shannon entropy over parent-child syntax-edge
  categories; `static_slop.structural_entropy` uses this proposal-aligned value.
- `surprisal`: optional mean/p90/max bits from the deterministic
  `requested-snapshot-leave-one-region-out-bigram-add-one/1` estimator. It is repository-local
  only when the invocation requests the repository.
- `redundancy`: clone, anti-pattern, dead/unused, and union line counts and ratios. Findings are
  assigned to one narrowest region and line-unioned, so overlapping findings do not double-count.
- `static_slop`: the inspectable `(C, X, A, R)` vector plus robust peer-relative evidence. Its
  authority is `transparent_vector_only`; no scalar or probability is emitted.

## New report fields

- `files`: function-weighted summaries with structural mass, SlopCodeBench-style erosion,
  upper-tail complexity/surprisal/structural-entropy/redundancy components, weighted
  mean/p90/max heuristic burden, union redundancy ratio, and the top hotspot name.
- `peer_groups`: the language/role/NLOC-bin populations used for median/MAD normalization.
- `change_dispersion`: `null` by default. The CLI populates it when `--from` is supplied, using
  Git numstat changed-line counts and normalized Shannon entropy across changed text files.

## Compatibility

Clients must negotiate `/7`. They must not:

- read absent surprisal as zero; a one-region language population legitimately has no peers;
- interpret a missing robust z-score as zero; groups below eight or with zero MAD abstain;
- reinterpret repository-local bigram surprisal as an LLM probability;
- sum redundancy sub-ratios; use `union_ratio` when one non-overlapping total is required;
- use `static_slop`, file summaries, change entropy, heuristic burden, or outliers as write
  authority, human readability, health, authorship, or refactor necessity.

The existing M8 calibration remains `evidence_only`. `/7` has not been refitted against the M8
capture or maintenance outcomes, so the new vector deliberately has no candidate scalar.
