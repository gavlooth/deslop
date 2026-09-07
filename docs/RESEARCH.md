# Research ledger (P0)

Status: P0 in progress. Maintained bibliography, operational definition, and
construct pointer required by `docs/RESEARCH_PLAN.md` P0. Records what primary
sources were actually read — not what the plan cites. No finding below is a
validation claim for deslop's detectors.

Canonical claim registry: `crates/deslop-eval/evaluation/research/registry.json`
(`deslop.research-registry/1`). That registry is the machine-readable source
for rule/metric/recipe claims, including every per-facility fidelity status.
This ledger makes NO per-facility fidelity assertion; §4 is a construct-level
pointer only. Generate the registry inventory with the offline binary:

```sh
cargo run -p deslop-eval --bin research-registry -- check   # or: ... -- claims
```

## 1. Bibliography (plan §7 queue, 17 items)

Legend: `[R: §§]` = text accessed; the listed sections are the ONLY sections
reviewed, all unlisted sections are unread. `[A]` = abstract/record/metadata
only; NO empirical numbers are reproduced from these (withheld per P0
acceptance — a factual scientific claim must resolve to a read section/table).
`[U]` = full text not accessible after the alternate attempts in §5. A paper
license (e.g. arXiv CC-BY) is the paper's license, never a dataset license.

| Source ID | Citation | Status | License note |
| --- | --- | --- | --- |
| `paul-2025-smells` | Paul, Zhu, Bayley, *Investigating The Smells of LLM Generated Code*, arXiv 2510.03029v1 (extended version of ICCBDCS 2025 conference paper; submitted to Information and Software Technology) | [R: full HTML §§1–8] | Paper CC-BY 4.0; dataset license not stated — no import claim |
| `liu-2023-refining` | Liu et al., *Refining ChatGPT-Generated Code*, arXiv 2307.12596v1 | [R: §§1–6] (RQ1–RQ3; threats/related-work skimmed) | Paper arXiv perpetual non-exclusive license; figshare replication copy exists, license unchecked — no import claim |
| `velasco-2024-smells` | Velasco et al., *How Propense Are LLMs at Producing Code Smells?*, arXiv 2412.18989v1 | [R: §§I–V] (methods §II, case study §III, scope discussion §III-A/§V; references skimmed) | Paper CC-BY-SA 4.0; dataset/code at github.com/WM-SEMERU/CodeSmells, license unchecked — no import claim |
| `zhang-2024-copilot` | Zhang et al., *Copilot Refinement* (v1 title; plan cites *Copilot-in-the-Loop*), arXiv 2401.14176v1 | [R: §§1–6] | Paper CC-BY 4.0; dataset license not stated — no import claim |
| `orlanski-2026-slopcodebench` | Orlanski et al., *SlopCodeBench*, arXiv 2603.24755v1 | [R: §1, §2 intro–§2.3, §3–§4 partial] (later result sections unread) | Paper CC-BY 4.0; dataset license not stated — no import claim |
| `mathai-2026-trim` | Mathai et al., *TRIM*, arXiv 2607.18161v1 | [R: §§I–IVB] (evaluation sections unread) | Paper CC-BY 4.0; dataset license not stated — no import claim |
| `hindle-2012-naturalness` | Hindle et al., *On the Naturalness of Software*, ICSE Jun 2012, doi:10.1109/ICSE.2012.6227135 | [R: §§I–IV] author PDF (softwareprocess.es/pubs/hindle2012ICSE.pdf) | Author manuscript; no dataset import |
| `posnett-2011-readability` | Posnett, Hindle & Devanbu, *A Simpler Model of Software Readability*, MSR May 2011, doi:10.1145/1985441.1985454 | [R: §§1–4.3] author PDF (softwareprocess.es/pubs/posnett2011MSR-readability.pdf); rest unread | Author manuscript; Buse dataset reused from public trove, not re-imported |
| `ray-2016-buggy-code` | Ray et al., *On the "Naturalness" of Buggy Code*, ICSE May 2016, doi:10.1145/2884781.2884848 (preprint arXiv:1506.01159) | [R: abstract + §§1–4 partial] preprint HTML (method §§2–3, RQ1–RQ3 results §4; RQ4–RQ5 comparison unread) | Preprint arXiv perpetual non-exclusive license; no dataset import |
| `scalabrino-2018-readability` | Scalabrino et al., *A Comprehensive Model for Code Readability*, JSEP Jun 2018, doi:10.1002/smr.1958 | [R: §§1–4.3] author PDF (sscalabrino.github.io); results §§4.4+ unread | Author manuscript; no dataset import |
| `mccabe-1976-complexity` | McCabe, *A Complexity Measure*, IEEE TSE Dec 1976, doi:10.1109/TSE.1976.233837 | [R: abstract + §§I–IV, VI–VII] scanned PDF (Def. 1 v(G)=e−n+p; predicates+1 simplification; nonstructured graphs a–d; testing methodology; rest skimmed) | Scanned copy; no dataset import |
| `neron-2015-name-resolution` | Néron et al., *A Theory of Name Resolution*, ESOP 2015 / LNCS, doi:10.1007/978-3-662-46669-8_9 (author PDF: web.cecs.pdx.edu/~apt/esop15.pdf) | [R: abstract + §§1–2.4] author PDF pp.1–13 (scope graphs, resolution calculus, LM language, imports; §§2.5+ unread) | Author manuscript (Portland State); no dataset import |
| `buse-2010-readability` | Buse & Weimer, *Learning a Metric for Code Readability*, IEEE TSE Jul 2010 (vol 36 no 4, 546–558), doi:10.1109/TSE.2009.70 | [U] Crossref + Semantic Scholar metadata only; umich author PDF TLS cert failure (http and https), virginia old path 403 | — |
| `torres-2025-entropy` | Torres et al., *Information-theoretic detection of unusual source code changes*, Empirical Software Engineering 30:153 (2025), doi:10.1007/s10664-025-10644-y — © The Author(s) 2025, open access (PDF retrieved from SpringerLink) | [R: §§1–4.4] full-text PDF (intro, review §§2–2.3, methods §3, results §§4.1–4.4; validity/discussion §§5+ skimmed) | Open access; supplementary data Zenodo 11180885 (not imported) |
| `bergum-2024-comprehension` | Bergum et al. comprehension data, Zenodo 14229849 | [A] Zenodo record/README only; paper not read | M8 registry pins revision `zenodo-14229849` + sha256 — reuse, do not re-pin |
| `themis-codepreference` | Themis-CodePreference frozen artifact, `crates/deslop-eval/evaluation/m8/dataset_registry.json` | [A] registry file read | Local frozen artifact, not a paper; license/checksum from that file |

Counts: 13/17 text accessed at section level (only listed sections reviewed);
2/17 abstract/record familiarity only (`bergum-2024-comprehension`,
`themis-codepreference`); 2/17 metadata-only (`sjoberg-2013-maintenance`,
`buse-2010-readability`). P0 source review is NOT complete; §5 tracks
per-source blockers and next attempts.
## 2. Operational definition

A **cleanup hypothesis** in deslop is a proposed transformation of a code
region plus its expected benefit, preconditions, counter-evidence, and
unknowns. Three concepts stay separate:

1. **Degradation evidence** — measurements on a snapshot (duplication,
   concentrated complexity, indirection, growth vs. a comparable revision).
2. **Cleanup hypothesis** — a candidate transformation; a smell is not proof
   removal is desirable.
3. **Verification result** — the exact candidate state, checks run, outcomes,
   and residual uncertainty. Passing tests is evidence, never a general proof
   of behavioral equivalence. Stronger proof language requires a justified
   semantic argument plus satisfied preconditions (see TRIM note in §3).

Claim units and the evidence each unit requires:

- **syntax region**: exact byte span + parse provenance (`complete` with no
  diagnostics) + the finding evidence attached to that span.
- **callable**: region evidence plus scope/containment identity (which
  callable owns the region) — no bare-name attribution.
- **file**: per-file aggregated evidence with stated roll-up policy
  (exclusive vs. inclusive aggregation declared).
- **patch**: exact before/after bytes + revision guard + check-command,
  coverage, and characterization evidence bound to that exact state.
- **checkpoint sequence**: ordered base→target revision identities under one
  scope/grammar/config, with introduction attribution (introduced, inherited,
  removed, moved, uncertain) — never an intent claim.
- **human maintenance task**: preregistered task, participant/task
  independence, correctness/completion/time outcomes plus review effort —
  none exists yet for deslop (P7 blocked).

## 3. Primary findings (section-anchored, no validation claims)

- `paul-2025-smells` (§3.2.1): the "Documentation" implementation smell means
- `liu-2023-refining` (§§1, 4–6): 2,033 LeetCode tasks → 4,066 ChatGPT
  Java/Python programs; pass@1 by difficulty (Table 1), recency, and size
  (Figs. 1–2, Table 2). RQ2: static-analysis clean-code rates fall with
  difficulty (Fig. 3); open-card-sort taxonomy (§5.2: compile/runtime errors,
  wrong outputs, style/maintainability Tables 5–6, performance); RQ3:
  self-repair with tool feedback partially mitigates. Population: ChatGPT
  (GPT-3.5, Mar 2023), LeetCode Java/Python — not current agents, not other
  languages.
- `velasco-2024-smells` (§§II–III, §V): PSC from next-token logits with
  alignment/aggregation (Eqs. 1–2, λ=0.5); CodeSmellData ~142k Python
  method-level instances, 13 Pylint types (Table I, 100 sampled per type,
  ≤400 tokens); CodeLlama-7B + Mistral-7B both above threshold on 10/13
  types (Table II, Fig. 1). Limits: training-data hypothesis unconfirmed
  ("a separate study is necessary", §III-A); method-level Pylint only;
  future work names sampling-bias, higher-level smells, multi-tool alignment
  (§V). PSC is a propensity probe, not detector validation.
  Copilot Chat v0.12.2 (GPT-4) — not a general repair claim.
- `orlanski-2026-slopcodebench` (§1–§2.3, §3–§4 partial, v1 HTML): 20
  problems / 93 checkpoints; the agent extends its own prior code under
  evolving specs with no prescribed internal interfaces and hidden tests
  (§2.1). Two trajectory quality signals: verbosity = flagged+clone lines
  over LOC (Eq. 4) and structural erosion = share of complexity mass in
- `neron-2015-name-resolution` (abstract + §§1–2.4, author PDF): scope graphs
  (references/declarations/scopes/imports, Fig. 1) + resolution calculus
  (reachability/visibility/specificity ordering, Figs. 2–3) as a
  language-independent formalism; two-stage process (language-specific graph
  construction, language-independent resolution); sound+complete algorithm
  claimed (§5, unread); LM toy language with modules/imports (§2.4).
  Supports scope-graph design rationale (explicit scopes, ambiguous/unresolved
  as first-class outcomes); does NOT establish deslop's implementation
  completeness. Later sections (§§2.5+ on variants/coverage/construction)
  unread.
- `torres-2025-entropy` (§§1–4.4, open-access PDF): textual entropy HTOKEN
  (Eq. 3) + structural AST-edge entropy HAST_EDGE (Eq. 4); systematic review
  finds low maturity and thin empirical validation (§2.3); methods: 95 Java
  projects, commit-by-commit traversal, per-file McCabe recorded (§3);
  results: total entropy trends upward in ~98% of projects; entropy–McCabe
  correlations weak (file-level avg 0.13, commit-level 0.10, Tables 4–5);
  anomaly detection true-positive rates 58–83% depending on memory/strictness
  (Table 7, §4.4). Population: 95 Java projects only — supports
  entropy-as-distinct-dimension evidence, NOT readability labels, NOT
  cross-language transfer. Validity/discussion sections skimmed only.
  (§III-C). TRIM searches the agent's repair trajectory (edit–feedback
- `mathai-2026-trim` (§§I–VI partial): CodeSlop = *removable functional
  redundancy* (behavioral definition, Eq. 1). The minimal patch is an
  **ideal**; practical algorithms compute approximation MP via the agent's
  test suite TF in Env (§III-C, §V-B). TRIM searches the repair trajectory
  (edit–feedback sequences, §IV-B) hierarchically (§IV-C). Evaluation design:
  Live-kBench (534 kernel vulns; 433 repairs, 293 non-trivial) +
  SWE-Bench-Verified (500 issues; 333 SWE-Agent trajectories), 4 scaffolds,
  4,544 trajectories (§V-A); metrics ΔSlop (Eq. 2), TF executions, preserved
  tests (§V-B). Validity: minimization uses ONLY the repair agent's TF —
  held-out oracle behavior is the open risk; agentic self-minimization fails
  (invalid/larger) in 3.8–44.9% of cases (§VI). No reduction percentages or
  cost ratios reproduced here (evaluation tables unread). Standing
  refutation of proof language: test-passing minimization never proves
  arbitrary equivalence.
- `posnett-2011-readability` (§§2–4.3, author PDF): Buse's 100-snippet /
  12,000-judgment data re-modeled; 2-predictor model (lines + Halstead V)
  outperforms Buse's 25-feature model (Fig. 1); size alone insufficient
  (RQ2); byte entropy adds little (RQ4); token entropy ≈ Buse but <
  Halstead (RQ5). Population: 100 small Java snippets (4–11 lines), 120
  student raters — not functions/classes, not other languages; §4.4 warns
  all-large-function classifications lack credibility. Supports size +
  Halstead-volume axes as evidence, never shipped readability labels.
- `ray-2016-buggy-code` (§§1–4 partial, preprint): cache-LM entropy Eq. 5
  (§3.3); RQ1–RQ2 results read — buggy lines higher-entropy than non-buggy
  and entropy drops after fixes with statistical significance (Wilcox;
  effect shrinks as bug size grows, table max-delete 2→30); copy-paste
  counterexamples where entropy rises after fix (examples 4–5); RQ3: entropy
  ordering beats random inspection (AUCEC, partial/full credit). RQ4–RQ5
  (static-finder comparison) unread. Population: ~8,296 bug-fix commits, 10
  Java projects. Supports entropy-as-triage-ordering evidence only.
- `sjoberg-2013-maintenance`, `buse-2010-readability`: metadata only; no
- `mccabe-1976-complexity` (§§I–IV, VI–VII, scanned copy): v(G) = e−n+p
  (Def. 1); structured-program simplification v = predicates+1 (compound
  conditions count per condition; CASE uses N−1); regions-count method via
  Euler; nonstructured characterization (branch in/out of loop/decision,
  graphs a–d, Results 1–2); testing methodology: v is the minimal independent
  test-path count, explicitly "will by no means guarantee or prove the
  software" (§VII). No assertion about deslop's implementation; that
  comparison is P3 fidelity work, not done.
- `bergum-2024-comprehension` (Zenodo record): EEG/eye-tracking Java
  atoms-of-confusion trials (24 participants per M8 registry).
  Comprehension-correctness/time evidence, not preference evidence; keep the
  targets separate.
- `themis-codepreference`: frozen local artifact; see M8 dataset report for
  revision/license/checksum. Existing exploratory evidence, not a fresh
  holdout (plan §2 table).

M8 frozen numbers (unchanged): challenger accuracy 0.5700 (95% 0.5134–0.6248),
ECE 0.0764; disposition `evidence_only`. See `docs/M8_MODEL_CARD.md`.

## 4. Construct-to-facility table (GENERATED — do not hand-edit)

Regenerate with:
`cargo run -p deslop-eval --bin research-registry -- inventory`
Validate with:
`cargo run -p deslop-eval --bin research-registry -- check`
Per-entry detail:
`cargo run -p deslop-eval --bin research-registry -- claims`
Per-facility fidelity lives ONLY in the registry (`fidelity` field per
`claim_id`); this section asserts none.

| claim | kind | fidelity | facilities |
| --- | --- | --- | --- |
| fac:quality-claim:m8-evidence-only | quality-claim | none | claim:m8-evidence-only |
| fac:quality-claim:metrics-evidence-only | quality-claim | none | claim:metrics-evidence-only |
| fac:quality-claim:no-authorship-detection | quality-claim | none | claim:no-authorship-detection |
| fac:quality-claim:propose-verify-apply | quality-claim | none | claim:propose-verify-apply |
| fac:quality-claim:spec-catalog-prioritization | quality-claim | inspiration-only | claim:spec-catalog-prioritization |
| fac:recipe:rust-enabled-catalog | recipe-group | none | recipe:rust-convert-exhaustive-chain-to-match, recipe:rust-extract-sese-branch-method, recipe:rust-factor-equivalent-branch-fragments, recipe:rust-inline-exact-single-use-helper, recipe:rust-inline-exact-single-use-temporary, recipe:rust-inline-single-use-conversion-allocation, recipe:rust-invert-guard-clause, recipe:rust-merge-adjacent-conditions, recipe:rust-remove-independent-unused-literal-local, recipe:rust-remove-literal-dead-arm, recipe:rust-remove-unreachable-literal-statement, recipe:rust-remove-unused-pure-literal-expression, recipe:rust-sort-hoisted-private-function-block, recipe:rust-sort-simple-import-block, recipe:rust-split-dependence-cohesive-callable, recipe:rust-split-independent-branch-actions |
| fac:rule:analyzer-confirmed | rule-group | none | rule:needless-return, rule:unused-arg, rule:unused-binding, rule:unused-namespace, rule:unused-private-def |
| fac:rule:comment-narration | rule-group | none | rule:comment-block, rule:narrating-comment |
| fac:rule:duplication | rule-group | inspiration-only | rule:duplicate-block, rule:near-duplicate |
| fac:rule:incompleteness | rule-group | none | rule:incompleteness |
| fac:rule:language-idiom | rule-group | none | rule:js-loose-equality, rule:js-unnecessary-await, rule:js-var-declaration, rule:let-and-return, rule:py-dict-keys-membership, rule:py-list-comprehension-wrapper, rule:py-none-comparison, rule:py-range-len, rule:redundant-closure, rule:redundant-do, rule:reimpl-boolean, rule:reimpl-eachindex, rule:reimpl-empty?, rule:reimpl-isempty, rule:reimpl-isnothing, rule:reimpl-not=, rule:reimpl-seq, rule:reimpl-some?, rule:reimpl-vec, rule:single-use-binding, rule:useless-format |
| fac:rule:long-method-complexity | rule-group | inspiration-only | rule:long-method |
| fac:rule:magic-number | rule-group | none | rule:magic-number |
| fac:rule:needless-clone | rule-group | none | rule:needless-clone |
| fac:rule:never-auto-review | rule-group | none | rule:accepted-config-inert, rule:accepted-config-no-behavioral-reach, rule:adoption-chain-incomplete, rule:confidence-derived-after-lossy-commit, rule:confidence-provenance-lost, rule:config-key-shadowed, rule:config-key-unconsumed, rule:config-key-unread, rule:contract-chain-incomplete, rule:hot-path-work-duplicated, rule:julia-jet, rule:mechanism-gate-contract-split, rule:mechanism-live-gate-retired, rule:missing-reference, rule:operational-identity-stale, rule:owner-consumer-contract-split, rule:owner-moved-consumer-stale, rule:partition-boundary-not-preserved, rule:producer-verifier-schema-drift, rule:producer-verifier-schema-mismatch, rule:published-identity-not-live, rule:same-path-expensive-work-repeated, rule:scope-collapse-after-refactor, rule:sibling-admission-gates-diverged, rule:sibling-admission-guards-asymmetric, rule:telemetry-claim-unbound, rule:telemetry-not-bound-to-claim, rule:test-contract-dimension-uncovered, rule:test-oracle-lag |
| fac:rule:safe-auto-whitespace | rule-group | none | rule:consecutive-blank-lines |
| fac:rule:slop-score-report | rule-group | none | rule:slop-score |
| metric-complexity | metric | none | metric:functions[].complexity.cognitive, metric:functions[].complexity.complexity_mass, metric:functions[].complexity.cyclomatic, metric:functions[].complexity.maintainability_index, metric:functions[].complexity.max_nesting, metric:functions[].complexity.nloc, metric:functions[].complexity.structural_mass, metric:functions[].span.start_line, metric:functions[].span.end_line, metric:functions[].span.start_byte, metric:functions[].span.end_byte |
| metric-expressivity | metric | none | metric:functions[].expressivity.ast_edge_entropy, metric:functions[].expressivity.byte_entropy_bits_per_byte, metric:functions[].expressivity.comment_to_code_ratio, metric:functions[].expressivity.decision_density, metric:functions[].expressivity.information_volume, metric:functions[].expressivity.structural_entropy, metric:functions[].expressivity.token_entropy, metric:functions[].expressivity.tokens, metric:functions[].expressivity.unique_token_ratio, metric:functions[].expressivity.vocabulary |
| metric-halstead | metric | none | metric:functions[].halstead.difficulty, metric:functions[].halstead.distinct_operands, metric:functions[].halstead.distinct_operators, metric:functions[].halstead.lexical_effort, metric:functions[].halstead.total_operands, metric:functions[].halstead.total_operators, metric:functions[].halstead.volume |
| metric-surprisal | metric | none | metric:functions[].surprisal.estimator, metric:functions[].surprisal.max_bits, metric:functions[].surprisal.mean_bits, metric:functions[].surprisal.p90_bits, metric:functions[].surprisal.peer_tokens, metric:functions[].surprisal.sample_size, metric:functions[].features.axes.surprisal.measurements.max_bits.estimator, metric:functions[].features.axes.surprisal.measurements.max_bits.sample_size, metric:functions[].features.axes.surprisal.measurements.max_bits.value, metric:functions[].features.axes.surprisal.measurements.mean_bits.estimator, metric:functions[].features.axes.surprisal.measurements.mean_bits.sample_size, metric:functions[].features.axes.surprisal.measurements.mean_bits.value, metric:functions[].features.axes.surprisal.measurements.p90_bits.estimator, metric:functions[].features.axes.surprisal.measurements.p90_bits.sample_size, metric:functions[].features.axes.surprisal.measurements.p90_bits.value, metric:functions[].features.axes.surprisal.unknowns[] |
| metric-redundancy | metric | none | metric:functions[].redundancy.anti_pattern_lines, metric:functions[].redundancy.anti_pattern_ratio, metric:functions[].redundancy.clone_lines, metric:functions[].redundancy.clone_ratio, metric:functions[].redundancy.dead_or_unused_lines, metric:functions[].redundancy.dead_or_unused_ratio, metric:functions[].redundancy.finding_count, metric:functions[].redundancy.nloc, metric:functions[].redundancy.union_lines, metric:functions[].redundancy.union_ratio, metric:functions[].features.axes.redundancy.measurements.anti_pattern_line_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.anti_pattern_line_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.anti_pattern_line_ratio.value, metric:functions[].features.axes.redundancy.measurements.clone_line_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.clone_line_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.clone_line_ratio.value, metric:functions[].features.axes.redundancy.measurements.dead_or_unused_line_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.dead_or_unused_line_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.dead_or_unused_line_ratio.value, metric:functions[].features.axes.redundancy.measurements.redundancy_union_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.redundancy_union_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.redundancy_union_ratio.value, metric:functions[].features.axes.redundancy.unknowns[] |
| metric-static-slop | metric | none | metric:functions[].static_slop.authority, metric:functions[].static_slop.complexity_mass, metric:functions[].static_slop.contextual_surprisal_p90, metric:functions[].static_slop.peer_relative.complexity_mass.mad, metric:functions[].static_slop.peer_relative.complexity_mass.median, metric:functions[].static_slop.peer_relative.complexity_mass.percentile, metric:functions[].static_slop.peer_relative.complexity_mass.positive_robust_zscore, metric:functions[].static_slop.peer_relative.complexity_mass.robust_zscore, metric:functions[].static_slop.peer_relative.contextual_surprisal_p90.mad, metric:functions[].static_slop.peer_relative.contextual_surprisal_p90.median, metric:functions[].static_slop.peer_relative.contextual_surprisal_p90.percentile, metric:functions[].static_slop.peer_relative.contextual_surprisal_p90.positive_robust_zscore, metric:functions[].static_slop.peer_relative.contextual_surprisal_p90.robust_zscore, metric:functions[].static_slop.peer_relative.peer_count, metric:functions[].static_slop.peer_relative.peer_group, metric:functions[].static_slop.peer_relative.redundancy_ratio.mad, metric:functions[].static_slop.peer_relative.redundancy_ratio.median, metric:functions[].static_slop.peer_relative.redundancy_ratio.percentile, metric:functions[].static_slop.peer_relative.redundancy_ratio.positive_robust_zscore, metric:functions[].static_slop.peer_relative.redundancy_ratio.robust_zscore, metric:functions[].static_slop.peer_relative.structural_entropy.mad, metric:functions[].static_slop.peer_relative.structural_entropy.median, metric:functions[].static_slop.peer_relative.structural_entropy.percentile, metric:functions[].static_slop.peer_relative.structural_entropy.positive_robust_zscore, metric:functions[].static_slop.peer_relative.structural_entropy.robust_zscore, metric:functions[].static_slop.redundancy_ratio, metric:functions[].static_slop.structural_entropy, metric:peer_groups[].id, metric:peer_groups[].lang, metric:peer_groups[].role, metric:peer_groups[].size_bin, metric:peer_groups[].count |
| metric-burden-hotspots | metric | none | metric:functions[].heuristic_burden.basis, metric:functions[].heuristic_burden.complexity_burden, metric:functions[].heuristic_burden.entropy_burden, metric:functions[].heuristic_burden.information_burden, metric:functions[].heuristic_burden.interaction_burden, metric:functions[].heuristic_burden.measurement_support, metric:functions[].heuristic_burden.repo_relative.percentile, metric:functions[].heuristic_burden.repo_relative.zscore, metric:functions[].heuristic_burden.score, metric:functions[].heuristic_burden.size_support, metric:heuristic_outliers[].basis, metric:heuristic_outliers[].heuristic_burden, metric:heuristic_outliers[].kind, metric:heuristic_outliers[].measurement_support, metric:heuristic_outliers[].name, metric:heuristic_outliers[].path, metric:heuristic_outliers[].rank, metric:heuristic_outliers[].reasons[], metric:heuristic_outliers[].repo_relative.percentile, metric:heuristic_outliers[].repo_relative.zscore, metric:heuristic_outliers[].size_support, metric:heuristic_outliers[].span.end_byte, metric:heuristic_outliers[].span.end_line, metric:heuristic_outliers[].span.start_byte, metric:heuristic_outliers[].span.start_line, metric:heuristic_burden_distribution.count, metric:heuristic_burden_distribution.flat, metric:heuristic_burden_distribution.max, metric:heuristic_burden_distribution.mean, metric:heuristic_burden_distribution.median, metric:heuristic_burden_distribution.min, metric:heuristic_burden_distribution.p25, metric:heuristic_burden_distribution.p75, metric:heuristic_burden_distribution.relative_outlier_eligible, metric:heuristic_burden_distribution.stddev, metric:hotspots[].name, metric:hotspots[].path, metric:hotspots[].rank, metric:hotspots[].reasons[], metric:hotspots[].score, metric:hotspots[].span.end_byte, metric:hotspots[].span.end_line, metric:hotspots[].span.start_byte, metric:hotspots[].span.start_line, metric:heuristic_model.authority, metric:heuristic_model.experimental, metric:heuristic_model.gating_permitted, metric:heuristic_model.human_calibrated, metric:heuristic_model.id, metric:heuristic_model.meaning |
| metric-features-axes | metric | none | metric:functions[].features.aggregation_policy, metric:functions[].features.axes.cohesion.unknowns[], metric:functions[].features.axes.entropy.measurements.ast_edge_entropy_normalized.estimator, metric:functions[].features.axes.entropy.measurements.ast_edge_entropy_normalized.sample_size, metric:functions[].features.axes.entropy.measurements.ast_edge_entropy_normalized.value, metric:functions[].features.axes.entropy.measurements.ast_kind_entropy_normalized.estimator, metric:functions[].features.axes.entropy.measurements.ast_kind_entropy_normalized.sample_size, metric:functions[].features.axes.entropy.measurements.ast_kind_entropy_normalized.value, metric:functions[].features.axes.entropy.measurements.byte_entropy_bits_per_byte.estimator, metric:functions[].features.axes.entropy.measurements.byte_entropy_bits_per_byte.sample_size, metric:functions[].features.axes.entropy.measurements.byte_entropy_bits_per_byte.value, metric:functions[].features.axes.entropy.measurements.token_entropy_normalized.estimator, metric:functions[].features.axes.entropy.measurements.token_entropy_normalized.sample_size, metric:functions[].features.axes.entropy.measurements.token_entropy_normalized.value, metric:functions[].features.axes.entropy.unknowns[], metric:functions[].features.axes.impact.unknowns[], metric:functions[].features.axes.lexical_visual.measurements.comment_to_code_ratio.estimator, metric:functions[].features.axes.lexical_visual.measurements.comment_to_code_ratio.sample_size, metric:functions[].features.axes.lexical_visual.measurements.comment_to_code_ratio.value, metric:functions[].features.axes.lexical_visual.measurements.halstead_volume.estimator, metric:functions[].features.axes.lexical_visual.measurements.halstead_volume.sample_size, metric:functions[].features.axes.lexical_visual.measurements.halstead_volume.value, metric:functions[].features.axes.lexical_visual.measurements.token_count.estimator, metric:functions[].features.axes.lexical_visual.measurements.token_count.sample_size, metric:functions[].features.axes.lexical_visual.measurements.token_count.value, metric:functions[].features.axes.lexical_visual.measurements.unique_token_ratio.estimator, metric:functions[].features.axes.lexical_visual.measurements.unique_token_ratio.sample_size, metric:functions[].features.axes.lexical_visual.measurements.unique_token_ratio.value, metric:functions[].features.axes.lexical_visual.measurements.vocabulary_size.estimator, metric:functions[].features.axes.lexical_visual.measurements.vocabulary_size.sample_size, metric:functions[].features.axes.lexical_visual.measurements.vocabulary_size.value, metric:functions[].features.axes.lexical_visual.unknowns[], metric:functions[].features.axes.redundancy.measurements.anti_pattern_line_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.anti_pattern_line_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.anti_pattern_line_ratio.value, metric:functions[].features.axes.redundancy.measurements.clone_line_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.clone_line_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.clone_line_ratio.value, metric:functions[].features.axes.redundancy.measurements.dead_or_unused_line_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.dead_or_unused_line_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.dead_or_unused_line_ratio.value, metric:functions[].features.axes.redundancy.measurements.redundancy_union_ratio.estimator, metric:functions[].features.axes.redundancy.measurements.redundancy_union_ratio.sample_size, metric:functions[].features.axes.redundancy.measurements.redundancy_union_ratio.value, metric:functions[].features.axes.redundancy.unknowns[], metric:functions[].features.axes.safety.measurements.parse_complete.estimator, metric:functions[].features.axes.safety.measurements.parse_complete.sample_size, metric:functions[].features.axes.safety.measurements.parse_complete.value, metric:functions[].features.axes.safety.unknowns[], metric:functions[].features.axes.structural.measurements.cfg_components.estimator, metric:functions[].features.axes.structural.measurements.cfg_components.sample_size, metric:functions[].features.axes.structural.measurements.cfg_components.value, metric:functions[].features.axes.structural.measurements.cfg_edges.estimator, metric:functions[].features.axes.structural.measurements.cfg_edges.sample_size, metric:functions[].features.axes.structural.measurements.cfg_edges.value, metric:functions[].features.axes.structural.measurements.cfg_points.estimator, metric:functions[].features.axes.structural.measurements.cfg_points.sample_size, metric:functions[].features.axes.structural.measurements.cfg_points.value, metric:functions[].features.axes.structural.measurements.cognitive_complexity.estimator, metric:functions[].features.axes.structural.measurements.cognitive_complexity.sample_size, metric:functions[].features.axes.structural.measurements.cognitive_complexity.value, metric:functions[].features.axes.structural.measurements.complexity_mass.estimator, metric:functions[].features.axes.structural.measurements.complexity_mass.sample_size, metric:functions[].features.axes.structural.measurements.complexity_mass.value, metric:functions[].features.axes.structural.measurements.cyclomatic_complexity.estimator, metric:functions[].features.axes.structural.measurements.cyclomatic_complexity.sample_size, metric:functions[].features.axes.structural.measurements.cyclomatic_complexity.value, metric:functions[].features.axes.structural.measurements.max_nesting.estimator, metric:functions[].features.axes.structural.measurements.max_nesting.sample_size, metric:functions[].features.axes.structural.measurements.max_nesting.value, metric:functions[].features.axes.structural.measurements.nloc.estimator, metric:functions[].features.axes.structural.measurements.nloc.sample_size, metric:functions[].features.axes.structural.measurements.nloc.value, metric:functions[].features.axes.structural.measurements.structural_mass.estimator, metric:functions[].features.axes.structural.measurements.structural_mass.sample_size, metric:functions[].features.axes.structural.measurements.structural_mass.value, metric:functions[].features.axes.structural.unknowns[], metric:functions[].features.axes.surprisal.measurements.max_bits.estimator, metric:functions[].features.axes.surprisal.measurements.max_bits.sample_size, metric:functions[].features.axes.surprisal.measurements.max_bits.value, metric:functions[].features.axes.surprisal.measurements.mean_bits.estimator, metric:functions[].features.axes.surprisal.measurements.mean_bits.sample_size, metric:functions[].features.axes.surprisal.measurements.mean_bits.value, metric:functions[].features.axes.surprisal.measurements.p90_bits.estimator, metric:functions[].features.axes.surprisal.measurements.p90_bits.sample_size, metric:functions[].features.axes.surprisal.measurements.p90_bits.value, metric:functions[].features.axes.surprisal.unknowns[], metric:functions[].features.exclusive, metric:functions[].features.id, metric:functions[].features.schema, metric:functions[].features.subject.kind, metric:functions[].features.subject.lang, metric:functions[].features.subject.name, metric:functions[].features.subject.span.end_byte, metric:functions[].features.subject.span.end_line, metric:functions[].features.subject.span.start_byte, metric:functions[].features.subject.span.start_line, metric:feature_schema.aggregation_policy, metric:feature_schema.axes[].aggregate, metric:feature_schema.axes[].meaning, metric:feature_schema.axes[].name, metric:feature_schema.id, metric:feature_schema.locality, metric:functions[].kind, metric:functions[].lang, metric:functions[].name, metric:functions[].path, metric:functions[].role |
| metric-calibration | metric | none | metric:readability_calibration.capture_id, metric:readability_calibration.disposition, metric:readability_calibration.evidence, metric:readability_calibration.readability_label_permitted, metric:readability_calibration.schema, metric:readability_calibration.transparent_axes_preserved |
| metric-report-envelope | metric | none | metric:schema, metric:status, metric:analyses[].analysis.diagnostics[], metric:analyses[].analysis.status, metric:analyses[].lang, metric:analyses[].path, metric:files[].behavioral_nloc, metric:files[].behavioral_regions, metric:files[].complexity_mass, metric:files[].complexity_mass_p90, metric:files[].contextual_surprisal_p90, metric:files[].heuristic_burden_max, metric:files[].heuristic_burden_p90, metric:files[].heuristic_burden_weighted_mean, metric:files[].lang, metric:files[].path, metric:files[].redundancy_ratio_p90, metric:files[].redundancy_union_ratio, metric:files[].structural_entropy_p90, metric:files[].structural_erosion, metric:files[].structural_mass, metric:files[].top_hotspot, metric:change_dispersion.authority, metric:change_dispersion.changed_lines, metric:change_dispersion.files[].added_lines, metric:change_dispersion.files[].binary, metric:change_dispersion.files[].changed_lines, metric:change_dispersion.files[].deleted_lines, metric:change_dispersion.files[].path, metric:change_dispersion.from, metric:change_dispersion.normalized_entropy, metric:change_dispersion.to, metric:change_dispersion, metric:analyses[], metric:functions[], metric:files[], metric:peer_groups[], metric:heuristic_outliers[], metric:hotspots[], metric:feature_schema.axes[], metric:change_dispersion.files[] |

## 5. Access blockers and next attempts (P0 source review NOT complete)

- `sjoberg-2013-maintenance`: UNAVAILABLE. Author PDF
  (uio.no …/tse13.pdf) 404 on direct fetch; OpenAlex marks the DOI closed
  with no repository fulltext; Scholar search API rate-limited (429) so no
  Scholar OA verdict obtainable here; web search surfaced no authorized
  author manuscript (only citations/discussions). Next: library copy.
- `buse-2010-readability`: UNAVAILABLE. umich author PDF: TLS cert failure on
  https; plain-http redirects to https (cert failure, unreadable). virginia
  old path: HTTP 403 on direct fetch. umich --insecure fetch: HTTP 404.
  Semantic Scholar API: openAccessPdf status CLOSED, empty URL. OpenAlex:
  is_oa false, no repository fulltext, single location (DOI only). No
  authorized fulltext reachable; next: library copy. (Posnett/Scalabrino
  re-describe Buse's design but are not substitutes for Buse's own text.)
