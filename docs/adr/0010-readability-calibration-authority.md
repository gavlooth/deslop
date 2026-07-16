# ADR 0010: Readability calibration authority

Status: accepted, 2026-07-16.

## Decision

Deslop stores eight exclusive transparent evidence axes under
deslop.readability-features/1: structural, lexical/visual, surprisal, entropy, redundancy,
cohesion, impact, and safety. Counts and samples belong to one owned syntax slice. Inclusive
views must use the declared roll-up policy and may not sum overlapping nodes.

Cyclomatic complexity is McCabe E-N+2P over the complete exact owned CFG. When no such graph
exists, the legacy syntax branch estimate remains visibly marked as a fallback. Every entropy
measurement records its estimator and sample size. Missing axes are explicit unknowns rather
than zeroes.

A readability label is a separate authority. It is permitted only by
deslop.readability-policy/1 after frozen project/language holdouts beat size,
NLOC/complexity, and lexical baselines by the declared margin, clear the lower confidence
floor, meet the calibration floor, cover the required projects/languages, use direct human
perceived-readability labels, and have all eight axes.

The M8 pilot failed those gates. deslop.metrics/6 therefore embeds
readability_calibration.disposition=evidence_only and
readability_label_permitted=false. It exposes the axes but no readability score, class,
probability, refactor confidence, or rewrite authority.

## Consequences

- Aggregate accuracy cannot override a failed language/project holdout or calibration gate.
- Broad maintainability preferences and timed comprehension remain separate targets.
- Repo-relative surprisal and entropy cannot be relabeled as readability.
- Human/LLM provenance may be retained at dataset level for limitations, but row-level
  authorship is neither a feature nor a prediction target.
- Readability evidence never changes a transformation safety class or grants write authority.

## Supersession

A later model may replace evidence-only UX only with a new versioned policy, pinned dataset
registry/capture, model card, and held-out evidence. It may not weaken the frozen M8 result in
place.
