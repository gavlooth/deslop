# M8 readability ranking model card

Schema: deslop.readability-model-card/1

## Decision

Disposition: evidence_only. No portable or language/role readability model ships.
Readability labels and probabilities are prohibited. The product retains all eight
transparent axes.

## Evaluation

One immutable capture contains 300 blinded preference pairs across eight languages and four
roles. It also records 1,727 timed/correct comprehension observations, 240 cleanup tasks, and
40 unsafe near-misses. The same capture produced all baselines, holdouts, confidence
intervals, calibration results, and ablations.

| Frozen score | Accuracy | Wilson 95% interval | ECE |
| --- | ---: | ---: | ---: |
| Portable challenger, all 300 | 0.5700 | 0.5134–0.6248 | 0.0764 |
| Size baseline | 0.3933 | 0.3397–0.4496 | 0.0173 |
| NLOC/complexity baseline | 0.3933 | 0.3397–0.4496 | 0.0175 |
| Lexical baseline | 0.5333 | 0.4768–0.5890 | 0.0815 |
| Challenger, 226 size-controlled | 0.5664 | 0.5012–0.6293 | 0.0779 |

The challenger improves aggregate accuracy over each baseline, but fails the frozen ship bar:
its overall lower bound is below 0.60, ECE exceeds 0.05, several language holdouts fail or
lose to the lexical baseline, published project identity yields only one unknown stratum, the
preference target is broader than direct human readability, and four axes are unavailable.
Language/role-specific models were not fitted or evaluated, so they cannot be selected.

## Intended use

Display and rank structural, lexical/visual, entropy, and other available evidence with their
authority and unknowns. Use the evaluation artifact to reproduce the evidence-only decision.

## Prohibited use

Do not infer authorship, AI generation, developer ability, removability, behavior
preservation, safety, or rewrite permission. Do not relabel heuristic burden as readability.

## Artifacts

- crates/deslop-eval/evaluation/m8/dataset_registry.json
- crates/deslop-eval/evaluation/m8/corpus.json
- crates/deslop-eval/evaluation/m8/evaluation_report.json
- docs/M8_PILOT_PROTOCOL.md
