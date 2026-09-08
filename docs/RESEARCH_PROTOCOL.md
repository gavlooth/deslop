# Research protocol: study questions and decision gates (P1 — NOT frozen)

Status: P1 engineering facility implemented; the confirmatory protocol is
still NOT frozen. This file states the P1 study questions and decision gates
for a future confirmatory run. No independent human-study data, licensed
external import, validated detector result, or maintenance-benefit result
exists. The repository fixtures and CLI smoke evidence described below are
fictional engineering checks only. Freezing happens in P1 before real test
results are inspected; this file will record the freeze date and hash when
that happens.

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

## Scope of the first slice (engineering facility; study still planned)

The facility is bounded to four families: duplication, concentrated
complexity/long methods, unnecessary wrappers/indirection, and branch
simplification. Rust and Python are the first strata; Clojure, Julia,
JavaScript, and TypeScript join as separate strata with their own corpora and
verification environments. Planning targets (240 natural units across
≥12 repository families + 240-case challenge set) remain planning targets,
not a power calculation, actual sample, or claim of adequate evidence for
release. Confirmatory sample size comes from pilot variance, repository
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

Independent annotators, licensed external data imports, the frozen protocol
and all confirmatory results remain blocked. Read-only engineering evaluation
may proceed; independent validation, population claims, human-benefit claims,
and automatic cleanup authorization remain blocked.

### Implemented read-only facility (engineering only)

`crates/deslop-eval/src/pilot.rs` provides strict, versioned import and
evaluation schemas: `deslop.pilot-import/1`, `deslop.pilot-case/1`,
`deslop.pilot-eval/1`, and `deslop.pilot-manifest/1`. The import path validates
the typed `source_license` and independent `annotation_license` grants,
retrieval/source checksums, source ranges, protocol and full effective
analyzer-config pins, duplicate unit identity, and transitive split leakage
before writing case files. Case IDs are importer-derived
`pilot1_` + 64 hexadecimal BLAKE3 characters and include the complete
semantic input plus protocol/config pins.

Each case retains raw source bytes, provenance, task contract, preconditions,
counterexamples, selection probability, workload stratum, split, two
distinct reviewer records, optional adjudication, derived resolution and
eligibility, and untrusted `declared_check`/`check_evidence`/
`supplier_outcome` fields. The importer derives agreement and eligibility;
callers cannot self-supply either. `evaluate_dir` revalidates stored raw and
derived data, excludes sealed cases before analysis and counters, runs only
the within-file analyzer on every available unsealed case, and keeps actual
analysis status separate from supplier outcomes. The in-memory scorer is
private; public entry points are `import_bundle` and `evaluate_dir`.

Reports expose per-case `predictions`, `supplier_outcomes`,
`analyses_complete`/`analyses_unavailable`, `failures`, and strata keyed by
family/language/provenance/unit kind. Confusion rates use eligible known
labels only; unsupported, unavailable, unresolved, synthetic, and sealed rows
do not become independent truth. Raw analysis coverage and observable
recommendation counts remain separate from scoring; precision/recall and all
zero-denominator rates are null rather than zero. Findings are constrained to
the selected `source_range`. Rust/Python branch simplification is explicitly
unsupported because `reimpl-boolean` has no emitter there, not a clean
negative; missing project context is unavailable, and threshold misses remain
genuine false negatives.

The CLI in `crates/deslop-eval/src/bin/pilot_eval.rs` accepts only the
flag-checked forms `import --bundle PATH --out DIR` and
`eval --dir DIR --protocol-pin PIN`; it performs no network, command,
or model execution. The last recorded smoke evidence
(`/tmp/deslop-p1-smoke.log`) uses fictional fixtures: import of four cases
and evaluation with one sealed case exits successfully; three unsealed cases
produce predictions `true,true,false`, supplier outcomes count three, and
`analyses_complete=3`, `analyses_unavailable=0`. A two-case challenge slice
reports TP=1, TN=1, precision=1, recall=1, recommendation rate 0.5, and
coverage 1. A one-case synthetic slice retains a positive prediction while
precision/recall are null and recommendation rate and coverage are 1. These
are execution checks, not empirical study results.

## P1 pilot rubric (engineering spec, versioned — NOT frozen)

Protocol version: P1-pilot/1 (draft). This section is versioned engineering
specification, not a frozen confirmatory protocol. Threshold, power, and
sample-size freeze requires real pilot variability, a declared minimum useful
effect, and an owner decision; none of those exist yet.

### Three separate judgments (never merged)

- `observation`: is the structural pattern observably present in context
  (per family rubric below)? Values: present / absent / uncertain. A purely
  structural call — it NEVER requires that cleanup would preserve the task.
- `cleanup judgment`: would removal/change be desirable given the declared
  `task_contract`? Values: desirable / undesirable / uncertain. Recorded
  independently of `observation` and of any tool verdict. Intentional
  duplication can be Present + Undesirable; an inherently complex algorithm
  can be Present + Undesirable; a thin public-API wrapper can be Present +
  Undesirable.
- `declared check`: the supplier's UNTRUSTED observation about a cleanup
  candidate, retained with its evidence binding. Values live in the machine
  schema (`DeclaredCheckObservation` plus optional `CheckEvidence`). P1 runs
  no checks: a `Preserved` claim without evidence binding is reported
  unbound; raw untrusted checks are never trusted preservation receipts and
  never filter static analysis. Reviewers never overwrite actual analysis by
  opinion.

Frozen M8 / legacy `Clean`/`Sloppy` corpus labels are regression inputs only
and MUST NOT be recycled as any of the three judgments above (plan P1 items
2, 7; `docs/M8_MODEL_CARD.md` disposition `evidence_only`).

LLM outputs are never independent human annotations. Rows with
`AnnotationProvenance` model-derived or unresolved MUST NOT serve as cleanup
ground truth.

### Initial four families with annotation rubric (planned scope)

Each family states observable positive / negative / uncertain criteria for
the STRUCTURAL observation only. Desirability is judged separately per case
under the declared `task_contract`, aided by explicit `counterexamples` —
known situations where the pattern is present but cleanup is undesirable.

1. **Duplication** (near-duplicate / copy-paste regions). Observation
   positive: two or more regions share the same or near-same token sequence
   in context. Negative: one-off code with no repeated region, or
   similar-looking code whose token sequences actually diverge. Uncertain:
   overlap too small to call, or whether the divergence is semantic is
   unclear. Cleanup counterexamples (Present yet Undesirable):
   version-conditional branches, test fixtures, or compatibility shims kept
   intentionally redundant despite real repetition.
2. **Concentrated complexity / long methods.** Observation positive: one
   callable concentrates multiple responsibilities or branch clusters.
   Negative: flat sequences or declarative tables with no
   responsibility/branch concentration. Uncertain: long but the
   responsibility boundary is unclear. Cleanup/provenance counterexamples: a
   single inherent algorithm that is long yet has no clean split line, so
   splitting would harm the contract; generated code kept as-is by provenance
   or regeneration policy despite real concentration.
3. **Unnecessary wrappers / indirection.** Observation positive: a closure
   or function that only forwards arguments unchanged with no additional
   computation (bare passthrough, needless formatting). Negative: the wrapper
   adds behavior — error mapping or type/lifetime adaptation. Uncertain:
   possible reflective or dynamic dispatch use. Cleanup counterexamples: a
   thin public-API stability or compatibility wrapper that is structurally a
   passthrough yet must stay (API compatibility ONLY here, never an
   observation negative).
4. **Branch simplification.** Observation positive: a boolean expression with
   observable literal-selection or redundant-looking boolean structure (e.g. a
   literal branch or repeated comparison shape). Negative: no such structure
   present. Uncertain: macro- or overload-dependent semantics, or whether the
   redundant-looking form actually preserves the truth table. Cleanup
   counterexamples: explicitness the contract requires (defensive checks,
   NaN-sensitive float comparisons) despite a redundant-looking form.

All other registry families retain honest heuristic status until separately
evaluated (plan P1 item 3). Rust and Python are the first strata; Clojure,
Julia, JavaScript, TypeScript join as separate strata with their own corpora
and verification environments (plan P1 item 4).

### Declared detector capability (before evaluation, never data-dependent)

- Branch simplification on Rust/Python currently has NO emitter
  (`reimpl-boolean` is Clojure-only): such cases can be stored and reported
  as capability-unavailable, but never scored as clean negatives.
- Analysis scope is within-file only: per-case evaluation scans one stored
  source file. Cross-file duplication truth needs a pinned multi-file scope
  that is explicitly unavailable — missing project context yields
  unknown/unavailable, never a false claim of full-project accuracy.
- Threshold misses are genuine false negatives inside the supported domain:
  no below-threshold positive is excluded and no unsupported pattern is
  filtered post-hoc from findings. Human truth is never altered from
  detector output.

### Sampling, annotation, and splits

- Sample natural units independently of deslop findings (plan P1 item 6):
  reviewers enumerate relevant issues within sampled units so missed
  findings can be counted. Record `selection_probability`
  (`0 < p <= 1` when known; `None` means unknown/non-sampling; natural rows
  with `None` are rejected at import) and `workload_stratum`; report rates as
  unweighted sampled-case descriptive estimates, never deployment precision or
  prevalence without a declared design.
- Two independent annotators, blinded to model identity and tool verdict,
  with third-party adjudication (plan P1 item 8). Retain distinct raw rater
  records plus adjudication; derive agreement server-side; retain original
  disagreement and uncertain labels — never force ambiguous examples into
  clean/sloppy. Model-derived outputs are never independent ground truth.
- Split by repository family before calibration (plan P1 item 9): group
  forks, near-clones, tasks, and all checkpoints of one trajectory via
  lineage identities. Sealed confirmatory rows are excluded BEFORE analysis
  and aggregations — their labels and outcomes never enter denominators.
### Estimators (honest sample-only rates)

Per stratum (family, language, provenance, unit-kind — never pooled): precision
= TP/(TP+FP) conditional on predicted-positive; recall = TP/(TP+FN) over all
eligible ground-truth positives including misses; reviewer-load and
abstention/coverage use explicit disjoint denominators; zero denominators
are null, never zero-as-success. No clustered confidence intervals and no
population claims without the frozen-protocol data and design.

### Machine contract (owned by p1-evidence-engine code, not this doc)

Schemas `deslop.pilot-case/1` + `deslop.pilot-eval/1` + strict typed manifest
`deslop.pilot-manifest/1` own the machine-readable fields. Every P1 wire
struct carries `deny_unknown_fields`, including each reused M8 `LicenseRecord`
itself (verified: `crates/deslop-eval/src/m8_calibration.rs` applies it on the
struct, so strictness is per-struct, not inherited from the outer bundle).
Unknown license metadata therefore fails loudly at import rather than silently
dropping a restriction — at the cost that any future M8 license field addition
breaks P1 import until the schemas move together. The code binds fields as
nested records so contradictory flags cannot be set independently: licensing is
TWO independent typed grants — `source_license` (all bundle source bytes) and
`annotation_license` (reviewer records); each is an M8 `LicenseRecord`
(approved decision, SPDX, evidence URI, calendar-date `checked_on`) with free-
text `reason` that is never parsed for policy. Provenance is a record
(`ProvenanceRecord.kind` natural / challenge / imported /
engineering-synthetic + `detail`); annotation is a record of distinct raw
`ReviewerRecord`s plus optional `AdjudicationRecord`
(`reviewer_declared_provenance`, `reviewer_declared_blinded`,
`adjudicator_declared_provenance`, `adjudicator_declared_blinded`);
`eligibility` and `agreement` are DERIVED server-side (sealed-confirmatory
split never scores; imported/synthetic, unresolved, and non-evaluated rows
never score as ground truth). Case identity is `pilot1_` + 64 blake3 hex bound
to the full input plus protocol and analyzer pins; the analyzer pin is the
full effective config digest via the public `analyzer_config_pin()` helper
(never a hand-pinned subset). Synthetic/unresolved rows still run the analyzer
and produce observable predictions/diagnostics, but never independent-truth
scores. Supplier disposition is retained as `supplier_outcome` (untrusted,
reporting-only) plus `DeclaredCheckObservation` + optional `CheckEvidence`
(`candidate_digest` / `evidence_ref`) — untrusted supplier data that never
gates static detection and never trims denominators. Humans supply observations
and adjudication records — never a self-attested `eligibility` or agreement
bit. Public types live in `crates/deslop-eval/src/pilot.rs` (`PilotCaseInput`,
`PilotCase` (with stored `supplier_outcome`), `OutcomeCounts` (actual analyzer
status), `StratumScore` (keyed family / language / provenance / `unit_kind`;
`input_total` / `eligible_total` = scored + abstained / `unsupported` /
`uncovered_overlapping`, TN inside scored), `PilotEvalReport`
(`cases_total` / `sealed_excluded` / `declaration_note` /
`confidence_interval_note` / `sampling_note` / `failures`),
`family_rules` / `stratum_rules` / `stratum_supported`, `analyzer_config_pin`,
`import_bundle`, `evaluate_dir`). Public entries are `import_bundle(bundle,
dir)` and `evaluate_dir(dir, protocol_pin)` (which reads the manifest licenses
+ config pin); the in-memory scorer is intentionally private so unscreened
cases cannot bypass the license gate. CLI usage in
`crates/deslop-eval/src/bin/pilot_eval.rs`
(`import --bundle PATH --out DIR`, `eval --dir DIR --protocol-pin PIN`;
flag-checked `kv2`, extra args rejected).
string is not a redistribution grant, and declared blinding is not proof of
blinding. Curators supply evidence; the program only verifies it is present
and well-formed.

### Freeze conditions (no TODO stubs; explicit gates)

The confirmatory protocol is frozen when ALL hold: (a) pilot-case and
pilot-eval schemas versioned and tagged; (b) extraction, inclusion/exclusion,
baselines, primary endpoints, failure/timeout handling, and analysis code
pinned by hash in `protocol_pin`; (c) pilot variance + clustering + minimum
useful effect recorded with owner sign-off setting sample size; (d) license,
checksum, redistribution, consent, and ethics prerequisites below satisfied;
(e) freeze recorded with date + hash in this file before test results are
inspected. Until then every P1 artifact stays draft. Verified machine inputs
are license/checksum attestations with evidence — never detector outputs
recycled as truth (plan P1 item 8, acceptance).

### Licensing / redistribution / consent / ethics (prerequisites)

- The license record is the existing M8 attestation shape (decision / spdx /
  evidence_uri / checked_on / reason plus `retrieval_checksum`): evidence,
  not a magic SPDX string — import requires an explicit approved grant with
  evidence and matching checksums covering BOTH source bytes and annotation
  data. Missing redistribution permission blocks import, not merely
  publication (plan P1 item 11). `checked_on` is validated for date accuracy
  only; no invented expiry clock without a declared policy.
- Record `retrieval_checksum` per source; redact secrets before storing
  trajectories; obtain participant consent and applicable ethics approval
  before any human study (plan P1 item 11).

### Feasibility (read-only repository evidence, 2026-09-08)

Per-source verdicts, reusing the registry license-record convention
(`decision` / `spdx` / `evidence_uri` / `checked_on` / `reason` + checksum):

- Themis-CodePreference (`crates/deslop-eval/evaluation/m8/dataset_registry.json`
  id `themis-code-preference-2025`: approved / Apache-2.0 / evidence
  `https://huggingface.co/datasets/project-themis/Themis-CodePreference#license`
  / checksum `sha256:8ea45581…aba414` / 24-parquet commit
  `7c366b23…`): classifier-selected + multi-model consensus rows with no
  project identity — NOT independent human P1 labels and unusable for
  family grouping. Usable only as an imported preference benchmark kept
  separate from natural/challenge results.
- Deslop controlled M8 pilot (`dataset_registry.json` id
  `deslop-controlled-m8-v1`: approved / MIT / `evidence_uri` `LICENSE` /
  240 fixed tasks across six languages): row schema carries no authorship
  field; producers are mixed human and LLM-assisted with 40 deterministic
  unsafe near-misses (`docs/M8_PILOT_PROTOCOL.md`). NOT independent human
  annotations; MUST NOT serve as P1 cleanup ground truth. License caveat:
  `evidence_uri` points to `LICENSE`, which is absent from this checkout
  (glob for `LICENSE*`/`LICENCE*`/`COPYING*` at any depth returns nothing;
  workspace `Cargo.toml` declares `license = "MIT"` cargo-package metadata
  only, which is not a redistribution grant for the dataset) — treat the
  archived M8 approval as unresolved until the grant document is located;
  do NOT assert the entire codebase is unlicensed from this single missing
  dataset-evidence file. `checked_on` 2026-07-16 predates this review.
- Bergum et al. AoC-FRP (`dataset_registry.json` id
  `brains-on-code-aoc-frp-2026`: approved / CC-BY-4.0 / Nature
  data-availability + Zenodo 14229849 / checksum
  `sha256:8c986528…c51d1`): genuine human data (24 participants, 1,727 Java
  callable trials) but for timed/correct atoms-of-confusion comprehension
  only — NOT Rust/Python cleanup labels and NOT any of the three P1
  judgments.
- Dorn readability mirror (`dataset_registry.json` id
  `dorn-general-readability-2012`): rejected / spdx null / card README has
  no redistribution grant / checked 2026-07-16 — public downloadability
  treated as insufficient authority (`docs/M8_DATASET_REPORT.md` Rejected
  source). Not imported.
- All other P0 literature sources
  (`crates/deslop-eval/evaluation/research/registry.json` sources): paper
  terms only (arXiv/author-manuscript/DOI access), dataset artifact licenses
  unverified or revisions unpinned — no import. Sjoberg/Buse full texts
  remain blocked; not retried per assignment.
- Annotators / consent / ethics: no independent annotator roster, no
  participant-consent record, and no ethics approval exist anywhere in the
  repository. No 12-family licensed Rust/Python import set exists either.
  External P1 study is therefore BLOCKED; reachable protocol/engineering
  requirements in this file are complete.

## Engineering interfaces after P1 (not a completed study)

The following interfaces produce review evidence. They do not freeze this
protocol, confer statistical promotion, or satisfy P7 human-benefit criteria.

### Execution and editor contracts

- CLI/MCP patch batches are prepared together, checked as a composed candidate
  when a selected check exists, and rejected without source writes if a patch,
  composed check or commit-boundary read set fails. Review-only results are
  not default write authorization. Transaction journals distinguish committed,
  rolled-back and recovery-required state.
- External selected commands run under a fresh systemd user scope, namespace
  sandbox, cleared environment, kernel per-file limit and output/time bounds.
  `deslop.verifier-plan/2` requires memory/process budgets; defaults are 2 GiB,
  no swap and 256 processes. Missing user manager/controllers, `prlimit`,
  sandbox or unsupported adapters fail closed. File-count watchdogs may
  detect an overrun after it occurs; they are **not a hard aggregate disk quota**.
  This is not certification for arbitrary hostile workloads.
- System executable/library directories and the staged workspace are the
  supported mounts. An environment allowlist does not authorize exposing host
  homes: HOME/CARGO_HOME/RUSTUP_HOME/TMPDIR resolve inside scratch space.
  Toolchains outside supported roots are unavailable, not a reason to mount
  host credentials. Automatic coverage/cargo-mutants/cosmic-ray adapters
  without a policy-bound launcher report unavailable; supplied outcomes
  remain evidence with their existing provenance limits.
- Rust native mutation validity supports a plain `cargo test` invocation with
  preserved build flags and a separate `--no-run` phase. Unsupported shell
  runners or unavailable toolchains have unknown viability. Unviable mutants
  and timeouts are not kills; incomplete runs do not become a no-survivor claim.
- Prepared Slim runs bind exact prompts, source/read sets, model and egress
  summary before consent. Source drift aborts. LSP initialization returns the
  standard capabilities envelope and edits bind an exact document version.
  MCP request failures do not terminate subsequent healthy requests.

### Revision comparison and proposals

```sh
deslop revision-cleanup --from BASE_DIR --to TARGET_DIR --scope src
deslop revision-cleanup --from BASE_DIR --to TARGET_DIR --scope src \
  --task 'Preserve public API, checks, and error behavior'
```

Both directories must be materialized under a matched configuration, grammar,
scope and build context. The lower-level strict snapshot API also records VCS
materialization identity; a revision label is not itself comparable evidence.
Results distinguish introduced/inherited/removed/moved/uncertain and carry
explicit incomparability reasons. Exact duplicate matches remain ambiguous.
Proposal mode uses existing shared work orders, source guards and verification
plans; it rechecks the target scan against the compared source manifest.
There is no matched independent result showing that these hypotheses outperform
snapshot-only analysis.

### Trajectory evidence

```sh
cargo run -p deslop-eval --bin trajectory-eval -- \
  adapt-opencode EXPORT.json BASE.json FINAL.json LICENSE.json OUTPUT.json WORKSPACE_ROOT
cargo run -p deslop-eval --bin trajectory-eval -- replay OUTPUT.json
cargo run -p deslop-eval --bin trajectory-eval -- candidates OUTPUT.json
```

The neutral `deslop.trajectory/1` schema requires revision/tree identities,
content-hashed edit events, license/integrity records and explicit missing
history. Imported observations are untrusted and never become executable
commands. Replay must reproduce the submitted final supported source inventory.
Supported snapshots are UTF-8 regular source files; symlinks/special files and
reserved `.git`, `.jj`, `.deslop`, `target` components are not source inventory.

The public OpenCode adapter is pinned to
[`dff8fbc149fb7492e4f07b713ac31ea70d9a541c`](https://github.com/anomalyco/opencode/tree/dff8fbc149fb7492e4f07b713ac31ea70d9a541c):
[`export.ts`](https://github.com/anomalyco/opencode/blob/dff8fbc149fb7492e4f07b713ac31ea70d9a541c/packages/opencode/src/cli/cmd/export.ts)
defines `{info,messages}`, and
[`edit.ts`](https://github.com/anomalyco/opencode/blob/dff8fbc149fb7492e4f07b713ac31ea70d9a541c/packages/opencode/src/tool/edit.ts)
defines public `filePath/oldString/newString/replaceAll` inputs. Only completed
public edit/write events are adapted, not private reasoning or shell execution.
Absolute file paths require the explicit original workspace root. Unsupported
patch events and contradictory replay reject rather than guessing.

Reversion proposals are bounded, deterministically ordered, replay-compatible
and strictly smaller under a local source-line positional cost. Caller-protected
paths and policy identity are retained; this is not a TRIM reproduction,
one-minimality result, or autonomous maintenance-benefit optimizer.
The cache hashes complete candidate source and complete declared check policy;
imported cache status is not authorization. The application API independently
stages the supplied patches, requires exact candidate inventory equality, then
uses the existing verifier/apply route. A default review-only proposal still
does not write; explicit owner approval is separate from replay.

### Disclosure and family cards

```sh
deslop rules --rule long-method --format json
cargo run -p deslop-eval --bin pilot-eval -- cards --dir PILOT_DIR --protocol-pin PIN
```

`deslop.findings/3` replaces /2 on current JSON scan surfaces and adds separate
per-rule research metadata. SARIF rule properties carry the same disclosure;
MCP `rules` can explain a rule. The bundle is embedded for installed use, not
loaded from the caller's checkout. Unknown third-party rules have no invented
research record. Claims, paper population, implementation fidelity, evaluation
artifacts and forbidden interpretations remain separate from ProofState.

Cards are derived through the validated P1 evaluator, not arbitrary report JSON:
four families × Rust/Python, disjoint observed strata, explicit missing samples,
unsupported capabilities, counterexamples and blocked promotion. No confidence
interval, deployable precision or independent evaluation is manufactured.
Frozen M8 report/model-card reproduction is an engineering reproducibility
check only; its `evidence_only` disposition remains unchanged.
