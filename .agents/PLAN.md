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

#### Active M1.11 execution plan — instrumentation and measured compaction

Active hypothesis: one revision-owned measurement surface can expose parse ownership, deterministic
node order, cold/repeated/incremental latency, and retained memory without adding consumer-specific
instrumentation or perturbing projection identities. The first measured profile should decide which
listed M1 allocations are material enough to compact; unmeasured micro-optimization is out of scope.

CONVERGENCE: instrument once over a fixed multi-language cold/repeated/incremental matrix, then use
the captured counters and size/timing decomposition to reach a terminal decision for every listed
cost center. Structural invariant failure means fix ownership/order before measuring performance;
a dominant measured allocation or lookup means compact that representation and rerun the same
matrix; no material regression or hotspot means retain the simpler representation and record the
number. Do not branch into serial canary experiments or use wall time alone as correctness evidence.

Current approach: inventory existing parse ledgers, arenas, query indices, aggregation storage, and
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

Next checkpoint: an instrumentation inventory identifies the existing authoritative counters and the
smallest missing API, with a fixed fixture and exact structural oracle ready before any optimization.

Negative-memory constraints: do not replace ledger evidence with global counters; expose parser or
borrowed-node internals; make timing a deterministic unit-test assertion; estimate retained memory
from source length alone; optimize an unmeasured representation; or allow instrumentation to enter
snapshot/projection identity.

Agent assignment: `/root` owns research, implementation, validation, and integration; no concurrent
file edits are assigned.

### M2 — Language-adapter contract

Implement capability manifests, grammar variants, query packs, canonical roles, operator/token policy,
parse-error policy, and golden fixture matrices for every supported language. Unsupported semantics become
machine-readable unknowns. This unlocks honest cross-language algorithms at `S0`/`S1`.

### M3 — Scope and project-name graph

Add lexical scopes, bindings, references, imports/exports, ambiguity, and resolution provenance; then link
files/modules/packages and optional compiler/LSP facts. Gate on duplicate-name, shadowing, aliasing, and
incremental-file fixtures before any semantic refactor uses `resolved` edges.

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
