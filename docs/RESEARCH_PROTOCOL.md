# Research protocol: study questions and decision gates (P1 — NOT frozen)

Status: P0 in progress. This file states the P1 study questions from
`docs/RESEARCH_PLAN.md` P1 and the decision gates that will govern the future
confirmatory run. **Nothing here is frozen, and no data, labels, thresholds,
or results exist yet for this new P1 study.** Freezing happens in P1 before
test results are inspected; this file will record the freeze date and hash
when that happens.

Lineage distinction: frozen M8/corpus artifacts (M8 capture, preference
pairs, comprehension trials, pilot corpus, evaluation reports) DO exist and
remain unchanged exploratory/regression evidence. They are NOT a fresh holdout
for this P1 study and must not be re-tuned or rebranded as independent
validation (plan §2 table; `docs/M8_MODEL_CARD.md` disposition
`evidence_only`).

## Study questions (plan P1, restated)

- **RQ1:** Do detectors locate a real structural issue in context, rather
  than merely unusual code?
- **RQ2:** Do cleanup candidates preserve the declared contract under the
  selected verification and independent checks?
- **RQ3:** Do changes improve comprehension or maintenance outcomes beyond
  simple baselines?
- **RQ4:** Does revision/trajectory information improve candidate quality or
  validation efficiency beyond snapshot analysis?

## Scope of the first slice (planned, not started)

Four families: duplication, concentrated complexity/long methods, unnecessary
wrappers/indirection, branch simplification. Rust and Python first; Clojure,
Julia, JavaScript, TypeScript join as separate strata with their own corpora
and verification environments. Planning targets (240 natural units across
≥12 repository families + 240-case challenge set) are NOT a power
calculation; confirmatory sample size comes from pilot variance, repository
clustering, and a declared minimum useful effect.

## Decision gates (planned)

- **Measurement available:** P3 fidelity/provenance checks pass (no benefit
  claim implied).
- **Validated review suggestion:** confirmatory precision/reviewer-load
  criterion met for the named population; per-language results reported,
  failures never pooled away.
- **Automatic application:** P2 verification policy authorizes each exact
  candidate; zero observed unsafe applications in the counterexample/
  confirmatory corpus; uncertainty published (zero observed ≠ zero risk).
- **Maintenance-benefit claim:** additionally requires P7 evidence.

Detection/benefit thresholds are product decisions frozen from the pilot —
never constants from papers, never lowered to turn a failure into a pass.

## What is blocked

Independent annotators, licensed data imports, the frozen protocol itself,
and all confirmatory results. Read-only engineering evaluation may proceed;
independent validation and human-benefit claims remain blocked.
