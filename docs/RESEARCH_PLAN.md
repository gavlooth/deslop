# Research-grounded deslop: implementation and validation plan

Date: 2026-09-07
Status: Proposed implementation and validation plan; P1 read-only engineering
facility checkpointed below; this is not an evaluation result.

## 1. Objective and boundaries

Make deslop's facilities traceable from published research to a measurable construct, an implementation, independent evaluation, and an appropriately limited product claim.

The target is useful cleanup, not AI-authorship detection and not indiscriminate code reduction. Code can be unnecessary relative to one requirement and essential relative to another. Tests, observability, compatibility, error handling, performance, and security are part of the relevant contract.

Use three separate concepts:

1. **Degradation evidence:** measurements or observations of duplication, concentrated complexity, indirection, or growth. This can be obtained from a snapshot; claims about growth require comparable revisions.
2. **Cleanup hypothesis:** a proposed transformation, its expected benefit, preconditions, counter-evidence, and unknowns. A smell is not proof that removal is desirable.
3. **Verification result:** the exact candidate state and checks exercised, their outcomes, and remaining uncertainty. Passing tests is not a general proof of behavioral equivalence. Use stronger proof language only for a transformation with a justified semantic argument and satisfied preconditions.

Research support, detector performance, and write authorization are independent dimensions. A citation never grants write authority; a high detector precision never substitutes for verification; a passing check does not establish a human maintenance benefit.

### Non-goals

- A universal slop/readability/health score or an authorship classifier.
- Reimplementing every published model, adopting paper thresholds without validation, or creating a new analyzer framework.
- Claiming universal behavioral equivalence from finite tests or mutation scores.
- Inferring abandoned agent intent from a final snapshot.
- Automatically executing commands embedded in benchmark repositories or imported trajectories.
- Broad unrelated refactoring of the workspace while adding the research contract.

## 2. Starting point: reuse, qualify, or validate

These are repository observations, not certifications of complete implementation fidelity.

| Existing facility | Evidence/location | Treatment |
| --- | --- | --- |
| Slop-catalog motivation | `SPEC.md:37-43`, Paul/Zhu/Bayley paper | Preserve citation; verify which individual rules its taxonomy actually supports. Do not generalize its Java/model sample to all languages or current agents. |
| Algorithm research discussion | `.agents/ALGORITHM_AUDIT.md`, especially research section | Promote useful bibliography and limitations into maintained public documentation; do not treat historical audit findings as current behavior. |
| Scope/name-resolution design | `docs/adr/0002-scope-and-name-resolution.md`, Néron et al. | Reuse existing evidence-bounded scope/resolution contracts. A design citation is not proof of implementation completeness. |
| M8 datasets and evaluation | `docs/M8_DATASET_REPORT.md`, `M8_PILOT_PROTOCOL.md`, `M8_MODEL_CARD.md`; `crates/deslop-eval/evaluation/m8/` | Keep the frozen artifacts unchanged. They are existing exploratory evidence, not a fresh holdout. Separate model-derived preference labels from direct human comprehension outcomes. |
| M8 outcome | Model card reports accuracy 0.5700 and ECE 0.0764; disposition `evidence_only` | Preserve the failed promotion decision. Do not retune and rebrand that same dataset as independent validation. |
| SlopCodeBench-inspired metrics | `docs/M11_METRICS_MIGRATION.md`; `crates/deslop-metrics/src/lib.rs` has structural mass and erosion | Already partially incorporated. Audit definitions and aggregation against a pinned paper/reference version rather than build a second implementation. |
| Rule catalog and labeled corpus | `crates/deslop-core/src/lib.rs::rules::RULES`; `tests/corpus/`; `crates/deslop-eval/src/lib.rs` | Reuse for deterministic regressions. File-level `Clean`/`Sloppy` labels alone cannot establish task-specific unnecessary code or maintenance benefit. |
| Historical analysis and promotion | `crates/deslop-parse/src/contract_history.rs`; `crates/deslop-eval/src/refactor_eval.rs` | Reuse snapshot windows, coverage, per-family scores, and promotion patterns. Keep contract-pathology detection distinct from cleanup minimization. |
| Work-order and safety infrastructure | `deslop-protocol`, `deslop-verify`, `deslop-recipes` | Extend the existing evidence/receipt contracts, not a parallel authority system. Address verified execution-boundary defects before automatic cleanup experiments. |

## 3. Dependency map and ownership

```text
P0: definitions and research ledger
  +--> P1: preregistration and corpus --> P3: measurement fidelity --> P4: detector validation
  +--> P2: trusted execution and evidence contracts ------------------+
                                                                    |
                       P1 + P2 + P4 --> P5: revision-aware cleanup   |
                                               |                    |
                                               +--> P6: trajectories |
                                                                    |
                    P1 + P2 + P4/P5/P6 --> P7: independent benefit study
                                                     |
                    completed applicable gates --> P8: product release
```

P1 and P2 can proceed independently after P0 defines their contracts. Read-only measurement work can proceed before P2 is complete; execution of imported projects and automatic writes cannot. P6 is optional for a snapshot/revision release, but mandatory for claims of trajectory-aware CodeSlop reduction. Do not postpone a complete, honestly scoped snapshot release to wait for trajectory support.

Ownership boundaries:

- **Research/evaluation owner:** literature, annotation rubric, datasets, frozen splits, evaluation and statistical reporting in `deslop-eval`.
- **Analysis owner:** measurements and detectors in `deslop-parse`, `deslop-analyzer`, and `deslop-metrics`.
- **Verification owner:** staged candidates, command policy, commit/recovery, and evidence receipts in `deslop-verify`.
- **Integration owner:** shared schemas and complete CLI/MCP/LSP/Slim/report migrations. This owner controls shared exported contract changes.
- **Independent reviewers:** label adjudication and assessment of semantic counterexamples; model judgments may assist but are not independent ground truth.

Before a schema-changing batch, agree on required fields and failure behavior. Use symbol references to locate every caller. Parallelize independent files; serialize shared schema edits and then run integration validation once.

## P0. Define the construct and build the research ledger

### Work

1. Read primary full texts and available artifacts. Record DOI/arXiv version, publication/peer-review status, dataset revision, license, studied population, measurements, comparison baseline, and threats to validity.
2. Separate a paper's findings from deslop's hypotheses. Correct bibliographic mismatches, including Ray et al.: *On the Naturalness of Buggy Code*, ICSE 2016.
3. Define the unit of each claim: syntax region, callable, file, patch, checkpoint sequence, or human maintenance task. Define what evidence is required at each unit.
4. Inventory every shipped rule, metric, recipe, and user-visible quality claim. Mark each as empirically motivated, formally/algorithmically grounded, or an engineering heuristic. Allow multiple categories; they are not a single ranking.
5. Record counterexamples: intentional duplication, public wrappers, compatibility layers, domain constants, explanatory comments, generated files, test fixtures, reflection, macros, and required defensive checks.
6. Distinguish three fidelity statuses: paper method reproduced; modified method with disclosed differences; inspiration only. Do not attribute the paper's measured performance to deslop.
7. Update README/SPEC terminology and metric documentation so no surface says tests prove arbitrary equivalence or a smell establishes removability.

### Deliverables and locations

- `docs/RESEARCH.md`: maintained bibliography, operational definition, construct-to-facility table, and limits.
- `docs/RESEARCH_PROTOCOL.md`: study questions and decision rules, completed in P1 before results are inspected.
- `docs/RESEARCH_LIMITATIONS.md`: generalization, unavailable evidence, and known counterexamples, updated at releases.
- A versioned claim registry under `crates/deslop-eval/evaluation/research/`, consumed by catalog/evaluation checks. It is the canonical machine-readable source; derive catalog tables from it rather than maintaining a duplicate rule list.

Minimum registry fields: stable claim ID; research references; exact supported claim; studied population; construct and unit; rule/metric/recipe IDs; implementation symbols; estimator/threshold origin; fidelity status; counterexamples; evaluation artifact IDs; permitted and prohibited interpretations.

### Acceptance

- Every catalog rule and exported metric is accounted for, including explicit unsupported/heuristic entries.
- Each factual scientific claim resolves to a specific source and relevant section/table, not only an abstract.
- No source is presented as validating languages, models, or transformations it did not study.
- Current M8/M11 evidence-only restrictions remain in force.

> **P0 execution checkpoint (2026-09-08): engineering DONE; both requested
> primary-fulltext access blockers RESOLVED.**
> Registry `crates/deslop-eval/evaluation/research/registry.json`
> (`deslop.research-registry/1` v1.0.0) passes
> `cargo run -p deslop-eval --bin research-registry -- check crates/deslop-eval/src`:
> 65 rules, 16 recipes, 281 metric fields, 31 claims.
> Sjøberg and Buse–Weimer author manuscripts are reviewed and checksummed;
> transport, licensing and remaining section/record scope are explicit in
> `docs/RESEARCH.md` §5. No dataset/model import or detector promotion follows.
> Zero fully `reproduced` methods; M8/M11 evidence-only restrictions hold.
> This does NOT mark the independent P1/P4–P7 empirical studies complete.

## P1. Preregister the evaluation and build licensed, independently labeled data

### Study questions

- **RQ1:** Do detectors locate a real structural issue in context, rather than merely unusual code?
- **RQ2:** Do cleanup candidates preserve the declared contract under the selected verification and independent checks?
- **RQ3:** Do changes improve comprehension or maintenance outcomes beyond simple baselines?
- **RQ4:** Does revision/trajectory information improve candidate quality or validation efficiency beyond snapshot analysis?

### Work

1. Extend the existing evaluation model with case-level evidence: repository family/revision, task requirements, source ranges, observation label, cleanup judgment, preconditions, counterexamples, verification results, and annotation provenance. Keep region labels separate from original corpus file labels.
2. Version the evaluation schema when its meaning changes. Retain legacy fixtures as regression inputs; do not silently reinterpret historical `Clean`/`Sloppy` labels as human judgments about removal.
3. Begin deep validation with four families: duplication, concentrated complexity/long methods, unnecessary wrappers/indirection, and branch simplification. Inventory all other families in P0 and retain honest heuristic status until separately evaluated.
4. Use Rust and Python for the first deep-validation slice. Preserve existing language support, but publish no cross-language validation claim based on this slice. Add Clojure, Julia, JavaScript, and TypeScript as separate strata when their corpora and verification environments are ready.
5. Proposed pilot target: 240 naturally sampled units across at least 12 unrelated repository families, plus a separate 240-case challenge set distributed across the four families and two initial languages. These are planning targets, not a power calculation or a claim of adequate evidence for release.
6. Sample natural units independently of deslop findings. Have reviewers enumerate relevant issues within sampled units so missed findings can be counted. Record selection probabilities and workload/size strata. A balanced challenge set is for boundary sensitivity, not deployment precision or prevalence.
7. Include genuine positives, intentional counterexamples, and ambiguous cases. Do not make all negative cases trivial or all positives synthetic. Maintain separate labels for observable pattern, desirability of cleanup, and verification outcome.
8. Use two independent annotators, blinded to model identity and tool verdict, with third-party adjudication. Retain original disagreement and uncertain labels; do not force ambiguous examples into clean/sloppy. Report agreement by construct.
9. Split by repository family before calibration; group forks, near-clones, tasks, and all checkpoints of one trajectory. Human-study participants must not see both versions of the same task. Keep confirmed test repositories sealed from threshold tuning.
10. Freeze extraction, inclusion/exclusion, baselines, primary endpoints, handling of failures/timeouts, and analysis code before the confirmatory run. Use pilot variance, repository clustering, and a declared minimum useful effect to set confirmatory sample size.
11. Record licenses and retrieval checksums. Missing redistribution permission blocks import, not merely publication. Redact secrets before storing trajectories. Obtain participant consent and applicable ethics approval before a human study.

### Baselines and analysis

- Detection: existing linter findings, size/complexity-only triage, and current deslop.
- Cleanup: no cleanup, existing deterministic recipes, and a task-appropriate bounded minimization baseline for historical/trajectory experiments.
- Keep natural-sample performance, challenge-set results, and imported preference benchmarks separate.
- Report precision, recall, reviewer load, abstention and supported-scope coverage, language/role breakdowns, effect sizes, and uncertainty.
- Use repository/task-clustered intervals where observations are dependent. Correctness/latency comparisons are paired where possible. Declare secondary/exploratory endpoints and multiple-comparison treatment.
- Count rejected, timed-out, unparsable, and unbuildable candidates; never report only successful rewrites.

### Acceptance

A deterministic data import and evaluation command reproduces the same case IDs, split memberships, and metrics. License and leakage checks pass. Labels are not detector outputs recycled as ground truth. The protocol and confirmatory gates are frozen before test results are inspected. If independent annotators or sufficient data are unavailable, read-only engineering evaluation can finish, but independent validation and human-benefit claims remain blocked.

> **P1 engineering checkpoint (2026-09-08): read-only facility implemented; external study blocked.**
> `deslop-eval` now has strict versioned pilot import/evaluation schemas,
> typed independent source/annotation license gates, full analyzer-config and
> source integrity pins, deterministic full-length case IDs, derived
> annotation eligibility, leakage/duplicate-unit validation, sealed-row
> exclusion before analysis, and within-file prediction/reporting with
> disjoint scoring and coverage counters. Public entry points are
> `import_bundle` and `evaluate_dir`; the in-memory scorer remains private.
> The CLI is flag-checked and performs no imported command, network, or model
> execution. The implementation is covered by fictional engineering fixtures
> and the recorded smoke evidence `/tmp/deslop-p1-smoke.log`; this is not
> empirical data or independent validation.
>
> The P1 protocol is still draft and not frozen. No licensed 12-family
> natural/challenge corpus, independent human roster, participant consent,
> ethics approval, or population inference is available. P1 engineering can
> finish after the final test, CLI, and registry gates; independent validation,
> human-benefit claims, and automatic cleanup authorization remain blocked.

## P2. Establish trustworthy execution and shared evidence contracts

### Work

1. Verify the complete composed candidate snapshot, not individual patches in isolation. Execute selected checks against exactly the state intended for commit, then validate the full declared read set and target revisions again at the commit boundary.
2. Consolidate applicable write paths around the existing transaction/atomic machinery. Make backup and recovery behavior explicit, protect filesystem boundaries, preserve required metadata, and report committed paths if any post-commit auxiliary operation fails.
3. Keep commands server/user-policy-owned. Imported repositories, work orders, and trajectories supply data, not command authority. Enforce the existing sandbox and resource-policy contracts; unavailable enforcement yields a structured failure.
4. Bind egress summary, consent, work orders, and prompts to one prepared run. Input drift aborts rather than silently triggering a second scan under the old consent.
5. Fix request-local MCP error handling and LSP initialization/version binding so one malformed request or stale editor action cannot invalidate the user-visible safety contract.
6. Distinguish scientific support metadata from `ProofState`. A paper association must never be represented as proof of a specific source fact. Reuse `WorkOrderEvidence`, provenance, and verification receipts for their existing purposes; add a narrowly typed research-reference field only if consumers need it.
7. Preserve content-addressed feature-vector identity where equal payloads intentionally deduplicate. If consumers require source occurrence identity, reference existing revision-bound source/node identities separately. Do not assume equal feature hashes alone are a defect.
8. Build regression cases for composed-check failures, stale inputs, filesystem/backup boundaries, failed commits, stale editor edits, and request-local protocol errors. These tests protect externally observable behavior, not source wiring.

### Locations

`crates/deslop-verify/src/{lib,transaction,atomic,runtime,authority,evidence}.rs`; `deslop-fix`; `deslop-protocol`; `deslop-slim`; `deslop-cli`; `deslop-mcp`; `deslop-lsp`.

### Acceptance

A failed pre-commit verification makes no live source changes. Committed state is the verified state. Recovery and receipts agree with live files. No citation, heuristic score, imported command, or client-provided verdict widens authority. Exercise actual CLI/MCP/LSP/apply flows; library tests alone are insufficient. This phase gates execution of imported benchmark projects and automatic cleanup trials, but not static data curation.

## P3. Audit measurement fidelity against published definitions

### Work

1. For each research-linked metric, write a short implementation specification: unit, formula, normalization, aggregation, estimator, missing-data behavior, and supported languages.
2. Check exact CFG cyclomatic complexity separately from syntax fallback. Verify exclusive region accounting and nested-container aggregation.
3. Compare existing structural mass and erosion with a pinned SlopCodeBench reference. Use published examples or independently computed small cases first, then licensed benchmark inputs. Document every deviation and version-dependent definition.
4. Audit entropy/surprisal axes independently. Repository-local bigram surprisal is not an LLM probability, and entropy is not a readability label. Declare training scope and leave-region-out behavior.
5. Check whether a named estimate actually matches the paper's implementation, including zero samples, tiny populations, unsupported grammar, Unicode, and file/path identity.
6. Reproduce the frozen M8 evaluation as an engineering check, explicitly distinguishing reproduction from new scientific validation. Keep preference and comprehension datasets separate.

### Deliverables and acceptance

Update `deslop-metrics`, relevant `deslop-parse` projections, M8/M11 documentation, and the research ledger. Reference-case calculations agree within a declared numerical tolerance; estimator differences are explicit. Partial analysis never becomes a confident zero or a default healthy score. No aggregate score or stronger claim is introduced just because a metric matches a paper.

## P4. Calibrate detectors and validate transformation hypotheses

### Work

1. Run the P1 pilot without changing detector thresholds. Diagnose false positives/negatives by family, language, role, size, and unsupported semantic context.
2. Fix measurement and semantic-source defects before tuning thresholds. Calibrate only on development/calibration splits. Maintain interpretable thresholds and abstention conditions.
3. Use concrete counterexamples to harden recipes: generated-name capture, type-sensitive equality-to-pattern rewrites, side effects, exception timing, evaluation order, reflection, public API compatibility, and intentional duplication.
4. Compile/typecheck and execute behavior checks for applicable candidate fixtures. Ensure mutation operators do not count uncompilable mutants as killed behavioral mutants; report invalid/timeout/survived/killed separately.
5. Produce per-family cards with detection performance, intended scope, reviewer burden, preconditions, counterexamples, and remaining uncertainty. A detector may pass while its automatic rewrite remains disabled.
6. Reuse the per-family promotion pattern in `deslop-eval/src/refactor_eval.rs`. Add statistical acceptance criteria to the frozen protocol rather than hide threshold selection in implementation.

### Promotion policy

- **Measurement available:** P3 fidelity and provenance checks pass; this does not require a claim of maintenance benefit.
- **Validated review suggestion:** confirmatory performance meets the preregistered precision/reviewer-load criterion for the named population, with abstention and coverage reported. Failure in one language cannot be hidden by pooling.
- **Automatic application:** semantic preconditions and the P2 verification policy authorize each exact candidate independently. No observed unsafe application is acceptable in the counterexample/confirmatory corpus; publish uncertainty rather than interpreting zero observed failures as zero risk.
- **Maintenance-benefit claim:** additionally requires P7 evidence.

Numerical detection and benefit thresholds are product decisions, not constants supplied by these papers. Freeze them using the P1 pilot and declared costs before confirmatory results; do not lower them to turn a failed study into a passing release.

### Acceptance

The four initial families have reproducible cards, independent held-out results, and explicit language-specific disposition. A failed family remains honestly heuristic/review-only or is disabled; it is not silently advertised as validated. Existing broader catalog coverage is retained with its declared evidence status.

## P5. Add revision-aware cleanup without pretending to know agent intent

### Work

1. Extend the existing snapshot/history pipeline to compare base and target under the same scope, grammar/config, and build context. Handle empty diffs, deletion, renames, Unicode paths, and directory versus VCS snapshots consistently.
2. Attribute changes as introduced, inherited, removed, moved, or uncertain. A moved block is not automatically new duplication. When configuration or scope changes make metrics incomparable, report that instead of a degradation delta.
3. Add bounded findings for introduced redundancy, unnecessary indirection hypotheses, and concentration/verbosity growth. Reuse existing detector and metrics implementations.
4. Generate cleanup proposals against the target revision, carrying task requirements, affected-region identities, declared read set, preconditions, and verification plan.
5. Evaluate snapshot-only versus revision-aware analysis on the same tasks and candidate budgets. Measure extra true findings, false positives, review load, and verification cost.
6. If minimization is used, retain multiple incomparable successful candidates rather than equate minimum line count with best design. Exclude protected tests, checks, error handling, and public contracts from an automatic size objective.

### Locations and acceptance

Reuse `deslop-parse` snapshot/history types, `deslop-analyzer` history machinery, `deslop-metrics`, `deslop-protocol`, and `deslop-eval/src/refactor_eval.rs`. Extend the existing CLI surface only after agreeing on the shared protocol contract; MCP must consume the same implementation.

An unchanged tree produces no introduced findings. Scope/renames are consistent. Findings name evidence of introduction but never claim abandoned agent intent. Candidate application passes P2. A controlled comparison reports whether historical context provides measurable benefit, including a negative result if it does not.

## P6. Add optional trajectory-aware CodeSlop facilities

### Work

1. Read the pinned TRIM full method and artifacts, including its task oracle, edit dependency treatment, minimization objective, cost accounting, and reported regressions. Label an adapted implementation honestly; do not call a generic reducer a TRIM reproduction.
2. Define one vendor-neutral trajectory import containing base/final revision identities, ordered edit events, snapshot/diff references, declared test observations, and explicit missing history. Use public export formats; private reasoning traces are neither required nor an acceptable dependency.
3. Add one real agent-export adapter and a validated neutral format first. Keep ingestion separate from analysis and avoid a new crate until ownership actually warrants one.
4. Replay content-addressed edits in isolation and require the replayed final tree to match the submitted final snapshot. Bind imported check observations to snapshots, but treat them as untrusted observations until selected checks are rerun by trusted policy.
5. Build event dependency groups and generate candidate reversions for superseded/speculative-change hypotheses. Incomplete or contradictory histories yield partial evidence, not invented causation.
6. Use bounded validation, deterministic candidate ordering, explicit budgets, and caching keyed by the complete candidate state and check policy. Do not execute command strings from the trajectory.
7. Compare no cleanup, snapshot-only, revision-aware, a bounded delta-debugging baseline, and trajectory-aware reduction on identical tasks/oracles/budgets. Report removed changes and validation cost alongside correctness, independently held-out checks, and retained task functionality.
8. Test whether smaller patches remain understandable and maintainable; trajectory minimization is a hypothesis about useful cleanup, not a replacement for P7.

### Acceptance

A real supported export imports, replays, produces an evidence-linked candidate, and passes the same verifier/apply flow end to end. Malformed or incomplete histories fail explicitly. No model/agent is required for ordinary snapshot operation. A trajectory benefit claim is made only after the paired comparison passes the frozen protocol; otherwise ship the bounded facility as experimental/review-only.

## P7. Measure human usefulness and independent behavioral outcomes

### Work

1. Run a pilot followed by a preregistered confirmatory study with developers performing realistic comprehension and modification tasks on original versus cleaned code.
2. Use randomized, counterbalanced allocation across equivalent tasks. Blind tool/model identity. Do not expose both versions of the same task to a participant or treat multiple trials from one participant as independent developers.
3. Primary outcomes: task correctness, successful maintenance completion, and completion time. Secondary outcomes: review effort, defects introduced during modification, perceived readability, and patch size. Keep preference separate from correctness.
4. Include no-cleanup and standard-linter/refactoring baselines. Analyze participant and repository/task dependence. Set the minimum useful effect and confirmatory sample size from pilot variability, not a convenient snippet count.
5. Maintain independently authored behavior/contract checks that were not used to select candidates. Where appropriate, use property/differential tests to look beyond existing examples. These increase evidence; they do not prove arbitrary equivalence.
6. Add a held-out iterative-extension experiment: continue development from original and cleaned checkpoints under matched tasks, models/configuration, budgets, and seeds where supported. Report downstream correctness and degradation across checkpoints; do not generalize beyond the evaluated setting.
7. Preserve failed tasks and negative results. Do not optimize multiple readability metrics and report only whichever improved.

### Acceptance

Publish protocol, anonymized/licensed data, analysis, uncertainty, attrition, and limitations. A maintenance-benefit claim requires the preregistered benefit criterion and non-degradation criterion to pass; fewer lines or fewer smells alone cannot satisfy it. If participants or suitable tasks are unavailable, structural facilities may ship within their narrower evidence contract, but human-benefit claims remain unmade.

## P8. Integrate, document, and release with bounded claims

### Work

1. Surface research references, detector validation scope, counter-evidence, and verification limits through existing rules/report/explain facilities. Keep the default CLI readable; full evidence belongs in structured output and detailed explanation.
2. Migrate every affected CLI/MCP/LSP/Slim/report/eval consumer in one deliberate schema cutover. Strict wire parsers reject unsupported versions; do not add permissive legacy defaults to make missing authority look present.
3. Add fast CI gates for registry/catalog consistency, schema contracts, deterministic reference calculations, and meaningful regression cases. Run licensed external benchmarks and human studies separately; routine builds must not fetch datasets, call a model, or require participant services.
4. Test the actual installed artifact with the required MCP feature and exercise representative report, explanation, protocol, and apply/recovery behavior. Apply existing release/security gates to distribution surfaces used by the experiment.
5. Publish family cards and a release evidence table: implemented, definition-checked, independently evaluated, supported population, and permitted application mode. Update README/SPEC, M8/M11 migration notes where affected, adapter documentation, and release notes.
6. Archive manifests/checksums, toolchains, grammar versions, commands/policies, resource limits, and seeds. Record external-service/model version limitations where exact reproducibility cannot be guaranteed.

### Acceptance

No surface overstates research support or verification strength. A new rule cannot claim validation without a matching frozen evaluation artifact. Drift in grammar, detector, recipe, or policy invalidates the affected performance claim until re-evaluated; it does not invalidate unrelated evidence indiscriminately. A counterexample can demote a family or recipe using existing negative-memory mechanisms without waiting for a full release cycle.

## 4. Milestones and review gates

| Milestone | Required result | Explicitly not claimed |
| --- | --- | --- |
| M-A: Scientific contract | P0 ledger covers all shipped facilities; P1 protocol, labels, provenance and pilot import are executable | Validated detector quality or human benefit |
| M-B: Trustworthy baseline | P2 end-to-end execution safety and P3 measurement fidelity verified | Arbitrary equivalence or universal metric validity |
| M-C: Validated snapshot facilities | P4 independent cards and supported-scope decisions for the initial families; applicable P8 integration complete | Generalization to untested languages or trajectories |
| M-D: Historical/trajectory facilities | P5 and, for trajectory claims, P6 pass their own task-level comparisons and integration gates | Proof of intent from a snapshot or globally minimal necessary code |
| M-E: Demonstrated usefulness | P7 confirmatory outcomes and applicable P8 release evidence | Universal maintenance benefit across all repositories |

Each milestone is independently reviewable. Failed evidence gates change the claim or keep the feature unpromoted; they do not justify silently weakening safety. Extending validation to the rest of the catalog is a sequence of P1/P3/P4 family studies under the same ledger, not an assumption inherited from the first four families.

## 5. Risks and stop conditions

| Risk | Required response |
| --- | --- |
| Dataset licenses or participant consent absent | Stop that import/study; finish reachable static/protocol work without claiming missing validation. |
| Labels reflect taste rather than a reliable construct | Retain disagreement; revise the rubric on pilot data, then freeze a new protocol before a new confirmatory run. |
| Test-driven minimization deletes necessary untested behavior | Reject/demote the transformation; strengthen independent checks and protect task contracts. Never reward shrinking the tests. |
| Improvement is only a size proxy | Compare with size/linter baselines and report human/task outcomes separately. |
| Cross-language evidence transfer unsupported | Publish language-specific scope and abstention; do not pool away a failing stratum. |
| Changes exploit metric definitions | Evaluate all declared outcomes, semantic counterexamples, and held-out tasks. |
| Reference paper/artifact changes | Pin versions, disclose divergence, and rerun fidelity comparisons before claiming replication. |
| Concurrent source/config/grammar drift | Abort prepared execution/egress; regenerate evidence on the new snapshot. |
| Research work creates duplicate infrastructure | Reuse current catalog, snapshots, history, evidence, evaluator and verifier contracts; one integration owner enforces cutovers. |

## 6. First implementation batch

Start here; do not begin with a universal score or a trajectory optimizer.

1. Complete P0 primary-source review and the all-facility inventory.
2. Write the research ledger and resolve terminology in README/SPEC/M8/M11 documentation.
3. Freeze the pilot rubric, dataset/provenance schema, and split policy; implement the importer and evaluation extensions in `deslop-eval`.
4. In parallel with licensed data curation, complete the P2 safety/evidence prerequisites using the audited failure cases as regression inputs.
5. Produce the P3 reference calculations and untuned P4 pilot baseline.
6. Review the results and freeze confirmatory sample sizes and promotion policy before running the sealed evaluation.

The review package for this batch is the ledger, runnable pilot evaluation, baseline report, counterexample corpus, and actual execution-boundary evidence. It must clearly distinguish code implemented, experiments run, external prerequisites missing, and claims not yet supported.

## 7. Initial primary-source reading list

This is the required review queue, not a statement that every method below is already implemented or independently replicated.

- Paul, Zhu, Bayley, *Investigating The Smells of LLM Generated Code*: <https://arxiv.org/abs/2510.03029>.
- Liu et al., *Refining ChatGPT-Generated Code: Characterizing and Mitigating Code Quality Issues*: <https://arxiv.org/abs/2307.12596>.
- Velasco et al., *How Propense Are Large Language Models at Producing Code Smells? A Benchmarking Study*: <https://arxiv.org/abs/2412.18989>.
- Zhang et al., *Copilot-in-the-Loop: Fixing Code Smells in Copilot-Generated Python Code using Copilot*: <https://arxiv.org/abs/2401.14176>.
- Orlanski et al., *SlopCodeBench: Benchmarking How Coding Agents Degrade Over Long-Horizon Iterative Tasks*: <https://arxiv.org/abs/2603.24755>.
- Mathai et al., *TRIM: Reducing AI-Generated CodeSlop via Agent Trajectory Minimization*: <https://arxiv.org/abs/2607.18161>.
- Sjøberg et al., *Quantifying the Effect of Code Smells on Maintenance Effort*: <https://doi.org/10.1109/TSE.2012.89>.
- Néron et al., *A Theory of Name Resolution*: <https://doi.org/10.1007/978-3-662-46669-8_9>.
- McCabe, *A Complexity Measure*: <https://doi.org/10.1109/TSE.1976.233837>.
- Buse and Weimer, readability study: <https://doi.org/10.1109/TSE.2009.70>.
- Posnett, Hindle, Devanbu, readability model: <https://doi.org/10.1145/1985441.1985454>.
- Hindle et al., *On the Naturalness of Software*: <https://doi.org/10.1109/ICSE.2012.6227135>.
- Ray et al., *On the Naturalness of Buggy Code*, ICSE 2016: <https://doi.org/10.1145/2884781.2884848>.
- Scalabrino et al., readability features: <https://doi.org/10.1002/smr.1958>.
- Torres et al., entropy/evolution work: <https://doi.org/10.1007/s10664-025-10644-y>.
- Bergum et al. comprehension data: <https://zenodo.org/records/14229849>.
- Themis-CodePreference: use the exact revision, license and checksum in `crates/deslop-eval/evaluation/m8/dataset_registry.json`.
