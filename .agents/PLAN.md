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
