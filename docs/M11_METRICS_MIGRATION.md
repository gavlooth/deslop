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

## Measurement specification

The following definitions are normative for `/7`; values are evidence, not a combined score.

- `nloc` is the count of nonblank, non-comment physical lines in the region's exclusive owned
  byte ranges. Token counts and AST counts are integer counts; ratios are dimensionless.
- Exact CFG cyclomatic complexity is `E - N + 2P` (edges, points, weak components) only when the
  stored graph is complete, non-recovered, and every edge is exact. Otherwise the report uses the
  syntax fallback `1 +` the owned adapter branch count and records an explicit structural unknown;
  fallback is never CFG authority.
- Structural mass is `cyclomatic * sqrt(nloc)` and complexity mass is
  `ln(1 + structural_mass)`. File structural mass sums exclusive behavioral-region masses.
  `structural_erosion` is the fraction of that mass belonging to behavioral regions with
  cyclomatic complexity greater than 10; it is not a causal or quality estimate.
  These equations match SlopCodeBench v1 §2.3, Eqs. 2–3
  (<https://arxiv.org/html/2603.24755v1>). Deslop's exclusive owned ranges,
  cross-language syntax fallback, file-region fallback when no behavioral
  region exists, and `ln(1 + mass)` feature are local adaptations, not a
  reproduction of the paper's Python callable population or result.
- Shannon entropy uses the plug-in distribution `p_i = count_i / sum(counts)` and base-2 logs.
  Normalized entropy divides by `log2(k)` for the `k` observed positive-count categories and is
  `0` for zero or one observed category. AST-edge entropy is computed from exclusive owned
  parent-child edge counts. Byte entropy is bits per byte, not a compression ratio.
- Repository surprisal is an optional bits/token add-one bigram estimate. Each region is scored
  against same-language peers with that region's tokens, contexts, and vocabulary removed
  (leave-region-out); `mean`, `p90`, and `max` are over the region's token-start pairs. A
  region with no tokens, no peer tokens, or no peer vocabulary has unavailable surprisal, never
  confident zero. It is not an LLM probability.
- Small samples remain measured only where the stated estimator has observations; peer robust
  z-scores require at least eight regions and nonzero MAD. Missing/unsupported projections retain
  explicit unknown reasons and are not imputed as zero.
- Feature IDs are content-addressed by schema, subject (`lang`, name, kind, span), and axis
  evidence. Equal feature content intentionally deduplicates; a consumer needing occurrence
  counts must retain the surrounding region/path occurrence separately. Paths are presentation
  identity and are not added to the content digest.

Reference regressions use exact arithmetic where applicable and floating-point tolerances of
`1e-12` for small formula cases; serialized/projection comparisons require exact equality.
