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

## P1 pilot rubric (engineering spec, versioned — NOT frozen)

Protocol version: P1-pilot/0 (draft). This section is versioned engineering
specification, not a frozen confirmatory protocol. Threshold, power, and
sample-size freeze requires real pilot variability, a declared minimum useful
effect, and an owner decision; none of those exist yet.

### Three separate labels (never merged)

- `observation_label`: is the structural pattern observably present in context
  (per family rubric below)? Values: present / absent / uncertain.
- `cleanup_judgment`: would removal/change be desirable given the declared
  `task_contract`? Values: desirable / undesirable / uncertain. Recorded
  independently of `observation_label` and of any tool verdict.
- `verification_result`: outcome of the exact candidate state under the
  selected verification (tests, verifier policy per `docs/VERIFIER_POLICY.md`,
  independent checks). Values live in the machine schema (`outcome` plus
  `abstained`); reviewers never overwrite this field by opinion.

Frozen M8 / legacy `Clean`/`Sloppy` corpus labels are regression inputs only
and MUST NOT be recycled as any of the three labels above (plan P1 items 2, 7;
`docs/M8_MODEL_CARD.md` disposition `evidence_only`).

LLM outputs are never independent human annotations. Rows with
`annotation_provenance` model-derived or unresolved MUST NOT serve as cleanup
ground truth.

### Initial four families (planned scope)

1. Duplication (near-duplicate / copy-paste regions).
2. Concentrated complexity / long methods.
3. Unnecessary wrappers / indirection.
4. Branch simplification.

All other registry families retain honest heuristic status until separately
evaluated (plan P1 item 3). Rust and Python are the first strata; Clojure,
Julia, JavaScript, TypeScript join as separate strata with their own corpora
and verification environments (plan P1 item 4).

### Planning targets (NOT power calculation)

240 naturally sampled units across >=12 unrelated repository families, plus a
separate 240-case challenge set across the four families and two initial
languages. Confirmatory sample size comes later from pilot variance,
repository clustering, and the declared minimum useful effect (plan P1 item 10).

### Sampling and annotation requirements

- Sample natural units independently of deslop findings (detector-independent
  sampling/enumeration); reviewers enumerate relevant issues within sampled
  units so misses are countable. Record `selection_probability` and
  `workload_stratum` (plan P1 item 6).
- Include genuine positives, intentional counterexamples, and ambiguous cases;
  negatives are not all trivial, positives not all synthetic (plan P1 item 7).
- Two independent annotators blinded to model identity and tool verdict, plus
  third-party adjudication. Retain `original_disagreement` and uncertain
  labels; never force ambiguous cases into clean/sloppy (plan P1 item 8).
- Split by `repo_family` before calibration; group forks, near-clones, tasks,
  and all checkpoints of one trajectory via `clone_group`. Human-study
  participants never see both versions of the same task. Confirmatory holdout
  sealed (`eligibility=sealed-confirmatory`) before calibration (plan P1 item 9).
- Count rejected, timed-out, unparsable, and unbuildable candidates in
  `outcome`; never report only successful rewrites. Use repository/task
  cluster-aware intervals; no pooled failure hiding (plan P1 baselines).
- Baselines: detection — existing linter findings, size/complexity-only
  triage, current deslop; cleanup — no cleanup, existing deterministic
  recipes, task-appropriate bounded minimization for trajectory experiments.

### Machine contract (owned by p1-evidence-engine code, not this doc)

Schemas `deslop.pilot-case/1` + `deslop.pilot-eval/1` (strict
`deny_unknown_fields`) own the machine-readable fields. Canonical names
(agreed with p1-evidence-engine): `case_id`, `repo_family`, `clone_group`,
`revision`, `unit_kind`, `language`, `source_range`, `task_contract`,
`observation_label`, `cleanup_judgment`, `verification_result`, `outcome` +
`abstained`, `preconditions`, `counterexamples`, `selection_probability`,
`workload_stratum`, `split`, `eligibility`, `protocol_pin`, `annotator_ids`,
`annotation_provenance`, `adjudication`,
`agreement_retained`/`original_disagreement`, `provenance`, `license_spdx`,
`license_evidence`, `retrieval_checksum`. This document defines meaning only
and duplicates no machine manifest.

### Freeze conditions (no TODO stubs; explicit gates)

The confirmatory protocol is frozen when ALL hold: (a) pilot-case and
pilot-eval schemas versioned and tagged; (b) extraction, inclusion/exclusion,
baselines, primary endpoints, failure/timeout handling, and analysis code
pinned by hash in `protocol_pin`; (c) pilot variance + clustering + minimum
useful effect recorded with owner sign-off setting sample size; (d) license,
checksum, redistribution, consent, and ethics prerequisites below satisfied;
(e) freeze recorded with date + hash in this file before test results are
inspected. Until then every P1 artifact stays draft.

### Licensing / redistribution / consent / ethics (prerequisites)

- Distinguish `license_spdx`/`license_evidence` (source-repo terms permitting
  import, i.e. license-known vs reviewed-grant) from the annotation-data
  license governing the new labels. Missing redistribution permission blocks
  import, not merely publication (plan P1 item 11).
- Record `retrieval_checksum` per source; redact secrets before storing
  trajectories; obtain participant consent and applicable ethics approval
  before any human study (plan P1 item 11).

### Feasibility (read-only repository evidence, 2026-09-08)

- Licensed reusable inputs: Themis-CodePreference Apache-2.0 and Bergum
  AoC-FRP CC-BY-4.0 are approved imports for their own M8 targets only
  (`crates/deslop-eval/evaluation/m8/dataset_registry.json`, `docs/M8_DATASET_REPORT.md`).
  Neither provides P1 cleanup ground truth (preference/model-consensus and
  Java atoms-of-confusion targets; Themis lacks project identity).
- Dorn readability mirror: rejected — no redistribution license
  (`docs/M8_DATASET_REPORT.md` Rejected source). Sjoberg/Buse full texts
  remain blocked; not retried. All other P0 literature sources: paper terms
  only, dataset licenses unverified, no import
  (`crates/deslop-eval/evaluation/research/registry.json` sources).
- Controlled M8 pilot rows mix human and LLM-assisted producers with blinded
  authorship (`docs/M8_PILOT_PROTOCOL.md`); they are NOT independent human
  annotations and MUST NOT serve as P1 cleanup ground truth.
- No independent annotators, no 12-family licensed import set, and no ethics
  approval exist in the repository. External P1 study is therefore BLOCKED;
  reachable protocol/engineering requirements in this file are complete.
