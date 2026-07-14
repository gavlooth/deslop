# Deslop MCP Improvement Plan

Date: 2026-06-25

## Objective

Improve `deslop-mcp` so coding agents get a safer, clearer cleanup workflow instead of a bag
of low-level tools. The goal is not more automation by default; it is better triage, better
explanations, stronger MCP metadata, and fewer repeated scans while keeping the deterministic
verify/apply boundary intact.

## Active Hypothesis

The best next improvement is a layered MCP UX:

1. make existing tools easier and safer for agents to choose;
2. add read-only workflow tools that summarize what to do next;
3. add stable scan/session handles to avoid inconsistent rescans;
4. expose resources/prompts only after the tool contract is clear.

This follows the evidence from MCP tool-description research: richer descriptions can improve
agent task success, but too much tool/schema context increases execution steps and cost. Keep
descriptions compact, explicit, and behavior-oriented.

## Negative-Memory Constraints

- Do not weaken `apply`: default writes only verifier `Removable` patches.
- Do not make MCP default builds network-capable; server-run LLM stays behind `slim-llm`.
- Do not treat metrics or smell detectors as proof. They triage; `verify` proves.
- Do not overfit to AI authorship. deslop detects/removes behavior-preserving bloat regardless
  of whether a human or model wrote it.
- Do not hide uncertainty. False positives and non-removable findings should be explicit outputs.

## Phase 0 - MCP Contract Audit

Target files:
- `crates/deslop-mcp/src/lib.rs`
- `README.md`
- `SPEC.md`
- `docs/CONFIG.md`

Work:
- Review every MCP tool description for: purpose, when to use, output schema, write behavior,
  network/source-egress behavior, and common failure mode.
- Add tool annotations or `_meta` risk hints where supported by the current protocol shape:
  read-only for `scan`, `propose`, `metrics`, `rules`, `fix mode=prompts`; destructive for
  `apply`; network/egress-sensitive for `fix mode=auto`.
- Keep descriptions compact. Prefer a sentence plus critical constraints over long prose.

Validation:
- Snapshot-style MCP `tools/list` tests assert descriptions/annotations for all tools.
- Default MCP build remains network-free.
- Gate: `cargo fmt --all && cargo test -p deslop-mcp && cargo test -p deslop-mcp --features slim-llm`.

Checkpoint:
- Report exact tool metadata added and any MCP spec limitations found.

## Phase 1 - `triage` Tool

Add a read-only `triage` tool that runs scan/metrics and returns an agent-oriented plan:

- `safe_now`: safe-auto/analyzer-confirmed findings with available deterministic edits.
- `needs_rewrite`: work orders appropriate for prompt-mode `fix`.
- `needs_coverage`: findings blocked by coverage unknown.
- `needs_characterization`: weak-oracle rewrites that need behavior-pinning tests.
- `likely_false_positive_or_non_removable`: explain why the tool should stop or lower priority.
- `hotspots`: top metric hotspots, explicitly marked as triage signals, not proof.

Inputs:
- `paths`, `config`, `analyzer`, `top_k`, optional `baseline`.

Output:
- `deslop.triage/1` structured JSON plus concise text summary.

Validation:
- Deterministic fixture covering each bucket.
- No writes, no network.
- Existing `scan`, `propose`, `metrics` outputs unchanged.
- Gate: `cargo fmt --all && cargo test -p deslop-mcp && cargo test --workspace`.

Checkpoint:
- Decide whether `triage` should become the default recommended MCP entrypoint in README/SPEC.

## Phase 2 - `explain_finding` / `explain_workorder`

Add read-only explanation tools for one finding or work order.

Inputs:
- Either `finding` object, `workorder` object, or `{ path, rule, span/fingerprint }`.
- Optional `config` and inline `analyzer`.

Output:
- Rule meaning and safety class.
- Exact evidence span and threshold/config used.
- Why it fired.
- Why it is or is not removable.
- Next safe action: ignore, rewrite, add coverage, characterize, or apply only after verifier proof.

Validation:
- Fixtures for long-method threshold, duplicate/near-duplicate, and safe-auto Clojure rule.
- Tests assert configured thresholds appear in explanations.
- Gate: `cargo fmt --all && cargo test -p deslop-mcp && cargo test -p deslop-analyzer`.

Checkpoint:
- If explanation logic starts duplicating analyzer internals, stop and extract rule metadata from
  analyzer instead of hand-maintaining parallel prose.

## Phase 3 - Scan Handles and Snapshot Reuse

Add in-memory scan/session handles inside the MCP server process.

Behavior:
- `scan` returns a `scan_id` in addition to current output.
- `propose`, prompt-mode `fix`, `metrics`, `triage`, and `explain_*` can accept `scan_id`.
- Each `scan_id` stores source path list, config digest, file fingerprints, reports, and metrics
  if requested.
- If files changed, callers get a clear stale-scan error with a suggested rescan.

Constraints:
- No persistent daemon database.
- No behavior change for callers that do not use `scan_id`.
- Bounded memory with simple LRU or max-scan count.

Validation:
- Reuse test proves no rescan path is taken when `scan_id` is fresh.
- Stale test modifies a file and gets a stale-scan error.
- Back-compat tests for raw `scan`/`propose` still pass.

Checkpoint:
- Measure whether this materially reduces repeated scan cost on `crates/`.

## Phase 4 - MCP Resources and Prompts

After the tool workflow is stable, add MCP resources/prompts:

Resources:
- `deslop://rules`
- `deslop://config/effective`
- `deslop://schemas/workorder`
- `deslop://schemas/patch`
- `deslop://last-scan/{scan_id}`

Prompts:
- `deslop-cleanup-loop`: agent instructions for scan/propose/rewrite/verify/apply.
- `deslop-review`: read-only review workflow using triage and explain tools.

Validation:
- `resources/list`, `resources/read`, and prompts-list tests.
- Resource output is stable, bounded, and does not include secrets.

Checkpoint:
- Update README/SPEC only if clients actually benefit; resources are optional context, not a
replacement for structured tool outputs.

## Phase 5 - Slim Auto Config Parity

> Status (2026-06-26): DONE. `SlimOptions` now carries an `analyzer: AnalyzerConfig`;
> `load_or_propose_work_orders` scans via `propose_work_orders_with_config`. CLI `fix` passes
> `analyzer_config(..)` and MCP `fix mode=auto` passes `mcp_analyzer_config(args)`, so auto mode
> honors thresholds + suppression (disabled rules / ignored paths) before the rewrite pipeline.
> Covered by `deslop-slim::auto_mode_suppression_drops_work_orders_for_disabled_rule`.

Close the known gap: MCP `fix mode=auto` delegates to `deslop-slim`, which currently uses slim's
existing scan path rather than MCP inline analyzer config.

Options:
- Add analyzer config to `SlimOptions`, then thread it through `deslop-slim::run_slim`.
- Or let MCP auto mode precompute work orders using `mcp_analyzer_config` and pass them into
  `SlimOptions.workorders`.

Preferred path:
- Precompute work orders in MCP for auto mode when analyzer/config overrides are present. This
  keeps slim's standalone CLI behavior unchanged.

Validation:
- MCP `fix mode=auto` mock test with per-language long-method threshold.
- Default no-feature MCP build still compiles without network code.
- Gate both feature states.

## Phase 6 - Research-Backed Rule Roadmap

Use papers as rule/backlog input, not as proof of correctness.

Candidate backlog:
- Better architectural/volume signals from AI-generated smell studies.
- Explicit false-positive reporting and expert-review mode, motivated by SmellBench false-positive
  rates for architectural smells.
- More robust codebase health reporting that separates structural metrics, rule findings, and
  verifier confidence.
- Maintain mutation as a verification signal, not a slop detector, until equivalent-mutant handling
  is substantially better.

Validation:
- Every new rule needs corpus cases with precision/recall measurement.
- Any write path must still go through `verify`/`apply`.

## Final Gate For Any Implementation Round

Run the smallest relevant test first, then:

```sh
cargo fmt --all --check
cargo build --workspace
cargo build -p deslop-slim --no-default-features
cargo test --workspace
cargo test -p deslop-mcp --features slim-llm
cargo clippy --workspace -- -D warnings
```

Update `.agents/SESSION_REPORT.md` at each meaningful checkpoint, then run `jj describe`.

---

# Review-Driven Improvement Plan (2026-07-02)

Scope: the current uncommitted working tree (~4,350 insertions / ~1,866 deletions across 29
files). Full-diff review verified: workspace compiles clean, `cargo test` all green. The
feature layer (suppression system, per-language `long_method_nloc`, rule registry in
`deslop-core`) is solid. The problems are concentrated in the refactoring layer and one
piece of unfinished wiring.

## Active Hypothesis

The extraction sweep across `deslop-slim`, `deslop-cli`, `deslop-eval`, `deslop-metrics`,
`deslop-lsp`, and the analyzer packs was driven by dogfooding deslop's own default
`long_method_nloc = 40` on itself (no repo-root `deslop.toml` exists, so defaults apply).
That pressure produced degenerate extraction: single-use delegation wrappers and named
match arms that scatter linear pipelines. Evidence: `deslop scan crates/deslop-slim/src`
on the refactored code now reports `near-duplicate` findings (e.g. lines 920-921 vs 331)
— the refactor traded one slop class for another. The systemic fix is to configure the
tool for its own repo using the new suppression/threshold features, then re-calibrate the
worst extractions, rather than continuing to contort code to satisfy a blunt default.

## P1 — Wire `spec.rs` into `deslop-mcp` (finish MCP Plan Phase 0)

`crates/deslop-mcp/src/spec.rs` (untracked) implements exactly what Phase 0 above calls
for — tool annotations (`readOnlyHint`, `destructiveHint`, ...) and behavior-oriented
descriptions — but `lib.rs` has no `mod spec;` and keeps its own `tool_definitions()`.
The file is currently dead code, and two copies of every schema helper will drift.

- Add `mod spec;` to `lib.rs`; route `tools_list_result` through `spec::tool_definitions()`.
- Delete the now-duplicated helpers from `lib.rs`: `tool_definitions`, all `*_tool_spec`
  functions, `tool`, `object_schema`, `required_schema`, `string_schema`, `config_schema`,
  `analyzer_schema`, `paths_schema`, `patches_schema`, `coverage_schema`,
  `characterization_tests_schema`. They live only in `spec.rs`.
- Extend the `tools/list` test to assert annotations per tool (read-only for scan/propose/
  metrics/rules/verify/characterize/verify_characterization; destructive for apply; fix
  marked destructive + open-world).
- Risk: low. Verified the existing description assertions (`contains("slim-llm")`, the
  exact analyzer-schema description string) hold against the `spec.rs` copies.

## P2 — Unify the duplicated config struct hierarchies

CLI (`AnalyzerConfigSection` / `RuleConfigSection` / `AnalyzerLangConfigSection`) and MCP
(`McpAnalyzerConfig` / `McpRuleConfig` / `McpAnalyzerLangConfig`) define field-identical
serde structs. The 2026-06-28 pass dedup'd the *collection logic* via
`SuppressionBuilder::add_section`; the *struct definitions* and the apply/threshold
plumbing (`analyzer_thresholds` + `lang_threshold` vs `apply_mcp_analyzer_config` +
`apply_mcp_lang_config`) are still parallel.

- Move one shared `#[derive(Deserialize)] #[serde(deny_unknown_fields)]` set into
  `deslop-analyzer` (it already owns `AnalyzerConfig`, `Suppression`, `RuleSuppression`).
  Serde deserializes the same struct from TOML (CLI) and JSON (MCP inline args).
- Give it two methods: `apply_thresholds(&mut AnalyzerConfig)` and
  `collect_suppression(&mut SuppressionBuilder)`. CLI and MCP each become a thin caller.
- This also retires the misleadingly named CLI `analyzer_thresholds()` (it returns a full
  `AnalyzerConfig` with a default suppression that the caller overwrites — works, but the
  name and return type lie).
- serde is already in the workspace stack; adding the `derive` feature to
  `deslop-analyzer` is not a new dependency.

## P3 — Self-configure, then re-calibrate the extraction sweep

First the systemic fix, then the mechanical one:

1. Add a repo-root `deslop.toml` that dogfoods the new features on deslop itself, e.g.
   `[analyzer.rust] long_method_nloc = 55` (calibrate against the actual distribution via
   `deslop metrics crates/`). This is both the fix for the extraction pressure and a live
   test of the per-language override + suppression features.
2. `deslop-cli` progress lines: collapse the six single-use `*_progress_line` functions
   (`started_`, `rewrite_`, `characterizing_`, `verified_`, `outcome_`, `finished_`) back
   into the `slim_progress_line` match. Naming each arm of a data-formatting match adds
   indirection, not meaning.
3. `deslop-slim::run_slim_with_progress`: keep the three-stage split
   (`rewrite_work_orders` / `verify_rewrites` / `apply_verified_patches`) — it maps to
   real pipeline stages. Inline the pure-delegation wrappers: `emit_started_progress`,
   `public_characterization_report`, and fold `finish_slim_report` back into the
   orchestrator (it only assembles the return struct). Keep `rewrite_candidate_count`
   (two call sites) and `emit_outcome_progress` (self-contained loop).
4. `deslop-cli` fix chain: `fix()` → `resolve_fix_request()` → `run_fix_request()` →
   `run_real_provider_fix()` with a `FixRequest` shuttle struct has no second consumer
   (verified: no references outside `main.rs`). Collapse `run_fix_request` +
   `run_real_provider_fix` into one function; keep `resolve_fix_request`/`FixRequest`
   only if MCP auto-mode is about to reuse them, otherwise inline.
5. Re-run `deslop scan crates/` after each collapse; the new `near-duplicate` findings in
   `deslop-slim` should disappear or be justified.

## P4 — `code_lines` allocation in clojure.rs

`code_lines` collects `Vec<(usize, String)>` — one `String` per line per call, and it is
called by three rule functions per file (3× full-file allocation per scan). The previous
inline pattern borrowed `&str` from `strip_comment` with zero allocation. Return
`impl Iterator<Item = (usize, &str)> + '_` instead; call sites keep their shape.

## P5 — Suppression glob matching vs absolute paths (documented gap)

`match_path` only strips a leading `./`. Scanning an absolute path (`deslop scan
/srv/proj`) produces absolute finding paths, so relative globs like `crates/**` silently
never match (leading-`**/` globs still do). Docs currently say "globs match the scanned
path", so this is documented-but-surprising. Improvement: also try the candidate relative
to the current working directory before giving up. Low priority; do after P1–P3.

## Explicitly Fine (reviewed, no action)

- Suppression design: builder + validation against `deslop_core::rules`, `Arc` inner,
  post-production filtering covering external analyzers. Keep as-is.
- Rule registry centralization and `deny_unknown_fields` everywhere.
- `magic-number`/`incompleteness` AST masking, per-language threshold fallback,
  `cached_coverage_assessment` + `coverage_assessment` dedup in `deslop-verify` (these
  extractions remove real 4× duplication — the good kind).
- Test-helper extraction in lsp/mutate/parse tests (genuine repetition removal).
  `assert_json_eq` in `deslop-report` is more clever than the three assertions it
  replaced — optional revert, not worth a task.

## Validation Path

Per-task: smallest relevant crate test first (`cargo test -p <crate>`), then the Final
Gate above (fmt check, workspace build, slim no-default-features build, workspace test,
mcp slim-llm test, clippy -D warnings). P3 additionally gates on a self-scan
(`deslop scan crates/`) showing no new duplicate/near-duplicate findings versus the
pre-collapse baseline.

## Next Checkpoint

P1 wired and green (it is the smallest, unblocks the dead file, and completes an already
planned phase). Then P2, then P3.

Signature: Claude (Fable 5), full-diff review iterated into prioritized improvement plan, 2026-07-02.

---

# Structural Readability Capability (2026-07-10)

Status: IMPLEMENTED; focused numerical/MCP/CLI checks and full workspace gates passed.

## Objective

Add a deterministic per-region readability assessment to `deslop metrics` by combining
control-flow complexity with lexical/structural entropy. Expose a 0-100 score, a separate
confidence value, calibration status, and component burdens in text and JSON/MCP output.

## Active Hypothesis

Complexity and entropy capture complementary sources of comprehension burden. A transparent,
bounded interaction model can provide useful structural-readability triage now, provided it is
explicitly marked uncalibrated until evaluated against independent human ratings.

## Current Approach

- Extend region metrics with normalized token entropy and AST-node-kind entropy.
- Compute information volume from token count and token entropy.
- Combine complexity, information volume, entropy deviation, and their interaction into a
  deterministic bounded burden; convert that burden to a 0-100 readability score.
- Expose `measurement_confidence` from parse/sample reliability separately from
  `refactor_confidence`, which combines readability burden with measurement confidence and
  size support. Size strengthens complexity/entropy evidence but cannot flag simple code alone.
- Retain nested metric regions so containers and members (class/impl plus methods/functions) are
  each scored, and rank regions crossing the absolute refactor-confidence threshold.
- Preserve the existing safety boundary: readability remains triage-only and never authorizes a
  rewrite or apply operation.

## Validation Path

One fixture matrix compares simple, repetitive, and nested/dense regions and numerically verifies
boundedness, expected ordering, complexity contribution, entropy contribution, and interaction.
Then run a CLI text/JSON smoke and the repository's full Rust gate.

## Next Checkpoint

The metric crate, text/JSON contract, MCP description, docs, and numerical tests are green; then
record measured fixture values and remaining calibration risk in the session report.

## Negative-Memory Constraints

- Do not call the score a probability or claim human calibration without a labelled corpus.
- Do not transplant legacy Java/snippet coefficients into the cross-language model.
- Do not use low entropy alone as proof of poor readability; preserve separate component factors.
- Do not let readability affect deterministic fix/apply safety classes.

Signature: Codex (GPT-5), readability implementation plan, 2026-07-10.

## Confidence Distribution Normalization (2026-07-10)

Status: IMPLEMENTED; numerical flat/outlier/tie tests, real-repo smoke, and full workspace gates
passed.

Active hypothesis: absolute refactor confidence and repo-relative position answer different
questions and must both be exposed. Mean/stddev/quantiles make the distribution inspectable;
z-score plus tie-aware empirical percentile makes outliers actionable when absolute scores are
compressed. Relative selection is disabled for small or flat distributions so normalization cannot
manufacture a refactor target.

Validation: one numerical matrix covers exact summary statistics, a clear low-absolute outlier, a
flat distribution, and all-tied values. Terminal success requires the outlier to surface via the
relative gate while flat/tied data produce no candidate.

Signature: Codex (GPT-5), confidence-normalization plan, 2026-07-10.

## Labeled Confidence JSON Contract (2026-07-10)

Status: IMPLEMENTED; band-boundary, CLI/MCP JSON, workspace test, and clippy gates passed.

Target: bump the additive metrics output to `deslop.metrics/2` because `refactor_confidence`
changes from a number to a one-entry labeled object (`{"high": 0.70}`). Preserve
`refactor_confidence_score` as the numeric companion for sorting, arithmetic, and consumers that
should not inspect dynamic keys. Bands are `very_low`, `low`, `moderate`, `high`, `very_high`.

Validation: exact boundary mapping at 0.00/0.20/0.40/0.60/0.80/1.00, one-key serialization,
score equality between the object and numeric companion, and CLI/MCP JSON contract checks.

Signature: Codex (GPT-5), labeled-confidence packaging plan, 2026-07-10.

## Explicit Confidence Basis and Repo Context (2026-07-11)

Status: IMPLEMENTED; exact CLI/MCP JSON contracts, workspace tests, and clippy gates passed.

Target contract (`deslop.metrics/3`): pair the labeled intrinsic score with
`confidence_basis: "tree_intrinsic_v1"` and nest local normalization under
`repo_relative: {zscore, percentile}`. Remove flat z-score/percentile keys from `/3`; keep the
top-level distribution summary and scalar companion unchanged.

Validation: exact region and candidate JSON shapes, absence of legacy flat keys, preservation of
candidate selection/ranking, CLI smoke, MCP contract, and full workspace gates.

Signature: Codex (GPT-5), confidence-basis packaging plan, 2026-07-11.

---

# Product Backlog — Tool Improvements (2026-07-02)

Beyond the diff cleanup (P1–P5 above) and the MCP UX phases (triage tool, session handles,
FP reporting — already planned in the first section; not repeated here). Verified against
the code, not speculative.

## Tier 1 — gaps that undermine existing promises

1. **LSP ignores `deslop.toml`.** `deslop-lsp` calls `scan_source`, which hardcodes
   `AnalyzerConfig::default()`. Suppression and thresholds configured for the CLI do not
   apply to editor diagnostics — the same file shows different findings per surface.
   Fix: LSP resolves `deslop.toml` from the workspace root at initialize, uses
   `scan_source_with_config`, re-reads on config file change. Highest trust-per-effort.
2. **Inline suppression comments.** `// deslop:ignore <rule> [-- reason]` (per-line or
   next-line), the standard escape hatch of every mature linter (`#[allow]`, `noqa`,
   `eslint-disable-next-line`). Path globs are too coarse for one deliberate magic
   number. Natural extension of the new suppression layer; an LSP "suppress this
   finding" code action falls out for free. Parse during masking (string/comment ranges
   are already computed), so cost is near zero.
3. **Python idiom pack is empty.** `PYTHON_RULES` is a zero-length array — Python gets
   agnostic rules only, while coverage/mutation/threshold surfaces imply parity.
   Either seed it (`== None` → `is None`, `range(len(x))` → `enumerate`, `key in
   d.keys()`, `list(comprehension)` wrappers) with corpus cases per the eval gate, or
   document the asymmetry.

## Tier 2 — capability

4. **Cross-file duplication.** `duplicate_token_sequences` operates within one file.
   Copy-pasted helpers across modules are the dominant slop pattern in AI-assisted
   repos. Winnowing/rolling-hash fingerprints over token windows across the scan set;
   report pairs above `min_duplication_tokens`. Likely the single biggest detection win.
5. **Git-aware scan + ratchet.** `deslop scan --changed[=<ref>]` (diff-scoped scanning
   for pre-commit and PR CI) and `deslop baseline update` as an explicit command, so
   legacy repos can adopt a no-new-slop ratchet without fixing 200 findings first.
   Baseline fingerprints already exist; this is plumbing plus docs.
6. **TypeScript/JavaScript pack.** The largest population of AI-generated code.
   `analysis_pack!` + tree-sitter make this incremental; agnostic rules
   (long-method, duplication, magic numbers, narrating comments) work day one, idiom
   rules can trickle in.

## Tier 3 — polish and performance

7. **Parallel file scanning.** The `push_reports_for_path` loop is serial; rayon is
   already used for mutation scoring. Per-file scans are independent — collect paths,
   `par_iter` the scan, sort after (ordering already sorted post-hoc).
8. **`deslop fix --diff`.** Print unified diffs of safe-auto fixes instead of (or
   before) writing. Cheap trust-builder for the write path; `undo` already exists as
   the counterpart.
9. **FP feedback loop into the eval corpus.** `deslop feedback <fingerprint>
   --false-positive` appends a corpus case + expectation. Turns real-world false
   positives into permanent precision regression tests — the concrete mechanism for
   the "explicit false-positive reporting" item in the research-informed backlog above.
10. **SARIF/GitHub integration recipe.** SARIF rendering exists; a documented GitHub
    Actions workflow (scan → SARIF upload → code-scanning annotations, with
    `--fail-on` + baseline) makes findings appear on PRs with zero new code.

Sequencing suggestion: 1 and 2 first (both complete the suppression story shipped in
this changeset), then 5 (adoption), then 4 (detection depth). 3 and 6 as corpus time
allows; 7–10 opportunistic.

Signature: Claude (Fable 5), product backlog tiered from verified gaps, 2026-07-02.

---

## Graph-first algorithm remediation (2026-07-12)

Status: AUDIT COMPLETE; implementation not performed because this task requested an audit/report.

Active hypothesis: a single owned Tree-sitter analysis snapshot, with per-node exclusive evidence
and semantic graph projections, will remove the correctness, performance, identity, and work-order
integration failures that cannot be fixed by further readability-weight tuning.

Current approach: first repair duplicate work orders, false graph resolution, TypeScript/Python/
Clojure adapter contracts, partial-parse handling, and misleading metric gates. Then build the shared
node arena and migrate analyzer, metrics, graph, LSP, protocol, and slim consumers together.

Validation path: exact language construct matrix; one-parse instrumentation; node containment and
stable-key tests; duplicate-name scope resolution; clone-class maximality; work-order uniqueness;
clean/sloppy smoke; then one instrumented human-labelled benchmark with leave-project/language-out
calibration and explicit terminal outcomes.

Next checkpoint: P0 contract repair passes the exact live probes recorded in
`.agents/ALGORITHM_AUDIT.md`, with no health/readability/refactor-confidence gate retained without
external calibration.

Negative-memory constraints: repo-relative unusualness is not absolute evidence; passing the
current test suite is not a semantic oracle; byte/token entropy, surprisal, and compression are not
interchangeable; readability does not imply removability or safety.

Agent assignments for the completed audit: `/root` integration/verification;
`architecture_audit` parser/graph/work-order review; `metrics_audit` formulas and numerical probes;
`literature_review` primary-source evidence. Ruflo was unavailable, so built-in read-only agents
were used.

Signature: Codex (GPT-5), graph-first algorithm remediation plan, 2026-07-12.

---

# Ultimate Generic Deslop Plan (2026-07-12)

Status: AUTHORITATIVE PRODUCT ROADMAP. Earlier sections remain as decision history; where they
conflict with this section, this section wins. Execution is tracked in `.agents/TODO.md`.

## Mission and product contract

Deslop will be a language-extensible refactoring intelligence and safety kernel for humans and
LLM agents. It will answer four distinct questions without conflating them:

1. **What is the code?** A lossless, revision-bound syntax and semantic graph.
2. **What makes it difficult or wasteful?** Per-node evidence for readability, structural load,
   anomaly, duplication, coupling, and change impact.
3. **What transformations could improve it?** Explicit recipes with preconditions, graph deltas,
   dependency order, and counter-evidence.
4. **What may be changed safely?** Static and dynamic verification scoped to the exact patch and
   its impact cone.

The LLM is the planner and code author where judgment is useful. Deslop is the deterministic
observer, constraint system, transaction manager, and verifier. Neither an LLM suggestion nor a
high slop/readability score is permission to edit code.

“Generic” means a shared graph contract and algorithm family with explicit language capabilities;
it does not mean pretending Tree-sitter alone supplies types, effects, reflection behavior, macro
expansion, or whole-program semantics. Missing evidence must be reported as `unknown`, never
silently replaced by a weaker heuristic while retaining a stronger label.

### Non-goals

- Detect whether code was written by a human or an AI.
- Collapse readability, unusualness, complexity, removability, payoff, and safety into one number.
- Claim semantic equivalence from syntax similarity.
- auto-apply transformations that change public APIs, evaluation order, effects, exceptions,
  concurrency, generated code, macros, reflection, or dynamic dispatch without adequate proof.
- Make every grammar look identical. The common layer normalizes roles while adapters preserve
  language-specific facts and limitations.

## Active hypothesis and convergence

Active hypothesis: a single owned parse snapshot per file, projected into a layered program graph
and evaluated at exclusive nodes/regions, can give LLMs compact and actionable refactoring context
while eliminating the current duplicate-work-order, false-resolution, repeated-parse, metric-label,
and language-normalization failures.

The roadmap resolves this hypothesis in increasing semantic depth. Each milestone has a terminal
gate. Failure at a layer downgrades the advertised capability at that layer; it does not trigger
another round of threshold tuning. The decisive experiment is one instrumented, cross-language
benchmark that records all graph facts, features, proposals, patches, and verification outcomes in
one run so that ablations and thresholds can be evaluated post hoc.

## Core architecture: Universal Program Analysis Graph

The public abstraction is a revision-bound `ProjectAnalysis` over one immutable `ProjectSnapshot`,
not a collection of independently parsed reports. One source snapshot owns several linked projections:

```text
source revision
  -> lossless Tree-sitter CST + token/trivia spans
  -> canonical node/region arena
  -> scope and name-resolution graph
  -> CFG + hierarchical SESE/PST regions
  -> PDG/SDG control and data dependencies
  -> file/module/package/build dependency graph
  -> clone and structural-motif graph
  -> findings, metrics, transformation candidates, impact cones
  -> verification evidence and patch outcomes
```

This is a code-property-graph architecture, but with provenance and capability information on every
node, edge, fact, and conclusion. All CLI, MCP, LSP, evaluator, metrics, analyzer, and slim workflows
must consume projections of the same snapshot.

### 1. Lossless syntax and owned node arena

- Parse each file once per content revision with its correct grammar variant.
- Retain named and anonymous syntax, tokens, comments/trivia, byte and point spans, parse errors,
  field names, child order, and grammar provenance.
- Copy required facts out of borrowed Tree-sitter nodes into an owned arena. A scan-local `NodeId`
  is an arena index; it is never serialized as a durable identity.
- Give externally visible nodes a revision-bound `NodeKey` containing repository, normalized path,
  language/grammar, source revision, canonical role, span/structural anchor, and collision ordinal.
- Keep three identities separate: scan-local `NodeId`, best-effort baseline/finding fingerprint across
  revisions, and exact `RevisionGuard` for stale-write protection. A fuzzy baseline match can never
  authorize an edit.
- Make containment explicit. Every token belongs to one smallest exclusive metric region; every
  aggregate declares whether descendants are exclusive or inclusive.
- Allow derived line, blank/trivia-run, and comment-region nodes when line/visual metrics need a unit
  that does not correspond to one grammar node; link them to exact source/token ownership.
- Preserve raw source slices for exact patches and normalized token/role forms for comparison.
- Support Tree-sitter edit/changed-range invalidation without pretending identities survive arbitrary
  edits. Durable handles must either re-anchor with evidence or expire explicitly.

### 2. Canonical semantic roles and language adapters

Adapters map grammar-specific node kinds to a small, stable role vocabulary: project, module,
declaration, type, callable, parameter, block, statement, expression, branch, loop, match/case,
call, read, write, literal, comment, import/export, error, and generated/opaque region. Roles can be
composed; raw kinds remain available.

Every adapter must provide or explicitly decline these capabilities:

- grammar selection, including variants such as JavaScript, TypeScript, TSX, and dialect/version;
- Tree-sitter queries for declarations, references, scopes, control constructs, comments, and
  language-specific generated/opaque regions;
- canonical-role and operator/token classification;
- lexical scope, binding, import/export, and shadowing rules;
- CFG lowering rules, abrupt exits, exception edges, async/yield behavior, and evaluation order;
- def/use and conservative effect classification;
- formatting or CST-safe splice strategy for supported transformations;
- focused fixtures, parse-error policy, external analyzer integration, and capability declaration.

Capability tiers are monotone and machine-readable:

- `S0 syntax`: lossless parse, node roles, spans, tokens, comments.
- `S1 local structure`: regions, local metrics, clone normalization, syntactic recipes.
- `S2 local semantics`: scopes, name resolution, CFG, def/use, effects, local PDG.
- `S3 project semantics`: imports/exports, call/dependency graph, SDG, API impact.
- `S4 verified change`: compiler/type evidence plus targeted dynamic verification.

A finding or recipe requiring `S2` cannot be emitted as confirmed on an `S1` adapter.

### 3. Scope and name-resolution graph

- Represent definitions, references, imports, exports, lexical scopes, shadowing, visibility, and
  unresolved/ambiguous candidates as first-class facts.
- Use a declarative, file-incremental model in the style of stack graphs where it fits; allow an
  adapter or compiler/LSP bridge to provide higher-authority resolution.
- Never resolve by bare name alone. A `resolved` edge requires a unique path under the adapter's
  semantics; otherwise retain all candidates with an `ambiguous` or `unresolved` status.
- Record resolution authority (`syntax`, `adapter`, `compiler`, `runtime`) and provenance.

### 4. Control, region, and dependence graphs

- Build a CFG for each callable/initializer with entry, exit, normal, exceptional, suspension, and
  abrupt-control edges as supported by the adapter.
- Derive hierarchical single-entry/single-exit regions with a Program Structure Tree or an explicit
  irreducible-region representation. This gives stable units for branch merging, splitting,
  flattening, extraction, and exclusive metric aggregation.
- Build a PDG from control dependence and def/use data dependence. Build an SDG when calls,
  parameters, returns, globals, and cross-file resolution are sufficiently authoritative.
- Store dominance/post-dominance, liveness, reaching definitions, effects, aliases at the available
  precision, and an explicit uncertainty set. Conservative uncertainty blocks automatic edits; it
  does not disappear from reports.

### 5. Project dependency, clone, and evidence graphs

- Model files, modules, packages, build targets, public API surfaces, generated boundaries, imports,
  calls, inheritance/implementation, data ownership, tests, and configuration/build edges.
- Compute strongly connected components, the condensation DAG, topological layers, fan-in/fan-out,
  instability, change coupling, and architecture-rule violations. A cycle is a planning constraint,
  not automatically a defect.
- Represent clones as maximal clone classes, not repeated pair findings. Link normalized token,
  subtree, and semantic-motif evidence back to all participating nodes.
- Attach every metric/finding to evidence nodes and every aggregate to a declared roll-up policy.
  Store authority, confidence basis, exclusions, and counter-evidence separately from severity.

## Per-node evidence model

Every named node and every canonical region receives an exclusive local feature vector. Inclusive
subtree/project views are derived; they are not recomputed from overlapping slices. Keep these axes
separate in storage and protocol responses:

1. **Structural load:** NLOC, nesting, branch/loop counts, cyclomatic and essential complexity,
   fan-in/out, dependency depth, live variables, parameters/outputs, and control/data coupling.
2. **Lexical/visual readability evidence:** identifier quality, expression length, indentation and
   line-shape distribution, comment placement, vocabulary, token/operator density, and role-specific
   learned features.
3. **Naturalness/anomaly:** token-model surprisal normalized by node role and project/language;
   explicitly repo-relative unless a calibrated external model establishes otherwise.
4. **Distributional entropy:** Shannon entropy over declared token/AST-edge categories with sample
   size and estimator. Do not call source-byte compression or language-model surprisal “entropy”
   without qualifying it.
5. **Redundancy:** exact/renamed/near clone class membership, repeated graph motifs, boilerplate,
   and duplicated responsibility.
6. **Cohesion and coupling:** PDG clusters, shared state, slices, call/dependency communities, and
   responsibility dispersion.
7. **Change impact/payoff:** callers, dependents, test coverage, churn, blast radius, expected graph
   delta, and number of findings resolved.
8. **Reliability and safety:** adapter capability, resolution authority, parse completeness, static
   preconditions, coverage/mutation evidence, and verification results.

If human-labelled models are shipped, expose calibrated probabilities per model/language/role and
their version. A cross-language “readability” score ships only if it beats size/simple baselines on
held-out projects and languages with acceptable calibration. Otherwise deslop exposes the evidence
vector and rankings without the label.

## Transformation opportunity engine

Detectors consume graph facts and emit `TransformationCandidate`s. They do not write edits. Each
candidate is deduplicated by snapshot, target region, and recipe; multiple findings become evidence
on one candidate.

### Branch and control-flow transformations

For each CFG/PST branch region, evaluate these recipe families:

- merge equivalent arms or adjacent conditions only when bodies, effects, evaluation order,
  short-circuit behavior, exceptions, and bindings are compatible;
- factor common branch prefixes/suffixes when dominance and post-dominance preserve behavior;
- split a compound branch when its predicates/actions form independent dependence slices;
- replace nested conditionals with guards when exits and resource/exception behavior are preserved;
- invert conditions to reduce nesting, remove dead/unreachable arms, or turn exhaustive chains into
  pattern/table dispatch where language semantics make that representation clearer;
- extract a branch or repeated decision into a named predicate/action only when captured inputs,
  outputs, mutations, and control exits are explicit.

Branch “clarity” must be predicted from the resulting region graph, not from line-count reduction.
The candidate includes before/after nesting, complexity, dependence cuts, effects, and readability
evidence, plus reasons the recipe might be rejected.

### Function transformations

- Find extract-method candidates from SESE/PST regions and complete PDG computation or object-state
  slices, preferring coherent action blocks over arbitrary line windows.
- Compute exact inputs, outputs, mutations, exits, exceptions, async/yield constraints, ownership/
  lifetime constraints, and affected call sites before proposing extraction.
- Split multi-responsibility callables using dependence cohesion and named action clusters.
- Merge over-fragmented helpers when a trivial single-use wrapper adds indirection without a useful
  abstraction boundary and inlining preserves dispatch, visibility, stack/exception, and API facts.
- Inline temporaries, introduce explanatory variables, simplify expressions, and reorder independent
  statements only under explicit def/use and effect constraints.
- Treat long methods and high complexity as search signals, never proof that extraction improves code.

### Dependency and module transformations

- Use SCCs and the condensation DAG to expose cycles, dependency order, and legal transformation
  sequencing. Suggest cycle-breaking seams from dependency direction, API ownership, and data flow;
  never auto-break a cycle from topology alone.
- Compare declared architecture rules and inferred layers with actual dependencies. Identify stable
  abstractions, unstable dependencies, bypasses, hub modules, misplaced declarations, and tests that
  cross unintended boundaries.
- Suggest move/split/merge-module operations from structural and semantic cohesion, coupling, public
  API impact, and change history when available. Clustering is candidate generation, not ground truth.
- Repair import/declaration order only when order is semantically irrelevant or the language/toolchain
  provides authority. Preserve initialization and side-effect order.
- Derive the refactoring schedule from prerequisites and invalidations rather than applying a fixed
  bottom-up or top-down rule.

### Duplication, ceremony, dead code, and clarity

- Build maximal clone classes using exact subtree fingerprints, renamed-token normalization, scalable
  candidate indexing, and graph-context verification. One clone class yields one coordinated work order.
- Distinguish incidental token similarity, generated/schema repetition, tests, parallel public APIs,
  and intentional redundancy before recommending abstraction.
- Find forwarding layers, redundant conversions/allocations, defensive wrappers, needless comments,
  dead declarations/branches, and repeated error handling through graph facts plus adapter rules.
- Evaluate names and comments in scope and role context. Prefer explaining invariants, decisions, and
  effects; do not delete documentation merely because it restates syntax under a simplistic matcher.

## Transformation recipe and safety contract

Every executable recipe is versioned and contains:

- applicable languages, roles, and minimum capability tier;
- required nodes/edges/facts and provenance;
- positive preconditions and explicit forbidden/unknown conditions;
- parameter schema, edit builder, formatting strategy, and collision handling;
- expected graph delta and findings expected to resolve or regress;
- safety class, impact-cone query, validation plan, and rollback data;
- minimal counterexample fixtures and property/regression tests.

Preconditions are three-valued: `Proven`, `Disproven`, or `Unknown`, each with authority and evidence.
Only `Proven` satisfies an automatic obligation; absence of a counterexample is not proof. LLMs may
select candidates and propose semantic boundaries, names, tests, or patches, but cannot promote a safety
class, mark an edge resolved, or bypass a failed/unknown gate.

Safety classes:

- `safe-auto`: locally proven under complete preconditions and still verified after application;
- `analyzer-confirmed`: requires authoritative compiler/type/effect information;
- `safe-with-tests`: static preconditions plus adequate targeted dynamic evidence;
- `suggest-only`: useful candidate with unresolved semantic uncertainty;
- `llm-design`: architectural intent or API choice requiring judgment and review;
- `blocked`: known ambiguity, incomplete parse/graph, generated boundary, or failed verification.

The apply transaction is: pin revision -> revalidate preconditions -> produce patch -> parse and
rebuild impacted graph -> compare expected/actual graph delta -> format -> compile/type/lint -> run
targeted tests and optional characterization/differential/mutation checks -> report -> atomically
commit or roll back. Verification commands execute under explicit resource, filesystem, environment,
and network policies. “Tests passed” is evidence, not a proof of equivalence.

When a risky rewrite needs characterization, capture and approve that behavior on the pinned pre-change
snapshot before authoring the rewrite. Tests generated after the rewrite cannot independently establish
what behavior was meant to be preserved.

## Work-order dependency planner

One `WorkOrder` is a reviewable transformation transaction, not a finding. It contains a unique ID,
snapshot, target region, recipe/parameters, evidence and counter-evidence, safety class, impact cone,
prerequisites, conflicts, expected graph delta, patch budget, and verification contract.

Its machine-level access summary declares `Reads`, `Writes`, `Requires`, and `Invalidates` over node,
symbol, file, API, build, and test resources. Multi-file edits are one atomic transaction.

Build a dependency graph over work orders:

- add prerequisite edges for identity, signature, move/rename, extraction, test, and graph-authority
  requirements;
- add conflict edges for overlapping edits, shared public surfaces, mutually exclusive recipes, and
  invalidation of another candidate's target;
- collapse genuine atomic groups; block or ask for a design decision on unresolved cycles;
- topologically schedule independent transactions and run disjoint impact cones in parallel;
- after every committed patch, incrementally reparse and replan affected candidates. Stale orders
  expire; they are never blindly rebased by span.

This graph is how deslop decides whether branches/functions/modules should be merged or split and in
which order. The objective is a Pareto improvement across clarity, structure, coupling, duplication,
impact, and safety—not maximum finding-count reduction.

## LLM-facing protocol

The primary agent workflow is query -> plan -> patch -> verify -> replan:

1. `index` returns snapshot/capabilities, parse gaps, architecture summary, and cache state.
2. `triage` returns ranked transformation candidates, not a flood of raw findings.
3. `explain` returns a bounded graph slice: source nodes, scope, CFG/PST/PDG facts, dependencies,
   clones, evidence, counter-evidence, impact cone, and relevant tests.
4. `plan` returns the work-order DAG and exposes alternatives/trade-offs; the LLM selects or edits it.
5. `propose_patch` accepts recipe-grounded edits or an LLM patch and validates touched-node identity,
   declared intent, edit overlap, and scope.
6. `verify` runs the transaction contract and returns structured failures at the responsible node,
   edge, precondition, or test—not only command text.
7. `apply` is permitted only for the requested safety policy and pinned revision, produces undo data,
   and triggers incremental reanalysis.

All responses have schema versions, deterministic ordering, pagination/budgets, provenance, explicit
unknowns, and handles that expire with their revision. Provide compact source-plus-graph context so
an LLM does not need the whole repository or an opaque scalar. MCP, CLI, LSP, and library APIs expose
the same domain objects.

## Scale and incrementality

- Cache parse/arena/adapter facts by content, grammar, adapter, and schema version.
- Invalidate through Tree-sitter changed ranges and graph dependencies; rebuild only affected scope,
  CFG/PDG, clone buckets, callers/dependents, candidates, and metrics.
- Keep name-resolution facts file-isolated where possible, then stitch project paths lazily.
- Use content fingerprints and ordered indices for clone search instead of all-pairs comparison.
- Store the project graph in a compact, deterministic representation with bounded query APIs.
- Parallelize independent files/regions and verifier jobs, but serialize graph commits and stable output
  ordering. Measure parse count, cache hit rate, latency, peak memory, and invalidation fan-out.
- Degrade explicitly under budgets: complete local facts may be returned while project semantics remain
  pending/unknown. Do not attach high-authority conclusions to partial analysis.

## Convergent evaluation and release gates

Build one versioned benchmark harness that captures every candidate feature and outcome once. It must
contain small gold fixtures and licensed real repositories across Rust, JavaScript, TypeScript/TSX,
Python, Clojure, and Julia; macro/dynamic/generated/error cases; clean and deliberately degraded code;
human- and LLM-produced behavior-preserving refactors; and seeded unsafe near-misses.

Evaluate these independently:

- parse coverage/error fidelity and canonical-role agreement;
- name-resolution and dependency-edge precision/recall, including duplicate names and ambiguity;
- CFG/PST/PDG/SDG edge agreement on gold fixtures;
- candidate precision/recall, precision at the review budget, maximal clone-class quality, duplicate
  work-order rate, and unsupported-capability leakage;
- patch applicability, precondition rejection quality, expected/actual graph delta, compile/type/lint
  success, behavioral regression rate, and rollback integrity;
- human readability rankings, timed/correct comprehension, inter-rater agreement, calibration, and
  gain over size/complexity baselines;
- LLM task success, review edits, tokens/context, retries, semantic regressions, and wall time with and
  without graph-grounded work orders;
- cold/full and warm/incremental latency, parse counts, throughput, cache hits, memory, deterministic
  output, and invalidation fan-out.

Use leave-project-out splits for every learned/ranked claim and leave-language-out tests before calling
anything generic. Publish per-language/role confidence intervals and failure cases. Release gates are:

1. no duplicate work-order IDs and no falsely `resolved` ambiguous references in the gold corpus;
2. no confirmed finding/recipe above the adapter's declared capability tier;
3. zero known behavior-changing patches in `safe-auto`; any counterexample immediately demotes the recipe;
4. deterministic output and successful atomic rollback under injected verifier failures;
5. a claimed readability model materially and consistently beats size/simple baselines on held-out data
   and is calibrated; otherwise retain only transparent evidence axes;
6. graph-grounded LLM refactoring improves successful verified tasks over the same model/tool budget
   without increasing semantic regressions;
7. incremental analysis demonstrates bounded changed-region invalidation and a measured advantage over a
   full scan; otherwise do not advertise incrementality.

Threshold values beyond the zero-tolerance correctness gates are set once from labelled benchmark data
and frozen before the held-out run. They are not chosen from deslop's own clean/sloppy smoke corpus.

### Benchmark minimums and provisional acceptance floors

These values make the target falsifiable. They are provisional until a benchmark pilot measures label
quality and resource variance; then they are frozen before the release holdout and may not be relaxed after
seeing it.

- Canonical microcorpus: at least 600 programs, at least 100 for each of the six non-generic language
  adapters, spanning control flow, nested callables, calls/imports, comments/strings/Unicode, malformed
  subtrees, macros/dynamic behavior, and language-specific syntax.
- Transformation corpus: at least 1,000 labelled opportunities and 1,000 hard negatives with gold target,
  protected spans/APIs, expected safety class, behavior oracle, and allowed transformation family.
- Project corpus: 18 pinned real repositories, three per language, stratified by size and containing tests,
  public APIs, generated boundaries, and reproducible build commands.
- Human/LLM set: at least 300 blinded readability pairs and 240 fixed refactoring tasks, balanced by
  language and opportunity family. Authorship can be a reporting slice but is never the prediction target.
- Graph floors: canonical-role macro F1 >= 0.99 with no language below 0.97; exact containment and owning-
  callable assignment on all gold fixtures; intra-callable control-edge F1 >= 0.98; resolved local-reference
  precision >= 0.98 at declared coverage >= 0.80; incremental and clean-rebuild graphs exactly equal.
- Candidate floors: actionable precision lower 95% bound >= 0.90 overall and 0.85 per language; recall lower
  bound >= 0.70 overall and 0.60 per language/family; hard-negative actionable false-positive upper bound
  <= 0.02 overall and 0.05 per language; candidate calibration ECE <= 0.05.
- Behavior floors: 100% parse/build/typecheck and declared behavior-oracle success for accepted benchmark
  patches; no unauthorized public API/test weakening; zero confirmed semantic failures in `safe-auto`.
- Human-quality floors: blinded preference lower 95% bound >= 0.60 overall and 0.55 per language, with at
  least 90% of accepted patches improving the task's declared primary axis and no hidden project-level
  coupling/API/aggregate-complexity regression above the declared tolerance.
- LLM floors: graph-rich work orders improve behavior-and-quality accepted-patch rate by at least 10
  percentage points over findings/raw-code context with paired 95% confidence excluding zero; no language
  or model family regresses by more than 5 points; out-of-scope edits <= 2%; correct unsafe/impossible
  abstention >= 90%.
- Initial reference-machine scale budget: cold 1 MLOC scan <= 60 seconds and <= 3 GiB peak RSS; single-file
  incremental edit p95 <= 500 ms and <= 5% of full-scan time; tenfold corpus growth <= 12x wall time and
  <= 11x memory. The machine, filesystem/cache state, corpus, and toolchain must be recorded.

Report macro, worst-language, worst-family, confidence interval, abstention/coverage, and prior-release
delta together. A pass cannot be manufactured by pooling languages, dropping abstentions, cherry-picking
work orders, or weakening tests/coverage/verification.

## Dependency-ordered implementation roadmap

```text
M0 contract truth
  -> M1 immutable parse snapshot
    -> M2 adapter contract
      -> M3 scope/project names
        -> M4 CFG/PST/PDG/SDG
          -> M5 candidates + executable recipe slices
            -> M6 work-order DAG + LLM protocol
            -> M7 hardened verification authority

M1 + M2 ---------------------> M8 feature capture/calibration
M1 + M3 + correctness gates -> M9 incremental/project scale
M6 + M7 + M8 + M9 ----------> M10 external release evidence
```

Schema work for M6, verifier contracts for M7, feature instrumentation for M8, and performance
instrumentation for M9 begin earlier, but their completion gates retain the dependencies above. In
particular, every M5 recipe is implemented as a thin verified vertical slice; M7 then broadens and
hardens the shared verification authority rather than postponing safety until after recipe creation.

### M0 — Repair present contracts

Group findings into one work order per target/recipe; fix ambiguous name resolution; select the correct
TypeScript/TSX grammars; repair Python regions and Clojure branch semantics; make partial-parse behavior
consistent; remove or relabel uncalibrated health/readability/refactor-confidence gates; add exact probes
from `.agents/ALGORITHM_AUDIT.md`. This milestone is a prerequisite for trusting later measurements.

### M1 — One parse, one owned syntax snapshot

Create the revision/source store, node arena, IDs/keys, containment index, token/trivia ownership, parse
diagnostics, role hooks, and query surfaces. Migrate metrics, analyzer, graph, protocol, LSP, evaluator,
and slim consumers so instrumentation proves one parse per file revision.

#### Active M1.9 execution plan — analyzer and metrics shared-analysis migration

Active hypothesis: making `Arc<ProjectAnalysis>` the only primary analyzer/metrics input, plus one
owned adapter-fact projection per file, will preserve current language behavior while reducing every
complete file revision to one parser invocation and eliminating nested-region metric double ownership.

Current approach: root owns integration. `deslop-parse` evaluates existing `LangPack` node hooks
internally against its retained private Tree and publishes only owned facts aligned to `NodeId`s.
Analyzer and metrics receive borrowed file contexts over one `Arc<ProjectAnalysis>`; they use pinned
bytes, line starts, grammar selection, `NodeView` traversal, owned adapter facts, query captures, and
exclusive/reset-aware aggregation. Path/source compatibility APIs build exactly one snapshot and
delegate; rules never reread, reselect grammar, or call legacy parse functions. Boundary inputs are
captured at orchestration, and external results remain revision-bound.

Validation path: pin the existing simple Rust metric vector and five-region Python identities; prove
the Python file's 364 bytes and 12 physical NLOC have one declared owner despite previously summing to
649 bytes/21 NLOC; prove parse ledger `1/1/1/0`, legacy parse counter zero, unchanged ledger across
repeated consumers, deterministic output, malformed mixed-snapshot downgrade, path alias collapse,
external/suppression parity, and full workspace contracts. The terminal outcomes are either exact
behavior with one parse and conserved ownership, or an explicit blocked/downgraded adapter capability;
legacy per-rule/per-region parsing is not an accepted fallback.

Next checkpoint: primary `scan_analysis` and `metrics_analysis` APIs pass focused numerical contracts,
and `scan_paths`/`metrics_paths` are thin one-snapshot adapters with no consumer `parse_source` calls.

Checkpoint update (2026-07-13): the primary metrics projection now passes its numerical contracts.
It owns reset-aware bytes, physical lines, source-wide lexical tokens, and nested AST resets; binds a
deterministic `ProjectionId` to analysis/config/capability/exact adapter schemas; preserves the pinned
legacy intrinsic vector; and cleanly downgrades mixed malformed snapshots. The active next track is
the prepared analyzer context and owned rule migration, followed by the shared planner/presentation
adapter for both legacy path surfaces. The physical-line rule assigns each nonblank line to the
earliest semantic metric owner occurring on that line, falling back to File residual only when the
line contains no metric-owned byte; this preserves prefixed TS/TSX callable NLOC while assigning a
same-line nested callable line to the outer owner exactly once.

Analyzer checkpoint update (2026-07-13): the source-only owned analyzer projection now migrates
file-local agnostic/language/Rust rules, source-wide duplication masks and segments, cross-file
duplication, configured suppression, and inline comment directives. It rejects enabled boundary
analysis without a pinned manifest and records requested external analyzers unavailable instead of
consulting live paths. The active next step is `PreparedAnalyzerAnalysis` plus snapshot-pinned
boundary/external manifests, followed by the shared root/discovery/read/presentation planner and
legacy path adapter cutover. The temporary internal `SourceFile` text bridge must be removed or made
non-reparsable before M1.9 completion.

Planner/prepared-analyzer checkpoint update (2026-07-13): analyzer and metrics path APIs now share
one root/repository/scope/discovery/read/presentation planner and build one immutable snapshot. Auto
repository identity uses normalized VCS remote/root-commit evidence when available and a path-bound
identity only for unversioned roots. Analyzer boundary discovery pins every candidate TOML/YAML/JSON
artifact before analysis; its private completeness witness cannot be forged by callers. Boundary
evidence uses only NodeViews and pinned artifact bytes, with no reread or parse. Partial sources or
invalid UTF-8 artifacts withhold all repository-negative boundary claims. Presentation paths are
bound into projection identity before findings/fingerprints/suppression/cross-file messages, and one
cached `AnalyzerFile`/adapter-fact projection is reused across local, duplication, and boundary passes.
Repeated prepared scans remain byte-identical after live files are mutated; rebuilding changed inputs
changes projection identity. The active terminal work is removal of the reparsable internal
`SourceFile` bridge, static primary-surface no-parse/no-read guards, and an explicit no-grammar text
source contract or documented M2 invalidation before checking M1.9.

M1.9 terminal update (2026-07-13): DONE. The reparsable analyzer bridge and all legacy analyzer/
metrics parse pipelines were removed. `AnalyzerText` exposes pinned text/line operations but cannot
enter `parse_source`; path and `SourceFile` compatibility calls build a single overlay snapshot and
delegate to the owned projection. File-wide static guards reject `parse_source`, live reads, and
path/`Lang` pack reselection across analyzer and metrics production. Source compatibility tests are
deterministic with a zero legacy parse counter, and the shared planner preserves the caller's exact
display path so suppression semantics remain stable across invocation working directories. The full
workspace test, strict clippy, build, rustdoc, format, and whitespace gates pass. Proposal corpus
goldens now encode the authoritative external boundary: unpinned live clj-kondo results are reported
unavailable and cannot create revision claims.

Scoped M2 invalidation: the deleted no-grammar `.testpack` analyzer shim was test-only and had no
honest syntax identity. Until M2.1 versions `TextSource` capability/adapter identity and explicitly
defines report-only analyzer support plus metrics exclusion, registered adapters without a grammar
artifact fail before snapshot publication. Do not restore grammarless analysis by bypassing
`ProjectAnalysis` or by synthesizing a generic grammar.

Negative-memory constraints: do not expose borrowed Tree-sitter nodes; duplicate `LangPack` raw-kind
logic; infer durable identity from reanchors; union nested inclusive metric regions; reread paths after
snapshot construction; use path-selected language instead of stored grammar; or count an incremental
old-Tree seed as whole-file reuse. M1.8 reanchors are correlation only, so every M1.9 projection binds
the new `ProjectAnalysisId` and is rebuilt on every successor.

Agent assignments: `/root` integration and final verification; `core_surface_audit` analyzer ownership;
`contract_test_audit` metrics numerical contracts; `integration_surface_audit` orchestration and
compatibility boundary. All agent tracks are read-only.

#### Active M1.10 execution plan — downstream shared-analysis consumers

Active hypothesis: one planner-owned `ProjectAnalysis` plus explicit presentation/source maps can
serve graph extraction, proposal grouping, evaluation, LSP document analysis, MCP dispatch, and slim
reconstruction without any downstream parse, reread, or language-pack reselection.

CONVERGENCE: the terminal decision is whether every named consumer can operate from a retained
analysis and pinned bytes while preserving its current numerical/JSON/LSP contracts. One static
consumer guard plus a workspace parse-ledger matrix collapses the decision tree: pass means all named
surfaces are migrated; any remaining parse/read/reselection identifies the exact owning adapter that
must be redesigned, never a fallback to serial compatibility calls.

Current approach: migrate graph first because it still owns borrowed Tree nodes and path/`Lang` pack
selection. Add an owned graph projection over `Arc<ProjectAnalysis>`, port extraction to
`NodeId`/`NodeView`, and make `graph_paths` a shared-planner adapter. Then retain analysis and
presentation in `ScanContext` so protocol proposal grouping uses pinned text plus owned enclosing
regions instead of rereading and reparsing. Evaluator and MCP delegate through those projections.
LSP document state builds overlay successors and analyzes the retained revision; slim reconstruction
uses proposal-pinned sources and contexts, with file reads limited to explicit load/apply/recheck I/O.

Validation path: preserve the 21-file/74-symbol/197-edge/123-syntactic graph vector and all ambiguity
probes; preserve proposal IDs/targets and stale-input rejection; prove LSP UTF-16 diagnostics and
incremental edits over owned successor analyses; prove repeated graph/analyzer/proposal consumers do
not change parse ledgers; add production guards for `parse_source`, `SourceFile::read`, live source
`read_to_string`, and path/`Lang` pack selection in the migrated surfaces; finish with full workspace
tests, strict clippy, build, rustdoc, format, and whitespace gates.

Next checkpoint: graph has an owned projection and one-snapshot path adapter with exact numerical
parity, zero legacy parser calls, and no borrowed Tree-sitter nodes in graph production.

Graph checkpoint update (2026-07-13): DONE. `GraphProjection` binds config and presentation to the
owned `ProjectAnalysis`; `graph_paths` is now a one-planner/one-analysis compatibility adapter.
Extraction traverses a graph-local facade over `NodeId`/`NodeView`, exact stored grammar language,
and pinned text. Graph production no longer depends on `deslop-lang`, `ignore`, or `tree-sitter`, and
a static guard bans borrowed nodes, legacy parse/read, and path/`Lang` pack selection. The 24 graph
tests, strict graph/parse clippy, CLI graph-resolution probes, and the exact
21-file/74-symbol/197-edge/123-syntactic M0 vector pass. A repeated two-file owned projection retains
cold `1/1/1/0` ledgers and records zero legacy parser calls. The active next checkpoint is protocol
proposal grouping from analyzer-retained analysis/presentation/pinned text, without source rereads or
`SourceFile::enclosing_region_for_span` reparses.

Protocol/evaluator checkpoint update (2026-07-13): DONE. Analyzer `ScanContext` now retains its
`Arc<ProjectAnalysis>` and presentation map. Protocol proposal grouping constructs text helpers only
from pinned input contents, resolves enclosing rewrite regions through owned containment plus stored
`SyntaxAdapterFacts`, and derives proposal revision guards from pinned bytes; it performs no
post-scan source reread, provenance parse, or enclosing-region reparse. A static production guard and
repeat-proposal test record zero legacy parser calls. Evaluator batches every manifest case into one
analyzer snapshot rather than invoking a file compatibility scan per case; the quality baseline and
zero-legacy counter pass. Protocol 18/18, analyzer 67/67, evaluator 3/3, MCP 20/20, slim 22/22,
proposal CLI, M0 numeric, strict clippy, format, and whitespace checks pass. The active next checkpoint
is LSP document-state ownership and incremental successor analysis; MCP and slim already delegate to
the migrated protocol/analyzer surfaces, while their remaining reads are explicit config, JSONL,
apply, or stale-state recheck I/O rather than analysis inputs.

LSP/terminal checkpoint update (2026-07-13, superseded by the correction below): the first terminal
implementation retained one immutable analysis per `DocumentState`. Its single-document lifecycle
oracle passed, but startup negative memory correctly identified that separate dirty-document
snapshots can mix workspace revision authority.

LSP workspace correction (2026-07-13): DONE. `LspState` now owns one `ProjectAnalysis` and
presentation map for every open document. Open/change/close rebuild one exact-logical overlay
generation through the shared planner and an immutable successor; unchanged dirty documents are
ledger-reused, the changed document parses exactly once, every document logical path resolves in the
same analysis, and all open-document diagnostics refresh after workspace changes. Save with text
follows the same successor path; save without text reruns policy over the retained generation. The
LSP policy still disables project-boundary claims because open buffers are not a complete artifact
manifest. Single- and two-document lifecycle oracles prove revision identity, predecessor
immutability, exact parse/reuse counts, and zero legacy parser calls. The all-feature workspace gate
passes after the correction. M1.10 is complete on the stronger workspace ownership contract.

Negative-memory constraints: do not expose Tree-sitter nodes from `deslop-parse`; reuse M1.9 source
compatibility calls once per downstream rule; reread files after a projection exists; select adapters
from display paths; let proposal grouping reparse merely to find enclosing regions; or call live
external tools without a prepared revision-isolated plan.

Agent assignment: `/root` remains integration, implementation, and verification owner for this
checkpoint; prior M1.9 audit agents are complete and no files are concurrently edited.

#### Completed M1.11 execution plan — instrumentation and measured compaction

Confirmed hypothesis: one revision-owned measurement surface exposes parse ownership, deterministic
node order, cold/repeated/incremental latency, and retained memory without adding consumer-specific
instrumentation or perturbing projection identities. The first measured profile should decide which
listed M1 allocations are material enough to compact; unmeasured micro-optimization is out of scope.

CONVERGENCE: instrument once over a fixed multi-language cold/repeated/incremental matrix, then use
the captured counters and size/timing decomposition to reach a terminal decision for every listed
cost center. Structural invariant failure means fix ownership/order before measuring performance;
a dominant measured allocation or lookup means compact that representation and rerun the same
matrix; no material regression or hotspot means retain the simpler representation and record the
number. Do not branch into serial canary experiments or use wall time alone as correctness evidence.

Completed approach: inventoried existing parse ledgers, arenas, query indices, aggregation storage, and
incremental transition data; add a stable instrumentation report at the `ProjectAnalysis` boundary;
lock exact structural counters and deterministic node-order digests in normal tests; place latency
and retained-memory measurements in an explicit ignored probe with numerical output and tolerant
regression policy. Measure the TODO's field-path/revision-key repetition, allocating children/range
lookups, query preorder maps and retained strings, aggregation values, and successor transition
assembly before choosing compaction work.

Validation path: first prove one owner/one parser invocation per revision, dense deterministic node
order, identical repeated projections, and predecessor immutability. Then run the instrumented probe
on cold, repeated, and one-file incremental revisions, report exact node/file/byte/count and timing
values, and apply only optimizations justified by that decomposition. Finish with parse-focused tests
and strict clippy, then workspace-wide gates before checking M1.11.

Terminal outcome: the fixed matrix retains 3 files, 188 source bytes, 94 nodes, 91 child edges, and
one pinned node-order digest. Shared per-file revision keys and interned field paths reduce node-key
storage from 75,873 to 36,195 bytes. After adding compact 1,880-byte key and 1,504-byte query indices,
the visible retained lower bound is 61,900 bytes versus the 98,234-byte baseline, a 36,334-byte
(37.0%) reduction. `NodeView::children` and exact zero-width point results are allocation-free;
range/key lookup is logarithmic; query capture names share query-owned payloads; all-descendant
aggregation no longer retains a redundant declared projection; and update reports expose exact edit,
rebuild, successor assembly, and transition counts. Five ignored runs keep cold/repeated/incremental
wall time observational rather than authoritative.

Next checkpoint: execute M1.DoD on the gold fixture matrix, proving all scan/propose paths preserve
one parse owner per file revision, no borrowed-node lifetime leaks, and non-overlapping exclusive
metric ownership before beginning M2.

Negative-memory constraints: do not replace ledger evidence with global counters; expose parser or
borrowed-node internals; make timing a deterministic unit-test assertion; estimate retained memory
from source length alone; optimize an unmeasured representation; or allow instrumentation to enter
snapshot/projection identity.

Agent assignment: `/root` owns research, implementation, validation, and integration; no concurrent
file edits are assigned.

#### Completed M1.DoD execution plan — terminal owned-analysis proof

Confirmed hypothesis: the migrated public workflows converge on `ProjectAnalysis`; one joined
gold-matrix contract plus consumer-local ledger guards can prove the terminal M1 boundary without
adding another parser counter or duplicating every consumer implementation in a test harness.

CONVERGENCE: build the multi-language gold fixture matrix once, assert exact cold ownership and warm
reuse ledgers, run analyzer/metrics/graph projections repeatedly on the same immutable analysis, and
enumerate every exclusive byte/line owner. A ledger mismatch terminates in a workflow migration fix;
a byte or line visit other than one terminates in a metrics ownership fix; any public Tree-sitter
handle or serializable `NodeId` terminates in an API boundary fix. If all three pass, audit the named
CLI/evaluator/protocol/MCP/LSP/slim routes for delegation to those proven owners, run the unchanged M0
numeric gate, then close M1. Do not split this into per-language serial experiments.

Completed approach: protocol proposal batches retain their exact analysis and ledger; the
extend the existing metrics-private ownership oracle to the fixed multi-language matrix so every
source byte and physical line is visited exactly once despite nested regions; add a parse public-
surface guard for borrowed Tree-sitter handles and `NodeId` serialization; then combine those with
the existing analyzer, graph, evaluator, LSP, MCP, and slim zero-legacy/delegation tests.

Validation path: smallest parse/protocol/metrics tests first; strict affected-crate clippy; the M0
definition-of-done numeric snapshot; then all-feature workspace test/build/rustdoc/clippy/format/diff.
The checkpoint publishes exact file/node/byte/line/ledger/work-order counts and separates commands
run from static delegation evidence.

Terminal outcome: the joined five-language matrix locks 5 files, 1,651 bytes, 746 nodes, 700
gap-free exclusive regions, 21 analyzer findings, 17 metric regions, a 45-node/49-edge graph, and 9
work orders grouping 17 findings. Cold ownership is `5/5/5/0`; unchanged warm reuse is `5/5/0/5`
with all 746 transitions retained. Every disk source is read once, analyzer/metrics/graph share one
analysis and repeat byte-for-byte, proposal exposes its exact analysis, and the private metrics
oracle assigns all 1,651 bytes and 67 nonblank lines exactly once across 17 semantic owners. Static
and compile-fail guards keep borrowed Tree-sitter handles and serialized `NodeId`s out of the public
surface. Named evaluator/MCP/LSP/slim consumers pass their delegation, ledger, egress, stale-state,
and zero-legacy checks in the workspace suite.

Next checkpoint: begin M2.1 by versioning the language-adapter capability schema for S0-S4 while
preserving the now-locked M0 and M1 executable snapshots.

Negative-memory constraints: acceptance authority is the per-analysis `Arc<ParseLedger>`, never the
global/thread-local legacy counter; nested callable spans may overlap but exclusive metric ownership
must not; public `NodeView` borrows an analysis but never a Tree-sitter node; verifier stale-state
rereads are not scan/propose reparses; and delegated MCP/slim routes must not be mistaken for separate
parser implementations.

Agent assignment: `/root` owns the M1.DoD integration, implementation, and verification; no concurrent
file edits are assigned.

### M2 — Language-adapter contract

Implement capability manifests, grammar variants, query packs, canonical roles, operator/token policy,
parse-error policy, and golden fixture matrices for every supported language. Unsupported semantics become
machine-readable unknowns. This unlocks honest cross-language algorithms at `S0`/`S1`.

#### M2.1 terminal checkpoint — versioned total capability manifests

Active hypothesis: a single ordered capability catalog can version S0-S4 without prematurely claiming
tiers. Each manifest declares every catalog entry as provided, unsupported, or unknown with its evidence
authority; the highest complete tier is derived from the declarations and therefore cannot drift from
the granular facts.

CONVERGENCE: freeze one exact JSON vector covering every S0-S4 capability, validate totality,
uniqueness, schema/adapter-schema compatibility, and support/authority consistency, then bind that
manifest into `LanguageAdapterIdentity` and projection invalidation. A missing/duplicate declaration,
provided capability without authority, authority on unknown/unsupported capability, or claimed tier
with a lower-tier gap is a terminal schema failure. One manifest/schema change must change stored
adapter identity and derived projection identity while leaving raw snapshot/analysis identity stable,
as ADR 0001 requires. If these pass for all registered packs and custom test packs, M2.1 is done;
do not branch into role/query/operator implementation belonging to M2.2-M2.5.

Current approach: define serde-owned `SemanticTier`, granular `AdapterCapability`, support and
authority enums, declarations, and `LanguageAdapterCapabilityManifest` in `deslop-lang`. Make the
catalog exhaustive and let manifests compute `highest_complete_tier`. Require `LangPack` to return a
validated manifest. Production packs initially report only capabilities genuinely implemented by the
current raw syntax/region hooks; canonical roles remain unknown, so no pack falsely claims complete
S0 before M2.2. Store the exact manifest alongside name/schema in each snapshot adapter identity.

Validation path: lang schema truth table and pinned JSON; parse snapshot rejection for malformed
manifests; snapshot/projection identity invalidation when only a capability declaration changes;
registered-pack matrix; then affected tests/clippy and workspace-wide gates.

Next checkpoint: every registered adapter publishes one valid total manifest, exact wire schema and
catalog order are pinned, current complete-tier claims are numerically honest, and capability-only
changes invalidate derived identities.

Negative-memory constraints: do not encode TSX as a new public `Lang`; grammar dialect remains stored
grammar provenance. Do not put M2 roles into `NodeKey/1`. Do not use a default manifest that silently
upgrades third-party/test packs. Do not equate existing syntactic graph heuristics with S2/S3 semantic
authority, or syntax/parse success with verified change S4.

Terminal result: PASS. `deslop.language-adapter-capabilities/1` freezes 23 ordered capabilities with
exact S0-S4 counts `6/4/6/5/2`. Each declaration is provided, unsupported, or unknown; only provided
facts carry syntax, adapter, compiler, or runtime-verification authority. Validation rejects wrong
schema/adapter schema, missing or reordered catalog entries, and inconsistent authority. All seven
registered packs publish valid manifests and currently derive no complete tier because canonical
roles remain unknown. A capability-only adapter change preserves raw analysis identity and changes
the stored adapter/projection identity. Exact JSON, tier derivation, registry, snapshot rejection,
and identity tests plus every workspace gate pass.

Next checkpoint: begin M2.2 by defining a versioned canonical-role vocabulary that supplements rather
than replaces every raw grammar kind and field, and prove all supported adapters retain both views.

Negative-memory constraints carried forward: TSX remains grammar provenance rather than `Lang`; roles
must not enter `NodeKey/1`; syntactic graph heuristics remain below S2/S3 authority; and no pack may
claim canonical-role support until a total fixture-backed mapping exists.

Agent assignment: `/root` owns M2.1 integration and M2.2 continuation; no concurrent file edits are
assigned.

#### M2.2 terminal checkpoint — canonical roles beside raw grammar facts

Active hypothesis: a small composable role set can normalize portable syntactic categories without
erasing grammar-specific evidence if role facts are a versioned derived projection over the immutable
raw arena, not a mutation of `NodeKey/1` or `ProjectAnalysisId`.

CONVERGENCE: pin one exact role vocabulary/wire vector, exercise multi-role composition and strict
deserialize ordering, then build one custom adapter projection over a real retained Tree. Every role
fact must retain its `NodeId`, raw kind/id, raw grammar kind/id, raw parent field, and canonical set;
the projection must retain its owning analysis and derive identity through the stored adapter schema
and manifest. Unknown canonical-role capability must fail as unavailable rather than return an empty
authoritative mapping. If the custom projection aligns node-for-node with the raw arena, raw identity
and `NodeKey/1` remain unchanged, and workspace gates pass, M2.2 is done. Production mapping quality
and golden fixtures stay in M2.6-M2.10.

Current approach: define `deslop.canonical-roles/1`, the exact project/module/declaration/type/
callable/parameter/block/statement/expression/branch/loop/match/case/call/read/write/literal/comment/
import/export/error/generated/opaque-region catalog, and a sorted duplicate-free composable role set
in `deslop-lang`. Add a default-empty `LangPack` callback but gate public role projection execution on
the exact stored `CanonicalRoles` capability. Refactor the existing private Tree/raw-arena alignment
walk once so legacy syntax-hook facts and role facts share the same mismatch checks.

Validation path: exact role JSON and catalog-size test; malformed schema/order/duplicate rejection;
custom adapter node-for-node role/raw fact oracle including aliased grammar names and raw fields;
unknown-capability rejection for a production pack; projection ownership/identity and unchanged raw
analysis/`NodeKey/1` assertions; focused tests/clippy followed by workspace-wide gates.

Next checkpoint: M2.2 terminal proof with numerical node/role/raw-field counts and no production pack
claiming canonical-role authority ahead of its language fixture milestone.

Negative-memory constraints: canonical roles are derived and may be composed; never overwrite raw
kind/grammar-kind/field data, place roles in `NodeKey/1`, treat an empty role set as confirmed when the
capability is unknown, or implement query-pack/scope/control semantics assigned to M2.3 and later.

Terminal result: PASS. `deslop.canonical-roles/1` freezes 23 composable roles and a deterministic
schema-bearing set whose wire rejects wrong schemas, duplicates, and reordering. The capability-gated
`deslop.canonical-role-projection/1` retains its exact `Arc<ProjectAnalysis>` and pairs every projected
`NodeId` with raw visible kind/id, raw grammar kind/id, parent field, and canonical set. The fixed
custom Rust-grammar adapter fixture locks 32 nodes, 11 raw fields, and 22 role assignments, including
the visible `type_identifier` versus grammar `identifier` alias. Repeated projection is deterministic;
unknown production capability is typed unavailable. Raw analysis identity and `NodeKey/1` remain
unchanged, and all workspace gates pass.

Next checkpoint: begin M2.3 with versioned query-pack declarations and exact owned capture contracts
for declarations, references, scopes, control, comments, and opaque/generated code. Keep production
packs honest until each language fixture milestone supplies its actual queries.

Negative-memory constraints carried forward: role mapping remains a derived projection; an empty set
is not support when the capability is unavailable; raw grammar evidence is never normalized away;
query packs must not imply name resolution, CFG, generation provenance, or other higher-tier facts.

Agent assignment: `/root` owns M2.2 integration and M2.3 continuation; no concurrent file edits are
assigned.

#### M2.3 terminal checkpoint — total versioned query packs

Active hypothesis: adapter queries can be portable contract inputs without overstating semantics if
each required capture family is declared total, exact query source/capture metadata is stored in the
adapter identity, and compiled captures remain raw syntactic evidence with explicit authority.

CONVERGENCE: freeze one six-family query-pack wire vector for declarations, references, scopes,
control, comments, and opaque/generated code; reject missing/order/support/source/capture errors;
compile one fully provided custom pack against its exact stored grammar; verify declared capture names
equal compiled Tree-sitter names and execute every family over one fixture with pinned counts. A
query-only adapter change must preserve raw analysis identity and invalidate derived projection
identity. If unavailable production entries remain explicit and all workspace gates pass, M2.3 is
done; no capture may be promoted to resolved binding, CFG edge, or generated-code provenance.

Current approach: define `deslop.language-query-pack/1`, six ordered families, total declarations,
capture metadata with canonical role sets, and provided/unsupported/unknown validation in
`deslop-lang`. Require `LangPack` to return an exact pack, defaulting all entries to unknown. Store and
validate it in `LanguageAdapterIdentity`. Add an analysis-owning compiled query-pack projection in
`deslop-parse` that reuses `SyntaxQuery`, checks exact capture contracts, and retains unavailable
declarations rather than suppressing them.

Validation path: pinned JSON and malformed truth table; registry-wide totality; adapter-schema
mismatch rejection; custom six-query compile/capture/result oracle; query-only identity invalidation;
focused tests/clippy followed by full workspace gates.

Next checkpoint: all six families are explicit for every adapter, the custom pack compiles and runs
node-owned captures without reparsing, and production packs remain honest unknowns until M2.6-M2.10.

Negative-memory constraints: query captures are syntactic facts, not S2/S3 conclusions; absence of a
capture is not proof of semantic absence; opaque/generated is a capture category rather than verified
provenance; exact grammar dialect and stored pack must drive compilation; no path-based reselection.

Terminal result: PASS. `deslop.language-query-pack/1` freezes six ordered total families with explicit
support, authority, exact source, unique capture names, and canonical role metadata. Snapshot adapter
identity stores the validated exact pack using length-framed identity parts and rejects adapter-schema
mismatch. `deslop.language-query-projection/1` retains its analysis, compiles only provided families
against the stored grammar, preserves all unavailable declarations, and rejects declared/compiled
capture drift. The fixed custom pack executes `[1,1,2,1,1,2]` captures by family (8 total) without
changing its one-parse ledger. Query-only change preserves raw analysis identity and invalidates the
projection; all seven production registry packs remain six-for-six unknown. All workspace gates pass.

Next checkpoint: begin M2.4 with a versioned operator/token classification and lexical-policy schema.
Use exact owned token regions and stored dialect packs; do not reinterpret current Halstead token lists
as a complete lexical contract.

Negative-memory constraints carried forward: captures are syntactic evidence, not resolution or CFG;
capture absence proves nothing semantic; opaque/generated capture labels are not verified provenance;
query source/captures belong in exact derived identity and must compile against stored grammar only.

Agent assignment: `/root` owns M2.3 integration and M2.4 continuation; no concurrent file edits are
assigned.

#### Terminal M2.4 execution plan — declarative lexical classification

Resolved hypothesis: exact token classification is language-specific and deterministic without a
second tokenizer when adapters publish an ordered, versioned rule table over raw grammar kind plus
optional exact token text, with explicit identifier/comment policy and a total fallback. Explicitly
classified composite grammar nodes own their complete spans; unclaimed composites traverse to leaves.

CONVERGENCE: freeze token/operator vocabularies and one strict lexical-policy wire vector; reject
provided policies without authority, identifier policy, ordered unique rules, or a terminal wildcard
fallback; classify a custom retained grammar's positive-width leaves and pin class/operator counts.
Policy-only changes must preserve raw analysis identity and invalidate derived identity. If whitespace
gaps remain owned trivia rather than invented tokens, every classified fact retains raw kind/text, and
production packs remain unknown pending M2.6-M2.10, M2.4 is done.

Implemented approach: define `deslop.language-lexical-policy/1`, token and operator classes, exact ordered
kind/text rules, case/Unicode identifier behavior, and line/block comment delimiters in `deslop-lang`.
Store the validated policy in adapter identity. Project non-overlapping positive-width Tree-sitter
token owners through the stored rule table while retaining the analysis; do not reuse the metrics text
tokenizer or infer multi-character operators independently of the grammar.

Validation result: exact JSON/malformed truth table, registry totality, adapter-schema mismatch,
custom raw-text/classification oracle including full composite comments, literals, Unicode identifiers,
and multi-character operators, no-reparse and identity assertions, and all workspace gates pass.

Negative-memory constraints: current Halstead operator arrays are partial metric seeds, not lexical
authority; trivia gaps are not tokens; comments come from grammar ownership/policy, not substring
search; rules classify syntax only and do not imply effects, precedence, or evaluation order.

Agent assignment: `/root` owns M2.4 schema, integration, and verification; no concurrent file edits
are assigned.

Terminal checkpoint (2026-07-14T01:03:19+02:00): M2.4 is complete. The versioned policy schema,
stable enum identity framing, strict totality/shadow rejection, stored adapter identity, public
projection, policy-only invalidation/mismatch checks, and fixed 26-fact numerical fixture all pass.
The initially attempted leaf-only boundary was invalidated because Rust comments are composite CST
nodes; explicit composite ownership with descendant suppression now preserves exact full comments and
non-overlapping spans. Next checkpoint: begin M2.5 policy contracts for parse errors, unsupported
constructs, macros, generated code, and dialects.

#### Terminal M2.5 execution plan — construct, recovery, and dialect policy

Resolved hypothesis: parse recovery, unsupported/opaque regions, macro boundaries, generated markers,
and exact dialect support can be represented as one versioned adapter policy without treating query
captures as semantic provenance or reconstructing grammar selection from paths.

CONVERGENCE: freeze one exact aggregate wire vector with independently explicit support/authority for
parse recovery, unsupported constructs, macros, generated code, and dialect variants. Ordered exact
raw-kind/optional-text rules must reject duplicates and shadowing; unavailable sections must be empty;
provided dialects must exactly match the stored grammar dialect/id/version. Project a fixed malformed
custom fixture and numerically lock error/missing, unsupported, macro, and generated facts, exact
authority/handling, policy-only invalidation, dialect mismatch rejection, and no reparse. If production
packs stay honest unknown pending M2.6-M2.10 and every workspace gate passes, M2.5 is done.

Implemented approach: add `deslop.language-construct-policy/1` in `deslop-lang`, with a fail-closed parse-
recovery declaration, three ordered construct-rule sections, and exact dialect declarations. Store and
validate the policy in `LanguageAdapterIdentity`. Add an analysis-retaining projection over existing
arena nodes: error/missing facts come only from stored grammar flags; construct facts come only from
adapter rules; dialect provenance comes only from the stored `GrammarSelection`. Region facts may nest
but never imply expansion, generated origin beyond their declared evidence, or semantic support.

Validation result: exact JSON and malformed truth table, registry totality, adapter-schema and exact-
dialect mismatch; malformed custom CST oracle with macro/generated/unsupported regions; deterministic
repeat, no-reparse, policy-only identity checks, affected strict checks, and full workspace gates pass.

Negative-memory constraints: M2.3 opaque/generated captures are categories, not verified provenance;
`has_error` is not permission to silently discard recovery nodes; macro invocation syntax does not
mean expansion is available; path suffixes must not reconstruct dialect; unavailable policy sections
cannot prove semantic absence; M2.4 composite token ownership does not apply to nesting region facts.

Agent assignment: `/root` owns M2.5 schema, projection, integration, and verification; no concurrent
file edits are assigned.

Terminal checkpoint (2026-07-14T01:14:19+02:00): M2.5 is complete. The fixed malformed fixture emits
exactly four facts in source order: generated `attribute_item`, opaque `unsafe_block`, opaque
`macro_invocation`, and syntax-authority file-incomplete `ERROR`. Exact stored dialect provenance is
provided, mismatched claimed dialect fails typed, unknown policy emits no construct/recovery facts,
and each analysis parses once. All workspace gates pass. Next checkpoint: M2.6 Rust adapter and golden
fixtures using the now-frozen M2.1-M2.5 contracts.

#### Terminal M2.6 execution plan — Rust production adapter and goldens

Resolved hypothesis: the existing Rust region/metrics hooks plus Tree-sitter Rust honestly provide
complete S1 when the production pack supplies exact canonical roles, all six query families, lexical
classification, recovery/construct policy, and a frozen valid/malformed golden matrix; S2+ semantics
remain unknown. S1, rather than only S0, is derived because the pre-existing region, local-metrics,
clone-normalization, and syntactic-recipe declarations are already provided and workspace-verified.

CONVERGENCE: one valid Rust fixture must exercise declarations, references, nested scopes/control,
both comment forms, Unicode identifiers, literals/operators, macros, generated markers, and unsafe
opaque regions. One malformed fixture must lock exact recovery facts. Run every M2.2-M2.5 projection
from one retained analysis, pin role/query/token/construct counts and exact dialect, and prove one parse
per file. If the manifest derives exactly S0, no query/role/policy overclaims expansion, generated
origin, binding, CFG, effects, or types, production registry truth is updated, and all workspace gates
pass, M2.6 is done.

Implemented approach: upgraded only `RustPack` in `deslop-lang`. Canonical roles remain composable raw-kind
annotations; query captures remain syntactic; macro invocation/definition and unsafe blocks are opaque;
generated facts require exact marker attributes; lexical rules use grammar-owned kinds/text with a
terminal other fallback; dialect declaration exactly matches `rust/tree-sitter-rust/0.24.2`. Add
`tests/fixtures/rust/adapter_matrix.rs` and `malformed.rs`, plus a production-pack integration oracle in
`deslop-parse`.

Validation result: focused policy/query compilation, measured golden counts, malformed recovery oracle,
manifest S0 assertion, projection identity/ownership/no-reparse checks, affected strict checks, then
all workspace gates pass. The final manifest assertion is S1, as derived by the total catalog.

Negative-memory constraints: macro CST is not expanded semantics; generated query categories are not
provenance, so production generated facts require exact adapter markers; canonical read/write roles do
not resolve names; query control captures do not establish CFG; lexical operators do not establish
precedence/effects; do not promote existing syntactic graph heuristics or Clippy availability into S2+
or compiler authority.

Agent assignment: `/root` owns Rust policy, fixtures, integration, and verification; no concurrent
file edits are assigned.

Current checkpoint (2026-07-14T01:16:00+02:00): existing Rust grammar, region, metrics, analyzer, and
fixture surfaces audited; production M2.2-M2.5 policy implementation and goldens remain pending. One
broad trait-method patch was invalidated because its repeated `capability_manifest` context selected
the earlier Clojure implementation. Clojure and Julia were restored exactly and `cargo check -p
deslop-lang` passes; future Rust edits must anchor on `impl LangPack for RustPack` and use smaller
unique hunks before any fixture work.

Terminal checkpoint (2026-07-14T01:32:50+02:00): M2.6 is complete. The valid golden locks 161 nodes,
110 lexical token owners, 78 canonical-role assignments across 17 role categories, query captures
`[5,2,5,1,2,3]`, and six generated/macro/unsafe construct facts. The malformed golden locks one
file-incomplete `ERROR` fact for `=`. Both files parse once. The lexical schema was repaired so an
exact `*` grammar token does not collide with the terminal wildcard. All workspace gates pass. Next:
M2.7 JavaScript, TypeScript, and TSX production policies and goldens.

#### Active M2.7 execution plan — JavaScript, TypeScript, and TSX dialect goldens

Active hypothesis: JavaScript and TypeScript can share stable role/lexical/recovery helpers while
retaining separate query packs and exact dialect declarations for JavaScript/JSX and
TypeScript/TSX. Completing canonical roles should derive S1 without promoting typed syntax to type
authority or adding a public TSX language.

CONVERGENCE: compile every provided query family independently against JavaScript, TypeScript, and
TSX stored grammars; run all four M2 projections over fixed `.js`, `.ts`, and `.tsx` goldens plus
malformed typed fixtures; pin per-dialect role/token/query/construct counts, exact grammar identity,
Unicode/comments/operators, and one parse per file. Generated facts require exact markers; macros are
explicitly unsupported; S2+ stays unknown. If all three dialects derive S1 and every workspace gate
passes, M2.7 is done.

Current approach: add JS-family helpers beside the production packs, but keep grammar-specific query
builders where node catalogs diverge. Canonical roles remain raw-kind syntactic annotations. Lexical
rules cover ECMAScript/TypeScript token kinds with exact `*` handling and total fallback. Recovery is
file-incomplete; `with_statement` is an opaque unsupported construct; `/* @generated */` and exact
`@generated` decorators are generated markers; macro policy is explicitly unsupported. JavaScript
declares javascript/jsx over tree-sitter-javascript 0.25.0; TypeScript declares typescript/tsx over
their distinct tree-sitter-typescript 0.23.2 grammar ids.

Validation path: per-pack schema validation and S1 derivation; query compilation for all stored
dialects; numerical valid/malformed matrix; ownership/no-reparse assertions; affected strict checks;
full workspace gates.

Negative-memory constraints: TypeScript must never fall back to the JavaScript grammar; TSX remains a
stored dialect, not `Lang::Tsx`; syntactic type annotations do not grant compiler/type evidence;
decorators/comments count as generated only when exact policy markers match; dynamic calls, optional
chaining, JSX, and decorators do not grant name resolution, CFG, effects, or expansion authority;
repeated LangPack methods require implementation-specific patch anchors.

Agent assignment: `/root` owns M2.7 shared policy, dialect fixtures, integration, and verification; no
concurrent file edits are assigned.

Current checkpoint (2026-07-14T01:34:31+02:00): M0.4 grammar split and existing typed/TSX/JSX fixtures
audited. A shared composable canonical-role mapper is now wired to both production packs, and each
manifest derives S1 while keeping S2+ unknown; `cargo check -p deslop-lang` passes. Query, lexical,
construct/dialect policies and the numerical matrix remain pending.

Terminal checkpoint (2026-07-14T01:44:25+02:00): M2.7 is complete. JavaScript, TypeScript, and TSX
compile all six query families against their exact stored grammars and expose S1 role, lexical,
recovery, construct, and dialect policy without S2+ promotion. Valid goldens lock role/token totals
JS 61/42, TS 143/90, TSX 107/68; query vectors `[1,1,3,0,2,1]`, `[4,2,3,0,1,0]`, and
`[3,0,2,0,1,0]`; exact generated and opaque `with_statement` facts; and distinct JavaScript,
TypeScript, and TSX grammar identities. Malformed TS and TSX lock their exact `ERROR` facts. Every
file parses once and all workspace gates pass. Next: M2.8 Python production policy and goldens.

#### Active M2.8 execution plan — Python production policy and goldens

Active hypothesis: the single stored Python 0.25.0 grammar can supply the same complete S1 contract
without dialect branching. Canonical roles and query captures must agree for decorated, async, nested,
comprehension, and pattern-matching syntax; no syntactic type annotation grants compiler/type authority.

CONVERGENCE: compile all six query families against the stored Python grammar; run canonical-role,
query, lexical, and construct projections over one fixed valid golden plus a malformed golden; pin the
complete numerical vectors, exact `python/tree-sitter-python/0.25.0` identity, Unicode/comments/
operators, legacy unsupported constructs, exact generated markers, and one parse per file. If the
valid fixture derives S1, malformed recovery is exact, no query capture exceeds its role contract, and
all workspace gates pass, M2.8 is done.

Current approach: implement production policy directly on `PythonPack`. Treat module, function/class/
decorated definitions, parameters, blocks, statements, control flow, calls, writes, reads, literals,
comments, and errors as raw-kind syntactic roles. Query declarations/references/scopes/control/comments
and opaque legacy constructs independently. Use a total case-sensitive Unicode-aware lexical policy
with exact `*` handling. Recovery is file-incomplete; legacy Python 2 `exec_statement` and
`print_statement` are opaque unsupported constructs; macros are unsupported; exact `# @generated`
comments and `@generated` decorators are generated markers. Declare only the stored Python grammar.

Validation path: per-pack schema/S1 checks; actual grammar query compilation; numerical valid and
malformed projections; query/canonical-role semantic audit; ownership/no-reparse assertions; affected
strict checks; full workspace gates.

Negative-memory constraints: repeated `LangPack` methods require `impl LangPack for PythonPack`
anchors; query compilation alone does not prove capture-role consistency; exact-text `*` must not
collide with the terminal wildcard; Python annotations and pattern syntax remain syntactic and do not
grant name resolution, CFG, effects, macro/AST rewriting, or compiler/type authority. Hindsight search
mode `keyword` is invalid on the shared server; omit mode for targeted retrieval.

Agent assignment: `/root` owns M2.8 policy, fixtures, integration, and verification; no concurrent
file edits are assigned.

Current checkpoint (2026-07-14T01:47:47+02:00): audited the Python pack, exact grammar descriptor,
existing decorated/async/nested behavioral fixture, and grammar node catalog. Production M2 policy is
still unknown. The catalog contains explicit legacy `exec_statement` and `print_statement` nodes, so
the unsupported boundary can be fixture-tested without inventing grammar evidence.

Terminal checkpoint (2026-07-14T01:58:48+02:00): M2.8 is complete. Python now derives S1 with all
six queries compiled against `tree-sitter-python` 0.25.0, total lexical ownership, exact recovery/
construct/dialect policy, and no S2+ promotion. The valid golden locks 127 CST facts, 75 token owners,
108 role assignments across 21 roles, query vector `[4,1,8,3,2,2]`, two exact generated facts, and
two opaque legacy constructs. The malformed golden locks one `ERROR` for `return value +`. An
executable audit proves every actual query capture carries all declared canonical roles. Exact-text
keyword rules prevent named `await`/`lambda`/`type`/`yield` composites from suppressing operands.
Every file parses once and all workspace gates pass. Next: M2.9 Clojure production policy/goldens.

#### Active M2.9 execution plan — Clojure reader/macro policy and goldens

Active hypothesis: Clojure can complete S1 only if list roles and queries use evaluated list-head
context while reader forms remain explicit opaque syntax. Raw `list_lit` membership alone cannot
distinguish declarations, calls, scopes, branches, or macro templates, and syntax-quoted/quoted forms
must never fabricate runtime control flow.

CONVERGENCE: compile all six query families, including head-text predicates, against the stored
Clojure grammar; run all four projections over a fixed reader/macro/control golden plus malformed
input; pin numerical vectors, exact `clojure/tree-sitter-clojure/0.1.0` identity, Unicode/comments/
symbol operators, reader-macro and generated facts, query-to-role consistency, and one parse per file.
If evaluated forms derive S1 without assigning runtime roles inside quoted/discarded templates, every
capture honors its role contract, and all workspace gates pass, M2.9 is done.

Current approach: implement contextual canonical roles from `node_head_token` plus
`clojure_form_is_evaluated`; treat ordinary evaluated lists as syntactic calls, known defining/scope/
control heads specially, and symbols/literals/reader data by their exact raw kinds. Query predicates
will retain helper head captures with honest read roles. Lexical policy classifies exact operator text
on `sym_name` before identifiers and avoids claiming composite symbol nodes. Recovery is
file-incomplete; `#=` evaluation is opaque unsupported syntax; explicit reader forms are opaque macro
syntax; exact `;; @generated` comments and `^:generated` metadata are generated markers. Declare only
the stored Clojure grammar; no macroexpansion or active reader-conditional branch authority is added.

Validation path: schema/S1 checks; actual predicate query compilation/execution; numerical valid and
malformed projections; quoted/discarded non-leakage and query-role audit; ownership/no-reparse; affected
strict checks; full workspace gates.

Negative-memory constraints: uniform `list_lit` nodes require list-head and evaluated-reader context;
syntax-quoted templates are data except explicit unquotes; reader conditionals have no active-platform
selection authority; query compilation alone does not prove capture-role consistency; composite raw
kinds must not suppress token children; S1 grants no resolution, macroexpansion, CFG, effects, or
compiler/clj-kondo authority.

Agent assignment: `/root` owns M2.9 policy, fixtures, integration, and verification; no concurrent
file edits are assigned.

Current checkpoint (2026-07-14T02:00:00+02:00): audited the production pack, existing reader/macro/
control fixture, durable Clojure complexity constraint, and the complete grammar catalog. Production
M2 policy remains unknown. The grammar exposes only uniform data/reader node kinds plus `list_lit`, so
the implementation must be contextual rather than raw-kind-only.

Terminal checkpoint (2026-07-14T02:09:27+02:00): M2.9 is complete at the honest query boundary.
Clojure derives S1 with evaluated list-head roles, 160 CST facts / 183 assignments across 14 roles,
90 token owners, exact reader/recovery/generated/dialect policy, and no S2+ promotion. Safe provided
queries yield `[0,0,1,0,2,7]`; declaration/reference/control remain unknown because the stored query
contract cannot exclude arbitrary quoted/syntax-quoted ancestors. The golden proves a live `if` is a
branch while quoted `if` is neither branch nor call, locks two generated / six reader-macro / one
unsupported `#=` facts, and locks one malformed whole-file `ERROR`. Grammar-field head extraction
repairs metadata-prefixed definitions in canonical and existing metric/region hooks. Every file parses
once and all workspace gates pass. Deferred boundary: ancestry-aware contextual query filtering belongs
in M2.11 or a versioned query schema; it must not be approximated by leaking quoted forms. Next: M2.10
Julia production policy/goldens.

#### Active M2.10 execution plan — Julia macro/quote policy and goldens

Active hypothesis: Julia’s typed grammar nodes can provide all six S1 query families directly while
keeping macro calls/definitions and quoted ASTs opaque. Syntax annotations and external StaticLint/JET
availability do not add compiler/type authority to the adapter.

CONVERGENCE: compile all six query families against `tree-sitter-julia` 0.23.1; run all four retained
projections over fixed valid/malformed goldens; pin numerical vectors, exact dialect, Unicode/comments/
operators/interpolation ownership, macro/quote/generated facts, query-role consistency, and one parse
per file. If Julia derives S1, macros/quotes stay opaque, malformed recovery is exact, and all workspace
gates pass, M2.10 is done.

Current approach: classify source/module/function/type/import/export/parameter/block/control/call/
write/read/literal/comment/error roles by raw grammar kind. Provide direct grammar queries for all six
families. Build a total Unicode-aware lexical policy using exact `operator` text before identifier/
literal/keyword/delimiter rules, while leaving interpolated string composites unclaimed so embedded
expressions retain token ownership. Recovery is file-incomplete; quote expressions/statements are
opaque unsupported constructs; macro definitions/calls are opaque macro facts; exact `# @generated`
comments and `@generated` macro calls are generated markers. Declare only Julia 0.23.1 and retain S2+
unknown.

Validation path: schema/S1 checks; actual query compilation; numerical valid/malformed matrix; macro/
quote and interpolation spot checks; query-role audit; ownership/no-reparse; affected strict checks;
full workspace gates.

Negative-memory constraints: query compilation alone does not prove capture-role agreement; composite
string ownership must not suppress interpolation; repeated `LangPack` methods require Julia-specific
anchors; exact operators must precede raw-kind fallback; macro/quote syntax grants no expansion,
resolution, CFG, effects, compiler, StaticLint, or JET authority.

Agent assignment: `/root` owns M2.10 policy, fixtures, integration, and verification; no concurrent
file edits are assigned.

Current checkpoint (2026-07-14T02:10:50+02:00): audited `JuliaPack`, its exact descriptor, existing
region/analyzer integration, and the installed grammar catalog. Production M2 policy remains unknown;
the grammar provides direct definitions, calls, control, comments, macro, quote, and recovery kinds,
so no Clojure-style contextual query downgrade is expected.

Terminal checkpoint (2026-07-14T02:22:20+02:00): M2.10 is complete. Julia derives S1 with all six
queries compiled against `tree-sitter-julia` 0.23.1, exact role/lexical/recovery/construct/dialect
policy, and no S2+ or external-analyzer promotion. The valid golden locks 95 CST facts, 61 token
owners, 94 role assignments across 18 roles, query vector `[2,4,2,2,3,3]`, two generated / two macro /
one opaque quote facts, and independent interpolation identifiers. The malformed golden locks one
whole-file `ERROR`. Signature-only argument lists are parameters; call arguments are not. Named
assignment operators classify exactly. Every query capture carries its declared roles, every file
parses once, and all workspace gates pass. Next: M2.11 cross-adapter construct and capability-leakage
matrix.

#### Active M2.11 execution plan — cross-adapter construct and capability-leakage matrix

Active hypothesis: each production adapter is locally correct, but only a registry-wide retained-analysis
oracle can prove that dialect, construct, recovery, query, and semantic-tier authority does not leak from
one language or dialect into another.

CONVERGENCE: build one table-driven snapshot spanning Rust, JavaScript, TypeScript, TSX, Python,
Clojure, and Julia valid/malformed sources. In one run, assert exact grammar dialect triples, the
adapter-specific support matrix for unsupported constructs/macros/generated code and query families,
the absence of near-marker facts, exact malformed recovery authority/handling, no fabricated
constructs from malformed input, no S2-S4 capability promotion, and one parse per source. A single
passing oracle plus the existing language goldens and full workspace gates terminates M2.11.

Current approach: reuse the frozen production fixtures and public projection APIs inside a table-driven
`deslop-parse` test. Each row owns its exact path, expected dialect, construct-section support,
construct counts, query-family support, and malformed recovery facts. Add explicit negative probes for
Clojure's quoted contextual forms, unsupported macro policies in ECMAScript/Python, exact generated
markers versus near markers, and syntax-only malformed errors. Do not add semantic capability or alter
production policies merely to make the matrix uniform.

Validation path: run the new focused cross-adapter test first; then affected crate tests and strict
clippy; finally workspace test/build/doc/clippy/fmt/diff gates. Audit M2.DoD separately after M2.11;
do not infer it from matrix success.

Next checkpoint: the single matrix passes numerically for all seven grammar dialects without changing
production authority, and all workspace gates pass.

Negative-memory constraints: public `Lang` remains a language family while grammar dialect remains
path-selected provenance; query compilation is insufficient without capture-role checks; Clojure
contextual declaration/reference/control stays Unknown until a versioned ancestry filter exists;
unsupported/unknown sections must carry no payload; generated markers require exact text; parse errors
are syntax facts and cannot fabricate constructs or grant higher-tier authority.

Agent assignment: `/root` owns the M2.11 oracle, integration, and verification; no concurrent file
edits are assigned.

Current checkpoint (2026-07-14T02:30:00+02:00): M2.10 terminal memory is durable and M2.11 has a clean
`jj` working change. The production goldens and policy APIs provide all inputs needed for the convergent
matrix; no production implementation change is expected unless the matrix exposes a real leak.

Terminal checkpoint (2026-07-14T02:31:00+02:00): M2.11 is complete. One 21-source snapshot covers all
seven production grammar dialects with valid, malformed, and near-generated-marker inputs. The matrix
pins each exact dialect triple, construct-section and query-family support/payload, construct counts and
generated text sets, full S0/S1 manifests, absent S2-S4 authority, file-incomplete syntax recovery,
Clojure quoted-control non-leakage, and unchanged one-parse instrumentation after every projection.
Malformed files emit only their exact syntax-authority `ERROR`; near markers emit no generated facts.
No production adapter policy changed, and all workspace gates pass. Next: audit M2.DoD independently
against every emitted fact/projection and confirmed-output tier boundary.

#### Active M2.DoD execution plan — joined adapter provenance and tier ceiling

Active hypothesis: M2 can close without another production policy change if one joined integration
oracle proves the complete owner chain from every role/token/construct/query fact through its retained
projection, exact node, stored adapter identity, grammar dialect/version, capability/policy declaration,
and analysis identity—and independently proves downstream output never claims unresolved S2/S3/S4
authority.

CONVERGENCE: build one seven-dialect production snapshot and evaluate all four adapter projection
families. For every emitted role/token/construct/query capture, resolve its node in the same analysis,
compare raw grammar evidence, verify the exact stored policy/capability authority and grammar-bound
query identity, and lock aggregate counts. Then run analyzer/metrics/graph over that same analysis:
all reports must be complete, AnalyzerConfirmed findings must have separate recorded external authority
(none are expected in the closed fixture), and no non-containment graph edge may be Resolved while
NameResolution/CallGraph remain Unknown. Exact one-parse ownership and full gates terminate M2.DoD.

Current approach: add `crates/deslop-cli/tests/m2_definition_of_done.rs`, reusing the seven production
adapter fixtures and public `deslop-parse` projection APIs. Treat projection retention plus stored
identity as the declared provenance chain; do not duplicate adapter/version strings into every node
record. Assert versioned projection/policy schemas, exact grammar dialects, S1 manifests, raw node
agreement, construct/query declaration authority, capture-role agreement, and downstream tier ceiling.

Validation path: focused M2 DoD integration test; affected CLI/parse/analyzer/metrics/graph tests and
strict clippy; unchanged M0/M1 DoD tests; then full workspace test/build/doc/clippy/fmt/diff gates.

Next checkpoint: one numerical joined proof covers all seven dialects and all emitted fact families,
with no higher-tier confirmed output and no reparse.

Negative-memory constraints: a projection hash alone is not an auditable declaration, so the test must
walk the stored identity/policy chain; query captures remain syntax evidence; graph containment may be
structurally Resolved at S1 but non-containment resolution requires unavailable S2/S3 authority;
AnalyzerConfirmed is permissible only with separately recorded external capability, never inferred
from a syntax adapter; TypeScript and TSX dialect provenance remains distinct.

Agent assignment: `/root` owns the M2.DoD audit, integration test, and verification; no concurrent file
edits are assigned.

Current checkpoint (2026-07-14T02:35:00+02:00): read-only live probes over the seven fixtures report a
complete 15-symbol/42-edge graph with 27 syntactic edges and zero resolved non-containment edges; scan
reports four findings and zero AnalyzerConfirmed findings. The stored projection APIs expose enough
identity and policy data for the joined proof; implementation remains unverified.

Terminal checkpoint (2026-07-14T02:40:16+02:00): M2.DoD and M2 are complete. The joined public-surface
oracle binds all 854 canonical facts / 640 role assignments, 536 lexical facts, 28 construct facts,
and 88 query captures to their retained projection, exact raw node, stored adapter schema `/2`, grammar
dialect/version, and capability or policy authority. It found a real query-role mismatch: Rust
`scoped_identifier`/`field_expression` and ECMAScript `member_expression` call-function captures lacked
the declared Expression/Read roles. The root fix assigns those roles only in the exact
`call_expression.function` context and bumps the adapter contract version. Analyzer emits four findings
and zero AnalyzerConfirmed claims; metrics emits 15 regions; the 15-symbol/42-edge graph has 27
non-containment edges and zero resolved non-containment claims while every adapter remains S1 with
S2-S4 Unknown. All seven files parse once, unchanged M0/M1 DoD gates pass, and every workspace gate
passes. Next: M3.1 scope/resolution ADR.

### M3 — Scope and project-name graph

Add lexical scopes, bindings, references, imports/exports, ambiguity, and resolution provenance; then link
files/modules/packages and optional compiler/LSP facts. Gate on duplicate-name, shadowing, aliasing, and
incremental-file fixtures before any semantic refactor uses `resolved` edges.

#### Active M3.1 execution plan — scope and resolution authority ADR

Active hypothesis: M3 can avoid another false-resolution cycle only if the data model, resolution-path
semantics, ambiguity rules, build-context identity, and evidence-authority precedence are frozen before
implementation. A globally unique spelling must never be sufficient evidence.

CONVERGENCE: write ADR 0002 with one complete decision table covering scope/name/reference identities,
namespace separation, lexical and import path traversal, visibility/shadowing, all-candidate retention,
unique/ambiguous/unresolved versus incomplete outcomes, dynamic/opaque constructs, build-target context,
compiler/runtime evidence conflicts, and incremental invalidation. Include executable acceptance
requirements that map directly to M3.2-M3.8. If every current graph/2 shortcut is explicitly classified
as syntactic-only and the ADR leaves no authority-precedence branch undefined, M3.1 is done.

Current approach: define versioned conceptual `ScopeGraph/1` and `ResolutionProjection/1` contracts over
the retained M2 facts. Resolution paths preserve every traversed lexical/import/export/alias/glob/package
edge, candidate endpoint, rank, viability, rejection reason, authority, and source fact. Only complete S2
name-resolution coverage with one highest-precedence endpoint may be Unique; incomplete coverage is
Unknown, never Unresolved. Compiler evidence outranks adapter resolution for its exact build context;
runtime observations remain observed dynamic edges rather than overwriting static binding. Current
graph/2 remains below this contract.

Validation path: review ADR structure against ADR 0001, every M3 TODO item, M2 capability tiers, and
current graph failure modes; run Markdown/whitespace/link/path checks and full workspace gates because
the ADR becomes normative implementation input.

Next checkpoint: accepted ADR 0002 with explicit schemas, invariants, outcome/authority tables,
incremental invalidation, rejected alternatives, rollout, and numerical gold-gate requirements.

Negative-memory constraints: never resolve a bare name from repository-global uniqueness; never discard
ambiguous candidate paths or choose first/sorted winner; absence is Unresolved only under complete
coverage; syntax query captures are not bindings; TypeScript/TSX dialect identity remains stored; runtime
observation is not universal static proof; compiler/LSP facts must bind exact build context/version.

Agent assignment: `/root` owns ADR 0002, M3.1 integration, and verification; no concurrent file edits
are assigned.

Current checkpoint (2026-07-14T02:43:00+02:00): ADR 0001 style and current graph/2 routing are audited.
The existing graph intentionally labels non-containment edges Syntactic/Ambiguous, drops ambiguous
candidate lists, and uses heuristic module/name keys; ADR 0002 must supersede these surfaces before M3
can promote any capability.

Terminal checkpoint (2026-07-14T02:46:28+02:00): M3.1 is complete. Accepted ADR 0002 freezes
`deslop.scope-graph/1` and `deslop.resolution/1`, exact build-context identity, the scope/declaration/
definition/binding/reference/import/export fact model, adapter-owned namespaces and lookup precedence,
complete viable/rejected resolution paths, separate coverage and five terminal outcomes, evidence
authority/conflict rules, module/re-export stitching, incremental invalidation, consumer gates, twelve
executable verification requirements, rejected shortcuts, and M3 rollout. It explicitly keeps graph/2
non-containment edges syntactic. The 355-line/2,736-word structural contract check and all workspace
test/build/doc/clippy/fmt/diff gates pass. Next: M3.2 core scope-graph fact schemas and ownership.

#### Active M3.2 execution plan — owned scope-graph facts and identities

Active hypothesis: M3.2 is complete when one immutable projection can retain every structural name
fact required by ADR 0002, prove that its process-local IDs belong to the exact `ProjectAnalysis`, and
emit strict revision/build-context-bound wire identities without claiming that any production adapter
can resolve names.

Current approach: add the versioned `deslop.scope-graph/1` fact contract at the parse/owned-analysis
boundary. Separate dense analysis-owned handles from serializable keys; require exact `NodeId`/`NodeKey`
anchors and copied raw/canonical/adapter/grammar/capability evidence; model scopes, declarations,
definitions, bindings, references, imports, exports, build modules, dynamic boundaries, visibility,
and shadowing. Construct facts through a validating builder that rejects foreign nodes, dangling or
wrong-kind links, invalid source order, duplicate/empty identities, and incomplete namespace policy.
Derive projection and fact keys from the analysis, build context, schema, and complete deterministic
fact payload. Production language packs remain S1 with S2/S3 Unknown; M3.3 owns extraction/rules.

Validation path: focused parse schema/builder tests first; strict Serde unknown-field/schema tests;
foreign-analysis, dangling-link, shadowing, and determinism adversarial tests; static public-surface
guard for borrowed Tree-sitter types; then the full workspace all-feature test/build/rustdoc/clippy/
fmt/diff gates and unchanged M0/M1/M2 definition-of-done gates through the workspace suite.

Next checkpoint: the core module compiles with a minimal hand-labelled scope graph whose links, wire
keys, evidence, and retained `Arc<ProjectAnalysis>` round-trip exactly.

Negative-memory constraints: never infer scope from containment, never treat query/canonical roles as
bindings, never serialize `NodeId`, never use bare spelling/path/graph/2 IDs as fact identity, never
mark empty or partial construction complete, and never promote adapter capabilities before M3.3 rule
packs exist.

Agent assignment: `/root` owns M3.2 design, implementation, integration, and verification; no
concurrent file edits are assigned.

Current checkpoint (2026-07-14T02:52:02+02:00): ADR 0002, current analysis identities, adapter facts,
grammar/adapter snapshots, and `NodeKey` serialization are audited. The implementation boundary is
`deslop-parse`: it already owns `Arc<ProjectAnalysis>`, `NodeId`, `NodeKey`, adapter identity, grammar,
Serde, and projection hashing. No new crate or dependency is required.

Implementation checkpoint (2026-07-14T03:08:20+02:00): the `deslop.scope-graph/1` module now models
all ten structural fact classes through dense non-Serde `ScopeFactId` handles and payload-bound
`sf1_` wire keys. A 14-fact Rust fixture proves scope/declaration/definition/binding/reference/import/
export/build-module/dynamic-boundary/shadowing ownership, visibility, namespace policy, exact M2
canonical-role coherence, adapter/grammar/capability evidence, explicit coverage reasons, deterministic
build-context/policy-sensitive identities, and strict round-trip serialization. Adversarial tests reject
foreign IDs, wrong-kind/dangling/cyclic links, forged roles, invalid namespaces, corrupt source order,
payload/key mismatch, schema/unknown-field drift, and Complete coverage without Provided capability.
All 100 parse tests, two compile-fail doctests, focused all-target clippy, parse rustdoc, and whitespace
checks pass. Next checkpoint: full workspace gates and terminal M3.2 audit.

Terminal checkpoint (2026-07-14T03:10:07+02:00): M3.2 is complete in jj change `kxlpnnwt`. The new
2,659-line module includes the full public model and six focused executable tests. Its hand-labelled
projection contains 14 facts spanning every one of the ten ADR classes; fact IDs cannot serialize;
wire keys bind the complete payload, analysis revision, build context, fact policy, ordinal, and exact
node evidence; all incomplete coverage retains a reason. Strict documents validate schemas, keys,
adapter/capability/grammar/raw evidence, namespaces, visibility boundaries, typed links, scope cycles,
file-scope module constituents, and shadowing namespaces. Production manifests and graph code are
untouched, the existing no-authority-leak test passes, graph/2 remains syntactic, and M0/M1/M2 exact
definition-of-done tests pass unchanged. All workspace all-feature test/build/rustdoc/clippy/fmt/diff
gates pass with only the two designated slow probes ignored. Next: M3.3 versioned language resolution
rule packs and shared path engine; do not reuse M3.2 hand-labelled facts as semantic authority.

#### Active M3.3 execution plan — total language rule packs and shared traversal

Active hypothesis: M3.3 is complete when every stored grammar dialect has one strict, total,
versioned rule pack describing its namespace/scope/timing/shadowing/path/import/dynamic semantics, and
one shared engine can apply those declared relations to M3.2 facts without inventing a global lookup,
terminal outcome, or capability claim.

Current approach: add `deslop.resolution-rules/1` to `deslop-lang` and bind it into every `LangPack`
identity. Each section declares Provided/Unsupported/Unknown independently and only Provided sections
carry executable payload. Model scope creators/parent selection, extraction forms, namespace unions and
transitions, visibility, declaration timing, shadowing/duplicates, qualification roots and members,
imports/aliases/globs/preludes/exports/re-exports, module prerequisites, opaque boundaries, and a
lexicographic structured precedence relation. Populate exact dialect-specific rule metadata for Rust,
JavaScript/JSX, TypeScript/TSX, Python, Julia, and Clojure; retain explicit Unknown wherever current M2
queries cannot extract the required facts (notably Clojure declarations/references). Do not promote the
production capability manifests in this item. Add a parse-owned transient traversal engine over an
existing `ScopeGraphProjection`; it may enumerate reachable lexical/import steps and structured
precedence but must not serialize candidates or assign Unique/Ambiguous/Unresolved—M3.4 owns those.

Validation path: strict totality/Serde/schema/dialect tests in `deslop-lang`; exact seven-dialect pack
goldens; engine tests over hand-labelled nested scopes, namespaces, timing, explicit shadowing, aliases,
and opaque boundaries; adversarial no-global-name/no-sort-winner tests; identity-change tests; existing
M0/M1/M2/M3.2 authority gates; then full workspace gates.

Next checkpoint: a validated schema whose unknown sections are payload-free and whose exact dialect
selection changes stored adapter/projection identity.

Negative-memory constraints: repeated LangPack methods require uniquely anchored edits; query captures
are syntax seeds, not bindings; no universal hard-coded precedence; no float scores; no first-wins or
bare-name lookup; no terminal status in M3.3; no capability promotion while extraction or build inputs
remain incomplete.

Agent assignment: `/root` owns M3.3 schema, all per-language metadata, traversal, integration, and
verification; no concurrent edits are assigned.

Current checkpoint (2026-07-14T03:14:01+02:00): ADR rule-pack requirements, the total capability/query
catalogs, all six production `LangPack` implementations covering seven dialects, and the M3.2 public
fact surface are audited. Current packs expose query seeds but all S2/S3 capabilities remain Unknown;
Clojure declarations/references/control are explicitly Unknown. This is the authority baseline.

Schema checkpoint (2026-07-14T03:28:32+02:00): `deslop.resolution-rules/1` is implemented as a strict
ten-section declarative instruction schema. It models exact syntax selectors, scopes/parents, fact
extraction, portable/adapter namespaces and transitions, visibility/timing, shadowing/duplicates,
qualification/member traversal, import/export traversal, module prerequisites, dynamic boundaries,
and ordered non-floating precedence dimensions. Provided sections require adapter authority and
payload; Unknown/Unsupported sections reject both. Pack validation enforces total section order, exact
dialect triples, namespace declaration closure, and one structured precedence relation. Three strict
schema tests and focused clippy pass. `LangPack` has an unknown-by-default hook, and stored
`LanguageAdapterIdentity` now includes and hashes the validated rule pack; any provided rules must
declare the selected grammar dialect. Focused language/parse tests pass. Next: exact production
per-dialect packs, then traversal.

Rule-pack checkpoint (2026-07-14T03:35:11+02:00): all six production language families now return
strict total packs for the seven selected dialect triples (Clojure, Julia, Python, JavaScript, JSX,
TypeScript/TSX, and Rust). The catalogs explicitly declare each language's namespaces, unions and
transitions, known scope parents, visibility/timing, duplicate behavior, qualification, import forms,
module prerequisites, dynamic boundaries, and lexicographic precedence. Every precedence term now
pins `lower-first` or `higher-first`; duplicate dimensions are rejected. Extraction remains Unknown
and payload-free for every family, other incompletely supported sections remain Unknown, and all
production capability manifests are unchanged. The exact matrix, serialization distinctness, strict
round-trip, both focused crate suites, and focused all-target clippy pass. Next: implement and test the
parse-owned transient traversal, retaining every reachable candidate without a terminal outcome.

Traversal checkpoint (2026-07-14T03:43:55+02:00): `ResolutionTraversalEngine` now builds immutable
fact indexes and starts from the reference's exact scope and first qualification segment. It walks only
the lexical parent chain, retains every same-key declaration attempt, applies pack-declared namespace
unification/transitions and directional precedence components, observes visibility and binding timing,
links definitions/bindings/explicit shadowing, and exposes relevant imports and dynamic boundaries as
deferred observations. An unrelated sibling declaration is numerically excluded from the three retained
reachable candidates; wrong-namespace and declared-later attempts remain visible. Results deliberately
have no Serde implementation or terminal status, and a compile-fail doctest locks that M3.4 boundary.
Rule payload identity, focused language/parse suites, parse rustdoc, focused clippy, fmt, and diff checks
pass. Next checkpoint: full workspace gates, exact authority/TODO audit, and terminal M3.3 report.

Terminal checkpoint (2026-07-14T03:45:52+02:00): M3.3 is complete in jj change `xupxwnxm`. The
1,514-line rule module supplies strict total versioned production packs for six language families and
seven exact dialects; the 1,303-line transient engine applies their namespace and precedence relations
only to reachable M3.2 facts. All focused and workspace all-feature test/build/rustdoc/clippy/fmt/diff
gates pass, including unchanged M0/M1/M2 DoD and the no-semantic-authority-leak test. Production S2/S3
capabilities remain Unknown and extraction remains Unknown/payload-free. M3.3 is checked. Next: open a
fresh M3.4 change to define strict retained candidate paths, rejection evidence, coverage, and terminal
outcomes on top of this non-selecting traversal; do not retrofit status or Serde into the M3.3 types.

#### Active M3.4 execution plan — complete retained paths and coverage-bounded outcomes

Active hypothesis: M3.4 is complete when every reference result stores all attempted reachable paths,
including lower-precedence and rejected attempts, and derives status only after structured precedence,
endpoint equivalence, evidence authority, and coverage are evaluated independently. No repository-global
name match, deterministic sort, or candidate count may manufacture binding authority.

Current approach: add a strict `deslop.resolution/1` projection in `deslop-parse`, retaining its
`Arc<ProjectAnalysis>`, M3.2 projection identity/build context/fact policy, exact reference and source
facts, stored adapter/rule/grammar identity, every traversal edge and endpoint, structured precedence,
visibility/namespace/timing/condition/build checks, rejection/shadow reasons, dynamic observations,
coverage, authority, and diagnostics. Derive maximum-precedence viable endpoint sets without discarding
paths. Status is `Unique` only for complete authoritative coverage and one distinct maximum endpoint,
`Ambiguous` only for complete coverage and multiple maximum endpoints, `Unresolved` only for complete
zero-candidate coverage, and `Unknown` for every incomplete/provider-conflict/dynamic case.

Validation path: strict schema/Serde/identity tests; duplicate paths converging on one endpoint; equal
maximum paths to distinct endpoints; lower-precedence retention; declared-later/wrong-namespace/not-
visible/shadowed/dynamic/deferred-import rejection or incompleteness; zero-candidate complete versus
incomplete cases; no-global-name and stable-order invariance; then unchanged M0/M1/M2/M3.2/M3.3 authority
gates and full workspace checks.

Next checkpoint: audit ADR 0002's exact path, coverage, authority, and status contract against the
transient engine and M3.2 coverage fields, then freeze the strict schema before implementation.

Negative-memory constraints: do not add Serde/status to M3.3 traversal; do not collapse paths sharing an
endpoint; do not discard rejected or shadowed paths; do not let source/fact order break semantic ties;
do not call zero candidates Unresolved or one candidate Unique under incomplete coverage; do not resolve
deferred imports/modules without exact build/export paths; do not promote production S2/S3 capabilities.

Agent assignment: `/root` owns M3.4 schema, outcome derivation, integration, and verification; no
concurrent edits are assigned.

Schema/derivation checkpoint (2026-07-14T04:06:33+02:00): `deslop.resolution/1` is implemented as a
strict immutable projection over `Arc<ScopeGraphProjection>`. The 2,563-line module supplies
payload-bound `rp1_` path and `rr1_` result keys, non-Serde dense result handles, resolution-policy and
projection identity, complete edges/checks/source-fact provenance, structured precedence, rejection
reasons, dynamic observations, coverage, authority, and status. Derivation keeps every reachable path,
marks only lower declared precedence as shadowed, compares distinct maximum endpoints, and gates all
terminal statuses on Complete coverage. Eight focused tests prove Unique/Ambiguous/Unresolved/Unknown,
same-endpoint multi-path uniqueness, lower/rejected retention, unrelated-name exclusion, stable-order
non-authority, namespace/visibility/timing rejection, dynamic/import incompleteness, strict round-trip,
payload-key/status corruption rejection, policy identity, and owner checking. Focused parse tests,
clippy, rustdoc, fmt, and diff checks pass. Next: adversarial validation audit, full workspace gates, and
terminal M3.4 checkpoint if no uncovered contract remains.

### M4 — CFG, PST, PDG, and SDG

Lower control flow per adapter; compute dominance/post-dominance and SESE/PST regions; add liveness,
def/use, control/data dependence, effects, and interprocedural summaries. Preserve irreducible and unknown
regions. This unlocks branch/function recipes and impact-aware planning.

### M5 — Candidate detectors and transformation recipes

Implement in thin vertical slices with fixtures and counterexamples: branches first, then extract/split/
merge function, clone classes, dependency/module operations, and clarity/ceremony/dead-code recipes. Every
slice must go graph fact -> candidate -> patch -> expected graph delta -> verifier -> rollback.

### M6 — Work-order DAG and LLM protocol

Replace finding-shaped proposals with unique transactions, prerequisites/conflicts, SCC/atomic grouping,
topological scheduling, bounded graph slices, schema-versioned handles, and replan-after-change. Make MCP,
CLI, LSP, and library paths behaviorally identical.

### M7 — Verification authority

Expand impact-cone selection, compiler/type/lint adapters, targeted tests, coverage, characterization,
differential checks, mutation evidence, resource sandboxing, failure injection, and atomic undo. Build the
recipe demotion path for discovered counterexamples.

### M8 — Readability and ranking calibration

Capture per-node features once; import/licence-check published datasets; collect multilingual pairwise
ratings and comprehension outcomes; run size-controlled ablations, project/language holdouts, calibration,
and model cards. Decide explicitly between one portable model, language/role models, or evidence-only UX.

### M9 — Incremental project scale and integrations

Add persistent content-addressed caches, changed-range graph invalidation, scalable clone indexing, parallel
region analysis, query budgets, git-changed scans, ratchets, SARIF/CI, editor updates, and performance
regressions. Reuse the same project snapshot across tools and sessions.

### M10 — Dogfood, external evaluation, and stable release

Run deslop on itself and independent repositories through human and LLM workflows. Publish benchmark
results and failure taxonomy, close or document every release-gate exception, freeze schema/recipe/model
versions, complete migration/undo/security docs, and release only the capability tiers demonstrated by data.

## Required artifacts and decision records

Each milestone must leave: an ADR for new semantic authority, schemas and capability matrix, gold fixtures
and counterexamples, numerical benchmark results, migration notes, a session-report checkpoint, and updated
negative memory for invalidated algorithms or labels. `.agents/TODO.md` is the completion ledger; an item is
checked only after its listed evidence exists.

## Negative-memory constraints

- Tree-sitter is an imperative syntax substrate, not a universal semantic oracle.
- Repo-relative unusualness is not absolute readability or defect evidence.
- Byte/token/AST entropy, compression ratio, and language-model surprisal are distinct measures.
- Readability evidence does not imply removability, payoff, behavior preservation, or safe application.
- A green current test suite does not validate name resolution, graph edges, metric labels, or equivalence.
- Pairwise clone findings and span-derived work-order IDs do not constitute a stable transformation plan.
- Architectural clustering and dependency cycles generate hypotheses; intent and public-boundary decisions
  still require evidence and review.

## Primary references and design consequences

1. Ferrante, Ottenstein, and Warren, “The Program Dependence Graph and Its Use in Optimization”
   (TOPLAS, 1987), <https://doi.org/10.1145/24039.24041>. Represent control and data dependence explicitly;
   use dependence facts to constrain transformations.
2. Horwitz, Reps, and Binkley, “Interprocedural Slicing Using Dependence Graphs” (TOPLAS, 1990),
   <https://doi.org/10.1145/77606.77608>. Use system-dependence summaries for cross-call impact and slicing.
3. Yamaguchi et al., “Modeling and Discovering Vulnerabilities with Code Property Graphs” (IEEE S&P,
   2014), <https://doi.org/10.1109/SP.2014.44>. Unify syntax, control flow, and dependence in a queryable
   property graph while retaining provenance.
4. Johnson, Pearson, and Pingali, “The Program Structure Tree: Computing Control Regions in Linear Time”
   (PLDI, 1994), <https://iss.oden.utexas.edu/Publications/Papers/PLDI1994.pdf>. Use hierarchical SESE
   regions as principled branch/extraction units.
5. Creager and van Antwerpen, “Stack Graphs: Name Resolution at Scale” (EVCS, 2023),
   <https://doi.org/10.4230/OASIcs.EVCS.2023.8>. Make name resolution declarative and file-incremental,
   with paths rather than same-name guesses.
6. Tsantalis and Chatzigeorgiou, “Identification of Extract Method Refactoring Opportunities for the
   Decomposition of Methods” (JSS, 2011), <https://doi.org/10.1016/j.jss.2011.05.016>. Generate extraction
   candidates from complete computation and object-state slices.
7. Sajnani et al., “SourcererCC: Scaling Code Clone Detection to Big-Code” (ICSE, 2016),
   <https://doi.org/10.1145/2884781.2884877>. Use indexed candidates and token ordering rather than
   quadratic pair comparison for Type-1/2/3 clones.
8. Tarjan, “Depth-First Search and Linear Graph Algorithms” (SIAM J. Computing, 1972),
   <https://doi.org/10.1137/0201010>. Use SCCs and condensation DAGs to expose cycles and plan order.
9. Sangal et al., “Using Dependency Models to Manage Complex Software Architecture” (OOPSLA, 2005),
   <https://doi.org/10.1145/1094811.1094824>. Use dependency structure and design rules to reason about
   layering and architecture refactors.
10. McCabe, “A Complexity Measure” (TSE, 1976), <https://doi.org/10.1109/TSE.1976.233837>. Compute
    cyclomatic complexity from control flow, not token guesses.
11. Buse and Weimer, “Learning a Metric for Code Readability” (TSE, 2010),
    <https://doi.org/10.1109/TSE.2009.70>; Posnett, Hindle, and Devanbu, “A Simpler Model of Software
    Readability” (MSR, 2011), <https://doi.org/10.1145/1985441.1985454>; and Scalabrino et al., “A
    Comprehensive Model for Code Readability” (JSS/SMR, 2018), <https://doi.org/10.1002/smr.1958>.
    Learn from human-labelled local features, control for size, and avoid universal hand-set weights.
12. Hindle et al., “On the Naturalness of Software” (ICSE, 2012),
    <https://doi.org/10.1109/ICSE.2012.6227135>, and Ray et al., “On the ‘Naturalness’ of Buggy Code”
    (ICSE, 2016), <https://doi.org/10.1145/2884781.2884848>. Treat surprisal as project- and
    role-conditioned anomaly evidence, not readability or authorship proof.
13. Torres et al., software-evolution entropy study (EMSE, 2025),
    <https://doi.org/10.1007/s10664-025-10644-y>. Token and AST-edge entropy can help flag unusual
    changes, but the target and estimator must remain explicit.
14. Tree-sitter documentation, <https://tree-sitter.github.io/tree-sitter/>. Use fields, queries,
    cursors, edits, and changed ranges as the incremental concrete-syntax foundation; layer semantics
    above it.
15. Weiser, “Program Slicing” (TSE, 1984), <https://doi.org/10.1109/TSE.1984.5010248>. Define impact and
    extraction inputs/outputs from a `(program point, value)` slicing criterion, while recognizing that a
    slice alone need not be cohesive, contiguous, or extractable.
16. Komondoor and Horwitz, “Semantics-Preserving Procedure Extraction” (POPL, 2000),
    <https://doi.org/10.1145/325694.325713>. Check flow, anti-, output, and control dependences plus non-local
    transfers before moving selected statements.
17. Néron et al., “A Theory of Name Resolution” (ESOP, 2015),
    <https://doi.org/10.1007/978-3-662-46669-8_9>. Separate language-specific scope-graph construction from
    resolution and use binding equivalence to constrain rename/move operations.
18. Jiang et al., “DECKARD: Scalable and Accurate Tree-Based Detection of Code Clones” (ICSE, 2007),
    <https://doi.org/10.1109/ICSE.2007.30>. Use structural vectors/approximate neighbors for candidate
    retrieval, followed by semantic and refactorability checks.
19. Mitchell and Mancoridis, “On the Automatic Modularization of Software Systems Using the Bunch Tool”
    (TSE, 2006), <https://doi.org/10.1109/TSE.2006.31>. Treat cohesion/coupling optimization as a source of
    candidate module boundaries, not architectural truth.
20. Overbey and Johnson, “Differential Precondition Checking” (ASE, 2011),
    <https://doi.org/10.1109/ASE.2011.6100067>. Compare authoritative semantic facts before and after a
    transformation and centralize reusable preconditions.
21. Daniel et al., “Automated Testing of Refactoring Engines” (ESEC/FSE, 2007),
    <https://doi.org/10.1145/1287624.1287651>, and Soares et al., “Making Program Refactoring Safer”
    (IEEE Software, 2010), <https://doi.org/10.1109/MS.2010.63>. Use generated adversarial programs and
    pre/post differential tests to find unsound refactorings; passing tests remain evidence, not proof.

Signature: Codex (GPT-5), ultimate generic deslop roadmap, 2026-07-12.

### Terminal M3.4 checkpoint — complete path storage before outcomes

Status: complete and verified on 2026-07-14. `deslop.resolution/1` stores every reachable candidate
attempt before deriving status, including lower-precedence and rejected paths, complete edge/check/source
fact provenance, endpoint equivalence, directional precedence, per-path and result coverage, explicit
authority, and dynamic observations. Terminal status is allowed only under Complete coverage; incomplete
imports, qualifications, rule dimensions, dynamic boundaries, and duplicate rejection remain Unknown.

Validation: 12 focused resolution tests; 115 parse tests with one designated slow probe ignored; four
compile-fail doctests; full workspace all-feature test/build/rustdoc/clippy/fmt/diff gates. M0/M1/M2
definition-of-done and graph false-resolution probes remain green. Production adapter capabilities stay
at S1 with name resolution Unknown. No repository-global bare-name index, sorted/first winner, dependency,
live-state transition, migration, reload, or restart was introduced.

Negative constraints carried into M3.5: a result cannot claim Complete while any retained path is
incomplete; canonical identities use lowercase digests; exact result keys participate in projection
identity; a qualification prefix cannot stand in for an unresolved tail; explicit shadowing edges retain
their declaration facts; equal latest-visible positions remain tied; adapter duplicate rejection cannot
fall through to an outer candidate. M3.5 owns module/package/build-target stitching and must extend the
retained path graph rather than introduce an alternate lookup surface.

Next checkpoint: audit exact module/import/export facts, build-context identities, and invalidation APIs;
design one convergent stitching fixture that measures alias, wildcard, re-export, package, build-target,
and single-file invalidation behavior before implementing M3.5.

#### Active M3.5 execution plan — module-constrained paths and exact incremental parity

Active hypothesis: M3.5 can meet the ADR only if unchanged scope facts and resolution results keep stable
revision-bound keys across an unrelated `ProjectAnalysis` successor. The existing `sf1_` derivation
includes the whole-project `analysis_id` and positional fact index, so any unrelated revision or earlier
insertion churns every downstream path/result identity. That invalidates incremental reuse before module
stitching begins and must be corrected at the identity boundary.

Current approach:

1. Change `ScopeFactKey` derivation to bind schema, exact build context, fact policy, complete node/adapter/
   capability evidence, and fact payload—but not whole-project analysis identity or builder position.
   Keep dense `ScopeFactId` owner/index analysis-local and keep projection/document identity bound to the
   exact `ProjectAnalysisId` and ordered fact document. Add numerical successor tests proving unchanged
   facts retain keys while edited facts and projection identity change.
2. Build an exact module stitch index only from `BuildModule` facts in the current build context. Resolve
   importer ownership through declared file-scope constituents; match package/target/module paths rather
   than path stems or global names; retain wrong-target candidates as rejected paths.
3. Extend deferred import paths through module and export/re-export edges. Alias imports bind modules;
   selective/glob imports traverse the source module's declared export set; local targets and names stay
   constrained to constituent scopes. Incomplete module/export coverage remains Unknown. Re-export SCCs
   use a deterministic fixed point and never first-win.
4. Add `ResolutionProjection::successor` plus an explicit update report. Compare stable fact keys and
   reverse dependencies, rebuild only added/invalidated references, clone unchanged result wires, and
   prove the successor document is byte-for-byte equal to a clean rebuild. New module mappings also
   invalidate matching formerly-unresolved imports; new facts in a searched lexical/module scope
   invalidate dependents; unrelated files reuse exact result keys.

CONVERGENCE: one synthetic multi-file, two-target graph will exercise explicit module alias, selective
and glob imports, one re-export chain/cycle boundary, wrong-target rejection, an export edit, and an
unrelated-file edit. Terminal outcomes are: (a) clean and incremental documents differ—identity or
invalidation design is invalid; (b) unrelated keys churn—fact identity remains invalid; (c) target or
export ambiguity becomes order-dependent—stitching is invalid; or (d) exact parity and bounded reuse
pass, authorizing the full workspace gates. This single fixture collapses the identity, stitching, and
incremental decision tree before later M3.7 adversarial breadth and M3.8 performance measurement.

Validation path: smallest fact-key successor test; focused module-stitch cases; clean/incremental JSON
parity with numerical reused/rebuilt counts; parse crate tests/doctests; all-feature workspace test,
build, rustdoc, clippy, fmt, and diff checks; unchanged M0/M1/M2 and graph false-resolution gates.

Negative-memory constraints: no repository-global spelling candidate; no file-path-as-module inference;
no alias string substitution; no terminal result from incomplete module/export coverage; no stable-order
tie break; no reusing a result without proving all lexical/module/export dependencies unchanged; no
production S2 capability promotion from synthetic complete fixtures.

Agent assignment: `/root` owns identity correction, module stitching, successor invalidation, integration,
and verification. No sub-agent was requested, so no delegation is active.

Next checkpoint: make unchanged `sf1_` identities successor-stable and prove edited facts/projections
still expire before adding any module traversal.

Identity checkpoint (2026-07-14): complete. `sf1_` no longer hashes the whole-project analysis ID or
builder index; it still binds schema, build context, fact policy, complete revision-bearing evidence,
and fact data. The projection/document remain analysis-bound and dense IDs remain owner/index-local.
A two-file successor test changes the peer source and reverses builder order: the project/projection IDs
and peer key change, while the unchanged file key remains byte-identical. All seven scope-graph tests and
`git diff --check` pass. Next: add the exact module stitch index and module-constrained import paths.

Module/invalidation checkpoint (2026-07-14): focused implementation complete. `BuildModule` now carries
explicit export-set coverage, and Complete export coverage requires declared imports/exports adapter
authority. Deferred imports traverse only exact declared package/target/module mappings and their file
constituents. Alias imports end at modules; selective and glob imports traverse exact exports; re-exports
use cycle-aware reachability; wrong-target candidates remain rejected paths. Incomplete export sets and
pure cycles remain Unknown rather than authorizing a terminal result.

The new `ResolutionProjection::successor` compares stable fact keys, follows reverse scope/module/export
dependencies, rebuilds affected references, and copies unchanged strict result wires. One convergent
fixture proves byte-identical successor versus clean documents and measures: unrelated peer edit = five
reused/zero rebuilt; source export addition = one independent result reused/five dependents rebuilt; newly
matching module = zero reused/one formerly unresolved reference rebuilt. The parse package reports 121
passed, one designated instrumentation probe ignored, and four compile-fail doctests passed; focused
rustdoc/clippy/fmt/diff gates are clean. Next checkpoint: all-feature workspace terminal gates, targeted
fallback/capability audits, then either close M3.5 or record the exact failing boundary.

Terminal M3.5 checkpoint (2026-07-14): complete and verified. Exact declared package/target/module/file
constituent mappings now extend retained resolution paths through aliases, selective/glob exports, and
cycle-aware re-exports. Export-set completeness is explicit and capability-bound. Stable fact keys and
reverse dependency invalidation preserve exact unchanged result wires while rebuilding all measured
dependents; every successor fixture is byte-identical to a clean strict document.

Validation: 17 focused resolution tests; 121 parse tests with one designated instrumentation probe
ignored; four compile-fail doctests; full workspace all-feature test/build/rustdoc/clippy/fmt/diff gates;
unchanged M0/M1/M2 definition-of-done and graph false-resolution probes. Targeted source/diff audit found
no file-stem/global bare-name lookup, first/sorted winner, or production capability change. No dependency,
live-state transition, migration, reload, cache clear, or restart applies.

Next checkpoint: open M3.6 in a fresh jj child and audit the existing external-provider/artifact identity,
authority, and conflict surfaces before designing optional compiler/LSP semantic fact ingestion.

#### Active M3.6 execution plan — pinned provider conclusions without authority erasure

Active hypothesis: optional compiler/language-server facts need a separate immutable evidence projection,
not mutation of adapter scope facts. The current authority catalog has `Compiler` but no
`LanguageServer`; `ResolutionResult.authority` is required to equal the adapter-owned reference evidence;
and the existing provider-conflict case is a manually altered path rather than ingested evidence. The
external analyzer subsystem emits lint findings and does not retain binding endpoints, project-model
coverage, or exact result artifacts, so it is not a semantic-resolution provider contract.

Current approach:

1. Extend the versioned authority vocabulary with `LanguageServer`, ordered between Adapter and Compiler
   for static evidence only. Runtime verification remains orthogonal and cannot enter static precedence.
2. Add a strict `deslop.semantic-resolution-facts/1` projection bound to the exact analysis, scope graph,
   build context, provider kind/name/version, executable/configuration/project-model/result artifacts,
   and explicit coverage. Facts name an exact reference and retain Unique/Ambiguous/Unresolved/Unknown
   conclusions plus every internal or positively identified external endpoint. Stale graph/build keys,
   malformed terminal cardinality, incomplete terminal claims, duplicate provider queries, and forged
   payload IDs fail closed.
3. Join adapter and provider conclusions in `deslop.resolution/1` without blending lookup precedence and
   evidence authority. Retain every conclusion/path. Complete compiler evidence outranks complete LSP and
   adapter evidence; complete LSP outranks adapter evidence only with a complete project model and pinned
   artifacts. Any complete lower-authority disagreement yields Conflict while preserving a higher-
   authority preferred diagnostic conclusion. Equal-authority disagreement yields Conflict with no
   preferred winner. Incomplete provider facts cannot assert a conflict or terminal result.
4. Extend successor invalidation with semantic fact/artifact dependencies. A changed provider fact rebuilds
   only its referenced result; a provider/model/artifact identity change expires every fact carrying that
   identity; unchanged results retain exact keys. Successor and clean strict documents must be byte-equal.

CONVERGENCE: one two-reference provider fixture will exercise adapter-only, complete LSP, complete compiler,
lower-authority disagreement, equal-authority disagreement, incomplete project model, wrong build/scope
identity, internal and external endpoints, artifact revision change, and an unrelated provider edit. Its
terminal outcomes are: (a) stale or incomplete evidence authorizes a terminal result—schema invalid; (b)
provider rank changes lookup candidate precedence—join invalid; (c) any disagreement is dropped or a tied
provider wins by order—conflict model invalid; (d) successor differs from clean or rebuilds the unrelated
reference—invalidation invalid; or (e) all exact outcomes and numerical reuse counts pass, authorizing full
workspace gates. This single fixture collapses schema, authority, conflict, and invalidation decisions.

Validation path: authority-catalog unit tests; semantic fact strict round-trip/adversarial tests; focused
provider-join and successor parity tests with measured counts; parse/lang crate tests and doctests; all-
feature workspace test/build/rustdoc/clippy/fmt/diff gates; unchanged M0/M1/M2 and M3.4/M3.5 regression
probes; targeted audit that production adapters were not promoted.

Negative-memory constraints: never relabel LSP evidence as Compiler or Adapter; never infer artifact or
project-model completeness from provider output presence; never overwrite adapter conclusions; never rank
provider evidence through language lookup precedence; never first-win equal authority; never turn provider
absence into externality; never reuse across provider/config/model/result artifact identity changes.

Agent assignment: `/root` owns schema, join, invalidation, integration, and verification. No sub-agent was
requested, so no delegation is active.

Next checkpoint: implement and strictly validate the pinned semantic fact projection before changing
resolution outcome derivation.

Provider schema/join/invalidation checkpoint (2026-07-14): focused implementation complete.
`deslop.language-adapter-capabilities/2` adds the distinct LanguageServer authority without promoting any
production adapter declaration. New strict `deslop.semantic-resolution-facts/1` documents bind analysis,
scope graph, build context, provider kind/name/version, executable/configuration/project-model/result
artifacts, exact reference/endpoints, diagnostics, and coverage. Builder and strict wire validation reject
foreign graphs, duplicate provider queries, incomplete terminal facts, cardinality contradictions, forged
payloads, absent internal endpoints, and complete project models without an artifact.

`deslop.resolution/1` now retains one adapter conclusion plus every semantic conclusion/path. Complete LSP
agreement becomes the preferred diagnostic authority while leaving adapter evidence present. Complete
compiler evidence outranks LSP; any lower complete disagreement produces Conflict while retaining the
compiler preference. Equal compiler disagreement produces Conflict with no preferred source and identical
documents under reversed insertion order. Incomplete LSP evidence stays Unknown within its own path and
cannot authorize or conflict. Positive external endpoints require a pinned provider fact. Strict resolution
validation cross-checks each conclusion/path against its exact semantic fact and provider.

Semantic successor measurements: changing one result artifact reuses four of five exact results and
rebuilds one; changing the provider configuration changes both carried fact keys, reuses three, and rebuilds
two. Both successors are byte-identical to clean builds and report `SemanticFactChanged`. The old successor
API fails closed when a prior projection contains semantic facts. Focused status: 24 resolution tests, 128
parse tests plus one designated instrumentation probe ignored, four compile-fail doctests, 12 lang tests,
focused rustdoc/clippy/fmt/diff clean. Next: workspace-wide all-feature gates and production-authority/
fallback audit before terminalizing M3.6.

Terminal M3.6 checkpoint (2026-07-14): complete and verified. Versioned provider facts are immutable,
strict, graph/build/artifact-bound inputs; resolution retains distinct adapter/LSP/compiler conclusions and
derives Conflict without erasing disagreement or using lookup order as evidence rank. Equal-authority
disagreement has no preferred result. RuntimeVerification and Syntax cannot assert a terminal static
binding. Provider absence never proves externality; only an explicit pinned positive endpoint can do so.

Validation: 24 focused resolution tests; 128 parse tests with one designated instrumentation probe ignored;
four compile-fail doctests; 12 lang tests; full all-feature workspace test/build/rustdoc/clippy/fmt/diff
gates; unchanged M0/M1/M2 definition-of-done and graph false-resolution probes. Incremental clean parity is
exact at 4 reused/1 rebuilt for a result artifact and 3/2 for a shared configuration artifact. Production
capability declarations and existing external/LSP execution paths are unchanged; no provider process,
dependency, migration, reload, cache clear, or restart applies.

Next checkpoint: open M3.7 in a fresh jj child, inventory the existing M3.2-M3.6 fixtures against every ADR
adversarial dimension, and add only the missing joined cases with frozen expected paths/status/reasons.

#### Active M3.7 execution plan — frozen joined adversarial resolution corpus

Active hypothesis: M3.2-M3.6 already exercise each named adversarial behavior, but the evidence is spread
across independent tests and does not yet constitute one frozen gold corpus. M3.7 is complete only when a
joined, hand-labelled matrix locks the exact result and path surface for duplicate names, nested lexical
scope, alias/selective/glob imports, re-exports, dynamic boundaries, and terminal versus non-terminal
absence without adding any repository-global or ordering fallback.

Current approach:

1. Add test-only gold summaries over the strict public resolution surface: result status and coverage;
   path viability, endpoint kind, edge kinds, rejection reasons, check kind/state pairs, and dynamic-
   boundary counts. Keep opaque revision keys out of the labels while retaining exact semantic structure.
2. Reuse the executable nested-scope and multi-module fixtures in one joined matrix. Freeze complete
   duplicate ambiguity and language-specific duplicate handling, nearest-scope shadowing, selective/alias/
   glob/re-export paths including wrong-target rejection, dynamic and deferred unknown cases, complete
   zero-candidate unresolved, and partial zero-candidate unknown.
3. Run the joined gold test first, repair only measured contract gaps, then run the complete parse crate and
   workspace all-feature terminal gates. Leave M3.8 precision/recall measurement separate; M3.7 establishes
   the frozen labelled cases that M3.8 will score.

CONVERGENCE: one matrix test enumerates every named M3.7 case and compares its exact semantic summary. Its
terminal outcomes are: (a) a label disagrees because the expected fixture contract was wrong—correct the
hand label from retained path evidence; (b) the implementation drops or misclassifies evidence—repair that
single semantic boundary; or (c) all labels match, authorizing the full gates. The matrix collapses the
adversarial breadth into one run and gives M3.8 a stable denominator rather than a percentage-only claim.

Validation path: focused joined-gold test with exact case count; existing focused resolution tests; full
parse tests and compile-fail doctests; workspace all-feature test, build, rustdoc, clippy, fmt, and diff
checks; unchanged M0/M1/M2 and graph false-resolution gates.

Negative-memory constraints: no global spelling lookup; no path-stem module inference; no first/sorted
winner; no terminal result from incomplete reference/module/export/dynamic evidence; no omission of
rejected or unknown paths; no conversion of re-export cycles or deferred imports into absence proof; no
production capability promotion from synthetic fixtures.

Agent assignment: `/root` owns the gold schema, fixture integration, and verification. No sub-agent was
requested, so no delegation is active.

Next checkpoint: implement the test-only semantic summaries and run the single joined matrix to capture the
exact measured labels before any production-code change.

Joined-gold checkpoint (2026-07-14): focused implementation complete. A versioned
`deslop.resolution-adversarial-gold/1` artifact freezes 16 cases through semantic endpoint labels rather
than opaque revision keys. Every result locks status, coverage, authority, dynamic-boundary count, and all
paths; every path locks endpoint, viability, ordered traversal edges, rejection reasons, check states,
source-fact kinds, provider-fact count, authorities, and coverage. The executable matrix includes unrelated
same-spelled declarations in separate file scopes, equal-precedence duplicates, explicit nested shadowing,
namespace/visibility/timing rejections, selective/alias/glob/re-export paths, a re-export cycle, conditional
and mapped deferred imports, unresolved qualification, dynamic evidence, and complete/partial zero-
candidate cases.

Measured focused result: 16/16 gold labels match. The complete parse package reports 129 passed, zero
failed, one designated instrumentation probe ignored, and four compile-fail doctests passed. Focused
rustdoc, all-target clippy, fmt, and diff checks pass. No production resolution algorithm, schema, adapter,
or capability declaration changed. Next: run all workspace all-feature terminal gates and targeted audits
for matrix denominator, opaque-key absence, no global-name leakage, and unchanged prior gates.

Terminal M3.7 checkpoint (2026-07-14): complete and verified. The frozen 16-case corpus contains exactly 36
retained paths—13 viable, 18 rejected, and 5 unknown—with status counts 7 Unique, 1 Ambiguous, 2
Unresolved, and 6 Unknown. Semantic labels bind exact source spans or package/target/module identities while
excluding opaque revision keys. Full workspace all-feature test/build/rustdoc/clippy/fmt/diff gates pass,
including M0/M1/M2 locks and graph false-resolution probes. Targeted diff/source audits confirm the change
is test-only and introduces no global lookup, first/sorted winner, resolver/schema change, or production
capability promotion.

Next checkpoint: open M3.8 in a fresh jj child and design the exact confusion-matrix and incremental-
isolation scorer over the frozen M3.7 cases without changing or re-labelling the corpus.

#### Active M3.8 execution plan — exact corpus scorer and incremental isolation report

Active hypothesis: M3.8 must derive measurements from the immutable M3.7 gold rather than repeat its
expected labels in assertions. The supported subset is the ten Complete-coverage cases; all six expected
Unknown cases remain in a separate explicit denominator. Exact multiset agreement over complete path
objects and semantic endpoint labels provides honest path/endpoint precision and recall, while a full
five-status matrix exposes every misclassification rather than collapsing outcomes into correct/incorrect.

Current approach:

1. Deserialize the frozen M3.7 document into the test-only semantic gold types and factor construction of
   the 16 actual results into one reusable function. Compute the full Unique/Ambiguous/Unresolved/Unknown/
   Conflict confusion matrix, exact case matches, supported and Unknown denominators, retained-path
   multiset intersections, and endpoint-label intersections. Store all ratios as raw numerators and
   denominators; render 1.0 only in the human report.
2. Add four clean-parity incremental scenarios: an unrelated same-spelled peer addition (five existing
   results reused and one new reference), a reachable equal-precedence declaration (Unique to Ambiguous),
   an export addition reverse cone, and a formerly unresolved import after its exact module appears. Record
   previous/current/reused/rebuilt/added/removed counts and status transitions.
3. Publish `.agents/M3_8_RESOLUTION_REPORT.md` with corpus counts, the full status confusion matrix,
   supported and Unknown agreement, incremental counts, commands, failures, and scope limitations. Run the
   focused scorer first, then parse and all-feature workspace terminal gates.

CONVERGENCE: one scorer test consumes the frozen gold and returns one report structure. Terminal outcomes
are: (a) status/path/endpoint counts disagree—implementation or scorer mapping is wrong; (b) an incremental
scenario differs from clean or rebuilds outside the labelled cone—invalidation is wrong; or (c) every exact
count matches, authorizing publication and terminal gates. No serial threshold tuning or corpus relabelling
is permitted.

Validation path: exact M3.8 scorer; existing M3.7 frozen-gold test; focused incremental transition tests;
parse tests/doctests; full workspace all-feature test, build, rustdoc, clippy, fmt, and diff gates; unchanged
M0/M1/M2 and graph false-resolution gates.

Negative-memory constraints: never omit Unknown cases from denominators; never report a percentage without
numerator/denominator; never treat path order as endpoint authority; never deduplicate alternate paths to one
endpoint before scoring paths; never relabel frozen gold to fit actual output; never count incremental reuse
without clean-document equality; never mix graph/2 syntactic candidates into semantic resolution scoring.

Agent assignment: `/root` owns scoring, isolation measurement, publication, integration, and verification.
No sub-agent was requested, so no delegation is active.

Next checkpoint: factor the frozen corpus loader/actual builder and prove the exact 5x5 status matrix plus
supported/Unknown path and endpoint agreement before adding incremental measurements.

Measurement checkpoint (2026-07-14): focused scorer and isolation table complete. The exact 5x5 matrix has
only diagonal counts `[7, 1, 2, 6, 0]`. Complete supported cases are 10/10 status matches with 27/27 path
precision/recall and 18/18 endpoint precision/recall. All six expected Unknown cases remain counted with
9/9 paths and 5/5 retained non-null endpoints; total exact path agreement is 36/36.

Four successor scenarios are clean-document equal: unrelated same-spelled peer addition reuses five existing
results, rebuilds zero, and adds one reference; reachable equal-precedence addition rebuilds one and changes
Unique to Ambiguous; export addition reuses one/rebuilds five; exact module appearance rebuilds one. The last
scenario retains both ReachableScopeChanged and MatchingModuleAdded—reason dimensions are non-exclusive.
The focused report is published in `.agents/M3_8_RESOLUTION_REPORT.md`. Next: parse and workspace terminal
gates, report final commands/failures, source audit, then either close M3.8 or record the exact blocker.

Terminal M3.8 checkpoint (2026-07-14): complete and verified. The published report now records the final
commands and terminal status. Full workspace all-feature test/build/rustdoc/clippy/fmt/diff gates pass;
`deslop-parse` reports 131 passed, zero failed, one designated ignored probe, and four passing compile-fail
doctests. M0/M1/M2 numerical/authority locks and all graph false-resolution probes remain green. Targeted
diff audit confirms M3.8 contains only test-module scoring/isolation code and `.agents` reports—no production
resolver, schema, adapter, graph, consumer, or capability declaration changed.

Next checkpoint: open M3.DoD in a fresh jj child and audit every semantic-recipe consumer gate against the
frozen corpus statuses/authority, especially blocking Unknown, Ambiguous, Unresolved, Conflict, dynamic
boundaries, and incomplete reverse dependencies without graph/2 fallback.

#### Active M3.DoD execution plan — fail-closed unique-binding consumer gate

Active hypothesis: no semantic recipe type exists yet—M5.1 owns that schema—so wiring resolution into the
current syntactic/LLM work-order pipeline would conflate contracts. The semantic boundary M3 can complete is
a public, versioned, fail-closed eligibility decision that future M4/M5 consumers must use. It accepts only
an exact `ResolutionProjection`/`ResolutionResult`, the result's stored adapter capability manifest, a
declared minimum static authority and capability set, and projection-bound reverse-dependency evidence.
There is deliberately no graph/2 input or fallback path.

Current approach:

1. Add `deslop.resolution-consumer-gate/1` in `deslop-parse`. A unique-binding requirement must name the
   consumer, include NameResolution plus any additional capabilities, and require Adapter/LSP/Compiler
   static authority. Syntax is insufficient and RuntimeVerification is orthogonal.
2. Derive reverse-dependency evidence from an exact projection/result and allow only explicit downgrade,
   never caller-created Complete evidence. Evaluate status, complete result/path coverage, one distinct
   viable endpoint, no dynamic boundary, sufficient result authority, every stored adapter capability and
   capability authority, exact projection/result identity, and complete dependency evidence. Retain every
   block reason in a serializable decision; do not first-fail.
3. Join the frozen 16-case corpus to the gate: exactly the seven Complete Unique cases may pass a basic
   NameResolution/Adapter requirement; Ambiguous, Unresolved, Unknown, dynamic, and partial cases must retain
   exact blocks. Add provider Conflict, higher-authority, missing capability, dependency downgrade, foreign
   evidence, and decision strictness cases. Prove the API has no graph projection parameter or endpoint-only
   escape hatch.

CONVERGENCE: one frozen-corpus gate matrix plus one adversarial requirement/evidence matrix resolves the
whole decision. Terminal outcomes are: (a) any non-Unique/incomplete/dynamic case passes—gate invalid; (b) a
Complete Unique case with declared capability/authority/dependencies blocks—gate mapping invalid; (c) graph
or caller-forged Complete evidence can enter—API invalid; or (d) exact allow/block matrices pass, authorizing
workspace gates. M5 can then consume this contract without reopening M3 semantics.

Validation path: gate constructor/strict-wire tests; frozen-corpus eligibility counts and block matrices;
provider Conflict and capability/authority/dependency adversarial tests; parse crate/doctests; workspace
all-feature test/build/rustdoc/clippy/fmt/diff gates; M0/M1/M2 and graph false-resolution regression gates.

Negative-memory constraints: do not attach semantic authority to existing graph/2 or WorkOrder; do not let
endpoint presence bypass status; do not accept Preferred on Conflict; do not treat RuntimeVerification as
static rank; do not allow Syntax authority for semantic recipes; do not forge Complete dependencies; do not
first-fail and hide concurrent blockers; do not promote production adapter capabilities.

Agent assignment: `/root` owns the gate schema/API, corpus integration, adversarial validation, and terminal
integration. No sub-agent was requested, so no delegation is active.

Next checkpoint: implement strict requirement and projection-bound dependency evidence types before the
eligibility evaluator, then prove malformed/foreign evidence fails closed.

Gate implementation checkpoint (2026-07-14): focused contract complete. New public
`deslop.resolution-consumer-gate/1` decisions retain exact analysis/projection/scope-graph/build-context/
result identity, canonical consumer requirements, projection-bound dependency evidence, selected endpoint
only when eligible, and every block reason. Capability requirements are capability-specific and accept only
Adapter, LanguageServer, or Compiler static minima. NameResolution is mandatory. Dependency evidence can be
derived only from the exact projection/result and may be downgraded but not caller-upgraded.

The evaluator checks exact result/evidence ownership, status Unique, result/path coverage through derived
dependency evidence, no dynamic boundary, sufficient NameResolution result authority, sufficient stored-
manifest authority for additional capabilities, complete dependencies, and exactly one distinct viable
endpoint. Three focused tests pass: the frozen 16-case matrix permits exactly seven labelled Complete Unique
cases; requirement/capability/authority/dependency/foreign evidence failures block; and a compiler-preferred
Conflict remains ineligible with no selected endpoint. Next: parse and workspace validation plus targeted
API/source audit for graph/2 absence, decision provenance, and unchanged production capability declarations.

#### M3.DoD terminal checkpoint — complete and verified

The versioned `deslop.resolution-consumer-gate/1` boundary is complete. It has no graph/protocol dependency
or fallback, retains exact analysis/projection/scope-graph/build-context/result provenance, evaluates
capability-specific static authority plus projection-bound dependency evidence, and exposes an endpoint only
when every retained block is absent. The frozen corpus admits exactly 7/16 Complete Unique cases; all other
cases and the provider-conflict, authority, capability, dependency, and foreign-evidence adversarial probes
fail closed. Full workspace all-feature test/build/rustdoc/clippy/fmt/diff gates and the M0-M2 plus graph
false-resolution regressions pass.

Terminal outcome: M3.DoD is complete. The next active milestone is M4.1: define the versioned control-edge
schema and its fail-closed capability/authority boundary before adapter lowering begins.

#### Active M4.1 execution plan — versioned control-flow edge contract

Active hypothesis: CFG is a revision-bound local-semantic overlay over `ProjectAnalysis`, not an extension
of the syntactic project dependency `deslop.graph/2`. M4.1 must freeze enough identity, endpoint, transition,
coverage, uncertainty, and authority semantics that M4.2 adapter lowering cannot invent an ambiguous wire
shape or promote S0/S1 syntax observations to S2 ControlFlow evidence.

Current approach:

1. Add ADR 0003 and `deslop.control-flow/1` in `deslop-parse`. Represent each callable/initializer CFG with
   one virtual entry and one virtual exit, revision-bound syntax/synthetic points, stable payload-bound graph/
   point/edge keys, exact analysis/projection/policy identity, grammar/adapter identity, and the stored
   ControlFlow capability declaration.
2. Define disjoint typed transition families for entry, exit, normal, branch, loop, exceptional, abrupt,
   and suspension flow. Branch/loop/exception/abrupt/suspension sub-kinds and adapter-defined extensions are
   structured rather than free-form labels. Entry and exit boundary invariants are executable.
3. Separate graph coverage (`Complete`, `Partial`, `Unsupported`, `Failed`) from edge precision (`Exact` or
   conservative with an exact reason). Complete coverage requires provided ControlFlow capability at static
   Adapter/LanguageServer/Compiler authority, no recovered owner, and no unresolved uncertainty. Incomplete
   coverage retains canonical distinct reasons. Syntax and RuntimeVerification cannot authorize a CFG.
4. Add a frozen all-edge-family round-trip plus corruption/adversarial tests for stale keys, duplicate or
   dangling endpoints, boundary misuse, foreign-file nodes, invalid authority/support/coverage combinations,
   unknown fields, noncanonical ordering, and hidden graph/2 coupling. Do not implement production lowering or
   promote adapter manifests in M4.1; those are M4.2 responsibilities.

CONVERGENCE: one complete synthetic-adapter graph exercising every transition family plus one mutation matrix
resolves the schema decision. Terminal outcomes are: (a) malformed identity/topology/authority deserializes—
schema invalid; (b) every required edge family cannot round-trip distinctly—catalog invalid; (c) a current
production adapter can claim complete CFG evidence—authority boundary invalid; or (d) exact/adversarial matrices
and workspace gates pass, authorizing M4.2 to lower against the frozen contract.

Validation path: focused control-flow schema tests; strict JSON round-trip/corruption matrix; production
manifest non-promotion and graph/2 independence source audit; parse tests/doctests/rustdoc/clippy; workspace
all-feature test/build/rustdoc/clippy/fmt/diff gates; unchanged M0-M3 regression gates.

Negative-memory constraints: canonical/query control captures are syntax seeds, never CFG edges; do not merge
CFG with graph/2; do not infer complete coverage from enumerated edges; do not use runtime observations as
static CFG authority; do not collapse exceptional/abrupt/suspension edges into `normal`; do not omit virtual
boundaries or represent them as syntax nodes; do not let deterministic order resolve semantic uncertainty.

Agent assignment: `/root` owns schema design, implementation, adversarial tests, integration, and verification.
No sub-agent was requested, so no delegation is active.

Next checkpoint: write ADR 0003's normative invariants, then implement the strict schema and make the complete
all-family fixture round-trip before adding corruption cases.

M4.1 implementation checkpoint (2026-07-14): ADR 0003 and the public `deslop.control-flow/1` substrate are
implemented in `deslop-parse`. Each graph binds one exact executable owner, grammar, stored adapter identity,
ControlFlow support/authority, coverage, virtual entry/exit, canonical points/edges, and graph/point/edge keys.
Point and edge identities include the exact adapter manifest as well as revision and lowering policy. Syntax
and synthetic points must remain inside the owner's source region; cross-file or outside-owner evidence fails.

The edge catalog retains eight disjoint families and 35 distinct portable sub-kind instances. Complete graphs
require Provided ControlFlow at Adapter/LSP/Compiler authority, exact non-recovered edge evidence, and no
uncertainty reasons. Five focused tests pass: complete all-family stable round-trip, all-sub-kind non-collapse,
strict payload/topology/authority corruption, direct boundary/duplicate/conservative/outside-owner rejection,
and production Unknown non-promotion. Parse has 139 passing tests, one designated ignored probe, and four
passing compile-fail doctests; parse check/clippy/rustdoc/fmt/diff pass. Cargo-tree audit confirms no
`deslop-graph` dependency. Next: full workspace all-feature terminal gates, then close M4.1 if regressions remain
green.

#### M4.1 terminal checkpoint — complete and verified

The accepted ADR 0003 and `deslop.control-flow/1` implementation satisfy the frozen schema contract. Every
graph retains exact immutable provenance and static capability truth; one virtual entry/exit; owner-contained,
adapter-bound, payload-keyed points and edges; explicit complete/partial/unsupported/failed coverage; exact or
reasoned-conservative precision; and eight disjoint edge families with 35 exercised portable sub-kind values.
The strict document rejects schema/ID/key corruption, unknown fields, noncanonical order, duplicate/dangling
topology, boundary misuse, cross-file/outside-owner evidence, invalid support/authority/coverage combinations,
recovery, and conservative Complete claims.

Terminal validation passes: five focused M4.1 suites; parse 139 passed, zero failed, one designated ignored
probe, and four compile-fail doctests; full workspace all-feature test/build/rustdoc/clippy/fmt/diff gates;
unchanged M0/M1/M2 and graph false-resolution regressions; no `deslop-graph` dependency; and no production
ControlFlow capability promotion. M4.1 is complete. Next is M4.2 adapter lowering at each adapter's honestly
declared capability tier.
