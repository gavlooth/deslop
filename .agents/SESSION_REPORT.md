# Session Report

## 2026-06-24T09:56:44+02:00 — Mutation Parallelism Complete

Objective: Execute `.agents/NEXT_TASK.md` Task 15: parallelize native
`TreeSitterMutationProbe` scoring with a bounded scoped worker pool while keeping all aggregate
state evolution serialized and deterministic.

Changes:
- Started jj change `uvnoxnpv` on top of `lmmlzykp`.
- Replaced native mutation's serial scoring loop with a bounded `std::thread::scope` worker pool.
- Added channel actions:
  - `NativeMutantOutcome { detail, status }`
  - `NativeMutantError { id, reason }`
- Workers own only side effects: isolated temp workspace, per-worker build/depot env, mutant
  source write, check-cmd-with-timeout. They never mutate aggregate state.
- A single drain loop owns all tallies: viable, killed, timed out, unviable, errors, and stable
  lowest-id survivor selection.
- Wrapped worker execution in `std::panic::catch_unwind`; panics and spawn failures become
  `NativeMutantError` actions.
- Default concurrency is `std::thread::available_parallelism()`.
- Added `MutationConfig::AutoWithOptions { timeout, jobs }` and CLI `--mutation-jobs N` for
  `characterize`, `verify`, and `apply`.
- Set per-worker isolation env: `CARGO_TARGET_DIR` for Rust, `JULIA_DEPOT_PATH` for Julia, plus
  single-threaded test env (`RUST_TEST_THREADS=1`, `CARGO_BUILD_JOBS=1`, `JULIA_NUM_THREADS=1`).
- Updated `SPEC.md`.

Tests:
- `native_parallel_scoring_matches_serial_scoring` asserts serial and parallel summaries match.
- `native_parallel_worker_panics_are_errors_not_process_panics` proves worker panic capture.
- `native_parallel_concurrency_is_bounded` asserts in-flight workers never exceed configured jobs.
- CLI parser test covers `--mutation-jobs`.

Verification:
- Focused checks passed:
  - `cargo fmt --all && cargo test -p deslop-verify`
  - `cargo fmt --all && cargo test -p deslop-cli parses_mutation_jobs_override && cargo test -p deslop-verify native_parallel`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Deferred:
- Equivalent-mutant pruning.
- Cross-file mutation scheduling.

Queue status:
- This was the last mutation task.

Signature: Codex

## 2026-06-24T13:12:00+02:00 - Per-language analyzer threshold config

Objective: Answer and implement the MCP/config follow-up for `long-method`: make the
threshold configurable through MCP and per language where useful.

Target:
- `AnalyzerConfig` long-method threshold handling.
- CLI `deslop.toml` parsing.
- MCP `scan`/`propose` tool schemas and execution.
- Config documentation and example.

Changes:
- Added `AnalyzerLangConfig` and per-language `long_method_nloc` overrides for Rust,
  Clojure, Julia, Python, and generic sources.
- `long-method` now uses `AnalyzerConfig::long_method_nloc_for(source.lang)`, preserving
  the global default as the fallback.
- CLI config parsing accepts `[analyzer.rust]`, `[analyzer.clojure]`, `[analyzer.julia]`,
  `[analyzer.python]`, and `[analyzer.generic]` with `long_method_nloc`.
- MCP `scan`, `propose`, and prompt-mode `fix` now accept:
  - `config`: optional `deslop.toml` path for analyzer settings.
  - `analyzer`: inline overrides, including per-language `long_method_nloc`.
- Inline MCP analyzer settings override the config file for that tool call.
- Updated `docs/CONFIG.md` and `deslop.toml.example`.

Verification run:
- `cargo fmt --all && cargo test -p deslop-analyzer && cargo test -p deslop-cli && cargo test -p deslop-mcp`: pass.
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo build -p deslop-slim --no-default-features`: pass.
- `cargo test --workspace`: pass.
- `cargo test -p deslop-mcp --features slim-llm`: pass.
- `cargo clippy --workspace -- -D warnings`: pass.

Blockers:
- None for this config/MCP follow-up.

Next actions:
- None required. Future analyzer thresholds can follow the same global-plus-language
  shape if they become language-sensitive.

Signature: Codex

---

# Session Report — Dogfood Refactor Continuation

Date/time: 2026-06-24T12:36:43+02:00 Europe/Madrid

Objective: Continue refactoring deslop's own code after the dogfood cleanup checkpoint,
preserving behavior and gating each edited area.

Working-copy context:
- Continued in jj change `ysrptkzp` (`Dogfood debloat refactor pass`).
- Did not touch analyzer thresholds or detector semantics.
- Did not chase known false positives in redundant-closure/needless-clone ownership cases.

Changes made in this continuation:
- `deslop-metrics`: split hotspot detection, ranking, text rendering, and region metric
  construction into smaller helpers.
- `deslop-external`: split clippy JSON-line parsing into focused helpers.
- `deslop-verify`: shared coverage-assessment construction across LCOV/line coverage,
  split native mutation runner steps, moved the cosmic-ray SQLite script constant, and
  extracted verify test setup/assertion helpers.
- `deslop-analyzer`: split path walking/report pushing, token duplicate candidate matching,
  tokenizer masked-token handling, Rust tree-walk rule collection, Clojure regex capture
  walking, and analyzer corpus fixture assertions.
- `deslop-lsp`: split didOpen/didChange/didSave/didClose notification handlers.
- `deslop-mcp`: extracted tool-schema assertions, propose/result helpers, Rust LCOV fixture
  setup, and feature-gated slim mock scenario helpers.
- `deslop-mutate`: compacted exact mutant expectation tests through a shared assertion helper.
- `deslop-slim`: shared single-result, gating-count, written-path, and source-text assertions.
- `deslop-parse`: shared region assertion helper in tests.

Measured before/after:
- Starting checkpoint aggregate: score `7.294818761848697`,
  `comment-block=1`, `duplicate-block=33`, `long-method=19`,
  `magic-number=16`, `near-duplicate=55`.
- Final aggregate: score `5.2988593374181185`,
  `comment-block=1`, `duplicate-block=21`, `magic-number=16`,
  `near-duplicate=54`; no `long-method` findings.
- Honest split:
  - Real removals: all remaining long-method findings eliminated (`19 -> 0`);
    duplicate-block count reduced (`33 -> 21`).
  - Mostly residual/non-removable or low-value: `magic-number=16` unchanged,
    `comment-block=1` unchanged, near-duplicate only marginally changed (`55 -> 54`).

Focused gates after edits:
- `cargo fmt --all && cargo test -p deslop-metrics`: pass.
- `cargo fmt --all && cargo test -p deslop-external`: pass.
- `cargo fmt --all && cargo test -p deslop-verify`: pass after each verify refactor.
- `cargo fmt --all && cargo test -p deslop-analyzer`: pass after analyzer refactors.
- `cargo fmt --all && cargo test -p deslop-lsp`: pass.
- `cargo fmt --all && cargo test -p deslop-mcp`: pass.
- `cargo fmt --all && cargo test -p deslop-mutate`: pass.
- `cargo fmt --all && cargo test -p deslop-slim && cargo build -p deslop-slim --no-default-features`: pass.
- `cargo fmt --all && cargo test -p deslop-parse`: pass.

Final verification:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo build -p deslop-slim --no-default-features`: pass.
- `cargo test --workspace`: pass.
- `cargo clippy --workspace -- -D warnings`: pass.
- `cargo test -p deslop-mcp --features slim-llm`: pass, 11 tests.
- `cargo run -q -p deslop-cli -- scan crates --format text | rg 'long-method'`: no matches.

Residual hotspots/blockers:
- No verification blockers.
- Residual duplicate/near-duplicate clusters are mostly:
  - rule-table/test assertion structure in analyzer/MCP/slim/verify,
  - SARIF JSON-path assertions,
  - intentional provider/client API shape similarities,
  - low-value tiny test file near-duplicate noise.
- Further reductions are possible but now skew toward macro-like table compaction,
  assertion DSLs, or detector false-positive pressure rather than high-value refactoring.

Signature: Codex

## 2026-06-24T11:22:17+02:00 — Dogfood Debloat Refactor Pass

Objective: Deslop the codebase with behavior-preserving refactors, measured before/after and gated by cargo verification.

Change context:
- Started from jj change `ysrptkzp` on parent `lkrnsqtk` (`Add project README`).
- Preserved the existing README parent change; this pass only edited Rust sources and this report.

Before measurement:
- `target/debug/deslop slop crates --format json`
  - score: 11.982529004741801
  - comment-block=1
  - duplicate-block=44
  - long-method=41
  - magic-number=16
  - near-duplicate=59

Refactors completed:
- `deslop-slim`: split `run_slim_with_progress` orchestration into rewrite, verification, apply/report, and progress helpers; extracted deterministic test fixture setup and progress assertions.
- `deslop-lsp`: extracted code-action/test JSON-RPC helpers and shared safe action counting.
- `deslop-analyzer`: table-driven Julia rules; shared Clojure code-line scanning, safe edit construction, redundant-do finding construction, and precondition rule helper.
- `deslop-fix`: split `fix_paths` into fixable grouping, per-path application, atomic write, temp path, and shared fixable predicate.
- `deslop-eval`: shared JSON file loading; split corpus summary, case scanning, expectation scoring, unmatched finding scoring, finalized rule scores, and overall score.
- `deslop-parse`, `deslop-report`, `deslop-mutate`: small helper extractions for repeated test assertions and boolean mutation replacement.
- `deslop-mcp`: split the long MCP tool catalog into per-tool schema builders, keeping dispatch untouched.
- `deslop-cli`: split slim progress formatting, analyzer threshold extraction, config test assertions, and the `fix` command path into request resolution and provider execution helpers.

After measurement:
- `cargo run -q -p deslop-cli -- slop crates --format json`
  - score: 7.294818761848697
  - comment-block=1
  - duplicate-block=33
  - long-method=19
  - magic-number=16
  - near-duplicate=55

Notable file-level improvements:
- `crates/deslop-analyzer/src/julia.rs`: score 42.37 -> 0.00.
- `crates/deslop-slim/src/lib.rs`: score 20.29 -> 9.16; long-method 8 -> 0.
- `crates/deslop-lsp/src/lib.rs`: score 18.13 -> 5.20; long-method 4 -> 1, duplicate-block 4 -> 0.
- `crates/deslop-eval/src/lib.rs`: score 18.99 -> 3.60; long-method 2 -> 0.
- `crates/deslop-cli/src/main.rs`: score 11.88 -> 5.81; long-method 4 -> 0.
- `crates/deslop-fix/src/lib.rs`: score 14.12 -> 5.26; long-method 1 -> 0.

Verification:
- `cargo fmt --all`: pass.
- `cargo build --workspace`: pass.
- `cargo build -p deslop-slim --no-default-features`: pass.
- `cargo test --workspace`: pass.
- `cargo clippy --workspace -- -D warnings`: pass.
- Additional MCP feature check run after MCP schema refactor: `cargo test -p deslop-mcp --features slim-llm`: pass.

Residual hotspots:
- Remaining aggregate counts are mostly `deslop-verify`, MCP test scenarios, metrics/reporting helpers, exact expected-output vectors, and analyzer/token structural repetition.
- `magic-number` and `comment-block` counts were intentionally unchanged; these are not meaningful cleanup targets in this pass.
- Known precision-sensitive residuals such as redundant-closure false positives and harmless structural near-duplicates were not chased.

Blockers:
- No build/test/clippy blockers.
- No external-tool blocker affected this pass because verification used deterministic/unit-test surfaces.

Signature: Codex

## 2026-06-24T09:41:37+02:00 — Native Tree-Sitter Mutation Engine Complete

Objective: Finish `.agents/NEXT_TASK.md` Task 14: native tree-sitter mutation
generation, verifier scoring, timeout handling, and coverage-gated mutation.

Changes:
- Completed P2/P3 on top of the P1 checkpoint in jj change `lmmlzykp`.
- Added native `TreeSitterMutationProbe` as the `MutationConfig::Auto` default.
- Kept external recorded outcomes and live external probes as opt-in paths:
  - `MutationConfig::OutcomesFile` for recorded cargo-mutants/cosmic-ray style outcomes.
  - `MutationConfig::AutoWithCommand` for the previous cargo-mutants/cosmic-ray command probes.
- Threaded resolved `check_cmd`, coverage assessment, and per-mutant timeout into
  `MutationRequest`.
- Extended coverage assessment with covered line sets; native mutation restricts generated
  mutants to covered work-order lines when coverage data is present, and mutates the whole
  region when coverage is disabled/unknown.
- Added `wait-timeout` for per-mutant timeouts; timeout is classified as killed.
- Updated `SPEC.md`.

Tests:
- P1 exact CST mutant generation tests for Rust, Clojure, Julia, and Python.
- Native verifier tests for:
  - survived mutant downgrading a non-empty rewrite to `untested-risky`;
  - content-keyed check command killing all mutants;
  - timeout counting as killed;
  - covered-line restriction skipping uncovered-line mutants.
- Existing cargo-mutants/cosmic-ray recorded outcome tests remain passing.

Verification:
- Focused checks passed:
  - `cargo fmt --all && cargo test -p deslop-mutate`
  - `cargo fmt --all && cargo test -p deslop-verify`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Deferred:
- Equivalent-mutant pruning.
- Parallel mutant scoring.
- Finer distinction between check-command build failures and behavior-killed mutants.

Signature: Codex

## 2026-06-24T09:34:11+02:00 — Native Tree-Sitter Mutation Engine P1

Objective: Execute `.agents/NEXT_TASK.md` Task 14. Round 1 completed P1 only:
pure CST mutant generation in a new `deslop-mutate` crate.

Changes:
- Started jj change `lmmlzykp` on top of `xumlpqvs`.
- Added `deslop-mutate`, a pure tree-sitter mutant generation crate.
- Added portable operators:
  - relational swaps: `<`/`<=`, `>`/`>=`, `==`/`!=`; Clojure uses `=`/`not=`.
  - arithmetic swaps: `+`/`-`, `*`/`/`.
  - logical swaps: `&&`/`||`, Python `and`/`or`, Clojure `and`/`or`.
  - boolean literal flip.
  - condition negation.
- Wired `tree-sitter-python` into `deslop-lang` so Python participates in CST mutation.
- Adjusted Python verifier fixtures that had relied on TODO text inside strings; with Python CST,
  that string content is correctly ignored by the incompleteness rule.

Tests:
- Exact mutant-generation tests for Rust, Clojure, Julia, and Python.
- Restrict-lines generation test.
- Mutated-source output test.

Verification:
- Focused check passed:
  - `cargo fmt --all && cargo test -p deslop-mutate`
- Full round gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Next:
- P2/P3: integrate native `TreeSitterMutationProbe` into `deslop-verify`, add
  content-keyed scoring tests, timeout handling, and coverage-gated line restriction.

Signature: Codex

## 2026-06-24T08:38:26+02:00 — LSP Edges Final Queue Item

Objective: Execute `.agents/NEXT_TASK.md` Task 13 only: sharpen LSP diagnostics/code
actions/RPC coverage in priority order, keep dependencies isolated to `deslop-lsp`, and
complete the queued task list.

Changes:
- Started new jj change `xumlpqvs` on top of `oszlxpvn`.
- P1 precise UTF-16 diagnostics:
  - `Finding.span` byte offsets now map to LSP `Position` columns in UTF-16 code units.
  - Mapping handles multibyte UTF-8 without slicing at non-character boundaries.
- P2 fix-all:
  - Added `source.fixAll` action titled `deslop: fix all safe findings in file`.
  - Fix-all uses `deslop_fix::apply_findings_to_text` over all `SafeAuto` and
    `AnalyzerConfirmed` findings with edits.
  - Per-finding quickfixes remain.
  - Riskier classes still get no edit action.
- P3 real JSON-RPC loop test:
  - Uses `lsp_server::Connection::memory`.
  - Drives `initialize -> initialized -> didOpen -> publishDiagnostics -> codeAction ->
    shutdown -> exit` through the real `run_connection` loop.
- P4 partial:
  - Implemented incremental sync capability and ranged `didChange` application with UTF-16
    position-to-byte conversion.
  - Deferred workspace-wide scan. Reason: it needs explicit workspace-root semantics,
    cost controls, and dirty-buffer overlay behavior so unopened-file diagnostics do not
    conflict with open in-memory state.
- Updated `SPEC.md`.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Non-ASCII diagnostic range test verifies byte offsets map to UTF-16 columns.
- Fix-all test verifies two safe Clojure findings are edited together and riskier findings
  do not produce fix-all.
- Existing quickfix test updated to prove per-finding quickfixes still exist.
- Incremental sync test applies a UTF-16 ranged edit over non-ASCII text.
- Real JSON-RPC loop integration test covers diagnostics and quickfix/fix-all actions.

Verification:
- Focused check passed:
  - `cargo fmt --all && cargo test -p deslop-lsp`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Queue status:
- Task 13 is the last queued item. Items 1-13 are now implemented or explicitly deferred
  where documented.

Blockers:
- Workspace-wide LSP scan deferred for the design reasons above.

Signature: Codex

## 2026-06-24T08:24:27+02:00 — Slim Progress Events

Objective: Execute `.agents/NEXT_TASK.md` Task 12 only: add streaming-style slim
progress events, render CLI progress to STDERR without changing STDOUT, keep MCP no-op,
and do not start queued item 13.

Changes:
- Started new jj change `oszlxpvn` on top of `qpywotro`.
- Added `SlimProgress` and `SlimProgressOutcome` in `deslop-slim`.
- Added `run_slim_with_progress(client, options, sink)` and kept `run_slim` as the
  compatibility wrapper with a no-op sink. This avoids forcing MCP/tests to pass a callback
  while allowing CLI progress.
- Emitted events at existing slim loop points:
  - `Started`
  - `Rewriting`
  - `Characterizing`
  - `Verified`
  - `Outcome`
  - `Finished`
- Wired CLI `deslop fix` to render progress to STDERR:
  - default enabled only when STDERR is a TTY
  - new `--quiet` suppresses it
  - non-TTY STDERR is silent by default to avoid noisy CI/piped runs
  - STDOUT remains the final JSON report only.
- Left MCP `fix mode=auto` on `run_slim` / no-op progress sink; MCP streaming remains
  deferred.
- Updated `docs/CONFIG.md` and `SPEC.md`.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- `deslop-slim`: recording sink over a mock run asserts event sequence:
  `Started -> Rewriting -> Verified -> Outcome -> Finished`.
- `deslop-slim`: progress sink does not change the final report serialization.
- `deslop-cli`: progress written to a STDERR buffer does not change final report STDOUT
  rendering; help lists `--quiet`.

Verification:
- Focused checks passed:
  - `cargo fmt --all && cargo test -p deslop-slim && cargo test -p deslop-cli`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
- Required feature gate passed:
  - `cargo test -p deslop-mcp --features slim-llm`

Not started:
- Queued item 13.

Blockers:
- None.

Signature: Codex

## 2026-06-24T08:04:14+02:00 — Source Egress Consent

Objective: Execute `.agents/NEXT_TASK.md` Task 11 only: gate real-provider bundled LLM
calls behind affirmative source-egress consent, keep mock/RecordedClient local runs
unblocked, and do not start queued items 12-13.

Changes:
- Started new jj change `qpywotro` on top of `quvrtxsu`.
- Added shared pure consent primitives in `deslop-slim`:
  - `EgressDecision::{Granted, Prompt, DeniedNonInteractive}`
  - `resolve_egress_consent(explicit, is_interactive)`
  - env parsing for `DESLOP_SLIM_CONSENT`
  - provider/base-url message helpers
  - source-egress summary counting unique files and rewrite regions.
- Wired CLI `deslop fix`:
  - new `--yes` flag with `--consent` alias
  - `[slim] egress_consent = true`
  - consent sources: CLI flag > env/config folded into explicit consent > TTY prompt
  - real providers resolve consent before building `AnthropicClient`/`OpenAiClient`
  - prompt/error states provider, base URL, file count, and region count
  - API keys are never printed or read from config.
- Wired MCP `fix mode=auto` behind `slim-llm`:
  - schema adds `consent` and `config`
  - server is non-interactive, so real providers require explicit consent via `consent:
    true`, `DESLOP_SLIM_CONSENT=1`, or `[slim] egress_consent = true`
  - missing consent errors before provider-client construction/API-key lookup
  - mock/RecordedClient path bypasses consent.
- Added `egress_consent` to `deslop.toml.example`.
- Updated `docs/CONFIG.md` and `SPEC.md`.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- `deslop-slim`: truth table for `resolve_egress_consent`; env/message/base-url
  determinism.
- `deslop-cli`: flag/env/config consent sources grant independently; all config parsing
  includes `egress_consent`; help lists `--yes`.
- `deslop-mcp --features slim-llm`: real provider without consent returns the clear
  source-egress error without mentioning API keys; config consent parser works; existing mock
  e2e still passes without consent.

Verification:
- Focused checks passed:
  - `cargo fmt --all && cargo test -p deslop-slim && cargo test -p deslop-cli && cargo test -p deslop-mcp --features slim-llm`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
- Required feature gate rerun passed:
  - `cargo test -p deslop-mcp --features slim-llm`

Not started:
- Queued items 12-13.

Blockers:
- None.

Signature: Codex

## 2026-06-24T07:50:53+02:00 — Non-Rust Coverage Auto Wiring

Objective: Execute `.agents/NEXT_TASK.md` Task 10 only: make non-Rust coverage
providers' Auto/AutoWithCommand modes actually invoke live coverage tools where needed,
keep recorded file parsers and graceful degrade behavior intact, and do not start queued
items 11-13.

Before-state:
- `ClojureCloverageProvider` Auto was already live: it ran
  `lein cloverage --json --output <temp>` and parsed generated `coverage.json`.
- `JuliaCoverageProvider` Auto was incomplete: it only ran a Coverage.jl post-processing
  command and depended on preexisting `.cov` data.
- `PythonCoveragePyProvider` Auto was incomplete: it only ran `coverage json -o ...` and
  depended on preexisting `.coverage` data.

Changes:
- Started new jj change `quvrtxsu` on top of `mvnszkqq`.
- Added pure command builders for deterministic tests:
  - Clojure: `<cmd> cloverage --json --output <temp-dir>`
  - Julia: `<cmd> --startup-file=no --code-coverage=user -e "using Pkg; Pkg.test()"`
  - Python run: `<cmd> run -m unittest discover`
  - Python report: `<cmd> json -o <temp>/coverage.json`
- Kept `AutoWithCommand(cmd)` as executable override only; deslop still supplies the
  generated arguments.
- Refactored Clojure live execution through the builder while preserving its existing
  output strategy and parser.
- Reworked Julia Auto:
  - checks `julia --version`
  - copies the project to a temp directory
  - runs `Pkg.test()` under `--code-coverage=user`
  - locates generated `.cov` files in the temp copy
  - parses them with the existing `.cov` line parser after normalizing paths back to the
    original project root.
- Reworked Python Auto:
  - checks `coverage --version`
  - runs `coverage run -m unittest discover` with `COVERAGE_FILE` in a temp dir
  - runs `coverage json -o <temp>/coverage.json`
  - parses the generated JSON with the existing coverage.py parser.
- Any missing tool, failing command, or missing generated report still returns
  `CoverageStatus::Unknown` with a notice; it never rejects by itself.
- Updated `SPEC.md` with live coverage commands and report-location strategy.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Added command-construction tests for Clojure, Julia, and Python default and override
  command behavior.
- Added Auto-mode default mapping tests for `lein`, `julia`, and `coverage`.
- Added absent-tool verify-path degrade tests for Clojure, Julia, and Python; verdicts stay
  `CoverageUnknown`, not rejected.
- Existing recorded cloverage, Coverage.jl `.cov`, and coverage.py file parser tests remain
  green.

Verification:
- Focused gate passed:
  - `cargo fmt --all && cargo test -p deslop-verify`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Not unit-tested:
- Live successful runs, because they require the language toolchains/plugins/test
  dependencies to be installed in the target project.

Not started:
- Queued items 11-13.

Blockers:
- None.

Signature: Codex

## 2026-06-24T07:38:00+02:00 — Python Mutation Probe

Objective: Execute `.agents/NEXT_TASK.md` Task 9 only: add a real non-Rust mutation
probe where upstream tooling supports it, document Clojure/Julia blockers honestly, and
do not start queued items 10-13.

Changes:
- Started new jj change `mvnszkqq` on top of `mtxlzmys`.
- Added `PythonMutationProbe` in `deslop-verify`, registered alongside
  `RustCargoMutantsProbe` in `MutationRegistry`.
- Chose Cosmic Ray for Python because it is a Python mutation-testing tool with a project
  config, durable SQLite session, and source path/line outcome data that deslop can reduce
  to the existing `MutantOutcomes` region contract.
- Added live-mode behavior:
  - checks `cosmic-ray --version`
  - looks for a project Cosmic Ray config (`cosmic-ray.toml`, `cosmic_ray.toml`,
    `cosmic-ray.ini`, or `cosmic_ray.ini`)
  - runs `cosmic-ray init` and `cosmic-ray exec`
  - dumps the resulting SQLite session through Python stdlib `sqlite3`
  - degrades to `mutation-unknown` when the command/config/session inspection is absent or
    failing.
- Added recorded fixture parsing for Cosmic Ray-shaped source path/line outcomes.
- Added minimal Python language-pack registration so verifier work-order discovery can see
  `.py` fixtures; no Python-specific analyzer rules were added.
- Updated `SPEC.md` with mutation-tier coverage for Rust/Python and the Clojure/Julia
  deferrals.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Added deterministic Python mutation tests:
  - recorded Cosmic Ray survivor in region downgrades the passing patch to
    `UntestedRisky`
  - recorded killed/no-survivor outcome leaves the verdict at `CoverageUnknown`
  - absent Cosmic Ray auto command returns a mutation notice and does not reject the patch.
- Kept the existing cargo-mutants mutation tests green.

Clojure/Julia investigation:
- Clojure:
  - PITest-style JVM bytecode mutation does not provide the source-region contract deslop
    needs.
  - Heretic is Clojure-specific and promising, with JSON/EDN reporting, but the upstream
    README currently marks it experimental/not released and warns not to depend on the API
    or behavior. Deferred until its source-line machine-readable contract is stable enough
    for verifier gating.
- Julia:
  - Vimes.jl is the older mutation-testing path and reports patch diffs, but is legacy.
  - Gremlins.jl is a new 0.x source-splicing project announced in June 2026; it looks
    promising, but its report contract is too new for a stable verifier input. Deferred.

Verification:
- Focused gate passed:
  - `cargo fmt --all && cargo test -p deslop-verify`
- Full required gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Not started:
- Queued items 10-13.

Blockers:
- None for Python.
- Clojure/Julia mutation probes are blocked on stable, maintained, source-mappable
  machine-readable report contracts.

Signature: Codex

## 2026-06-24T07:21:54+02:00 — Analyzer Threshold Config

Objective: Execute `.agents/NEXT_TASK.md` Task 8 only: move the remaining analyzer
threshold constants into `AnalyzerConfig` and expose them through `deslop.toml [analyzer]`.
Do not start queued items 9-13.

Changes:
- Started new jj change `mtxlzmys` on top of `svrplorq`.
- Added `AnalyzerConfig` fields:
  - `long_method_nloc: usize`, default `40`
  - `min_meaningful_tokens: usize`, default `8`
  - existing `min_duplication_tokens` remains default `24`.
- Replaced `agnostic.rs` `LONG_METHOD_NLOC` usage with `config.long_method_nloc`.
- Replaced `tokens.rs` `MIN_MEANINGFUL_TOKENS` usage with
  `config.min_meaningful_tokens`.
- Threaded `&AnalyzerConfig` through agnostic duplicate-token calls so tokens can read both
  duplication thresholds from the same config.
- Extended CLI `[analyzer]` config parsing to accept:
  - `min_duplication_tokens`
  - `long_method_nloc`
  - `min_meaningful_tokens`
- Updated `deslop.toml.example`, `docs/CONFIG.md`, and `SPEC.md`.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Added analyzer default-preservation test for `24/40/8`.
- Added long-method config behavior test:
  - 39-NLOC Rust function is not flagged at default `long_method_nloc = 40`
  - same source is flagged when `long_method_nloc = 20`.
- Added duplicate-token config behavior test:
  - small duplicate fixture is suppressed with default `min_meaningful_tokens = 8`
  - same fixture emits `duplicate-block` when `min_meaningful_tokens = 1`.
- Extended CLI all-sections TOML parse test to assert all three analyzer threshold values
  reach `AnalyzerConfig`.

Verification:
- Focused checks passed:
  - `cargo test -p deslop-analyzer`
  - `cargo test -p deslop-cli`
- Full gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Not started:
- Queued items 9-13.

Blockers:
- None.

Signature: Codex

## 2026-06-24T07:08:19+02:00 — MCP Fix Auto Mode

Objective: Execute `.agents/NEXT_TASK.md` Task 7 only: add opt-in MCP `fix`
server-run LLM mode behind a `deslop-mcp` cargo feature while keeping default MCP builds
network-free. Do not start queued items 8-13.

Changes:
- Started new jj change `svrplorq` on top of `znzxmqym`.
- Added `deslop-mcp` cargo feature:
  - `slim-llm = ["deslop-slim/anthropic", "deslop-slim/openai"]`
  - default features remain empty.
- Extended MCP `fix` tool schema with `mode`:
  - `mode = "prompts"` default, always available, unchanged `deslop.fix/1` option-B output.
  - `mode = "auto"` opt-in option A, returning `deslop.slim/1`.
- Added auto-mode arguments:
  - `paths`, `provider`, `model`, `base_url`, `apply`, `allow_unverified`, `coverage`,
    `check_cmd`, `characterize`, `mock`.
- With `slim-llm` disabled, `mode=auto` returns the clear error:
  - `fix mode=auto requires deslop-mcp built with --features slim-llm`
- With `slim-llm` enabled, auto mode:
  - uses `RecordedClient::from_path` when `mock` is supplied
  - otherwise builds `AnthropicClient` or `OpenAiClient` from env-only API keys
  - resolves model through existing `deslop_slim::resolve_model`
  - parses coverage through shared `parse_coverage_mode`
  - runs `deslop_slim::run_slim` and returns its report JSON.
- Updated `SPEC.md` to document prompt-vs-auto MCP fix modes, the `slim-llm`
  feature, default network-free behavior, and feature-mode mock coverage.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Default build:
  - existing prompts test still verifies `schema = "deslop.fix/1"` and prompt payload shape.
  - new test verifies `mode=auto` returns the feature-required error.
  - tools/list schema test verifies `mode` enum/default and `slim-llm` documentation.
- Feature build:
  - new deterministic mock test under `--features slim-llm`:
    - LCOV-covered Rust `todo!` rewrite returns `deslop.slim/1`, verifies `removable`, and writes.
    - rejected rewrite remains rejected and does not write, even with `allow_unverified`.

Verification:
- Initial feature test run hung because the new test held the shared temp-fixture lock while
  constructing a second fixture. Fixed by scoping the first fixture so its guard drops before
  the second fixture is created.
- Default gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
- Default MCP network-free proof:
  - `cargo tree -p deslop-mcp -i ureq`
  - exited with Cargo's expected absence message: `package ID specification 'ureq' did not match any packages`.
- Feature gate passed:
  - `cargo test -p deslop-mcp --features slim-llm`
  - `cargo clippy -p deslop-mcp --features slim-llm -- -D warnings`

Not started:
- Queued items 8-13.

Blockers:
- None.

Signature: Codex

## 2026-06-23T23:14:29+02:00 — Project Config File

Objective: Execute `.agents/NEXT_TASK.md` Task 6 only: extend `deslop.toml`
project defaults for scan/fix/slim/analyzer while keeping `[external]` working, add
`--config`, document precedence, and complete the queued task list.

Changes:
- Continued in new jj change `znzxmqym` on top of `lnlzsupu`.
- Added global `--config <path>` with default `deslop.toml`; absent config files keep
  built-in defaults.
- Extended `DeslopConfig` with:
  - `[slim] provider/model/base_url`
  - `[fix] check_cmd/coverage/allow_unverified`
  - `[scan] fail_on/baseline`
  - `[analyzer] min_duplication_tokens`
  - existing `[external] clippy/julia_analyzer/julia_project` unchanged.
- Threaded the loaded config into `scan`, `propose`, and bundled `fix`.
- Added explicit resolution helpers for the affected options:
  - CLI > env > config > default for slim model (`DESLOP_SLIM_MODEL`)
  - CLI > config > default for scan/fix/slim fields without env equivalents.
- Kept API keys env-only; config never reads Anthropic/OpenAI/DESLOP slim API keys.
- Updated `fix` parsing so `--provider`, `--coverage`, and `--allow-unverified` retain
  "not supplied" state for config precedence. `--allow-unverified=false` is supported to
  override a true config value.
- Added `deslop.toml.example` and `docs/CONFIG.md`.
- Updated `SPEC.md` to document the implemented config surface and remove older
  unimplemented config promises.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Added deterministic CLI unit coverage for:
  - all config sections parsing
  - slim model precedence across CLI/env/config/default
  - scan fail-on/baseline precedence
  - fix coverage parsing through `parse_coverage_mode`
  - boolean forms for `--allow-unverified`.
- Existing external config tests remain green.

Verification:
- First full gate failed at clippy only:
  - needless borrow in `read_from`
  - needless struct update after setting all `AnalyzerConfig` fields.
- Fixed both clippy findings.
- Full gate passed:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
- Smoke:
  - `cargo run -q -p deslop-cli -- --config /tmp/nonexistent-deslop.toml scan tests/corpus/clean --format json >/tmp/deslop-config-smoke.json && wc -c /tmp/deslop-config-smoke.json`
  - passed; output size 1389 bytes. The command emitted the expected clj-kondo fallback
    notice because clj-kondo is not installed locally.

Deferred:
- Additional analyzer threshold knobs, including long-method thresholds, remain deferred
  until `AnalyzerConfig` owns those values directly.

Queue status:
- Task 6 is complete. This was the last queued item.

Blockers:
- None.

Signature: Codex

## 2026-06-23T23:00:02+02:00 — CI and Pre-commit Packaging

Objective: Execute `.agents/NEXT_TASK.md` Task 5 only: package existing deslop scan
gates for GitHub Actions and pre-commit, document CI usage, and add/cite fail-on exit-code
coverage. Do not start queued item 6.

Changes:
- Started a new jj change `lnlzsupu` on top of `wvzwxyuw`.
- Added root `action.yml` composite action:
  - inputs: `paths`, `fail-on`, `sarif`, optional `baseline`
  - installs deslop with `cargo install --path crates/deslop-cli --locked`
  - writes `deslop.sarif` via `deslop scan --format sarif ... > deslop.sarif`
  - runs the existing `deslop scan --fail-on <severity>` gate
  - passes `--baseline` when a baseline path is provided.
- Added `.github/workflows/deslop.yml` example:
  - checkout
  - Rust toolchain
  - local composite action
  - `github/codeql-action/upload-sarif@v3` with `if: always()`.
- Added `.pre-commit-hooks.yaml` with a system `deslop scan --fail-on major` hook and
  `pass_filenames: true`.
- Added `docs/CI.md` documenting:
  - GitHub Action usage
  - SARIF upload/code scanning
  - fail-on exit-code contract
  - baseline ratchet workflow
  - pre-commit consumer and local examples.
- Added `crates/deslop-cli/tests/scan_exit_codes.rs`, a process-level integration test for
  the built `deslop` binary:
  - sloppy Rust fixture with `todo!` exits non-zero under `--fail-on major`
  - clean Rust fixture exits zero.
- Added `tempfile` as a `deslop-cli` dev-dependency for that integration test.
- Updated `SPEC.md` with the CI/pre-commit packaging note and the exit-code/SARIF test
  coverage note.
- Touched `.agents/HEARTBEAT.md`.

YAML verification:
- `python3 - <<'PY' ... yaml.safe_load(...) ... PY`
  - `action.yml`: ok
  - `.github/workflows/deslop.yml`: ok
  - `.pre-commit-hooks.yaml`: ok
- Initial YAML parse caught an unquoted colon in `action.yml`; fixed by quoting the
  `fail-on` input description.

Rust verification:
- `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- Existing SARIF schema/shape coverage remains
  `deslop_report::tests::sarif_render_has_required_shape_and_locations`.
- MCP network-free check:
  - `cargo tree -p deslop-mcp -i ureq`: exits with no matching `ureq` package.

Not started:
- Queue item 6: config file.

Blockers:
- None.

Signature: Codex

## 2026-06-23T22:47:59+02:00 — LSP Server MVP

Objective: Execute `.agents/NEXT_TASK.md` Task 4 only: add an MVP synchronous LSP
server with live diagnostics and safety-gated code actions. Do not start queued items 5-6.

Changes:
- Started a new jj change `wvzwxyuw` on top of `wnyosyly`.
- Added workspace crate `crates/deslop-lsp`.
- Added binary `deslop-lsp`.
- Added justified LSP dependencies:
  - `lsp-server = 0.7.9`
  - `lsp-types = 0.97.0`
- Implemented a synchronous stdio LSP loop with `lsp_server::Connection`.
- Initialize capabilities:
  - `text_document_sync = FULL`
  - `code_action_provider = true`
- Maintains an in-memory `Uri -> { text, findings, version }` document map.
- Handles:
  - `textDocument/didOpen`
  - full-document `textDocument/didChange`
  - `textDocument/didSave`
  - `textDocument/didClose`
  - `textDocument/codeAction`
  - shutdown via `lsp-server`.
- Diagnostics analyze the in-memory text through `deslop_analyzer::scan_source`; no rule
  logic is duplicated.
- Finding -> diagnostic mapping:
  - range: zero-based whole-line range derived from `Finding.span`
  - severity: `Major -> ERROR`, `Minor -> WARNING`, `Info -> HINT`
  - source: `deslop`
  - code: finding rule
  - message: finding message.
- Code actions enforce the fix-safety lattice:
  - only `SafeAuto` and `AnalyzerConfirmed` findings with edits produce a `quickfix`
  - other safety classes produce no edit
  - edit generation reuses `deslop_fix::apply_findings_to_text`
  - MVP returns a whole-document `WorkspaceEdit` via `documentChanges`.
- Updated `SPEC.md` with the LSP crate, binary, sync deps, behavior, tests, and deferrals.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- Pure diagnostic mapping test verifies range, severity, source, code, and message.
- Pure code-action gating test verifies:
  - a safe fixable finding yields a quickfix with a non-empty edit
  - an `LlmOnly` finding yields no quickfix.

Verification:
- First gate caught a `didChange` version type mismatch; fixed by wrapping the version in
  `Some(...)`.
- Second gate passed tests but clippy rejected `WorkspaceEdit::changes` because
  `lsp_types::Uri` is a mutable key type; switched to `documentChanges`.
- After clippy fix:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- After SPEC/report update:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- MCP network-free check:
  - `cargo tree -p deslop-mcp -i ureq`: exits with no matching `ureq` package.

Deferred:
- Incremental sync.
- Precise UTF-16 columns beyond whole-line MVP ranges.
- Workspace-wide scan.
- Multi-fix/source actions.
- Full RPC loop tests.

Not started:
- Queue item 5: CI/pre-commit packaging.
- Queue item 6: config file.

Blockers:
- None.

Signature: Codex

## 2026-06-23T22:30:57+02:00 — MCP Coverage-Mode Parity

Objective: Execute `.agents/NEXT_TASK.md` Task 3 only: lift the CLI coverage-mode parser
into `deslop-verify`, make MCP `verify`/`apply` accept coverage as bool or mode string,
and keep MCP network-free. Do not start queued items 4-6.

Changes:
- Started a new jj change `wnyosyly` on top of `txmxlptr`.
- Added public `deslop_verify::parse_coverage_mode(&str) -> Result<CoverageConfig>`.
- Moved the existing mode semantics into the shared parser without CLI behavior change:
  - `disabled` / `off` / `none`
  - `auto`
  - `auto:<cmd>`
  - `lcov:<path>`
  - `cloverage:<path>`
  - `julia-cov:<path>` / `julia:<path>`
  - `coverage-py:<path>` / `coverage.py:<path>` / `python:<path>`
- Updated `deslop-cli` to delegate its slim coverage parser to
  `deslop_verify::parse_coverage_mode`; the existing `parses_slim_coverage_modes` test
  remains green.
- Updated MCP `verify_options` so `coverage` accepts:
  - absent or `false` -> `CoverageConfig::Disabled`
  - `true` -> `CoverageConfig::Auto`
  - string -> shared `parse_coverage_mode`
  - other JSON types -> clear error.
- Updated MCP `verify` and `apply` tool schemas to document `coverage` as boolean or mode
  string and list supported modes.
- Updated `SPEC.md` for the shared parser and MCP coverage mode-string behavior.
- Touched `.agents/HEARTBEAT.md`.

Tests:
- MCP `apply` with `coverage: "lcov:<path>"` on a covered Rust region verifies
  `removable` and writes without `allow_non_removable`.
- MCP `verify` back-compat:
  - absent coverage -> disabled / `coverage-unknown`
  - `coverage: false` -> disabled / `coverage-unknown`
  - `coverage: true` -> Auto / graceful coverage-unknown path
- MCP bad mode string returns a clear unsupported-mode error instead of panicking.
- Tool-schema test checks `coverage` has bool|string `anyOf`, default false, and mode docs.

Verification:
- After implementation/tests:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- After SPEC/report update:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- MCP network-free check:
  - `cargo tree -p deslop-mcp -i ureq`: exits with no matching `ureq` package, so MCP
    still does not pull the HTTP client dependency.

Not started:
- Queue item 4: LSP server.
- Queue item 5: CI/pre-commit packaging.
- Queue item 6: config file.

Blockers:
- None.

Signature: Codex

## 2026-06-23T22:14:04+02:00 — Slim Characterization Generation Loop

Objective: Execute `.agents/NEXT_TASK.md` Task 2 only: add the
`deslop fix --characterize` characterization-test generation loop to `deslop-slim`,
without starting queued items 3-6.

Changes:
- Started a new jj change `txmxlptr` on top of `rqmuzkxm`.
- Added `SlimPrompt.kind` with `Rewrite` and `Characterization` variants.
- Added `build_characterization_prompt(&WorkOrder)` for current-behavior test prompts.
- Added `SlimOptions.characterize` and CLI flag `deslop fix --characterize`, default off.
- `run_slim` now:
  - runs the initial rewrite verification;
  - computes `characterization_work_orders_for_patches` for weak-oracle rewrites when
    `--characterize` is enabled;
  - sends characterization prompts through the existing `LlmClient`;
  - constructs `deslop.characterization-test/1` candidates;
  - accepts only tests passing `verify_characterization_tests` on current unmodified code;
  - re-runs `verify_patches` with accepted tests in
    `VerifyOptions.characterization_tests`;
  - passes the same accepted tests into `apply_patches`.
- Extended `SlimReport` with a `characterization` section containing attempts,
  accepted/rejected tests, and verdict upgrades before -> after.
- Updated `SPEC.md` for `deslop fix --characterize`, the slim characterization loop, and
  deterministic accept/reject test coverage.
- Touched `.agents/HEARTBEAT.md`.

Deterministic tests:
- Accept path: a `coverage-unknown` rewrite plus accepted generated test upgrades to
  `removable` and applies under default removable-only gating.
- Reject path: a generated test that fails current code is rejected, the rewrite remains
  `coverage-unknown`, and default `--apply` holds it without changing the file.
- Existing `RecordedClient`/provider parser tests remain no-network/no-key.

Verification:
- After core loop/test implementation:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- After SPEC/report update:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- Help smoke:
  - `cargo run -q -p deslop-cli -- fix --help`: pass; output includes
    `--characterize`.

Not started:
- Queue item 3: MCP coverage-mode parity.
- Queue item 4: LSP server.
- Queue item 5: CI/pre-commit packaging.
- Queue item 6: config file.

Blockers:
- None.

Signature: Codex

## 2026-06-23T21:23:33+02:00 — OpenAI-Compatible Slim Provider

Objective: Execute `.agents/NEXT_TASK.md` Task 1 only: add an OpenAI-compatible LLM
provider to `deslop-slim`, expose `deslop fix --provider anthropic|openai` and
`--base-url`, keep MCP network-free, and do not start queued tasks 2-6.

Changes:
- Started new jj change `rqmuzkxm` on top of `otlwomyy`.
- Added `deslop-slim` feature `openai = ["dep:ureq"]`.
- Updated `deslop-slim` defaults to `default = ["anthropic", "openai"]`.
- Kept both HTTP clients optional; `cargo build -p deslop-slim --no-default-features`
  compiles neither provider client.
- Added `OpenAiClient` behind `#[cfg(feature = "openai")]`:
  - POSTs to `{base_url}/chat/completions`.
  - Sends `{ "model": ..., "messages": [{"role":"user","content": prompt.text}] }`.
  - Parses `choices[0].message.content`.
  - Strips markdown fences via existing `strip_code_fences`.
  - Defaults `base_url` to `https://api.openai.com/v1`.
  - Reads `OPENAI_API_KEY`, falling back to `DESLOP_SLIM_API_KEY`.
  - Does not log API keys.
- Added pure parser test for OpenAI-compatible response JSON; no network/key needed.
- Added OpenAI endpoint joining test for trailing slash handling.
- Updated CLI:
  - `deslop fix --provider <anthropic|openai>` with default `anthropic`.
  - `deslop fix --base-url <URL>` for OpenAI-compatible providers.
  - `--mock` still bypasses provider construction.
  - `deslop-cli` enables both `anthropic` and `openai` slim features.
- Added CLI parser test for `--provider openai --base-url ...`; no network/key needed.
- Updated `SPEC.md` for providers and feature boundary.

Verification:
- After adding `OpenAiClient`:
  - `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- After CLI provider/base-url wiring:
  - same full gate: pass.
- Help smoke:
  - `cargo run -q -p deslop-cli -- fix --help`: pass; output includes
    `--provider <PROVIDER>` with possible values `anthropic, openai`, and `--base-url`.
- MCP network-free reconfirmation:
  - `cargo tree -p deslop-mcp -i ureq` returns no matching `ureq` package, proving `ureq`
    is not in the MCP dependency tree.

Not started:
- Queue item 2: characterization-test generation loop.
- Queue item 3: MCP coverage-mode parity.
- Queue item 4: LSP server.
- Queue item 5: CI/pre-commit packaging.
- Queue item 6: config file.

Blockers:
- None.

Signature: Codex

## 2026-06-23T20:58:02+02:00 — MCP Fix Tool Option B

Objective: Execute `.agents/NEXT_TASK.md`: add an MCP `fix` tool using option B
agent-as-consumer semantics. The MCP server must not call an LLM; it returns
deslop-slim prompts and fingerprints, and the caller submits resulting patches through the
existing verify-gated `apply` tool.

Changes:
- Started a new jj change on top of `kxunkwxn`:
  - working copy `otlwomyy`
  - parent `kxunkwxn`
- Feature-gated `deslop-slim`'s HTTP client:
  - `ureq` is now optional.
  - `default = ["anthropic"]`.
  - `anthropic = ["dep:ureq"]`.
  - `AnthropicClient`, the ureq call, and Anthropic response parsing are behind
    `#[cfg(feature = "anthropic")]`.
  - `build_prompt`, `SlimPrompt`, `RecordedClient`, `run_slim`, and gating/report types
    remain available with `--no-default-features`.
- Set workspace `deslop-slim` dependency to `default-features = false`.
- Enabled `deslop-cli`'s slim dependency with `features = ["anthropic"]` so CLI behavior is
  unchanged.
- Added `deslop-mcp` dependency on `deslop-slim` with `default-features = false`.
- Added MCP `fix` tool:
  - tool name: `fix`
  - output schema: `deslop.fix/1`
  - payload: `prompts[]` entries with `workorder_id`, `path`, `region` line range,
    `region_fingerprint`, `contract`, `findings`, and `prompt`
  - `next` text instructing the caller to rewrite regions, create `deslop.patch/1`, and
    call `apply`
- Reused `deslop_slim::build_prompt` and
  `deslop_protocol::workorder_region_fingerprint`.
- Did not add `AnthropicClient` or any LLM call to MCP.
- Updated `SPEC.md` to document MCP `fix`, `deslop.fix/1`, the network-free feature
  boundary, and server-run MCP client as deferred.

Test outcomes:
- MCP tools list includes `fix`.
- `fix_tool_returns_slim_prompts_for_agent_consumer` verifies `deslop.fix/1`, at least one
  prompt, matching `region_fingerprint`, and prompt text containing the region text plus
  finding message.
- Existing MCP scan/propose/verify/apply tests still pass.

Network-free proof:
- `cargo build -p deslop-slim --no-default-features`: pass.
- `cargo tree -p deslop-mcp`: shows `deslop-slim` but no `ureq` dependency.

Verification:
- After slim feature split:
  - initial gate failed because `resolve_model` still referenced the removed `env` import;
    changed it to `std::env::var`.
  - re-run full gate passed.
- After MCP tool wiring:
  - initial gate failed because Cargo does not allow disabling default features only at a
    member dependency when the workspace dependency has defaults enabled.
  - fixed by moving `deslop-slim` workspace dependency to `default-features = false` and
    enabling `anthropic` explicitly in `deslop-cli`.
  - re-run full gate passed:
    `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`.

Deferred:
- MCP option A: server-run client / server-side LLM.
- Streaming progress.
- Additional provider clients.

Blockers:
- None.

Signature: Codex

## 2026-06-23T20:29:34+02:00 — deslop-slim Apply-Gating Fix

Objective: Execute `.agents/NEXT_TASK.md` surgical fix for `deslop-slim` apply gating
inside the existing `kxunkwxn` slim change. Restore graded-removability semantics:
default `--apply` writes only `removable`; behavior-unproven non-rejected verdicts are held
unless `--allow-unverified` is explicit.

Changes:
- Removed slim's hardcoded `allow_non_removable = true`.
- Added `SlimOptions.allow_unverified` and `SlimOptions.coverage`.
- `verify_options` now passes the selected `CoverageConfig` and sets
  `allow_non_removable` from `allow_unverified`.
- Added `SlimReport.gating` with `applied`, `held_unproven`, and `rejected` buckets.
  Held-unproven verdicts carry the suggestion to pass `--coverage`, add
  characterization tests, or use `--allow-unverified`.
- Added `deslop fix --allow-unverified`.
- Added `deslop fix --coverage <MODE>` parser mapping to existing `CoverageConfig`
  variants:
  - `disabled`
  - `auto`
  - `auto:<cmd>`
  - `lcov:<path>`
  - `cloverage:<path>`
  - `julia-cov:<path>`
  - `coverage-py:<path>`
- Updated `SPEC.md` and this report.

Gating before -> after:
- Before: slim `--apply` used `coverage = Disabled` and `allow_non_removable = true`, so
  verified-but-unproven `coverage-unknown` rewrites were written.
- After: slim default `--apply` uses `allow_non_removable = false`; only `removable`
  verdicts write. `coverage-unknown`, `untested-risky`, and `dead-candidate` are held
  unless `--allow-unverified` is explicit. `rejected` remains blocked.

Tests:
- Default `--apply`, coverage disabled: parseable rewrite -> `coverage-unknown` ->
  held-unproven, not written, file unchanged.
- `--allow-unverified`: same `coverage-unknown` rewrite is applied.
- Rejected rewrite remains blocked in both default and `--allow-unverified` modes.
- LCOV fixture: covered Rust region -> `removable` -> applied by default.
- CLI parser covers all slim coverage modes above.

Verification:
- Initial core-only gate failed at build because the CLI had not yet been updated for new
  `SlimOptions` fields.
- After CLI wiring:
  - `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- After SPEC/report update:
  - `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- Help smoke:
  - `cargo run -q -p deslop-cli -- fix --help`: pass; output includes
    `--allow-unverified` and `--coverage <MODE> [default: disabled]`.

Standalone apply:
- Unchanged. `deslop apply` still writes only `removable` by default; its existing
  `--allow-non-removable` remains the explicit opt-in.

Blockers:
- None.

Signature: Codex

## 2026-06-23T20:00:18+02:00 — deslop-slim Reference Consumer

Objective: Execute `.agents/NEXT_TASK.md` to build the new `deslop-slim` crate as a
bundled LLM consumer: propose/load work orders, build prompts, call a swappable
`LlmClient`, emit `deslop.patch/1`, verify patches, and default to dry-run unless
`--apply` is explicit. Start from a separate `jj new` change and gate after each change.

Changes:
- Started a new jj change before implementation:
  - working copy `kxunkwxn`
  - parent `yrzlsulx`
- Added `crates/deslop-slim` as a workspace member.
- Added workspace `ureq = { version = "3.3", features = ["json"] }` and isolated it to
  the slim crate as the minimal synchronous HTTP client for Anthropic Messages.
- Implemented `deslop-slim`:
  - `LlmClient` trait with `fn rewrite(&self, prompt: &SlimPrompt) -> Result<String>`.
  - `AnthropicClient` using `ureq` against Anthropic Messages, `ANTHROPIC_API_KEY`, and
    a model resolved from `--model`, `DESLOP_SLIM_MODEL`, or `claude-sonnet-4-6`.
  - `RecordedClient` for deterministic no-network replay/tests.
  - Prompt builder containing instruction, exact region text, finding rule/message/
    precondition, and contract constraints.
  - Markdown fence stripping for model output.
  - Work-order proposal from analyzer reports or JSONL loading from `--workorders`.
  - Patch construction with schema `deslop.patch/1`, `workorder_id`,
    `region_fingerprint`, replacement, and `by = deslop-slim/<model>`.
  - `run_slim` flow: work order -> prompt -> client -> patch -> `verify_patches` ->
    dry-run report or `apply_patches`.
  - `NeedsCharacterizationTest` work orders are skipped with an explicit reason.
- Wired `deslop fix` to the slim consumer with:
  - `--paths <PATH>...`
  - `--workorders <WORKORDERS>`
  - `--apply`
  - `--allow-unverified`
  - `--coverage <MODE>`
  - `--model <MODEL>`
  - `--mock <MOCK>`
  - `--check-cmd <CHECK_CMD>`
  - `--no-backup`
- Kept the existing `undo` path backed by `deslop-fix` backups.
- Updated `SPEC.md` so `deslop-slim` is no longer deferred and documents the consumer,
  swappable clients, default dry-run, and deferred MCP fix-tool parity/streaming/
  multiprovider work.
- Updated `.agents/HEARTBEAT.md` each implementation round.

Verification:
- After skeleton crate/dependency change:
  - `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- After core slim implementation:
  - First full gate failed on an exact trailing-newline test expectation; fixed the test
    to match the implemented output normalization.
  - Re-run full gate: pass.
- After CLI wiring:
  - First full gate failed in clippy because `CommandFactory` was imported in the binary
    build but only used in tests; moved the import into the test module.
  - Re-run full gate: pass.
- After SPEC update:
  - `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
- CLI help smoke:
  - `cargo run -q -p deslop-cli -- fix --help`: pass; output lists `--paths`,
    `--workorders`, `--apply`, `--allow-unverified`, `--coverage`, `--model`, `--mock`,
    `--check-cmd`, and `--no-backup`.

Deterministic tests added:
- Prompt unit proves region text, finding message, and contract constraints are present.
- Recorded-client e2e proves a valid rewrite becomes a patch, verifies as `removable` with
  recorded LCOV coverage, and writes by default with `--apply` in a tempdir without network
  or API keys.
- Default `--apply` with coverage disabled verifies a parseable rewrite as
  `coverage-unknown`, reports it as held-unproven, writes nothing, and leaves the file
  unchanged.
- `--allow-unverified` applies the same `coverage-unknown` rewrite.
- Rejection path proves a bad rewrite is rejected by verify in both default and
  `--allow-unverified` modes and leaves the file unchanged.
- Anthropic response parser unit extracts text blocks and strips code fences without
  making a network request.
- CLI parser unit covers `disabled`, `auto`, `auto:<cmd>`, `lcov:<path>`,
  `cloverage:<path>`, `julia-cov:<path>`, and `coverage-py:<path>`.

Important behavior note:
- Before this surgical fix, `deslop-slim` hardcoded `coverage = Disabled` and
  `allow_non_removable = true`, so explicit slim `--apply` wrote behavior-unproven
  `coverage-unknown` rewrites. After the fix, default slim `--apply` writes only
  `removable`; non-rejected but unproven verdicts are held unless `--allow-unverified` is
  explicit. The standalone `deslop apply` command keeps its existing stricter default unless
  `--allow-non-removable` is used.

Deferred:
- MCP fix-tool parity.
- Streaming progress.
- Additional provider clients beyond Anthropic and RecordedClient.
- First-run/source-egress consent was documented historically in the spec but not
  implemented in this pass.

Blockers:
- None for this requested slim pass.

Signature: Codex

Date/time: 2026-06-23 Europe/Madrid

Objective: Build `deslop` from `SPEC.md` v0.4.

Target: M1 deterministic Rust CLI scaffold: core types, parsing/language detection, analyzer reports, agent work orders, safe-auto fixes, baseline ratchet, undo, and rule listing.

Changes:
- Initialized local jj/git-backed version tracking and added `.gitignore` for generated/local artifacts.
- Added a Cargo workspace with crates:
  - `deslop-core`: severity, safety lattice, spans, edits, findings, fingerprints.
  - `deslop-parse`: source loading, language detection, line/region utilities.
  - `deslop-analyzer`: initial agnostic, Clojure, and Julia rule catalog.
  - `deslop-protocol`: `deslop.workorder/1` and `deslop.patch/1` data types.
  - `deslop-report`: text, JSON, and agent JSONL rendering.
  - `deslop-fix`: right-to-left safe-auto splice application, backups, undo.
  - `deslop-cli`: `scan`, `propose`, `fix`, `baseline write`, `undo`, `rules`.
- Implemented the safety constraint from memory/spec: `fix` writes only `safe-auto` findings with concrete edits. `reimpl-empty?`, `reimpl-seq`, Julia `eachindex`, etc. are report/propose only.
- Left the old Python prototype intact as semantic reference.

Commands run:
- `jj git init`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- CLI smoke test with a temporary Clojure file:
  - `scan --format json`
  - `propose`
  - `fix --dry-run`
  - `fix --no-backup`
  - `grep` assertions that safe-auto rewrites happened and `reimpl-empty?` was left unchanged.
- Baseline smoke test:
  - `baseline write`
  - `scan --baseline`

Results:
- `cargo fmt --all --check`: pass.
- `cargo test --workspace`: pass, 4 unit tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI smoke: pass, produced one work order for the non-safe `reimpl-empty?` and applied only safe-auto fixes.
- Baseline smoke: pass, known fingerprint suppressed.

Invalidated assumptions:
- None new. Existing negative memory remains active: parse/syntax validation is not behavior preservation, so non-`safe-auto` rules must not be fixed in place by default.

Current recommendation/checkpoint:
- M1 is implemented as a working Rust scaffold and verified.
- The parser/analyzer layer is still lightweight and line/CST-adjacent, not yet tree-sitter/scope-graph based. This is acceptable for the first M1 scaffold but should be upgraded before claiming the full "strong analyzer" thesis.

Blockers:
- None for current M1 scaffold.

Next actions:
- M2: implement `verify`/`apply` deterministic gate, stale region fingerprint rejection, defensive-code guard, and `--check-cmd`.
- Add tree-sitter grammars and richer region extraction before expanding fixable rules.
- Add clj-kondo/JET adapters for `analyzer-confirmed` rules.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- No server or live process restart required.

Signature: Codex

## 2026-06-23T19:05:47+02:00 — Tree-Sitter 0.26 Bump Blocked

Objective: Execute `.agents/NEXT_TASK.md` for a tree-sitter `0.25` -> `0.26`
dependency bump with grammar-crate compatibility and node-kind stability gates.

Result: blocked before dependency edit.

Compatibility checks:
- `cargo search tree-sitter --limit 5` reports latest `tree-sitter = "0.26.9"`.
- `cargo search tree-sitter-language --limit 5` reports latest
  `tree-sitter-language = "0.1.7"`.
- `cargo search tree-sitter-rust --limit 5` reports latest
  `tree-sitter-rust = "0.24.2"`.
  - Registry manifest dependency: `tree-sitter-language = "0.1"`.
  - Dev dependency: `tree-sitter = "0.25"`.
- `cargo search tree-sitter-julia --limit 5` reports latest
  `tree-sitter-julia = "0.23.1"`.
  - Registry manifest dependency: `tree-sitter-language = "0.1"`.
  - Dev dependency: `tree-sitter = "0.24"`.
- `cargo search tree-sitter-clojure --limit 5` reports latest
  `tree-sitter-clojure = "0.1.0"`.
  - Registry manifest dependency: `tree-sitter = "0.25.6"`.
  - Registry manifest dependency: `tree-sitter-language = "0.1.5"`.

Blocker:
- `tree-sitter-clojure 0.1.0` is the latest published `tree-sitter-clojure`
  crate and depends on `tree-sitter = "0.25.6"`. Under Cargo `0.x` semver,
  that does not allow `0.26.x`.
- The task explicitly says to stop if a grammar has no `0.26`-compatible release,
  and to not silently revert or vendor/patch a grammar in this pass.

Changes made:
- No `Cargo.toml` or `Cargo.lock` dependency changes.
- No parser/API/node-kind changes.
- Updated `.agents/HEARTBEAT.md` and this session report only.

Commands run:
- `cargo search tree-sitter --limit 5`
- `cargo search tree-sitter-rust --limit 5`
- `cargo search tree-sitter-julia --limit 5`
- `cargo search tree-sitter-clojure --limit 5`
- `cargo search tree-sitter-language --limit 5`
- `cargo info tree-sitter@0.26.9`
- `cargo info tree-sitter-rust@0.24.2`
- `cargo info tree-sitter-julia@0.23.1`
- `cargo info tree-sitter-clojure@0.1.0`
- Registry manifest inspection under `~/.cargo/registry/src/...`
- `cargo tree -p deslop-lang | rg -n "tree-sitter"`

Verification not run:
- The hard compile/eval/node-kind gate was not run because the dependency migration
  was not attempted after the grammar compatibility blocker was confirmed.

Recommendation:
- Wait for a `tree-sitter-clojure` crate release compatible with tree-sitter
  `0.26`, or schedule a separate explicit grammar replacement/vendor pass. That
  is outside this task's allowed scope.

Signature: Codex

## 2026-06-23T18:44:45+02:00 — Duplicate Removability Precision Pass

Objective: Execute `.agents/NEXT_TASK.md` for near-duplicate / duplicate-block
removability precision plus a couple of genuine extractions. No new dependencies,
no macros, and no `deslop/*.py` changes.

Baseline:
- `target/debug/deslop scan crates --format json` before this pass:
  - `duplicate-block`: 17
  - `near-duplicate`: 39

Changes:
- Extracted the repeated token-window equality check in
  `crates/deslop-analyzer/src/tokens.rs` into `token_windows_match(left, right,
  field)`.
- Added Rust CST suppression for non-removable pure enum/path mapping matches in
  the duplicate detector. This suppresses `From`/dispatch-style enum mapping
  boilerplate where the repeated structure differs only by identifiers and has no
  shared extractable body without a macro/new dependency.
- Added/extended guards:
  - `tests/fixtures/clean/precision_fp.rs` now contains enum-mapping boilerplate
    and is covered by the existing clean structural FP test.
  - `tests/corpus/clean/rust_clean.rs` now includes enum-mapping boilerplate with
    explicit `duplicate-block` / `near-duplicate` false expectations.
  - Existing behavioral duplication TP fixture remains the recall guard.
- Extracted repeated `deslop-verify` test fixture setup into
  `verify_fixture(FixtureKind, text)`, with `clojure_fixture` and `rust_fixture`
  wrappers. Only one-work-order Rust/Clojure fixture cases were converted; tests
  that build multiple files or custom `SourceFile`s were left explicit.

Measured split:
- After token equality extraction only:
  - `duplicate-block`: 17 -> 17
  - `near-duplicate`: 39 -> 38
- After Rust mapping precision suppression and verify fixture extraction:
  - `duplicate-block`: 17 -> 17
  - `near-duplicate`: 38 -> 36
- Overall before -> after:
  - `duplicate-block`: 17 -> 17
  - `near-duplicate`: 39 -> 36

Gate history:
- Token equality extraction: full gate passed.
- Initial precision test fixture was too small/threshold-sensitive; fixed by using
  the existing behavioral duplication corpus guard for TP recall.
- Inline enum-mapping FP test caused a new self-scan duplicate hit in
  `crates/deslop-analyzer/src/tests.rs`; moved the guard to fixture/corpus data.
- Precision suppression final gate passed.
- Verify fixture extraction final gate passed.

Verification:
- `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
  - Workspace tests: 60 unit tests plus doc-tests.
- `target/debug/deslop eval tests/corpus --format json`: pass.
  - Overall precision=0.9666666666666667
  - Overall recall=0.9666666666666667
  - Overall F1=0.9666666666666667
  - `duplicate-block`: precision=1.0 recall=1.0 tp=1 fp=0 fn=0
  - `near-duplicate`: precision=0.96 recall=1.0 tp=24 fp=1 fn=0
  - Known local fallback notice: `clj-kondo not on PATH; falling back to built-in T1 Clojure rules`
- Final `target/debug/deslop scan crates --format json`:
  - `duplicate-block`: 17
  - `near-duplicate`: 36

Residual target findings:
- Converged for this pass. Remaining hits are cohesive detector/provider/reporting
  bodies, test loops/fixtures, or idiomatic boilerplate. I did not force macros,
  new dependencies, or helper extraction that would fragment cohesive functions.
- Representative residuals include:
  - `crates/deslop-analyzer/src/agnostic.rs:15`
  - `crates/deslop-analyzer/src/clojure.rs:90`
  - `crates/deslop-analyzer/src/julia.rs:40`
  - `crates/deslop-analyzer/src/packs/rust.rs:182`
  - `crates/deslop-analyzer/src/tokens.rs:349`
  - `crates/deslop-cli/src/main.rs:813`
  - `crates/deslop-eval/src/lib.rs:110`
  - `crates/deslop-external/src/lib.rs:897`
  - `crates/deslop-lang/src/lib.rs:495`
  - `crates/deslop-mcp/src/lib.rs:302`
  - `crates/deslop-parse/src/lib.rs:189`
  - `crates/deslop-verify/src/lib.rs:1773`

Blockers:
- None. This pass is intentionally stopped at the removability boundary.

Signature: Codex

## 2026-06-23T18:10:59+02:00 — Rust Detector Precision Pass

Objective: Execute `.agents/NEXT_TASK.md` for the Rust `redundant-closure` and
`needless-clone` rules only, with every other analyzer rule frozen.

Target:
- `crates/deslop-analyzer/src/packs/rust.rs`
- target-rule corpus/unit tests only

Changes:
- Replaced the `redundant-closure` line regex with a tree-sitter Rust CST walk.
  It now fires only for a closure with exactly one identifier parameter and a body
  that is exactly one single-argument function call forwarding that parameter.
- Replaced the broad `needless-clone` `.clone()` line regex with tree-sitter Rust
  CST tells for real expression nodes only:
  - `&<expr>.clone()`
  - `.clone().iter()`
  - `.clone().iter_mut()`
  - `.clone().into_iter()`
- Kept message text, severity, safety class, and detection source unchanged.
- Added Rust analyzer unit tests for true positives and false positives for both
  target rules.
- Updated the Rust idiom corpus to use clone-then-borrow as the positive
  `needless-clone` fixture and raised the `needless-clone` corpus precision
  baseline to 1.0.
- Updated `.agents/HEARTBEAT.md` during each active iteration.

Before counts:
- `target/debug/deslop scan crates --format json` target-rule baseline before edits:
  - `needless-clone`: 11
  - `redundant-closure`: 3

Gate history:
- First full gate failed during `cargo test --workspace` compilation because the new
  tests shadowed the `source(...)` fixture helper with a local variable. Fixed by
  renaming the locals.
- Second full gate passed after the test fix.
- First after-scan then found one `needless-clone` hit in
  `crates/deslop-analyzer/src/tests.rs:252`, caused by the line-regex detector
  matching a Rust string fixture. This invalidated the regex approach for
  clone-then-borrow in this repo.
- Replaced `needless-clone` with CST expression detection and reran the full gate.

Verification run:
- `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`: pass.
  - Workspace tests: 60 unit tests plus doc-tests.
- `target/debug/deslop eval tests/corpus --format json`: pass.
  - Overall precision=0.9666666666666667
  - Overall recall=0.9666666666666667
  - Overall F1=0.9666666666666667
  - `needless-clone`: precision=1.0 recall=1.0 tp=1 fp=0 fn=0
  - `redundant-closure`: precision=1.0 recall=1.0 tp=1 fp=0 fn=0
  - Known local fallback notice: `clj-kondo not on PATH; falling back to built-in T1 Clojure rules`
- `target/debug/deslop scan crates --format json` target-rule after counts:
  - `needless-clone`: 0
  - `redundant-closure`: 0

Residual target-rule hits:
- None.

Known false positives explicitly not chased:
- The old non-forwarding `redundant-closure` false positives are eliminated by CST,
  not individually edited at call sites.
- The old bare ownership `.clone()` false positives are eliminated by CST, not
  individually edited at call sites.

Blockers:
- None for this detector-precision pass.

Signature: Codex

---

# Session Report — Finish Revalidation

Date/time: 2026-06-23T17:23:33+02:00 Europe/Madrid

Objective: Re-run final verification from the latest cleanup checkpoint and confirm residual
hotspots/blockers.

Verification:
- `cargo fmt --all --check && cargo build --workspace && cargo test --workspace &&
  cargo clippy --workspace -- -D warnings`: pass.
- `cargo run -p deslop-cli -- eval tests/corpus --format json`: pass.
  - precision=0.9508196721311475
  - recall=0.9666666666666667
  - F1=0.9586776859504132
  - expected fallback notice: `clj-kondo not on PATH; falling back to built-in T1 Clojure rules`

Current residual self-scan:
- `target/debug/deslop slop crates`: score=10.9/100.
- Rule counts:
  - comment-block=1
  - duplicate-block=15
  - long-method=17
  - magic-number=14
  - near-duplicate=37
  - needless-clone=11
  - redundant-closure=3 in raw scan aggregation

Metrics:
- `target/debug/deslop metrics crates`: repo health=42.5/100, regions=517, hotspots=75.
- Top hotspots remain `deslop-lang`, `deslop-verify` coverage providers, analyzer token
  duplication/tokenization, and eval scoring.

Blockers:
- No verification blockers.
- Local optional external tools remain unavailable/partial as previously recorded:
  `clj-kondo` missing, `lein` missing, `coverage.py` missing, Julia without Coverage.jl.

Signature: Codex

---

# Session Report — Behavior-Preserving Own-Code Debloat, Iteration 2

Date/time: 2026-06-23T16:41:41+02:00 Europe/Madrid

Objective: Continue the frozen-analyzer own-code debloat pass after the first refactor
checkpoint.

Before measurements for this iteration:
- `target/debug/deslop slop crates`:
  - score: 11.1/100
  - comment-block=1
  - duplicate-block=15
  - long-method=23
  - magic-number=14
  - near-duplicate=37
  - needless-clone=11

Changes:
- `crates/deslop-verify/src/lib.rs`
  - Extracted `read_report_text` for repeated contextual report reads.
  - Extracted `run_output_file_command` for external commands that write a temp output
    artifact (`cargo-llvm-cov`, Coverage.jl, coverage.py).
  - Reused `read_report_text` for cargo-mutants, LCOV, cloverage, Coverage.jl, and
    coverage.py file/report loading.
  - Split `write_prepared_patches` into grouping, per-file patch application, replacement
    writing, and temp-path construction helpers.

Gates:
- After provider/report helper extraction:
  - `cargo fmt --all && cargo build --workspace && cargo test --workspace &&
    cargo clippy --workspace -- -D warnings`: pass.
- After patch-writing split and local temp-path cleanup:
  - `cargo fmt --all && cargo build --workspace && cargo test --workspace &&
    cargo clippy --workspace -- -D warnings`: pass.

After measurements:
- `target/debug/deslop slop crates`:
  - score: 10.9/100
  - comment-block=1
  - duplicate-block=15
  - long-method=17
  - magic-number=14
  - near-duplicate=37
  - needless-clone=11

Attribution:
- Iteration delta:
  - score 11.1 -> 10.9
  - long-method 23 -> 17
  - comment-block, duplicate-block, magic-number, near-duplicate, needless-clone unchanged.
- Combined frozen-refactor delta from the original debloat baseline:
  - score 11.3 -> 10.9
  - duplicate-block 17 -> 15
  - long-method 25 -> 17
  - near-duplicate 40 -> 37
  - comment-block 1 unchanged
  - magic-number 14 unchanged
  - needless-clone 11 unchanged.

Known false positives still listed, not chased:
- Redundant-closure on non-forwarding compare closures:
  - `crates/deslop-verify/src/lib.rs:1126`
  - `crates/deslop-verify/src/lib.rs:2053`
  - `crates/deslop-verify/src/lib.rs:2121`
- Needless-clone/ownership false positives encountered:
  - `crates/deslop-verify/src/lib.rs:372`
  - `crates/deslop-verify/src/lib.rs:3147`
  - plus previously listed unchanged non-verify clones in analyzer tokens, metrics, protocol,
    and analyzer Rust pack files.

Long methods left intentionally:
- Analyzer rule/pack bodies remain untouched to keep analyzer behavior frozen.
- Remaining `deslop-verify` long methods are test scenario bodies; they can be cleaned in a
  focused test-fixture helper pass, but this iteration stopped after the production verifier
  helper boundaries were extracted.

Notes:
- `.agents/HEARTBEAT.md` appeared in the working copy during the session; it was not created
  or edited by this pass and was left untouched.

Signature: Codex

---

## Session Report — CLI Verification Boilerplate + Heartbeat

Date/time: 2026-06-23T16:40:06+02:00 Europe/Madrid

Objective: Add a stale-pane heartbeat artifact for the long-running Codex loop and trim
repeated CLI verification boilerplate.

Changes:
- Added `.agents/HEARTBEAT.md` as the stale-pane heartbeat file for tmux pane `0:1`.
- Added explicit iteration discipline to `.agents/NEXT_TASK.md`:
  - touch the heartbeat file every round;
  - run `jj describe -m "<round summary>"` at the end of each successful round.
- Extracted `verify_options(...)` in `crates/deslop-cli/src/main.rs` to centralize repeated
  `VerifyOptions` construction for `characterize`, `verify_characterization`, `verify`, and
  `apply`.
- Extracted `print_pretty_json(...)` in `crates/deslop-cli/src/main.rs` to remove repeated
  pretty-JSON printing boilerplate in the verify/apply command path.

Commands run:
- `date --iso-8601=seconds`
- `cargo fmt --all --check` initially failed on the helper call formatting
- `cargo fmt --all`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `jj describe -m "Add heartbeat and iteration discipline"`
- `jj describe -m "Extract CLI verify-options helper and refresh heartbeat"`

Results:
- Formatting, build, test, and clippy all passed after the formatting fix.
- The new heartbeat artifact is in place and refreshed for this iteration.
- CLI verification code is slightly less repetitive without changing behavior.

Invalidated assumptions:
- None.

Current recommendation/checkpoint:
- Continue with the remaining high-signal `deslop-cli` / `deslop-verify` duplication clusters
  only if the next scan shows a clear win; otherwise stop when the remaining clusters turn into
  low-signal plumbing.

Blockers:
- None.

Dependencies/restart requirements:
- No restart required.

Signature: Codex

---

# Session Report — Behavior-Preserving Own-Code Debloat

Date/time: 2026-06-23T16:18:39+02:00 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md`: debloat deslop's own Rust crates with real
behavior-preserving refactoring, keep the analyzer/metrics/lang rule surfaces frozen, keep
the existing `read_to_string_ctx` dedup in `deslop-verify`, and do not touch `deslop/*.py`.

Target:
- Extract shared helpers for genuine duplicate/near-duplicate boilerplate in
  `deslop-verify` and `deslop-cli`.
- Decompose only long methods with cohesive phase boundaries.

Before measurements:
- Step 0 `cargo build --workspace`: pass.
- Step 0 `cargo test --workspace`: pass.
- `target/debug/deslop slop crates`:
  - score: 11.3/100
  - comment-block=1
  - duplicate-block=17
  - long-method=25
  - magic-number=14
  - near-duplicate=40
  - needless-clone=11

Changes:
- `crates/deslop-verify/src/lib.rs`
  - Kept the existing `read_to_string_ctx` helper from the working copy.
  - Extracted `parse_jsonl_records` for patch and characterization-test JSONL loading.
  - Extracted `coverage_status_for_lines` for duplicated line coverage grading.
  - Extracted `visit_json_children` for repeated recursive JSON object/array traversal.
  - Extracted `PatchSignals`, `assess_patch_signals`, `assess_coverage_if_clean`, and
    `assess_mutation_if_clean` from `prepare_patch` around the semantic-gate/probe phase.
- `crates/deslop-cli/src/main.rs`
  - Added `read_to_string_ctx` and reused it for config, slop, and baseline reads.
  - Extracted `slop_score_for_file` from `slop_report`.
  - Changed `Baseline::read` from `&PathBuf` to `&Path` after clippy exposed the stricter
    signature during the refactor.

Gates after changes:
- After verify helper extraction: `cargo fmt --all && cargo build --workspace &&
  cargo test --workspace && cargo clippy --workspace -- -D warnings` passed after fixing a
  helper lifetime caught by the first build.
- After CLI extraction: same full gate passed after changing `Baseline::read` to `&Path`.
- After `prepare_patch` signal/probe decomposition: same full gate passed.
- After final probe helper split: same full gate passed.

After measurements:
- `target/debug/deslop slop crates`:
  - score: 11.1/100
  - comment-block=1
  - duplicate-block=15
  - long-method=23
  - magic-number=14
  - near-duplicate=37
  - needless-clone=11

Attribution:
- Refactoring-only delta with analyzer frozen:
  - score 11.3 -> 11.1
  - duplicate-block 17 -> 15
  - long-method 25 -> 23
  - near-duplicate 40 -> 37
  - comment-block, magic-number, needless-clone unchanged.

Known false positives listed, not chased:
- Redundant-closure on non-forwarding compare closures:
  - `crates/deslop-verify/src/lib.rs:1136`
  - `crates/deslop-verify/src/lib.rs:2079`
  - `crates/deslop-verify/src/lib.rs:2147`
- Needless-clone/ownership false positives encountered:
  - `crates/deslop-verify/src/lib.rs:372`
  - `crates/deslop-analyzer/src/packs/rust.rs:159`
  - `crates/deslop-analyzer/src/tokens.rs:203`
  - `crates/deslop-analyzer/src/tokens.rs:248`
  - `crates/deslop-metrics/src/lib.rs:286`
  - `crates/deslop-metrics/src/lib.rs:450`
  - `crates/deslop-metrics/src/lib.rs:453`
  - `crates/deslop-metrics/src/lib.rs:611`
  - `crates/deslop-metrics/src/lib.rs:612`
  - `crates/deslop-protocol/src/lib.rs:153`
  - `crates/deslop-verify/src/lib.rs:3148`

Long methods left intentionally:
- Analyzer pack/rule functions in `deslop-analyzer`: these are cohesive rule/dispatch bodies;
  changing them in this pass would be analyzer-surface-adjacent and risk mixing refactor with
  detector behavior.
- Provider load/run methods in `deslop-verify`: remaining long methods mostly wrap one
  external tool or fixture scenario; further splitting would be command plumbing rather than
  clearer behavior.
- Metrics/report/eval long methods: outside the requested high-confidence `deslop-verify` and
  `deslop-cli` duplicate clusters; left for a focused pass if desired.

Invalidated assumptions:
- Extracting the `prepare_patch` semantic-gate phase alone improved clarity but did not reduce
  the long-method count because the new helper was still above the threshold; splitting coverage
  and mutation probes along domain boundaries was required for the measured count drop.

Blockers:
- None.

Dependencies/restart requirements:
- No live services or restart required.

Signature: Codex

---

# Session Report — Near-Duplicate Precision Pass

Date/time: 2026-06-23T14:52:51+02:00 Europe/Madrid

Objective: Execute superseding `.agents/NEXT_TASK.md`: fix near-duplicate/duplicate-block
precision first, proving FP removal with corpus tests, then refactor any clearly real
remainder. Preserve the existing incompleteness CST/string/comment fix and long-method
threshold. Do not touch `deslop/*.py`.

Step 0:
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo run -p deslop-cli -- scan crates --format json` before:
  - duplicate-block=8
  - near-duplicate=68

Primary detector precision changes:
- `crates/deslop-analyzer/src/tokens.rs`
  - Added CST token masks for comments, data regions, and strings.
  - String literals are emitted as a single opaque token and different strings no longer
    normalize to the same token.
  - Comments and data-literal regions are skipped by token duplication.
  - Added disjoint byte-range enforcement before reporting duplicate sequences.
- `crates/deslop-lang/src/lib.rs`
  - Added pack-owned `is_duplication_data_region`.
  - Rust excludes array/struct initializer regions and `json!`/`vec!` macro token trees.
  - Clojure excludes map/set literals; vector literals remain visible so `let` bindings and
    function arg vectors still support TP detection.
  - Julia excludes vector/matrix/tuple data-expression regions.
- `tests/fixtures/clean/precision_fp.rs`
  - Added FP corpus case covering distinct struct-literal rule-table shape, long regex/string
    literal, and repeated `Ok(Response { ... })` construction.
- `crates/deslop-analyzer/src/tests.rs`
  - Added the precision FP fixture to the clean corpus test.
  - Existing TP corpus still asserts behavioral duplicates fire across Rust/Clojure/Julia.

Precision-only measurement:
- Before precision: duplicate-block=8, near-duplicate=68.
- After precision, before refactor: duplicate-block=12, near-duplicate=34.
- Attribution:
  - near-duplicate 68 -> 34 is detector precision: string interiors, data literals, and
    self-overlap noise removed.
  - duplicate-block 8 -> 12 increased because skipping declarative/data material exposed
    shorter exact repeated setup/test patterns; these were handled under refactor where clear.
- `cargo run -p deslop-cli -- slop crates --format json` after precision:
  - score=10.999594107052676
  - counts: comment-block=1, duplicate-block=12, long-method=18, magic-number=13,
    near-duplicate=34, needless-clone=9.

Secondary refactor:
- `crates/deslop-analyzer/src/tests.rs`
  - Extracted `finding_for_rule` and replaced repeated scan/find/assert setup in tests.
- Refactor-only measurement:
  - duplicate-block 12 -> 11
  - near-duplicate 34 -> 34
  - score 10.999594107052676 -> 10.829261366676832.

Final measurements:
- `cargo run -p deslop-cli -- slop crates --format json`:
  - score=10.829261366676832
  - counts: comment-block=1, duplicate-block=11, long-method=18, magic-number=13,
    near-duplicate=34, needless-clone=9.
- Total from Step 0:
  - near-duplicate 68 -> 34
  - duplicate-block 8 -> 11
  - slop score from previous final 15.791053539249472 -> 10.829261366676832
    (current pass self-scan score baseline was not re-run as `slop`, but scan counts were).

Eval:
- `cargo run -p deslop-cli -- eval tests/corpus --format json`: pass.
- overall: precision=0.9508196721311475, recall=0.9666666666666667, F1=0.9586776859504132.
- duplicate-block: TP=1 FP=0 FN=0 precision=1.000 recall=1.000.
- near-duplicate: TP=24 FP=1 FN=0 precision=0.960 recall=1.000.
- incompleteness: TP=3 FP=0 FN=0 precision=1.000 recall=1.000.

Remaining findings left with concrete reasons:
- `crates/deslop-analyzer/src/agnostic.rs:15`, `370`, `428`, `431`: analyzer rule plumbing and
  comment-line helper loops; small structural similarities, not enough duplicated behavior
  for a safe helper extraction in this pass.
- `crates/deslop-analyzer/src/clojure.rs:90`, `179`, `181`: Clojure rule table/test idiom
  repetition. Real table consolidation work, but out of scope for the requested detector
  precision pass.
- `crates/deslop-analyzer/src/tokens.rs:69`, `348`, `351`, `412`, `459`: detector internals
  now contain some expected symmetry between left/right token window logic; further cleanup
  risks obscuring the just-fixed precision behavior.
- `crates/deslop-cli/src/main.rs:232`, `279`, `434`, `671`, `676`: CLI config/default parsing
  shape repetition; real but broader CLI cleanup.
- `crates/deslop-verify/src/lib.rs:162`, `305`, `539`, `842`, `1141`, `1187`, `1198`, `1208`:
  repeated verify result/check/fixture patterns. Some are real helper candidates, but the
  highest-confidence small test refactor was already done; the rest should be handled in a
  dedicated verify cleanup pass.

Commands run:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo run -p deslop-cli -- scan crates --format json`
- `cargo test -p deslop-analyzer --lib`
- `cargo run -p deslop-cli -- slop crates --format json`
- `cargo run -p deslop-cli -- eval tests/corpus --format json`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`

Final verification:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- FP+TP corpus tests: pass via `cargo test -p deslop-analyzer --lib`.
- `deslop eval tests/corpus`: pass.

Invalidated assumptions:
- Clojure vector literals cannot be blanket-excluded as data: doing so hides `let` binding
  vectors and breaks the renamed behavioral clone TP. Clojure exclusion is therefore limited
  to map/set literals.

Deferred exactly:
- No requested detector precision fix deferred.
- Real remainder cleanup deferred to focused future passes: Clojure rule-table consolidation,
  CLI config parsing cleanup, and verify result/check fixture cleanup.

Blockers:
- None.

Dependencies/restart requirements:
- No live services. No restart required.
- `clj-kondo` is not on PATH, so eval prints the expected fallback notice.

Signature: Codex

---

# Session Report — Deslop Own-Slop Reduction

Date/time: 2026-06-23T14:30:31+02:00 Europe/Madrid

Objective: Execute superseding `.agents/NEXT_TASK.md`: reduce deslop's own slop with two
separate levers, keep the existing incompleteness CST/string/comment masking fix, preserve
behavior with cargo tests, and do not touch `deslop/*.py`.

Target:
- Lever 1 calibration: raise the long-method threshold from 12 to a realistic value and
  update the corpus so long-method recall remains covered.
- Lever 2 refactor: with analyzer rules frozen after calibration, reduce real duplication in
  external analyzer adapters, MCP boilerplate, and verify/test setup.
- Report calibration and refactor measurements separately.

Step 0 result before edits:
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo run -p deslop-cli -- slop crates --format json`:
  - score: 43.51606024794449
  - rule counts: comment-block=1, duplicate-block=17, long-method=185,
    magic-number=13, near-duplicate=68, needless-clone=8.

Lever 1 calibration:
- Changed `LONG_METHOD_NLOC` in `crates/deslop-analyzer/src/agnostic.rs` from 12 to 40.
- Reason: 12 NLOC flagged ordinary adapter/test functions as long; 40 NLOC is a defensible
  minimum for a report-only long-method smell while still catching single-routine bloat.
- Extended the Rust/Clojure/Julia long-method corpus fixtures so each still exceeds 40 NLOC.
- Updated manifest expectations and baseline for the resulting measured corpus.
- Calibration-only measurement (`deslop slop crates` after threshold, before refactors):
  - score: 16.5078308761065
  - long-method: 185 -> 19
  - duplicate-block: 17 -> 17
  - near-duplicate: 68 -> 68
- `deslop eval tests/corpus`: pass after corpus update; long-method precision=1.000,
  recall=1.000.

Lever 2 refactors:
- `crates/deslop-external/src/lib.rs`:
  - Extracted clj-kondo/clippy failure notice helpers.
  - Extracted shared Julia diagnostics JSON parsing and line/message fallback.
  - Preserved command behavior and graceful degradation.
- `crates/deslop-mcp/src/lib.rs`:
  - Extracted JSON-RPC success response and MCP tool result wrappers.
  - Extracted shared scan report loading, verify options, boolean argument parsing, and
    structured-content test helpers.
  - Extracted sample fixture setup for MCP tests.
  - Synced MCP rule text with CLI rule text for the new slop rules.
  - Added internal `deslop-core` dependency for the shared `FileReport` return type.
- `crates/deslop-verify/src/lib.rs`:
  - Extracted shared verification run setup for verify/apply.
  - Extracted pass-result construction and LCOV file flushing.
  - Extracted test `VerifyOptions` and Clojure fixture setup.

Refactor-only measurement:
- Comparing calibration-only to final:
  - duplicate-block: 17 -> 8
  - near-duplicate: 68 -> 68
  - score: 16.5078308761065 -> 15.791053539249472
- The requested MCP duplicate-blocks were removed from production code; remaining MCP
  duplicate-blocks were eliminated after test fixture extraction.
- The requested verify duplicate-blocks around stale/parse tests were removed; final verify
  duplicate-blocks from those exact setup spans are gone.

Final self slop:
- `cargo run -p deslop-cli -- slop crates --format json`:
  - score: 15.791053539249472
  - rule counts: comment-block=1, duplicate-block=8, long-method=17,
    magic-number=13, near-duplicate=68, needless-clone=8.
- Total score: 43.51606024794449 -> 15.791053539249472.
- Total attributed drops:
  - Calibration: long-method 185 -> 19 and score 43.516 -> 16.508.
  - Refactor: duplicate-block 17 -> 8 and score 16.508 -> 15.791.

Eval:
- `cargo run -p deslop-cli -- eval tests/corpus --format text`: pass.
- overall: TP=58 FP=3 FN=2 precision=0.951 recall=0.967 F1=0.959.
- long-method: TP=3 FP=0 FN=0 precision=1.000 recall=1.000.
- incompleteness: TP=3 FP=0 FN=0 precision=1.000 recall=1.000.
- duplicate-block: TP=1 FP=0 FN=0 precision=1.000 recall=1.000.
- near-duplicate: TP=24 FP=1 FN=0 precision=0.960 recall=1.000.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- Step 0: `cargo run -p deslop-cli -- slop crates --format json`
- Calibration: `cargo run -p deslop-cli -- eval tests/corpus --format text`
- Calibration: `cargo run -p deslop-cli -- slop crates --format json`
- Refactor gates after each refactor batch:
  `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`
- Final: `cargo fmt --all --check`
- Final: `cargo build --workspace`
- Final: `cargo test --workspace`
- Final: `cargo clippy --workspace -- -D warnings`
- Final: `cargo run -p deslop-cli -- eval tests/corpus --format text`
- Final: `cargo run -p deslop-cli -- slop crates --format json`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- `deslop eval tests/corpus`: pass.
- `deslop slop crates`: pass.

Remaining duplicate-blocks left with concrete reasons:
- `crates/deslop-analyzer/src/agnostic.rs:431`: test assertion shape around comment block
  findings; low-risk but outside requested adapters/MCP/verify clusters.
- `crates/deslop-analyzer/src/clojure.rs:90`, `179`, `181`: rule table/test idiom shapes for
  Clojure-specific syntactic rewrites; behavior-specific, should be handled in a Clojure rule
  table pass.
- `crates/deslop-analyzer/src/tests.rs:197`, `240`: analyzer fixture assertions; not part of
  requested external/MCP/verify refactor scope.
- `crates/deslop-lang/src/lib.rs:318`, `398`: repeated LangPack method declarations across
  language pack implementations; real structural repetition but requires a separate LangPack
  default-method/table cleanup to avoid obscuring per-language behavior.

Invalidated assumptions:
- Raising the long-method fixture by repeating a step chain initially polluted
  near-duplicate eval. The emitted spans were real repeated behavior, so they were explicitly
  labeled in the corpus rather than suppressed.
- The refactor pass did not reduce near-duplicate count overall; remaining near-duplicates are
  mostly broader analyzer/CLI/metrics structural similarities outside the requested duplicate
  block clusters.

Deferred exactly:
- No requested calibration/refactor deliverable deferred.
- Separate future cleanup candidates: Clojure analyzer rule-table consolidation,
  LangPack boilerplate consolidation, and broader CLI/metrics near-duplicate refactors.

Blockers:
- None.

Dependencies/restart requirements:
- No live services. No restart required.
- `clj-kondo` is not on PATH, so eval prints the expected fallback notice.

Signature: Codex

---

# Session Report — AI-Slop Rule Family + Narrating Comment Precision

Date/time: 2026-06-23T13:51:48+02:00 Europe/Madrid

Objective: Execute the superseding `.agents/NEXT_TASK.md`: add literature-grounded intrinsic
AI-slop rules, tune `narrating-comment`, measure each rule via `deslop eval`, ship rules only
if corpus precision is at least 0.8, and keep the Rust workspace gate green.

Target:
- Add `incompleteness`, `magic-number`, `long-method`, and `slop-score`.
- Fix `narrating-comment` precision from the previous eval baseline of 0.200 to >=0.8.
- Add multi-language clean/sloppy corpus coverage across Rust, Clojure, and Julia.
- General/multi-language implementation, no central `match Lang`.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo run -p deslop-cli -- eval tests/corpus --format json`: pass. Before numbers:
  - overall: TP=21 FP=7 FN=2 precision=0.750 recall=0.913 F1=0.824
  - `narrating-comment`: TP=1 FP=4 FN=0 precision=0.200 recall=1.000 F1=0.333

Changes:
- Added pack-owned long-method region classification to `deslop-lang::LangPack`:
  Clojure/Julia use behavioral CST regions; Rust uses `function_item`. Analyzer code queries
  the pack instead of switching on language.
- Added analyzer rules:
  - `incompleteness`: stubs/placeholders/TODO implementation holes, `llm-only`.
  - `magic-number`: inline numeric literals without named constants, `risky-suggest`.
  - `long-method`: pack-owned function/block regions over the NLOC threshold, `llm-only`.
- Added `deslop slop [PATHS...] [--format text|json]`: weighted 0-100 slop-rule density per
  file/repo using the intrinsic slop and bloat rules.
- Tuned `narrating-comment` by suppressing it inside multi-line full-line comment blocks so
  structural explanatory comments are not double-reported as narration.
- Added Rust/Clojure/Julia corpus files for clean intrinsic-slop cases and sloppy positives:
  stubs, magic numbers, long methods, and narrating comments.
- Updated `tests/corpus/manifest.json` and `tests/corpus/baseline.json`.
- Updated `SPEC.md` with the empirical smell-taxonomy basis
  (arxiv 2510.03029), the non-authorship-detector boundary, new rule catalog entries, and
  `deslop slop`.
- Updated `deslop rules` output to list `incompleteness`, `magic-number`, `long-method`, and
  `slop-score`.

After eval:
- `cargo run -p deslop-cli -- eval tests/corpus --format text`: pass.
- Corpus: 17 cases (6 clean, 11 sloppy), Clojure=5, Julia=6, Rust=6.
- overall: TP=39 FP=3 FN=2 precision=0.929 recall=0.951 F1=0.940.
- requested rules:
  - `incompleteness`: TP=3 FP=0 FN=0 precision=1.000 recall=1.000 F1=1.000
  - `magic-number`: TP=5 FP=0 FN=0 precision=1.000 recall=1.000 F1=1.000
  - `long-method`: TP=3 FP=0 FN=0 precision=1.000 recall=1.000 F1=1.000
  - `narrating-comment`: TP=3 FP=0 FN=0 precision=1.000 recall=1.000 F1=1.000
- All requested shipped rules clear the >=0.8 precision gate and remain enabled.
- Existing non-target residuals:
  - `near-duplicate`: precision=0.833 recall=1.000
  - `needless-clone`: precision=0.500 recall=1.000
  - `needless-return`: precision=0.500 recall=1.000
  - `unused-arg`/`unused-binding`: recall=0.000 in local eval because external analyzer tools
    are absent; graceful fallback remains active.

Self slop-score:
- `cargo run -p deslop-cli -- slop crates --format text`: pass.
- Deslop crates score: 44.0/100.
- Rule counts: duplicate-block=17, incompleteness=2, long-method=184, magic-number=13,
  near-duplicate=67, needless-clone=8.
- Top files: `crates/deslop-analyzer/src/agnostic.rs` 74.1, `clojure.rs` 66.1,
  `deslop-mcp/src/lib.rs` 64.7, `julia.rs` 59.3, `deslop-verify/src/lib.rs` 59.0.

Commands run:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo run -p deslop-cli -- eval tests/corpus --format json`
- `cargo run -p deslop-cli -- eval tests/corpus --format text`
- `cargo run -p deslop-cli -- slop crates --format text`
- `cargo run -p deslop-cli -- slop crates --format json`
- `cargo fmt --all`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo run -p deslop-cli -- rules`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- `deslop eval tests/corpus`: pass with requested rule precision >=0.8.
- `deslop slop crates`: pass.
- `deslop rules`: pass and shows new rules.

Invalidated assumptions:
- None new. The previous low `narrating-comment` precision was a context bug: comment-block
  examples were being double-labeled as narration. That path is fixed by excluding multi-line
  full-line comment blocks from narrating-comment detection.

Current recommendation/checkpoint:
- The requested intrinsic AI-slop rules are implemented, measured, and shipped enabled.
- `slop-score` is available as `deslop slop`.
- Existing non-target low-precision rules remain visible in the eval table but were not part
  of this pass.

Deferred exactly:
- No requested deliverable deferred in this pass.

Blockers:
- None.

Dependencies/restart requirements:
- Rust workspace only. No server or live process restart required.
- `clj-kondo` is not on PATH in this environment, so local eval prints the expected fallback
  notice.

Signature: Codex

---

## Session Report — Eval Corpus And Accuracy Ratchet

Date/time: 2026-06-23T13:37:17+02:00 Europe/Madrid

Objective: Execute rewritten `.agents/NEXT_TASK.md`: build a labeled clean/sloppy multi-language eval corpus, per-rule precision/recall harness, and baseline ratchet without changing detection rules.

Target: General, multi-language measurement of existing rules; no Rust-specific corpus logic; no new detection rules; no edits to `deslop/*.py`.

Step 0 result:
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 42 unit tests plus doc-tests before edits.

Changes:
- Added `crates/deslop-eval` with:
  - `tests/corpus/manifest.json` loading;
  - analyzer execution over corpus cases;
  - TP/FP/FN scoring per rule;
  - precision/recall/F1 computation;
  - text and JSON rendering;
  - baseline ratchet assertion against `tests/corpus/baseline.json`.
- Added `deslop eval [CORPUS] --format text|json`.
- Added labeled corpus under `tests/corpus/`:
  - clean and sloppy cases for Rust, Clojure, and Julia;
  - manifest expectations with rule, should-fire flag, line region, and note;
  - tricky clean negatives for structural repetition, explicit tail return, early return, and ownership-required clone;
  - unused-arg/unused-binding expectations to measure current analyzer-confirmed recall when external tools are absent.
- Added `tests/corpus/baseline.json` ratchet with current measured precision/recall.

Measured corpus:
- Cases: 9 total; 3 clean, 6 sloppy.
- Languages: Clojure 3, Julia 3, Rust 3.
- Rules with expectations: 23.

Measured accuracy:
- Overall: TP 21, FP 7, FN 2, precision 0.750, recall 0.913, F1 0.824.
- `narrating-comment`: TP 1, FP 4, FN 0, precision 0.200, recall 1.000, F1 0.333.
- `near-duplicate`: TP 1, FP 1, FN 0, precision 0.500, recall 1.000, F1 0.667.
- `needless-clone`: TP 1, FP 1, FN 0, precision 0.500, recall 1.000, F1 0.667.
- `needless-return`: TP 1, FP 1, FN 0, precision 0.500, recall 1.000, F1 0.667.
- `unused-arg`: TP 0, FP 0, FN 1, precision 1.000, recall 0.000, F1 0.000.
- `unused-binding`: TP 0, FP 0, FN 1, precision 1.000, recall 0.000, F1 0.000.
- All other measured rules: precision 1.000, recall 1.000, F1 1.000 on this corpus.

Poor-score backlog:
- Low precision/noisy: `narrating-comment`, `near-duplicate`, `needless-clone`, `needless-return`.
- Low recall/missed: `unused-arg`, `unused-binding` in the default local eval because analyzer-confirmed external tools are absent.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo run -p deslop-cli -- eval tests/corpus --format text`
- `cargo run -p deslop-cli -- eval tests/corpus --format json`
- `cargo test -p deslop-eval`
- `cargo fmt --all`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 44 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- Eval ratchet test: pass.

Invalidated assumptions:
- None new. The measured baseline confirms several existing rules are intentionally noisy under the new clean/sloppy labels; those are now explicit tuning backlog instead of anecdotal complaints.

Deferred exactly:
- Bootstrap-labeling from the removability prover.
- Mutation probe.
- Large-scale repo mining.

Blockers:
- None for this pass.

Dependencies/restart requirements:
- No server or live process restart required.

Signature: Codex

---

## Session Report — Coverage Verdict Prover

Date/time: 2026-06-23T13:25:45+02:00 Europe/Madrid

Objective: Execute rewritten `.agents/NEXT_TASK.md`: make `deslop-verify` produce confidence-tagged removability verdicts with an opt-in, general coverage adapter.

Target: Coverage must be pack/provider-driven like external analyzers, with Rust implemented first via `cargo-llvm-cov`; no central `match Lang` in verify core; graceful coverage degrade when the tool is absent; apply defaults to writing only `removable`.

Step 0 result:
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 40 unit tests plus doc-tests before edits.

Changes:
- Added `CoverageProvider`, `CoverageRequest`, `CoverageAssessment`, `CoverageStatus`, and `CoverageConfig` in `deslop-verify`.
- Added `VerificationVerdict` serialized as kebab-case: `removable`, `dead-candidate`, `untested-risky`, `coverage-unknown`, `rejected`.
- Kept the existing `passed` bool for compatibility, but added `verdict` and coverage reasons to every verify/apply result.
- Implemented `RustCargoLlvmCovProvider` behind the general provider registry:
  - `CoverageConfig::Auto` runs `cargo llvm-cov --workspace --lcov --output-path ...`.
  - `CoverageConfig::LcovFile` parses recorded LCOV fixtures for deterministic tests.
  - absent/failing coverage tool returns `coverage-unknown` instead of erroring.
- Changed `apply` semantics: default writes only `removable`; non-rejected non-removable verdicts require `allow_non_removable`.
- Wired CLI:
  - `deslop verify --patches FILE [--check-cmd CMD] [--coverage]`
  - `deslop apply --patches FILE [--check-cmd CMD] [--coverage] [--allow-non-removable] [--no-backup]`
- Wired MCP verify/apply schemas and structured output to include coverage and `allow_non_removable`; MCP tests assert verdict strings.

Tests added/updated:
- Recorded LCOV fixture: covered Rust region plus passing check -> `removable`.
- Recorded LCOV fixture: uncovered empty replacement -> `dead-candidate`.
- Recorded LCOV fixture: uncovered non-empty replacement -> `untested-risky`.
- Failing check command -> `rejected`.
- Missing coverage command -> `coverage-unknown`, no error.
- Apply default writes only `removable` patches.
- MCP verify structured content exposes `coverage-unknown` and `rejected` verdicts.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo test -p deslop-verify`
- `cargo test --workspace`
- `cargo test -p deslop-verify -p deslop-mcp`
- `cargo fmt --all`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo clippy --workspace -- -D warnings`
- Final `cargo build --workspace`
- Final `cargo test --workspace`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 42 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.

Invalidated assumptions:
- Binary pass/fail verification is insufficient for removability. Passing parse/check guards now means only “not rejected”; automatic apply requires the stronger `removable` verdict.

Deferred exactly:
- Mutation probe / `cargo-mutants`.
- Characterization-test generation.
- Non-Rust coverage providers: Clojure cloverage, Julia Coverage.jl, Python coverage.py.

Blockers:
- None for this pass. Local live coverage path is expected to degrade if `cargo-llvm-cov` is absent.

Dependencies/restart requirements:
- No server or live process restart required.

Signature: Codex

---

Date/time: 2026-06-23 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md` forced dogfood refactor pass on `deslop` itself with analyzer rules frozen.

Target: Refactor real bloat without editing detection rules, safety classes, metric definitions, thresholds, or reference-only `deslop/*.py`. Required measurement was before/after `deslop scan crates` with the same analyzer and a target >=40% drop for `near-duplicate`, `needless-clone`, and `duplicate-block`.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 41 unit tests plus doc-tests.

Baseline scan:
- Command: `deslop scan crates --format json > /tmp/deslop-forced-before.json`
- Counts: `near-duplicate=139`, `needless-clone=40`, `duplicate-block=10`, `needless-return=17`.

Changes:
- Extracted shared external analyzer command runner/parser path in `deslop-external` for clj-kondo, clippy, and Julia analyzer adapters.
- Split `deslop-verify::prepare_patch` into named stale-workorder, stale-fingerprint, stale-region, guard, check-cmd, and outcome steps.
- Changed `PreparedPatch` to carry only path/replacement/range instead of a cloned full `WorkOrder`.
- Collapsed `AnalysisPack` boilerplate in `deslop-analyzer/src/lib.rs` with a local macro and typed external-analyzer helpers.
- Extracted repeated verify/MCP/analyzer test fixture helpers.
- Collapsed repeated core/protocol serde schema boilerplate with local macros.
- Narrowed owned-value copies from raw `.clone()` call sites to `to_owned`, `to_path_buf`, range reconstruction, and splice iteration where ownership was still required.
- Centralized MCP tool schema object envelopes.

Final scan:
- Command: `deslop scan crates --format json > /tmp/deslop-forced-after.json`
- Counts: `near-duplicate=125`, `needless-clone=7`, `duplicate-block=6`, `needless-return=21`.
- Target status: `needless-clone` met target (82.5% drop), `duplicate-block` met target (40% drop), `near-duplicate` did not meet target (10.1% drop).

Commands run:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- Multiple `deslop scan crates --format json` dogfood scans after refactor rounds.
- `sha256sum -c /tmp/deslop-frozen-before.sha`
- `jj diff --stat`
- `jj describe -m ...` after each refactor round.

Results:
- Final `cargo fmt --all --check`: pass.
- Final `cargo build --workspace`: pass.
- Final `cargo test --workspace`: pass, 41 unit tests plus doc-tests.
- Final `cargo clippy --workspace -- -D warnings`: pass.
- Frozen file checksum verification: pass for `crates/deslop-analyzer/src/{agnostic,clojure,julia,tokens}.rs`, `crates/deslop-metrics/src/lib.rs`, and `crates/deslop-lang/src/lib.rs`.

Invalidated assumptions:
- The requested >=40% `near-duplicate` drop cannot honestly be claimed from the completed refactors. The frozen surfaces alone still account for a large fixed floor, and the remaining removable near-duplicate clusters require a broader module split of large files (`deslop-external`, `deslop-mcp`, `deslop-verify`, and CLI) rather than more local helper extraction.

Current recommendation/checkpoint:
- The pass is behavior-preserving and verified, but incomplete against the requested near-duplicate metric.
- Next action should be a deliberate module-split refactor for `deslop-external`, `deslop-mcp`, `deslop-verify`, and CLI, still keeping analyzer/metrics/lang rule surfaces frozen.

Blockers:
- No build/test blocker. The blocker is scope/time for a larger file/module decomposition needed to hit the remaining near-duplicate target honestly.

Dependencies/restart requirements:
- No server or live process restart required.

Signature: Codex

---

## Session Report — Dogfood deslop on deslop

Date/time: 2026-06-23T11:34:36+02:00 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md`: dogfood the installed `deslop` CLI on deslop's own Rust crates, iterate through scan/metrics/fix/propose-style review until no deterministic safe edits remain and remaining hotspots/findings are either addressed or justified.

Target: Use the installed CLI (`/home/christos/.cargo/bin/deslop`), keep the workspace green after every round, avoid editing `deslop/*.py`, and report scan counts, metrics before/after, false positives, and convergence status.

Step 0 result before dogfood edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 41 unit tests plus doc-tests.
- `deslop metrics crates/ --hotspots-only` before: health `0.0/100`, 236 regions, 40 hotspots.
- Initial `deslop scan crates/` counts:
  - `near-duplicate` / `llm-only`: 131.
  - `needless-clone` / `llm-only`: 40.
  - `needless-return` / `safe-with-precondition`: 29.
  - `duplicate-block` / `llm-only`: 6.

Rounds and changes:
- Round 1:
  - Ran `deslop fix crates/`; no changes and no `*.deslop.bak` files.
  - Tuned Rust `needless-return` detection in `crates/deslop-analyzer/src/packs/rust.rs` to require the next non-empty line to be `}`. This keeps the real tail-return fixture but stops flagging `let-else` and early-return guards.
  - Reinstalled the CLI with `cargo install --path crates/deslop-cli --force`.
  - `needless-return` dropped from 29 to 17.
  - Gate passed; `jj describe -m "Dogfood: deslop self-cleanup round 1"`.
- Round 2:
  - Split `deslop-metrics::tokenize_code` into word/operator/token helpers.
  - Split Julia external command building, JSON mapping, and failure notice handling out of `julia_file_with_command`.
  - Gate passed; `jj describe -m "Dogfood: deslop self-cleanup round 2"`.
- Round 3:
  - Removed aggregate Rust `mod_item` metric regions from `deslop-lang`; whole modules, especially `#[cfg(test)] mod tests`, were swamping function/impl metrics.
  - Completed analyzer module split by restoring `agnostic.rs`, `clojure.rs`, and `julia.rs` files and keeping `tokens.rs`/`tests.rs` on disk.
  - Gate passed; `jj describe -m "Dogfood: deslop self-cleanup round 3"`.
- Round 4:
  - Changed `deslop-metrics` health score to penalize by hotspot ratio instead of subtracting 5 points per hotspot. The old formula made medium repos collapse to `0.0` even when average maintainability remained nonzero.
  - Gate passed; `jj describe -m "Dogfood: deslop self-cleanup round 4"`.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `deslop metrics crates/ --hotspots-only`
- `deslop metrics crates/ --format json`
- `deslop scan crates/ --format json`
- `deslop scan crates/ --format text`
- `deslop fix crates/`
- `deslop propose crates/ -o /tmp/deslop-wo.jsonl`
- Multiple round gates: `cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`
- Multiple CLI refreshes: `cargo install --path crates/deslop-cli --force`
- Final: `cargo fmt --all --check && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`

Final results:
- Final scan counts:
  - `near-duplicate` / `llm-only`: 139.
  - `needless-clone` / `llm-only`: 40.
  - `needless-return` / `safe-with-precondition`: 17.
  - `duplicate-block` / `llm-only`: 10.
- Final `deslop fix crates/`: no changes, no backups.
- Final metrics: health `33.6/100`, 281 regions, 63 hotspots.
- Top final hotspots:
  - `crates/deslop-verify/src/lib.rs:149` `prepare_patch`: real complexity; high-value future split.
  - `crates/deslop-analyzer/src/packs/rust.rs:52` `rust_findings`: real rule-density hotspot; partially improved by false-positive tune.
  - `crates/deslop-lang/src/lib.rs:288` Rust `LangPack` impl: mostly declarative pack metadata.
  - `crates/deslop-metrics/src/lib.rs:536` hotspot detection: intrinsic metric aggregation logic.
  - `crates/deslop-analyzer/src/tokens.rs:15` token duplication detector: real algorithmic complexity.

False positives / tuning findings:
- `needless-return`: clear false positives on `return` inside `let-else` and early-return guards. Fixed partially by requiring the next non-empty line to be `}`. Residual `needless-return` findings still need CST-aware tail-position detection; line heuristics remain weak.
- `near-duplicate` / `duplicate-block`: many reports are structural Rust repetition, not cleanup:
  - trait impl methods with the same shape;
  - enum/struct serde fields;
  - test fixtures with intentionally parallel assertions;
  - protocol struct literals and JSON schema literals.
  Preferred next tuning: ignore declarations/field lists/test fixture literals or raise the default token threshold for Rust structural contexts.
- `needless-clone`: 40 reports remain, but clippy is green. Most are ownership-preserving clones in protocol/test construction and should only be actionable with clippy or borrow-check confirmation.
- Metrics false positive: Rust `mod_item` regions aggregate child functions and caused `#[cfg(test)] mod tests` to dominate hotspots. Fixed by removing `mod_item` from Rust metric regions.
- Metrics health false positive: raw hotspot-count penalty collapsed health to zero for medium repos. Fixed by using hotspot ratio.

Convergence decision:
- No `safe-auto` or `analyzer-confirmed` findings remain; `deslop fix` is a no-op.
- Remaining scan findings are non-deterministic (`llm-only`) or `safe-with-precondition` requiring stronger CST/typecheck semantics.
- Remaining hotspots are either real larger refactors (`prepare_patch`, token duplication, hotspot detection) or declarative/intrinsic pack metadata. They are not safe to rewrite further in this pass without broader design changes.
- Stopped at convergence under the current deterministic safety contract.

Verification:
- Final `cargo fmt --all --check`: pass.
- Final `cargo build --workspace`: pass.
- Final `cargo test --workspace`: pass, 41 unit tests plus doc-tests.
- Final `cargo clippy --workspace -- -D warnings`: pass.

Blockers:
- None for this dogfood pass.

Dependencies/restart requirements:
- Installed `/home/christos/.cargo/bin/deslop` was refreshed from the current workspace after each meaningful round.
- No services or live processes require restart.

Signature: Codex

---

# Session Report: Deslop Cleanup Continuation

Date/time: 2026-06-23 Europe/Madrid

Objective: Continue the dogfood cleanup after the first verified deslop pass, focusing on the analyzer monolith rather than individual low-confidence duplicate-token warnings.

Target: Split `crates/deslop-analyzer/src/lib.rs` into focused modules for agnostic rules, Clojure rules, Julia rules, token duplication, and analyzer tests while preserving behavior and safety classes.

Changes:
- Added `crates/deslop-analyzer/src/tokens.rs` and moved token duplication/tokenization logic out of `lib.rs`.
- Added `crates/deslop-analyzer/src/tests.rs` and moved analyzer unit tests out of `lib.rs`.
- Added `crates/deslop-analyzer/src/clojure.rs` and moved Clojure safe-auto/precondition/risky-suggest rule bodies plus Clojure comment/symbol helpers.
- Added `crates/deslop-analyzer/src/julia.rs` and moved Julia idiom rule bodies.
- Added `crates/deslop-analyzer/src/agnostic.rs` and moved blank-line/comment narration rules plus line-comment/banner helpers.
- `lib.rs` now keeps analyzer config, pack registry, scan orchestration, dispatch, sorting, external notices, and the shared finding constructor.

Commands run:
- `cargo fmt --all`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- MCP deslop `metrics` over analyzer module files
- `wc -l` over analyzer module files
- `jj diff --stat`
- `jj status`

Results:
- `cargo check --workspace`: pass.
- `cargo fmt --all --check`: pass.

---

## Session Report — Duplication Precision CST Pass

Date/time: 2026-06-23T13:07:50+02:00 Europe/Madrid

Objective: Execute rewritten `.agents/NEXT_TASK.md`: stop the Rust-specific duplication approach, get the workspace green, then implement duplication precision with tree-sitter CST node classification owned by each `LangPack`.

Target: No central `match Lang`, no regex/token-pattern language logic in the duplication detector, no edits to `deslop/*.py`, and FP/TP corpora covering Rust, Clojure, and Julia.

Step 0 result:
- `cargo test -p deslop-analyzer`: pass after correcting the interrupted state.
- `cargo test --workspace`: pass, 40 unit tests plus doc-tests.
- Baseline scan from the pre-pass CLI output in `/tmp/deslop-precision-before-target.json`: `near-duplicate=125`, `duplicate-block=6`, combined duplication findings `131`.

Changes:
- Added `RegionClass` to `deslop-lang::LangPack`; Rust, Clojure, and Julia packs now classify their own CST node kinds as `Behavioral`, `Declaration`, or `Other`.
- Reworked `deslop-analyzer::tokens` so duplication tokenization walks the tree-sitter CST, keeps only pack-classified behavioral regions, excludes declaration regions, and applies a secondary meaningful-token threshold.
- Added clean FP fixtures in `tests/fixtures/clean/` for Rust/Clojure/Julia structural repetition and behavioral TP fixtures in `tests/fixtures/dup/` for all three languages.
- Added analyzer tests proving clean structural code produces zero `near-duplicate`/`duplicate-block` findings while behavioral duplicate corpora still flag.
- Moved `needless-return` tail-position detection into a shared CST walker in `agnostic.rs`; Rust opts in through `LangPack::tail_position_class`, so Rust node kinds are no longer hard-coded in the analyzer rule.
- Removed the interrupted Rust-local tail-return walker from `crates/deslop-analyzer/src/packs/rust.rs`.

Measured scan result:
- Command: `target/debug/deslop scan crates --format json > /tmp/deslop-precision-after.json`
- After counts: `near-duplicate=56`, `duplicate-block=12`, combined duplication findings `68`.
- Combined drop: `131 -> 68`, a 48.1% reduction from the CST precision change.

Spot-checks of remaining duplication findings:
- `crates/deslop-analyzer/src/agnostic.rs`: repeated `finding(...)` construction inside rule functions, behavioral code.
- `crates/deslop-analyzer/src/clojure.rs`: repeated rule-loop and finding construction logic, behavioral code.
- `crates/deslop-cli/src/main.rs`: repeated enum conversion/config-test logic, behavioral/test code.
- `crates/deslop-external/src/lib.rs`: repeated external-adapter mapping/fallback logic, behavioral code.
- No sampled remaining finding was in a struct field list, namespace/import block, or other declaration-only region.

Commands run:
- `cargo test -p deslop-analyzer`
- `cargo test --workspace`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `target/debug/deslop scan crates --format json`
- `cargo clippy --workspace -- -D warnings`
- `jj diff --stat`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 40 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- FP corpus: Rust/Clojure/Julia clean structural fixtures produce zero duplication findings.
- TP corpus: Rust/Clojure/Julia behavioral duplicate fixtures still produce duplication findings.

Invalidated assumptions:
- The interrupted Rust-specific approach was the wrong boundary. The durable boundary is pack-owned CST classification plus shared detector traversal.

Blockers:
- None.

Dependencies/restart requirements:
- No server or live process restart required.

Signature: Codex
- `cargo test --workspace`: pass, 37 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- Analyzer `lib.rs` reduced to 452 lines; moved modules are:
  - `agnostic.rs`: 173 lines
  - `clojure.rs`: 244 lines
  - `julia.rs`: 61 lines
  - `tokens.rs`: 203 lines
  - `tests.rs`: 209 lines
- Residual deslop hotspots are now local to specific modules, mainly `tokens::duplicate_token_sequences`, `julia::findings`, `scan_paths_with_config`, and Clojure rule helpers.

Invalidated assumptions:
- None new. This was structural cleanup only; no rule safety class, edit generation, or external analyzer behavior changed.

Current recommendation/checkpoint:
- Analyzer ownership boundaries are materially clearer. Further cleanup should be behavioral-helper extraction inside specific modules, not another broad split.

Blockers:
- None.

Next actions:
- If continuing, refactor `tokens::duplicate_token_sequences` into match classification/report helpers and split `scan_paths_with_config` path walking from scan dispatch.

Dependencies/restart requirements:
- No server or live process restart required.

Signature: Codex

---

# Session Report: Deslop Cleanup Pass

Date/time: 2026-06-23 Europe/Madrid

Objective: Deslop the current codebase using deslop's own scan/metrics findings, while preserving the deterministic analyzer contract and safety lattice.

Target: Reduce high-confidence local bloat in analyzer orchestration, tokenizer branching, delimiter fallback parsing, and verify-test duplication without changing public CLI behavior.

Changes:
- Changed `deslop_lang::Rule::check` and analyzer pack `external_analyzer` dispatch to borrow `AnalyzerConfig`, removing repeated config cloning through rule execution.
- Extracted analyzer scan helpers:
  - `push_supported_report`
  - `analysis_pack_for_path`
  - `empty_report`
- Extracted tokenizer helpers from the main `tokenize` branch chain:
  - `next_token`
  - `string_token`
  - `identifier_token`
  - `number_token`
  - `one_char_token`
  - `token_from_slice`
  - `consume_while`
  - `skip_until_newline`
- Extracted `deslop-verify` fallback parse helpers:
  - `skip_until_newline`
  - `closes_last_open`
- Compressed repeated verify-test setup with fixture, work-order, patch, and verify helpers.

Commands run:
- `cargo fmt --all`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- MCP deslop `metrics` for `crates/deslop-analyzer/src/lib.rs` and `crates/deslop-verify/src/lib.rs`
- MCP deslop `scan` for `crates/deslop-analyzer/src/lib.rs` and `crates/deslop-verify/src/lib.rs`
- `jj diff --stat`
- `jj status`

Results:
- `cargo check --workspace`: pass.
- `cargo fmt --all --check`: pass.
- `cargo test --workspace`: pass, 37 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- Re-scan result: the previous `tokenize` top hotspot is gone from the top hotspot list; `deslop-verify` tests dropped from 166 NLOC to 116 NLOC. Residual findings remain in broader rule/test structure and are mostly `llm-only`/low-confidence duplication or intentional safety-gated suggestions.

Invalidated assumptions:
- None new. Existing negative memory remains active: parse/syntax validation is not behavior preservation, so the cleanup avoided changing analyzer rule safety classes or auto-fix policy.

Current recommendation/checkpoint:
- This cleanup pass is verified and behavior-preserving.
- Further cleanup should split large analyzer rule families and test modules into focused modules rather than trying to mechanically silence token-duplicate findings.

Blockers:
- Serena symbol extraction is unavailable for Rust in this project; it reports active language support as Python only. Local targeted reads were used instead.

Next actions:
- If continuing cleanup, split `crates/deslop-analyzer/src/lib.rs` into rule modules (`agnostic`, `clojure`, `julia`, `tokens`, tests) and split verify tests into an integration-style fixture module.

Dependencies/restart requirements:
- No server or live process restart required.

Signature: Codex

---

## Session Report — Julia T2 external analyzer pass

Date/time: 2026-06-23T10:14:49+02:00 Europe/Madrid

Objective: Execute the superseding `.agents/NEXT_TASK.md`: bring Julia to T2 external-analyzer parity while keeping external analysis default-off and gracefully degrading on the current machine where StaticLint/JET are not installed.

Target: Add a pack-local Julia `ExternalAnalyzer` through the existing trait, with StaticLint as the chosen analyzer, CLI opt-in/project selection, fixture mapping tests, live degrade coverage, SPEC update, and final fmt/build/test/clippy verification. Explicitly deferred: SARIF, bundled slim consumer, LSP.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 31 existing unit tests plus doc-tests.

Changes:
- Added `JuliaAnalyzer` in `deslop-external`.
  - Supports `JuliaAnalyzerKind::{StaticLint, Jet}` and shells out through `julia --startup-file=no`.
  - Passes `--project=...` when a Julia project path is configured.
  - Captures helper stdout/stderr and enforces a 10s timeout so helper failures produce one fallback notice instead of leaking analyzer output.
  - Maps recorded StaticLint JSON diagnostics:
    - `unused-arg` -> `SafetyClass::AnalyzerConfirmed`, `DetectedBy::JuliaAnalyzer`.
    - `unused-binding` -> `SafetyClass::AnalyzerConfirmed`, `DetectedBy::JuliaAnalyzer`.
    - `missing-reference` -> `SafetyClass::NeverAuto`, report-only.
  - Keeps JET diagnostics report-only/`never-auto` under the same subprocess contract.
- Extended `AnalyzerConfig` with:
  - `julia_external: JuliaExternal` defaulting to `Off`.
  - `julia_project: Option<PathBuf>`.
- Wired Julia external analysis in the Julia `AnalysisPack` only; no central `match Lang` dispatch was added.
- Added CLI options on `scan` and `propose`:
  - `--julia-external [staticlint|jet|off]`, with bare `--julia-external` selecting StaticLint.
  - `--julia-project <PATH>`.
- Added narrow `deslop.toml` support for `[external]`:
  - `julia_analyzer = "off" | "staticlint" | "jet"`.
  - `julia_project = "..."`.
  - `clippy = "off" | "on"` for parity with the existing Rust external switch.
  - CLI flags override config values.
- Updated `deslop rules` output for external analyzer-confirmed `unused-arg`/`unused-binding`.
- Updated `SPEC.md` to promote Julia StaticLint/JET from deferred to config-gated/default-off T2, document `[external] julia_analyzer=off|staticlint|jet` and `julia_project`, and record graceful fallback semantics.
- Added tests:
  - StaticLint recorded JSON fixture maps to expected findings and safety classes.
  - Absent Julia executable degrades cleanly.
  - Julia external is config-gated at the pack boundary.
  - Live current-machine StaticLint-missing path falls back and preserves T1 Julia findings.
  - CLI config parsing and CLI-over-config override precedence.

Analyzer choice:
- Chosen: StaticLint.jl.
- Reason: the task is code-bloat analysis, and StaticLint's unused argument/binding diagnostics map directly to analyzer-confirmed cleanup candidates. JET is correctness-oriented, so this pass keeps JET diagnostics report-only/never-auto.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- Targeted: `cargo test -p deslop-external`
- Targeted: `cargo test -p deslop-analyzer`
- `cargo run -p deslop-cli -- scan --help`
- `cargo run -p deslop-cli -- propose --help`
- CLI smoke: temp Julia file + `cargo run -p deslop-cli -- scan "$tmp/sample.jl" --julia-external --format json`
- Config smoke: temp `deslop.toml` with `[external] julia_analyzer = "staticlint"` + `deslop scan sample.jl --format json`
- Final: `cargo fmt --all --check`
- Final: `cargo build --workspace`
- Final: `cargo test --workspace`
- Final: `cargo clippy --workspace -- -D warnings`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 41 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI smoke: pass. It emitted one StaticLint unavailable notice because the package is not installed, then returned the T1 `reimpl-isnothing` finding in JSON.
- Config smoke: pass. `deslop.toml` enabled StaticLint, emitted the same one-line unavailable notice on this machine, and returned the T1 Julia finding.
- `scan --help` and `propose --help`: both show `--julia-external [<JULIA_EXTERNAL>]` with `staticlint`, `jet`, `off`, and `--julia-project`.

Invalidated assumptions:
- The first timeout-runner version inherited the Julia helper stderr, which violated the one-line degrade posture. Fixed by piping stdout/stderr before spawning.
- The earlier assumption that TOML config could stay documented-only was too narrow for the task contract. Fixed by adding a minimal `deslop.toml` parser for `[external]` keys.

Current recommendation/checkpoint:
- Julia has a T2 external-analyzer adapter under the same trait boundary as Clojure/Rust.
- StaticLint present-path behavior is fixture-tested because the local Julia environment lacks StaticLint and JET.
- Live degrade is verified on the current machine.

Deferred exactly:
- SARIF.
- Bundled slim consumer.
- LSP.

Blockers:
- None for this pass.
- Live StaticLint present-path execution requires a Julia project/environment with `StaticLint` installed and should be rechecked when that dependency is available.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- Julia 1.12.5 is on PATH for the live degrade smoke.
- No server or live process restart required.

Signature: Codex

---

# Session Report: MCP Server

Date/time: 2026-06-23 Europe/Madrid

Objective: Execute the superseding `.agents/NEXT_TASK.md`: expose deslop analyzer/propose/verify/apply/metrics/rules over MCP stdio.

Target: Add a feature-gated `deslop-mcp` crate and `deslop mcp` subcommand. Keep core/default CLI lean without the MCP dependency. Explicit deferrals: SARIF, bundled `slim` consumer, JET, LSP.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 27 unit tests plus doc-tests.

Library choice:
- Checked `rmcp` with `cargo info rmcp`; it is official/maintained (`rmcp 1.7.0`, repository `modelcontextprotocol/rust-sdk`).
- Chose a minimal JSON-RPC 2.0 stdio server for this pass anyway because the required MCP surface is only `initialize`, `tools/list`, and `tools/call`, and the minimal implementation keeps the feature network-free, dependency-light, and directly tied to deslop's existing serde schemas. No `rmcp` dependency was added.

Changes:
- Added `deslop-mcp` crate.
- Implemented stdio JSON-RPC handling:
  - `initialize`
  - `tools/list`
  - `tools/call`
- Exposed MCP tools:
  - `scan(paths, format?)`
  - `propose(paths)`
  - `verify(patches, check_cmd?)`
  - `apply(patches, check_cmd?, no_backup?)`
  - `metrics(paths, sigma?)`
  - `rules()`
- `tools/list` declares input schemas for every tool, including `deslop.patch/1` shape for verify/apply.
- Tool outputs include MCP `content` text and `structuredContent`.
- Reused existing deterministic crates:
  - `deslop-report` JSON for scan.
  - `deslop-protocol` work orders and patches.
  - `deslop-verify` verify/apply gate.
  - `deslop-metrics` metrics JSON.
- Added `deslop-cli` optional dependency and feature:
  - `[features] mcp = ["dep:deslop-mcp"]`
  - `deslop mcp` subcommand exists only with `--features mcp`.
- Fixed path fingerprint normalization so `./path` and `path` produce the same stable fingerprint. This was required for MCP propose/verify round-trips where path spelling can differ between direct path scans and repo walks.
- Updated `SPEC.md` to mark MCP as implemented and feature-gated.

Tests added:
- `tools/list` returns exactly `scan`, `propose`, `verify`, `apply`, `metrics`, `rules`, each with an input schema.
- `tools/call scan` on a fixture returns `deslop.findings/1` JSON with the expected finding.
- MCP propose -> verify round-trip accepts a clean patch and rejects a stale `region_fingerprint`.
- initialize -> tools/list -> tools/call scan stdio transcript test.
- Default/no-feature CLI build verified separately from `--features mcp`.

Handshake smoke:
- Ran `cargo run -q -p deslop-cli --features mcp --bin deslop -- mcp` with three newline-delimited JSON-RPC requests:
  - `initialize`
  - `tools/list`
  - `tools/call scan`
- It returned three JSON-RPC responses. The scan response had `structuredContent.schema = "deslop.findings/1"` and included a `reimpl-empty?` finding for the temp Clojure fixture.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo search rmcp --limit 5`
- `cargo info rmcp`
- `cargo check --workspace`
- `cargo check -p deslop-cli --features mcp`
- `cargo test -p deslop-mcp`
- `cargo test --workspace`
- `cargo test -p deslop-cli --features mcp`
- `cargo fmt --all`
- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo build -p deslop-cli --no-default-features`
- `cargo build -p deslop-cli --features mcp`
- `cargo clippy -p deslop-cli --features mcp -- -D warnings`
- MCP stdio smoke via `cargo run -q -p deslop-cli --features mcp --bin deslop -- mcp`
- Final `cargo build --workspace`
- Final `cargo build -p deslop-cli --no-default-features`
- Final `cargo build -p deslop-cli --features mcp`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 31 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- `cargo build -p deslop-cli --no-default-features`: pass.
- `cargo build -p deslop-cli --features mcp`: pass.
- `cargo clippy -p deslop-cli --features mcp -- -D warnings`: pass.
- MCP stdio smoke: pass.

Deferred exactly:
- SARIF.
- bundled `slim` consumer.
- JET.
- LSP.

Invalidated assumptions:
- Workorder fingerprints were sensitive to a leading `./` path spelling. Normalization now strips leading `./` before hashing.

Blockers:
- None for this pass.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- No server or live process restart required; `deslop mcp` is an on-demand stdio process.

Signature: Codex

---

# Session Report: Metrics / Health

Date/time: 2026-06-23 Europe/Madrid

Objective: Execute the superseding `.agents/NEXT_TASK.md`: add metrics/health complexity, expressivity, and repo-relative hotspot ranking.

Target: Build metrics on the LangPack abstraction with no central `match Lang`; each pack declares metrics node/token behavior. Explicit deferrals: MCP, SARIF, JET, LSP.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 23 unit tests plus doc-tests.

Changes:
- Extended `deslop-lang::LangPack` with metrics declarations:
  - `metrics_regions()`
  - `metrics_branches()`
  - `metrics_nesting()`
  - `metrics_flow_breaks()`
  - `halstead_operator_tokens()`
- Added `deslop-metrics` crate:
  - walks inputs with `ignore`;
  - collects per-region metrics from CST regions declared by the active pack;
  - falls back to text-level metrics for no-grammar/generic regions;
  - computes cyclomatic, cognitive, max nesting, NLOC, Halstead Volume/Difficulty/Effort, Maintainability Index;
  - computes decision density, unique-token ratio, comment-to-code ratio, and compression ratio.
- Compression ratio uses a byte-entropy proxy normalized to `0.0..1.0` instead of adding a deflate dependency.
- Added repo-relative hotspot ranking using median + `--sigma` standard deviations for high complexity and low expressivity. Low-expressivity hotspot checks require at least 16 tokens to avoid tiny-helper false positives.
- Added CLI:
  - `deslop metrics [PATHS…] [--format text|json] [--hotspots-only] [--sigma N]`
  - `deslop health` alias.
- Updated `SPEC.md` to promote metrics/health from deferred/experimental into a real section.

Tests added:
- Cyclomatic on Rust fixture with known branch count.
- Halstead on known snippet.
- Hotspot detection flags a deliberately bloated outlier and not clean functions.
- A throwaway pack declaring metric operator tokens drives Halstead without central edits.

Measured Halstead test numbers:
- snippet: `a + b * c`
- distinct operators: 2
- total operators: 2
- distinct operands: 3
- total operands: 3
- Volume: 11.609640
- Difficulty: 1.000000
- Effort: 11.609640

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo check --workspace`
- `cargo test -p deslop-metrics`
- `cargo fmt --all`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- CLI smoke:
  - `deslop metrics <tmp>/sample.rs --sigma 1.0`
  - `deslop metrics <tmp>/sample.rs --format json --sigma 1.0`
  - `deslop health <tmp>/sample.rs --hotspots-only`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 27 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI smoke: pass; text/JSON ranked `bloated` as the single hotspot, and `health` alias printed a no-hotspot report for a clean function.

Deferred exactly:
- MCP.
- SARIF.
- JET.
- LSP.

Invalidated assumptions:
- Entropy/compression is noisy on tiny regions. Low-expressivity hotspot checks now require at least 16 code tokens.

Blockers:
- None for this pass.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- No server or live process restart required.

Signature: Codex

---

# Session Report: Complete LangPack Abstraction

Date/time: 2026-06-23 Europe/Madrid

Objective: Execute the superseding `.agents/NEXT_TASK.md`: eliminate residual central per-language match arms from parse/analyzer core.

Target: Move extension detection, tree-sitter grammar selection, CST region extraction, and comment syntax into a low registry shared by parser and analyzer. Keep `fmt`/`build`/`test`/`clippy -D warnings` green and update `SPEC.md`.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 22 unit tests plus doc-tests.

Changes:
- Added `deslop-lang` crate as the shared low-level language registry.
- Moved language behavior into `deslop-lang::LangPack`:
  - `extensions()` for path detection.
  - `grammar()` for tree-sitter parser selection.
  - `enclosing_region(...)` for CST region extraction.
  - `line_comments()` for analyzer/tokenizer comment syntax.
- Moved the generic `Rule` trait into `deslop-lang`.
- Moved `ExternalFindings` and the generic `ExternalAnalyzer` trait into `deslop-lang`; `deslop-external` now implements and re-exports them for clj-kondo/clippy.
- Refactored `deslop-parse` to use `deslop-lang::Registry` for:
  - path-to-language detection;
  - parser creation;
  - region extraction.
- Refactored `deslop-analyzer` to use `deslop-lang::Registry` for:
  - supported path detection before scan;
  - comment-token lookup in `starts_line_comment`;
  - comment-token lookup in `line_comment`.
- Renamed the analyzer-side registry to `AnalyzerRegistry` and the analyzer-side pack trait to `AnalysisPack`, keyed by stable `Lang` id and using `deslop-lang::Rule`.
- Updated `SPEC.md` to document `deslop-lang` and the revised `LangPack` surface.
- Added a registry acceptance test with a throwaway `.testpack` language pack and a matching analyzer rule pack through scan.
- Moved the throwaway acceptance pack into `crates/deslop-analyzer/src/test_pack.rs` so the proof has an explicit pack module.

Central match arms removed:
- `crates/deslop-parse/src/lib.rs:134-137 before` region dispatch `match lang { Lang::Clojure => ..., Lang::Julia => ..., Lang::Rust => ..., _ => None }` -> gone; `enclosing_region` now calls `pack.enclosing_region(...)`.
- `crates/deslop-parse/src/lib.rs:142-148 before` extension-to-`Lang` match -> gone; `SourceFile::new` now calls `deslop_lang::detect_lang`.
- `crates/deslop-parse/src/lib.rs:168-190 before` tree-sitter grammar `match lang` -> gone; parser creation now uses `pack.grammar()`.
- `crates/deslop-analyzer/src/lib.rs:915-918 before` `starts_line_comment` `match source.lang` -> gone; it now calls `pack.line_comments()`.
- `crates/deslop-analyzer/src/lib.rs:985-988 before` `line_comment` `match lang` -> gone; it now calls `pack.line_comments()`.

Acceptance proof:
- Test name: `registry_discovers_registered_test_pack_through_scan_without_core_matches`.
- The throwaway pack declares `.testpack` detection, generic grammar fallback (`grammar() -> None`), comment syntax, and a matching analyzer rule.
- It scans a real temp file through injected language/analyzer registries and reports `test-pack-rule`.
- Files touched for the throwaway test pack:
  - `crates/deslop-analyzer/src/test_pack.rs` (pack module: detection, grammar fallback, region fallback, comment syntax, rule)
  - `crates/deslop-analyzer/src/lib.rs` (one test registration/use site)
- No production central match was added.
- For production low-level language additions, the required files are now exactly the new `deslop-lang` pack module plus the single `Registry::with_default_packs` registration line. Analyzer rules/external analyzers remain optional analysis packs keyed by `Lang`.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo fmt --all --check` (failed before rustfmt, formatting only)
- `cargo fmt --all`
- `cargo test --workspace`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo clippy --workspace -- -D warnings`
- Final rerun after moving `Rule`/`ExternalAnalyzer` low:
  - `cargo fmt --all --check`
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
- Final rerun after moving the throwaway pack into its own module:
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo build --workspace`
  - `cargo fmt --all`
  - `cargo fmt --all --check`
- CLI smoke: temporary Rust file scanned with `deslop scan --format json`, returning `lang: "rust"` and `needless-return` with `edit: null`.
- Audit: `rg` for the old parse/analyzer `Lang` match arms; only pack lookup calls in analyzer and pack implementations in `deslop-lang` remain.

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 23 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI smoke: pass.

Deferred:
- None newly deferred by this task.

Invalidated assumptions:
- The previous “Rust is pack-local” claim was incomplete: parse/analyzer still had central per-language behavior. This pass supersedes that by moving low-level language behavior to `deslop-lang`.

Blockers:
- None for this pass.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- No server or live process restart required.

Signature: Codex

---

# Session Report: Modular Plugin Refactor + Rust LangPack

Date/time: 2026-06-23 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md` in full against `SPEC.md` v0.4.

Target: Introduce registry-backed `LangPack` / `Rule` / `ExternalAnalyzer` architecture and prove it by adding Rust as a first-class language in the same pass. Explicit deferrals: MCP, SARIF, JET/StaticLint, LSP, metrics.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 18 unit tests total plus doc-tests.

Changes:
- Added `Rule` and `LangPack` traits plus a `Registry` in `deslop-analyzer`.
- Refactored analyzer dispatch so file scanning and source scanning use pack detection/registry lookup instead of per-language CLI dispatch.
- Put agnostic, Clojure, and Julia behavior behind pack/rule interfaces.
- Added Rust as a first-class language:
  - `Lang::Rust` and `.rs` detection.
  - `tree-sitter-rust` dependency.
  - Rust CST region extraction for function, impl, and module items.
  - Rust parser/error-node support in the tree-sitter parse path.
  - Rust idiom rules: `needless-return`, `useless-format`, `redundant-closure`, `let-and-return`, `needless-clone` with requested safety classes.
- Added `crates/deslop-analyzer/src/packs/rust.rs`; Rust analyzer rules and clippy selection live there.
- Added `ExternalAnalyzer` trait in `deslop-external`; clj-kondo implements it.
- Added config-gated clippy external analyzer:
  - shells out through `cargo clippy --message-format=json`;
  - maps recorded JSON lints to findings for `needless-return`, `needless-clone`, `let-and-return`, `useless-format`, and `redundant-closure`;
  - degrades cleanly when cargo/clippy is absent.
- Added CLI `--rust-external` opt-in for `scan` and `propose`.
- Updated `SPEC.md` to list Rust as first-class and formalize `LangPack`, `Rule`, and `ExternalAnalyzer`.

Hard acceptance check:
- Rust analyzer behavior is pack-local in `crates/deslop-analyzer/src/packs/rust.rs`.
- Core analyzer registration is one line: `registry.register(&packs::rust::RUST_PACK);`.
- CLI dispatch stays registry/config driven and has no Rust-specific scan/propose branch.
- Parser/core enum additions are the required shared language/grammar support, not analyzer dispatch logic.

Tests added/covered:
- Registry-driven dispatch discovers a test pack without core edits.
- Rust tree-sitter region extraction on a `.rs` fixture.
- Rust idiom detected with fix withheld without `--check-cmd`.
- Clippy adapter maps a recorded JSON fixture.
- Clippy absent-path degrades cleanly.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- Final `cargo fmt --all --check`
- CLI smoke: temporary `.rs` file scanned with `deslop scan --format json`, returning `lang: "rust"` and `needless-return` with `edit: null`.

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 22 unit tests total plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI Rust smoke: pass.

Invalidated assumptions:
- None new. Existing safety negative memory remains active: syntax/CST checks do not prove behavioral preservation, so Rust idioms remain non-auto unless safety and check-cmd gates allow application.

Current recommendation/checkpoint:
- Plugin architecture is in place and exercised by Rust. The next pass can add another language/analyzer by implementing a pack module plus a registry registration, with parser grammar support when the language is new.

Deferred exactly:
- MCP.
- SARIF.
- JET/StaticLint.
- LSP.
- metrics.

Blockers:
- None for this pass.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- No server or live process restart required.

Signature: Codex

---

# Session Report

Date/time: 2026-06-23 Europe/Madrid

Objective: Continue `deslop` from `SPEC.md` v0.4 and complete the AST-UPGRADE pass.

Target: Real tree-sitter parsing/regions, tree-sitter parse checks in verify, clj-kondo external adapter, and token-level duplication. Explicitly deferred: MCP, SARIF, JET/StaticLint, and LSP.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`, 0.23s.
- `cargo test --workspace`: pass, 9 unit tests total at start of session.

Changes:
- Added real tree-sitter dependencies:
  - `tree-sitter`
  - `tree-sitter-clojure`
  - `tree-sitter-julia`
- Upgraded `deslop-parse`:
  - parser construction for Clojure and Julia;
  - tree-sitter ERROR-node detection;
  - CST-based Clojure enclosing top-level `list_lit` region extraction;
  - CST-based Julia enclosing `function_definition` / `struct_definition` / `module_definition` region extraction;
  - generic delimiter balance remains only as fallback for unsupported languages.
- Updated work-order generation to use CST enclosing regions instead of the finding line span.
- Updated `deslop-verify` parse-check to use tree-sitter ERROR-node detection for Clojure/Julia and delimiter-balance fallback only when no tree-sitter grammar is available.
- Added new `deslop-external` crate:
  - shells out to `clj-kondo --lint PATH --config "{:output {:analysis true :format :json}}"`;
  - maps `unused-binding`, `unused-private-var`, `unused-namespace`, and `redundant-do` from clj-kondo JSON;
  - emits analyzer-confirmed findings;
  - attaches an analyzer-confirmed edit for clj-kondo-confirmed `redundant-do`;
  - degrades cleanly when `clj-kondo` is absent with a one-line notice and no hard error.
- Updated analyzer integration:
  - `scan_file` consults clj-kondo for Clojure files when available;
  - when clj-kondo is available, covered rules are delegated to it to avoid double-reporting;
  - when clj-kondo is absent, built-in T1 rules remain active.
- Replaced the old line-window duplicate detector with token-level duplicate detection:
  - exact token sequence clone => `duplicate-block`;
  - normalized renamed-identifier clone => `near-duplicate`;
  - both remain `llm-only`.
- Updated `fix` to permit concrete `analyzer-confirmed` edits in addition to `safe-auto`, while still refusing suggest-only classes.

Tests added:
- Clojure tree-sitter region extraction fixture.
- Julia tree-sitter region extraction fixture.
- Verify rejects a broken Clojure patch via tree-sitter ERROR-node parse-check.
- clj-kondo recorded JSON fixture mapping.
- absent clj-kondo path degrades cleanly.
- token duplication detects an exact clone.
- token duplication detects a renamed clone.
- token duplication ignores a non-clone.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo check --workspace`
- `cargo test -p deslop-analyzer --lib`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- Final `cargo fmt --all --check`
- Final `cargo build --workspace`
- Final `cargo test --workspace`
- Final `cargo clippy --workspace -- -D warnings`
- CLI smoke:
  - create temp Clojure `defn` containing `(= (count xs) 0)`;
  - `scan . --format agent`;
  - assert JSONL region spans full top-level defn (`start_line:1`, `end_line:2`);
  - construct `deslop.patch/1`;
  - `verify --patches patches.jsonl`;
  - assert verify passed.

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 18 unit tests total plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI smoke: pass.
- Local environment note: `clj-kondo` is not on PATH, and the CLI smoke emitted the intended one-line fallback notice. The recorded JSON fixture validates mapping behavior independent of local clj-kondo installation.

Invalidated assumptions:
- None new. Existing safety negative memory remains active: tree-sitter proves syntax structure, not behavior. Semantic-risk patches still require the verify/apply gate and appropriate `--check-cmd`.

Current recommendation/checkpoint:
- AST-UPGRADE pass is complete within the requested scope.
- The biggest remaining deterministic analyzer gaps are now the explicitly deferred integrations/features below.

Deferred exactly:
- MCP.
- SARIF.
- JET/StaticLint.
- LSP.

Blockers:
- None for this scoped pass.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- `clj-kondo` optional; absent path is graceful.
- No server or live process restart required.

Signature: Codex

---

# Session Report

Date/time: 2026-06-23 Europe/Madrid

Objective: Continue `deslop` from `SPEC.md` v0.4 and build M2 scoped to protocol + verify/apply.

Target: M2 core loop only: exact sec5 protocol surface, `scan --format agent`/`propose` JSONL work orders, `deslop-verify` deterministic network-free gate, and CLI `verify`/`apply`. Explicitly deferred: clj-kondo, token duplication, JET, and real tree-sitter.

Step 0 result before edits:
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 4 unit tests at start of session.

Changes:
- Updated `deslop-protocol` work orders to match SPEC sec5 serialized fields: `schema`, `id`, `path`, `region`, `findings`, `instruction`, `contract`. Removed the extra serialized `region_fingerprint` from work orders.
- Kept patch schema as SPEC sec5: `schema`, `workorder_id`, `region_fingerprint`, `replacement`, `by`.
- Added helper fingerprint/id functions so patches can carry the region fingerprint while work orders stay schema-exact.
- Added `deslop-verify` crate with no network dependencies.
- Implemented deterministic gate:
  - current work-order rediscovery from analyzer output;
  - stale/unknown workorder and stale `region_fingerprint` rejection;
  - current region byte comparison;
  - delimiter balance check as the scoped parse/re-parse substitute until real tree-sitter;
  - `--check-cmd` execution on a temp project copy with the patch applied;
  - defensive-code guard for deletion of try/catch/error/assert/precondition constructs;
  - `max_growth_ratio` guard;
  - `no_new_public_defs` guard;
  - atomic writes with `.deslop.bak` unless `--no-backup`.
- Wired CLI:
  - `deslop verify --patches FILE [--check-cmd]`
  - `deslop apply --patches FILE [--check-cmd] [--no-backup]`
- Added required tests:
  - protocol round-trip `workorder -> patch -> verify`;
  - deleting try/catch rejected;
  - stale region fingerprint rejected;
  - apply writes only passing patches.

Commands run:
- Step 0: `cargo build --workspace`
- Step 0: `cargo test --workspace`
- `cargo check --workspace`
- `cargo test -p deslop-verify`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt --all --check`
- Final `cargo build --workspace`
- Final `cargo test --workspace`
- CLI smoke:
  - create temp Clojure file with `(= (count xs) 0)`
  - `scan . --format agent`
  - construct `deslop.patch/1` from `wo_<fingerprint>`
  - `verify --patches patches.jsonl --check-cmd 'grep -q empty sample.clj'`
  - `apply --patches patches.jsonl --check-cmd 'grep -q empty sample.clj' --no-backup`
  - assert file contains `(empty? xs)`

Results:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass, `Finished dev profile`.
- `cargo test --workspace`: pass, 9 unit tests total plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- CLI smoke: pass, one work order emitted, verify passed, apply wrote patched file.

Invalidated assumptions:
- None new. Existing safety negative memory remains active: parse/balance proves syntax only, not behavior. The M2 gate is necessary but does not make semantic-risk patches safe without an appropriate `--check-cmd`.

Current recommendation/checkpoint:
- M2 protocol + verify/apply loop is implemented and verified within the requested scope.
- Next pass should replace the balance-only parse check with real tree-sitter and add clj-kondo/token-duplication/JET as requested deferred work.

Deferred exactly:
- clj-kondo integration.
- token-duplication detection.
- JET/StaticLint integration.
- real tree-sitter parsing and CST-level region extraction.

Blockers:
- None for the scoped M2 deliverable.

Dependencies/restart requirements:
- Rust 1.94 toolchain used.
- No server or live process restart required.

Signature: Codex
## Session Report — Mutation Probe Tier

Date/time: 2026-06-23T15:05:43+02:00 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md` goal item #2: add an opt-in MutationProbe tier to the removability prover using cargo-mutants for Rust, mirroring the existing CoverageProvider pattern. Keep it trait/provider driven, degrade gracefully when cargo-mutants is absent, and do not touch `deslop/*.py`.

Target:
- `deslop-verify`: MutationProbe trait, Rust cargo-mutants implementation, recorded outcomes fixture test, absent-tool degrade test, verdict integration.
- `deslop-cli`: `deslop verify --mutation` and `deslop apply --mutation`.
- `deslop-mcp`: parity boolean `mutation` for verify/apply tools.

STEP 0:
- `cargo build --workspace` passed.
- `cargo test --workspace` passed before edits: 45 existing unit tests plus doc-tests.

Changes:
- Added `MutationConfig`, `MutationStatus`, `MutationAssessment`, `MutationRequest`, and `MutationProbe` in `crates/deslop-verify/src/lib.rs`.
- Added `MutationRegistry`, parallel to `CoverageRegistry`, with a Rust `RustCargoMutantsProbe` provider.
- Rust provider supports Rust sources through its provider-local `supports` method; no central language dispatcher or central `match Lang` was introduced.
- Live mode checks `cargo mutants --version`, runs `cargo mutants --json --output <tempdir>` only when mutation is enabled, and reads `<tempdir>/outcomes.json`.
- Recorded-fixture mode `MutationConfig::OutcomesFile` parses cargo-mutants-style JSON defensively for tests and future format drift.
- Surviving/missed mutants feed the passing verdict:
  - empty replacement + surviving mutant -> `dead-candidate`
  - non-empty replacement + surviving mutant -> `untested-risky`
  - no surviving mutant or absent tool -> coverage-derived verdict remains unchanged
- Added CLI `--mutation` to `verify` and `apply`.
- Added MCP `mutation` boolean to verify/apply schemas and option construction.

Tests added:
- `cargo_mutants_fixture_survivor_feeds_dead_signal`: recorded outcomes fixture with one `Missed` mutant and one `Caught` mutant; asserts the missed region becomes `dead-candidate` and the caught region is not downgraded.
- `absent_cargo_mutants_degrades_without_rejecting_patch`: fake missing cargo command returns a mutation notice and leaves the patch passing with the coverage-derived verdict.

Local tool state:
- `cargo mutants --version` failed with `error: no such command: mutants`; this is the expected local graceful-degrade condition.

Verification after edits:
- `cargo test -p deslop-verify --lib` passed: 9 verifier tests.
- `cargo run -p deslop-cli -- verify --help` shows `--mutation`.
- `cargo run -p deslop-cli -- apply --help` shows `--mutation`.
- `cargo fmt --all --check` passed.
- `cargo build --workspace` passed.
- `cargo test --workspace` passed: 47 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings` passed.

Deferred:
- Non-Rust mutation providers: Clojure and Julia mutation tools are future work and should be added as providers, not central language branches.
- Mutation-probe targeting by exact function selector or cargo-mutants file filter is future optimization; current opt-in live mode consumes cargo-mutants outcomes and maps missed mutants back to workorder regions.

Invalidated assumptions:
- None. The local absence of cargo-mutants was expected and verified through the degrade test and `cargo mutants --version`.

Current recommendation:
- Keep mutation disabled by default because cargo-mutants is expensive. Use it as a high-signal optional tier after parse, defensive-code, check-cmd, and coverage evidence.

Signature: Codex
## Session Report — Characterization-Test Generation

Date/time: 2026-06-23T15:21:48+02:00 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md` goal item #3: manufacture a stronger oracle for weak removability proofs by emitting characterization-test work orders, accepting externally generated tests only when they pass the current unmodified code, and using accepted tests to gate later removal/simplification patches.

Target:
- Protocol-level/language-agnostic flow.
- The LLM or human writes the test externally; deslop only emits the request, verifies submitted tests, and gates patches with accepted tests.
- Keep all prior coverage and mutation fixes.
- Do not touch `deslop/*.py`.

STEP 0:
- `cargo build --workspace` passed.
- `cargo test --workspace` passed before edits: 47 unit tests plus doc-tests.

Changes:
- Extended `deslop-protocol`:
  - `WorkOrder.kind` with `rewrite-region` and `needs-characterization-test`.
  - `CharacterizationTest` schema `deslop.characterization-test/1` with `workorder_id`, `region_fingerprint`, `test_path`, `test_text`, and `by`.
  - `characterization_work_order_for` emits a work order that instructs an external agent to write a test pinning current observable behavior.
- Extended `deslop-verify`:
  - JSONL loading/parsing for characterization tests.
  - `characterization_work_orders_for_patches`: verifies patches and emits characterization work orders for passing weak-oracle verdicts: `coverage-unknown`, `untested-risky`, and `dead-candidate`.
  - `verify_characterization_tests`: accepts submitted tests only if their fingerprint is current and `--check-cmd` passes after writing the test into a temp copy of the current unmodified project.
  - `VerifyOptions.characterization_tests`: normal `verify`/`apply` can receive accepted tests. For matching regions, deslop first re-validates the test on current code, then writes both the candidate patch and the test into a temp project and runs the same `--check-cmd`. If it passes, the characterization oracle upgrades the patch verdict to `removable`; if it fails current or patched code, the patch is rejected.
  - Characterization test paths must be relative and cannot escape the temp project with `..`.
- Extended `deslop-cli`:
  - `deslop characterize --patches FILE [-o workorders.jsonl] [--check-cmd CMD] [--coverage] [--mutation]`.
  - `deslop verify-characterization --tests FILE --check-cmd CMD`.
  - `deslop verify/apply --characterization-tests FILE`.
- Extended `deslop-mcp`:
  - Tools `characterize` and `verify_characterization`.
  - `characterization_tests` input support on verify/apply.
- Updated `SPEC.md` with the weak-oracle characterization flow, the new protocol artifact, and the CLI/MCP schema surface.

Tests added:
- Weak verdict emits a `needs-characterization-test` work order.
- Submitted characterization test that passes current code is accepted.
- Submitted characterization test that fails current code is rejected.
- Accepted characterization test gates patch verification and can upgrade a passing characterized patch to `removable`.

Verification after edits:
- `cargo test -p deslop-verify --lib` passed: 13 verifier tests.
- `cargo check --workspace` passed.
- `cargo fmt --all --check` passed.
- `cargo build --workspace` passed.
- `cargo test --workspace` passed: 51 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings` passed.
- CLI help smoke passed:
  - `cargo run -p deslop-cli -- characterize --help`
  - `cargo run -p deslop-cli -- verify-characterization --help`
  - `cargo run -p deslop-cli -- verify --help`
  - `cargo run -p deslop-cli -- apply --help`

Deferred:
- Persisting accepted characterization tests in a project-local registry. Current flow is explicit: pass accepted test JSONL with `--characterization-tests`.
- Language-specific test scaffolding templates. Generation remains external by design.
- Richer MCP tests for characterize/verify_characterization beyond tools-list schema coverage.

Invalidated assumptions:
- None. The generated-test contract is deterministic as long as callers provide a meaningful `--check-cmd`; without `--check-cmd`, characterization verification rejects rather than guessing.

Current recommendation:
- Use `characterize` after weak verifier verdicts and before allowing deletion on uncovered regions. Treat accepted characterization tests as project artifacts owned by the caller or agent harness until a future registry is added.

Signature: Codex
## Session Report — Non-Rust Coverage Providers

Date/time: 2026-06-23T15:32:31+02:00 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md` goal item #4: add non-Rust `CoverageProvider` implementations for Clojure, Julia, and Python to the existing coverage gate, registry-driven, with recorded fixture mapping tests and graceful local degrade. Keep all prior verifier work and do not touch `deslop/*.py`.

Target:
- Extend `deslop-verify` coverage registry beyond Rust `cargo-llvm-cov`.
- Providers:
  - Clojure: cloverage JSON/EDN-style line coverage.
  - Julia: Coverage.jl `.cov` and LCOV.
  - Python: coverage.py JSON and simple Cobertura XML.
- No central `match Lang`; provider selection remains `CoverageRegistry` + provider-local `supports`.

STEP 0:
- `cargo build --workspace` passed.
- `cargo test --workspace` passed before edits: 51 unit tests plus doc-tests.

Changes:
- Added fixture-oriented `CoverageConfig` variants:
  - `CloverageFile(PathBuf)`
  - `JuliaCovFile(PathBuf)`
  - `CoveragePyFile(PathBuf)`
- Registered four coverage providers in `CoverageRegistry`:
  - `RustCargoLlvmCovProvider`
  - `ClojureCloverageProvider`
  - `JuliaCoverageProvider`
  - `PythonCoveragePyProvider`
- Added provider-local live degrade/load behavior:
  - Clojure default live command checks/runs `lein cloverage --json --output <tempdir>` and looks for `coverage.json`.
  - Julia default live command checks `julia --startup-file=no -e 'using Coverage'` and can emit LCOV via Coverage.jl when installed.
  - Python default live command checks `coverage --version` and runs `coverage json -o <tempdir>/coverage.json`.
  - Missing tool/report/package returns `coverage-unknown` with one-line reason; verification does not error.
- Added shared `LineCoverage` map and parsers:
  - LCOV reuse for Julia live LCOV.
  - cloverage-style JSON plus simple line-oriented EDN maps.
  - Julia `.cov` counts.
  - coverage.py JSON (`executed_lines` / `missing_lines`) plus simple XML line hits.
- Updated `SPEC.md` to list the registry-driven providers and graceful-degrade behavior.

Tests added:
- `cloverage_fixture_maps_covered_and_uncovered_regions`.
- `absent_cloverage_degrades_to_unknown`.
- `coverage_jl_cov_fixture_maps_covered_and_uncovered_regions`.
- `absent_coverage_jl_degrades_to_unknown`.
- `coverage_py_json_fixture_maps_covered_and_uncovered_regions`.
- `absent_coverage_py_degrades_to_unknown`.
- Existing Rust LCOV fixture test still passes.

Local tool state:
- `lein cloverage --help` failed: `lein` not found.
- `julia --startup-file=no -e 'using Coverage'` failed: Julia exists, Coverage package not installed.
- `coverage --version` failed: coverage.py command not found.

No-central-match check:
- `rg -n "match .*Lang|Lang::" crates/deslop-verify/src/lib.rs` shows only provider-local `supports` checks and Python test fixture construction, not a central dispatcher.

Verification after edits:
- `cargo test -p deslop-verify --lib` passed: 19 verifier tests.
- `cargo fmt --all --check` passed.
- `cargo build --workspace` passed.
- `cargo test --workspace` passed: 57 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings` passed.

Deferred:
- CLI flags for selecting fixture files per non-Rust provider; fixture modes are currently internal/test APIs, while live CLI remains `--coverage`.
- Python source analysis/detection. The Python provider supports `Lang::Python`, but this pass did not add a Python analyzer/LangPack.
- Deeper XML/EDN schema support beyond the simple deterministic forms parsed here.

Invalidated assumptions:
- The user note said none were installed locally. Actual local state is: lein missing, coverage.py missing, Julia installed but Coverage.jl package missing. The graceful-degrade path still matches the intended result.

Current recommendation:
- Keep non-Rust coverage opt-in under `--coverage`; use recorded reports for deterministic tests and allow future per-language project config to choose live commands/report files.

Signature: Codex
## Session Report — SARIF 2.1.0 Output

Date/time: 2026-06-23T15:38:08+02:00 Europe/Madrid

Objective: Execute `.agents/NEXT_TASK.md` goal item #5, final roadmap item: add SARIF 2.1.0 output as `scan --format sarif`, with findings mapped to SARIF results for code-scanning integrations.

Target:
- Add `sarif` beside existing scan formats `text`, `json`, and `agent`.
- Render in `deslop-report`.
- Map findings to SARIF results with `ruleId`, `level`, `message.text`, and `locations[].physicalLocation`.
- Include `runs[].tool.driver` with name/version/rules.
- Do not touch `deslop/*.py`.

STEP 0:
- `cargo build --workspace` passed.
- `cargo test --workspace` passed before edits: 57 unit tests plus doc-tests.

Changes:
- Added `render_sarif` in `crates/deslop-report/src/lib.rs`.
- SARIF document fields:
  - `$schema`: `https://json.schemastore.org/sarif-2.1.0.json`
  - `version`: `2.1.0`
  - `runs[0].tool.driver.name`: `deslop`
  - `runs[0].tool.driver.version`: crate package version
  - `runs[0].tool.driver.rules[]`: one per rule id, with `shortDescription.text` and `properties.safety`
- Finding-to-result mapping:
  - `ruleId` = finding rule
  - `level`: Major -> `error`, Minor -> `warning`, Info -> `note`
  - `message.text` = finding message
  - `locations[0].physicalLocation.artifactLocation.uri` = finding path
  - `locations[0].physicalLocation.region.startLine/endLine` = finding span lines
- Added `Sarif` to CLI `Format` enum and `scan` dispatch.
- Updated `SPEC.md` milestone wording to treat SARIF 2.1.0 as implemented, leaving optional `slim` and LSP as the only explicitly optional roadmap items.

Tests added:
- `sarif_render_has_required_shape_and_locations` validates:
  - valid JSON
  - `version == "2.1.0"`
  - `$schema` present
  - `runs[0].tool.driver.name == "deslop"`
  - `results` count matches input findings
  - Major/Minor/Info map to error/warning/note
  - physical location URI and startLine are present and correct
  - rule properties include safety class

Verification after edits:
- Focused: `cargo test -p deslop-report --lib` passed.
- CLI smoke: `cargo run -p deslop-cli -- scan tests/corpus/sloppy/comments_and_blanks.clj --format sarif > /tmp/deslop-sarif-final.json && jq -e '.version == "2.1.0" and .runs[0].tool.driver.name == "deslop" and (.runs[0].results | length) == 3 and .runs[0].results[0].locations[0].physicalLocation.artifactLocation.uri' /tmp/deslop-sarif-final.json` passed.
- `cargo fmt --all --check` passed after formatting.
- `cargo build --workspace` passed.
- `cargo test --workspace` passed: 58 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings` passed.

Deferred:
- Optional `deslop-slim` consumer.
- LSP.

Cleared:
- SARIF is no longer deferred.

Invalidated assumptions:
- None.

Current recommendation:
- Treat SARIF as the CI/code-scanning output path; use agent JSONL/MCP for rewrite loops.

Signature: Codex

---

# Session Report — Finish Verification and Residual Hotspots

Date/time: 2026-06-23T17:09:58+02:00 Europe/Madrid

Objective: Continue from the latest cleanup checkpoint, run remaining verification, and
report residual hotspots/blockers.

Working-copy context:
- Existing cleanup changes remain in `crates/deslop-cli/src/main.rs` and
  `crates/deslop-verify/src/lib.rs`.
- `.agents/HEARTBEAT.md` is present as an added file in the working copy but was not created
  or edited by this finish pass.

Verification run:
- `cargo fmt --all --check`: pass.
- `cargo build --workspace`: pass.
- `cargo test --workspace`: pass, 58 unit tests plus doc-tests.
- `cargo clippy --workspace -- -D warnings`: pass.
- `cargo run -p deslop-cli -- eval tests/corpus --format json`: pass.
  - overall precision=0.9508196721311475
  - overall recall=0.9666666666666667
  - overall F1=0.9586776859504132
  - known local fallback notice: `clj-kondo not on PATH; falling back to built-in T1 Clojure rules`
- SARIF smoke:
  - `cargo run -p deslop-cli -- scan tests/corpus/sloppy/comments_and_blanks.clj --format sarif | jq -e '.version == "2.1.0" and .runs[0].tool.driver.name == "deslop" and (.runs[0].results | length) == 3'`: pass.

Current self-scan/slop:
- `target/debug/deslop slop crates`:
  - score: 10.9/100
  - comment-block=1
  - duplicate-block=15
  - long-method=17
  - magic-number=14
  - near-duplicate=37
  - needless-clone=11
- Highest slop files:
  - `crates/deslop-analyzer/src/julia.rs`: 42.4
  - `crates/deslop-analyzer/src/clojure.rs`: 27.5
  - `crates/deslop-eval/src/lib.rs`: 19.0
  - `crates/deslop-analyzer/src/tokens.rs`: 18.7
  - `crates/deslop-analyzer/src/packs/rust.rs`: 14.6

Metrics health:
- `target/debug/deslop metrics crates`:
  - repo health: 42.5/100
  - regions: 517
  - hotspots: 75
- Top metric hotspots:
  - `crates/deslop-lang/src/lib.rs:358`
  - `crates/deslop-verify/src/lib.rs:1317`
  - `crates/deslop-analyzer/src/tests.rs:261` (comment-ratio hotspot)
  - `crates/deslop-verify/src/lib.rs:1420`
  - `crates/deslop-verify/src/lib.rs:1192`
  - `crates/deslop-verify/src/lib.rs:1523`
  - `crates/deslop-verify/src/lib.rs:842`
  - `crates/deslop-lang/src/lib.rs:270`
  - `crates/deslop-analyzer/src/tokens.rs:41`
  - `crates/deslop-eval/src/lib.rs:197`

Residual hotspots:
- Long methods remaining:
  - Analyzer rule/dispatch bodies: Clojure, Julia, Rust pack, token duplication/tokenization,
    and `scan_paths_with_config`.
  - Eval/reporting routines: `run_eval_with_manifest`, `score_case`.
  - Runtime/tooling routines: `deslop-fix`, `deslop-mcp`, `deslop-metrics`.
  - Verify test scenario bodies: `cargo_mutants_fixture_survivor_feeds_dead_signal` and
    `apply_writes_only_removable_patches_by_default`.
- Duplicate/near-duplicate clusters remaining:
  - Analyzer rule-table/test repetition.
  - Token window/mask symmetry in `deslop-analyzer/src/tokens.rs`.
  - CLI enum/config parsing shape repetition.
  - Verify JSON traversal/fixture/test setup residuals.

Blockers:
- No verification blockers.
- External optional analyzer/tool availability remains limited locally:
  - `clj-kondo` is not on PATH for eval.
  - Earlier coverage checkpoint also found `lein` missing, coverage.py missing, and Julia
    installed without Coverage.jl.
- Remaining cleanup would require either analyzer-surface refactoring or a focused test-fixture
  helper pass; neither is required for a green finish state.

Signature: Codex

## 2026-06-26 — Per-rule / per-path finding suppression

Objective: Resolve the complaint that deslop's `deslop.toml` had no per-rule suppression —
keys like `ignore_comments`, `http_status_allowlist`, `[rules.x] ignore_paths` were silently
ignored, leaving only the blunt global token thresholds (`min_meaningful_tokens` /
`min_duplication_tokens`) as a "sledgehammer, not a scalpel."

Target: `deslop-analyzer`, `deslop-cli`, `deslop-mcp`, docs.

Changes:
- `deslop-analyzer`: added `Suppression` (Arc-backed, `Clone`, no-op when empty) +
  `SuppressionBuilder`, a canonical `KNOWN_RULES` list + `is_known_rule`, and a new
  `AnalyzerConfig.suppression` field. Findings are filtered *after* production at the three
  scan chokepoints (`scan_file_with_pack`, `scan_source_with_pack`, agnostic branch of
  `scan_source_with_config`), so it applies uniformly to every pack and to external-analyzer
  findings. Globs use `globset` (promoted from transitive `ignore` dep to a direct workspace
  dep — zero new crates in the build graph). Path matching strips a leading `./`.
- `deslop-cli`: `[analyzer]` now supports `disabled_rules`, `ignore_paths`, and
  `[analyzer.rules.<rule>]` (`enabled`, `ignore_paths`). Added `#[serde(deny_unknown_fields)]`
  to the analyzer config sections so fabricated keys (e.g. `ignore_comments`) are hard parse
  errors. `analyzer_config[_from_config]` now returns `Result` and builds/validates suppression.
- `deslop-mcp`: inline `analyzer` object + config-file path now accept the same suppression keys
  (merged across both sources); schema (`analyzer_schema`) advertises them; unknown keys rejected.
- Docs: `docs/CONFIG.md`, `deslop.toml.example`, `README.md` document suppression.

Behavior change (intentional): unknown rule names in suppression and unknown `[analyzer]` keys
are now errors instead of silent no-ops. This is the core fix — the prior silent-ignore is
exactly what made the earlier preview "fabricated."

Commands run:
- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace -- -D warnings` — clean.
- `cargo build -p deslop-slim --no-default-features` — ok (default MCP/slim stay network-free).
- `cargo test --workspace` — all green (new: 6 analyzer, 3 CLI, 2 MCP suppression tests).
- `cargo test -p deslop-mcp --features slim-llm` — 16 passed.
- Real-binary smoke test confirmed: default fires 4 `consecutive-blank-lines`; `disabled_rules`
  drops to 0; `ignore_paths`/per-rule globs scope by path; unknown rule name and unknown
  `[analyzer]` key both exit non-zero with a clear error listing valid values.

Invalidated assumption: the earlier session's belief that `ignore_comments` /
`http_status_allowlist` / `[rules.x] ignore_paths` were supported — they never were. They are
now either implemented (`ignore_paths`, per-rule tables) under `[analyzer]`/`[analyzer.rules]`
or rejected loudly (`ignore_comments`, `http_status_allowlist`).

Current recommendation / next actions:
- `KNOWN_RULES` is hand-maintained in `deslop-analyzer`; the `RULES` const in `deslop-cli` and
  the `deslop rules` output remain a separate (slightly incomplete) list. A follow-up could make
  both derive from one source so they cannot drift.
- `deslop-slim` auto-mode does not yet thread suppression through its own scan path (it receives
  reports); MCP prompt-mode and CLI scan/propose are covered. Parity for slim auto is deferred
  (tracked alongside PLAN Phase 5).

Blockers: none.

Signature: Claude (Opus 4.8), per-rule/per-path suppression implemented + validated end-to-end, 2026-06-26.

## 2026-06-26 — Unify rule registry + slim auto-mode suppression parity

Objective: Follow-ups to the suppression work — (1) make the rule list a single source of
truth so suppression validation, `deslop rules`, and the MCP `rules` tool cannot drift; (2)
wire suppression through `deslop-slim` auto-mode so disabled rules / ignored paths never reach
the rewrite pipeline.

Target: `deslop-core`, `deslop-analyzer`, `deslop-cli`, `deslop-mcp`, `deslop-slim`.

Changes:
- `deslop-core`: new `pub mod rules` — canonical `RuleInfo` + `RULES` registry (30 rules,
  the union of internal + external-analyzer rules), `is_known`, `names_csv`, `render_table`.
  Three unit tests (no dup names, is_known matches registry, table lists every rule).
- `deslop-analyzer`: deleted the hand-maintained `KNOWN_RULES` const; `is_known_rule` and the
  suppression error message now delegate to `deslop_core::rules`.
- `deslop-cli` + `deslop-mcp`: deleted both duplicated `const RULES` text blocks. `deslop rules`
  and the MCP `rules` tool now render from `deslop_core::rules::render_table()` (dynamic column
  widths). The previously-missing rules (near-duplicate, needless-clone, redundant-closure,
  let-and-return, useless-format, needless-return, unused-private-def, unused-namespace,
  missing-reference) now appear and are suppressable.
- `deslop-slim`: `SlimOptions` gained an `analyzer: AnalyzerConfig` field; added
  `propose_work_orders_with_config` and routed `load_or_propose_work_orders` through it
  (`propose_work_orders` kept as a default-config wrapper). CLI `fix` now passes
  `analyzer_config(config, ..)?`; MCP `fix mode=auto` passes `mcp_analyzer_config(args)?`. This
  closes PLAN Phase 5 (slim auto config parity) — auto mode honors thresholds AND suppression.

Commands run (all green):
- `cargo fmt --all --check`; `cargo clippy --workspace -- -D warnings`.
- `cargo build -p deslop-slim --no-default-features`.
- `cargo test --workspace` (new: 3 core rules tests, 1 slim auto-suppression test).
- `cargo test -p deslop-mcp --features slim-llm` — 16 passed.
- `deslop rules` renders all 30 rules in one aligned table from the shared registry.

Invalidated assumption: prior report's note that `KNOWN_RULES` was a separate hand-maintained
list and that slim auto-mode didn't thread suppression — both resolved here.

Current recommendation / next actions:
- Registry is still manually kept in `deslop-core`; a compile-time check that every emitted
  rule literal appears in `RULES` would fully prevent drift but needs a rule-name macro/registry
  at emission sites (larger change, deferred).
- No blockers.

Signature: Claude (Opus 4.8), unified rule registry in deslop-core + slim auto-mode suppression parity, 2026-06-26.

## 2026-06-26 — magic-number + incompleteness precision (AST masking)

Objective: eliminate structural false positives in the `magic-number` and `incompleteness` rules surfaced while dogfooding deslop on the smart-genie Clojure codebase.

Root causes (crates/deslop-analyzer/src/agnostic.rs): both rules were line/text heuristics. `magic-number` did not exempt literals inside strings/comments or inside named-constant definitions, so (a) numbers in docstrings ("Return 5-20 entities; 16 types") and (b) the VALUE line of a multi-line `(def x\n  64)` were flagged — making the rule's own remedy ("introduce a named constant") un-actionable. `incompleteness` masked strings/comments already, but its `placeholder` alternative matched any identifier containing the substring (e.g. the fn name `placeholders`, bindings `fp-placeholders`).

Changes:
- deslop-lang: new `LangPack::is_constant_definition_region` (default false); overrides — Clojure `def`/`defonce` list_lit (via node_head_token), Rust `const_item`/`static_item`, Julia `const`.
- deslop-analyzer: `magic_numbers` now masks byte ranges from `string_comment_ranges` + new `constant_definition_ranges` and checks the literal's absolute byte before flagging; `first_magic_number` returns the byte offset. `incompleteness` regex `placeholder` -> `\bplaceholder\b`.
- 6 new tests (inline literal still flagged; multi-line const, docstring numbers, Rust multi-line const not flagged; identifier-with-placeholder not flagged; standalone placeholder still flagged).

Verification:
- `cargo test -p deslop-analyzer`: 32 passed. `cargo test --workspace`: all crates ok, 0 failed. `cargo fmt --all` applied.
- Dogfood on /srv/biotz/smart-genie/src (debug build vs installed 0.1.0): magic-number 83 -> 64; incompleteness 11 -> 0 (all were false positives); total 228 -> 200.

Notes: changes left UNCOMMITTED alongside pre-existing WIP (long_methods config plumbing in agnostic.rs; .agents/PLAN.md MCP-UX work). Did not bundle into a jj commit to avoid mixing unrelated work — owner to organize/commit.

Signature: Claude (Opus 4.8), magic-number/incompleteness AST masking (FP 83->64, 11->0 on smart-genie), 2026-06-26.

## 2026-06-28 — Readability pass: dedup suppression collection (option A)

Objective: Apply the readability-over-terseness reflection — remove the near-duplicate
suppression-collection loop and a couple of clever-terse expressions, without changing the
config schema or the `deny_unknown_fields` guarantee.

Target: `deslop-analyzer`, `deslop-cli`, `deslop-mcp`, `deslop-core`.

Changes:
- `deslop-analyzer`: new `RuleSuppression<'a>` borrowed view + `SuppressionBuilder::add_section`
  — the single place that defines what each suppression key means (disabled_rules / explicit
  `enabled = false` disable a rule; ignore_paths skip paths). `enabled == Some(false)` replaced
  by `matches!(.., Some(false))`.
- `deslop-cli::build_suppression` and `deslop-mcp::collect_mcp_suppression`: the ~14-line loop
  that was duplicated across both crates now adapts its `Option` fields into `add_section`'s
  borrowed inputs (`as_deref().unwrap_or_default()`); the meaningful logic lives once. Ironic
  near-duplicate in a slop detector removed.
- `deslop-core::rules::render_table`: replaced the `.chain(std::iter::once(header.len()))` width
  trick with a small `longest(cell, header)` closure using `.max(header.len())` — reads as
  "longest cell, but never narrower than the header."

Behavior: unchanged. `deslop rules` renders the identical 30-rule aligned table; suppression
semantics identical.

Commands run (all green): `cargo fmt --all --check`; `cargo clippy --workspace -- -D warnings`;
`cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm` (16 passed). Existing
suppression/rules/slim tests still pass with no test changes needed.

Blockers: none. Deferred (offered, not taken): grouping keys under a `[analyzer.suppression]`
table (option B) — a schema change, only worth it if explicit grouping is wanted later.

Signature: Claude (Opus 4.8), suppression-collection dedup via shared add_section + render_table clarity, 2026-06-28.

## 2026-07-02 — Full-diff review of working tree + improvement plan

Objective: review the uncommitted changeset (~4,350 insertions / ~1,866 deletions, 29 files:
suppression system, per-language long_method_nloc, rule registry, extraction refactors) and
produce a prioritized improvement plan. Review only — no source changes this session.

Commands run: git diff (full, per-crate), cargo check (clean), cargo test (all green),
grep verification of each finding, dogfood scan of crates/deslop-slim/src.

Findings (verified, not speculative):
- `crates/deslop-mcp/src/spec.rs` is dead code: no `mod spec;` in lib.rs, which keeps its own
  `tool_definitions()`. It is the unfinished Phase 0 (tool annotations) of the MCP plan.
  Existing description assertions hold against the spec.rs copies, so wiring is low-risk.
- CLI and MCP still define field-identical serde config structs (the 2026-06-28 pass dedup'd
  collection logic via `add_section`, not the struct definitions or threshold plumbing).
- Extraction sweep hypothesis: no repo-root deslop.toml exists, so dogfooding runs at default
  long_method_nloc = 40, pressuring degenerate extraction (single-use wrappers, named match
  arms). Evidence: self-scan of refactored deslop-slim reports NEW near-duplicate findings
  (lines 920-921 vs 331) — the refactor traded one slop class for another.
- `code_lines` in clojure.rs allocates Vec<String> per rule call (3x per file) where the prior
  inline pattern was zero-alloc.
- Suppression `match_path` only strips `./`; relative globs silently never match findings from
  absolute scan paths (documented, but surprising).
- `FixRequest`/`run_fix_request` chain in deslop-cli has no consumer outside main.rs.

Explicitly fine: suppression design, rule registry, deny_unknown_fields, AST masking,
`cached_coverage_assessment` dedup (removes real 4x duplication), test-helper extraction.

Artifact: prioritized plan appended to `.agents/PLAN.md` ("Review-Driven Improvement Plan
(2026-07-02)"): P1 wire spec.rs, P2 unify config structs in deslop-analyzer, P3 self-configure
via repo deslop.toml then re-calibrate worst extractions, P4 code_lines iterator, P5 glob/
absolute-path candidate. Validation: per-crate test first, then the Final Gate; P3 gates on
self-scan showing no new duplicate findings.

Blockers: none. Next action: implement P1.

Signature: Claude (Fable 5), review iterated into P1-P5 plan in .agents/PLAN.md, 2026-07-02.

## 2026-07-02 — Handoff: product backlog Tiers 1-3 dispatched to Codex

Objective: execute the "Product Backlog — Tool Improvements (2026-07-02)" section of
.agents/PLAN.md. Dispatched to Codex CLI in tmux pane 0:11 (repo cwd, YOLO permissions;
model auto-switched to gpt-5.3-codex-spark xhigh due to usage limits). Instruction: all
tiers in plan sequencing (1, 2, 5, 4, 3, 6, 7-10), build on uncommitted WIP without
reverting it, leave P1-P5 cleanup items alone (owned separately), respect PLAN.md
negative-memory constraints, per-item Final Gate, report+jj describe at checkpoints.

Risk: Codex reported <25% of its 5h usage limit remaining — the run may stall mid-tier;
check pane 0:11 and this report for its checkpoint entries.

Signature: Claude (Fable 5), backlog Tiers 1-3 handed off to Codex pane 0:11, 2026-07-02.
## 2026-07-02T00:00:00+02:00 — Product Backlog Tier Execution Started

Objective: Execute `Product Backlog — Tool Improvements (2026-07-02)` items 1–10 in mandated order.

Status at start:
- Confirmed baseline status and pre-existing WIP changes are present; no files were reverted.
- Identified concrete gaps for tiers 1–10.
- No code changes yet in this session.

Plan for this session:
1) Tier 1–2: verify/fix LSP config path load + inline suppression parser/tests.
2) Tier 5,4,3,6,7: `deslop scan --changed` + baseline update; analyzer cross-file duplication; Python idiom pack + JS/TS registry support.
3) Tier 8–10: fix `deslop fix --diff`, `deslop feedback`, and CI/SARIF recipe/docs.

Constraints preserved:
- Do not alter network-free MCP default.
- Keep existing WIP intact.
- Append feedback for each checkpoint.

Signature: Codex (GPT-5.5), started Product Backlog execution 2026-07-02, 00:00:00.
## 2026-07-02T00:00:00+02:00 — Backlog Tiers 1-3 Execution Start
Objective: Begin executing  product backlog items 1-3 in required sequence while preserving working-tree changes.
Scope: tiers 1,2,4,3+6,7,8,9,10 with no work on the Review-Driven Plan section. Preserve all existing WIP and do not revert unrelated edits.
Status: queued. First milestones will be LSP config/inline suppression validation, changed-scan + baseline command support, then duplicate-detection and rule-packs.
Validation: pending first-targeted tests after each section.
Signature: Codex (GPT-5.5), Backlog execution started with constraints honored, 2026-07-02.

## 2026-07-02T11:27:37+02:00 — Product Backlog Analyzer/Eval Checkpoint

Objective: Execute Product Backlog items 3, 4, 6, 7, and 9 foundations after confirming Tier 1 suppression/config surfaces were already present.

Changes:
- Registered Python, JavaScript, and TypeScript analyzer packs.
- Added Python idiom rules: py-none-comparison, py-range-len, py-dict-keys-membership, py-list-comprehension-wrapper.
- Added JavaScript/TypeScript idiom rules: js-loose-equality, js-var-declaration, js-unnecessary-await.
- Reworked analyzer path scanning to collect supported files, scan them in parallel with scoped standard-library workers, and append cross-file token-duplication findings.
- Added eval corpus positive/negative cases and baseline rows for every new rule.
- Added append_false_positive_feedback in deslop-eval and CLI plumbing started for deslop feedback.
- Added deterministic safe-auto diff support in deslop-fix and CLI plumbing started for deslop fix --diff.

Validation:
- cargo test -p deslop-analyzer: PASS (39 tests).
- cargo test -p deslop-eval: PASS (3 tests).

Negative-memory constraints:
- apply/default write behavior unchanged; new fix --diff path is read-only.
- MCP default build/network behavior untouched.
- Eval precision/recall measures new rules; metrics remain triage only.

Blockers: none at this checkpoint. Next actions: finish CLI compile/smoke tests, docs for changed scan/baseline/SARIF/feedback, then run final gate.

Signature: Codex (GPT-5.5), analyzer packs cross-file duplication parallel scan and feedback foundations, 2026-07-02.
## 2026-07-02T11:30:23+02:00 — Product Backlog CLI/Docs Checkpoint

Objective: Complete Product Backlog items 5, 8, 9, and 10 command/documentation surfaces while preserving existing WIP.

Changes:
- Added deslop baseline update as an explicit ratchet command that rewrites the baseline from the current accepted finding set.
- Added deslop fix --diff as a read-only deterministic safe-auto diff path.
- Added deslop feedback <fingerprint> --false-positive, resolving a live finding and appending a clean eval-corpus case.
- Added JavaScript/TypeScript analyzer config fields to CLI deslop.toml parsing.
- Updated docs/CI.md with changed-scan, baseline update, feedback, and direct SARIF/GitHub Actions recipe.
- Updated README.md, docs/CONFIG.md, and deslop.toml.example for JS/TS config, seeded Python/JS packs, fix --diff, feedback, and baseline ratchet.

Validation:
- cargo test -p deslop-cli: PASS (17 unit tests + scan_exit_codes integration test).
- Smoke: target/debug/deslop fix --diff --paths <temp sample.clj>: PASS, emitted diff and left file unchanged.
- Smoke: target/debug/deslop scan --changed in temp git repo: PASS, included changed sloppy.rs and excluded committed clean.rs.
- Smoke: target/debug/deslop baseline update in temp git repo: PASS, wrote deslop.baseline/1.
- Smoke: target/debug/deslop feedback <fingerprint> --false-positive with temp corpus: PASS, copied feedback case and updated manifest.

Negative-memory constraints:
- No apply behavior was widened; fix --diff is read-only.
- MCP network feature boundaries untouched.
- Feedback feeds eval precision/recall instead of suppressing uncertainty.

Blockers: none. Next actions: final gate commands from PLAN.md.

Signature: Codex (GPT-5.5), CLI ratchet diff feedback and SARIF docs complete, 2026-07-02.
## 2026-07-02T11:43:13+02:00 — Product Backlog Final Gate Complete

Objective: Finish all Product Backlog — Tool Improvements (2026-07-02) Tier 1-3 items in requested sequence and validate current disk state.

Completed backlog items:
- 1 LSP honors deslop.toml: existing WIP config load/reload was preserved; fixed LSP compile issues in config refresh/root handling and verified LSP tests in workspace gate.
- 2 Inline suppression comments: existing WIP parser/tests preserved; clippy cleanup changed manual prefix slicing to strip_prefix.
- 5 Git-aware scan + baseline ratchet: scan --changed was verified; baseline update implemented and smoke-tested.
- 4 Cross-file duplication: analyzer now collects supported files, scans in parallel, and appends cross-file duplicate/near-duplicate token findings.
- 3 Python idiom pack: seeded rules py-none-comparison, py-range-len, py-dict-keys-membership, py-list-comprehension-wrapper with eval corpus cases.
- 6 TypeScript/JavaScript pack: seeded rules js-loose-equality, js-var-declaration, js-unnecessary-await with eval corpus cases and JS/TS analyzer config support.
- 7 Parallel file scanning: implemented with scoped standard-library worker threads, keeping sorted output stable.
- 8 deslop fix --diff: implemented read-only deterministic safe-auto unified diff preview and smoke-tested unchanged source.
- 9 FP feedback into eval corpus: implemented deslop feedback <fingerprint> --false-positive and deslop-eval append API; smoke-tested with temp corpus.
- 10 SARIF/GitHub recipe: docs/CI.md now includes direct changed-scan baseline SARIF upload workflow.

Final gate commands on current tree:
- cargo fmt --all --check: PASS.
- cargo build --workspace: PASS.
- cargo build -p deslop-slim --no-default-features: PASS.
- cargo test --workspace: PASS.
- cargo test -p deslop-mcp --features slim-llm: PASS.
- cargo clippy --workspace -- -D warnings: PASS.

Additional targeted validation:
- cargo test -p deslop-analyzer: PASS.
- cargo test -p deslop-eval: PASS.
- cargo test -p deslop-cli: PASS.
- Smoke fix --diff, scan --changed, baseline update, and feedback false-positive: PASS.

Negative-memory constraints:
- apply remains verifier-Removable by default; no write gate widened.
- MCP default network-free boundary preserved; slim-llm remains feature-gated.
- New rules are tied to eval corpus precision/recall cases; feedback writes eval cases rather than hiding uncertainty.

Blockers: none.

Signature: Codex (GPT-5.5), Product Backlog Tiers 1-3 complete with final gate passing, 2026-07-02.
## 2026-07-05T10:38:04+02:00 — Julia eachindex Suggestion Guard

Objective: Fix invalid Julia `1:length` -> `eachindex` suggestions.

Target: Built-in Julia T1 rule `reimpl-eachindex`.

Changes:
- Replaced the broad line-only `for i in 1:length(x)` Julia rule with a conservative loop-body check.
- `reimpl-eachindex` now reports only when the loop variable is used only as `x[i]` for the same collection.
- Ordinal counter uses, other-collection indexing, and mixed ordinal/index uses no longer get an `eachindex` suggestion.
- Updated the user-facing rule catalog default text to `suggest (same collection indexing, not ordinal use)`.
- Added analyzer regressions for valid same-collection indexing and invalid ordinal/other-collection cases.

Commands run:
- `cargo fmt --all --check`: failed before formatting with rustfmt-only diff.
- `cargo fmt --all`: pass.
- `cargo fmt --all --check`: pass.
- `cargo test -p deslop-analyzer julia_eachindex -- --nocapture`: pass.
- `cargo test -p deslop-analyzer`: pass.
- CLI smoke with temp valid/ordinal Julia files and `jq`: pass after correcting the JSON query shape.
- `cargo test --workspace`: pass.
- `cargo clippy --workspace -- -D warnings`: pass.
- `cargo run -q -p deslop-cli -- rules | rg -n "reimpl-eachindex|same collection"`: pass.

Invalidated assumptions:
- A line-level `1:length(x)` rule is not precise enough to suggest `eachindex(x)` because the loop variable can be an ordinal counter.

Current recommendation:
- Keep this rule conservative. If broader Julia loop rewrites are desired, use CST body analysis or external analyzer evidence before emitting rewrite-like suggestions.

Blockers: none.

Signature: Codex (GPT-5), Julia eachindex suggestion guard, 2026-07-05.
## 2026-07-06T14:09:09+02:00 — Agent-Ready Refactor Graph

Objective: Add a generic dependency/refactor graph to deslop, suitable for LLM planning and
not tied to Python-specific tooling.

Target:
- Tree-sitter-backed, language-generic graph extraction through the existing Rust workspace.
- CLI and MCP surfaces that emit deterministic structured output for agents.

Changes:
- Added `deslop-graph`, producing `deslop.graph/1` with file/symbol/external-symbol nodes and
  `contains`, `imports`, `calls`, and `inherits` edges.
- Graph edges carry confidence: `resolved`, `external`, or `ambiguous`; only `resolved` means
  one local target was found.
- Added agent notes to the graph payload explaining how to use ownership and incoming edges for
  refactor impact planning, and that the graph is planning evidence, not verifier proof.
- Added CLI command `deslop graph [PATHS...] --format json|dot [--no-calls]`.
- Added MCP `graph` tool returning the same `deslop.graph/1` JSON for in-loop coding agents.
- Updated README and SPEC command/architecture/MCP docs.

Validation:
- `cargo check -p deslop-graph`: pass.
- `cargo test -p deslop-graph`: pass after final clippy fixes.
- `cargo check -p deslop-cli -p deslop-mcp`: pass.
- `cargo test -p deslop-mcp`: pass before the final clippy-only graph edits.
- `cargo test -p deslop-mcp graph_tool_returns_refactor_graph_json`: pass.
- `cargo test -p deslop-mcp tools_list_returns_expected_tool_set_with_schemas`: pass.
- CLI smoke: `deslop graph crates/deslop-graph/src/lib.rs --format json` emitted
  `deslop.graph/1`, 1 file, and 605 edges.
- CLI smoke: `deslop graph crates/deslop-graph/src/lib.rs --format dot` emitted
  `digraph deslop_graph`.
- `cargo test -p deslop-cli`: pass before adding the parser regression.
- `cargo test -p deslop-cli parses_graph_command`: pass.
- `cargo fmt --all --check`: pass.
- `cargo test --workspace`: pass before final clippy-only graph edits.
- `cargo build -p deslop-slim --no-default-features`: pass.
- `cargo clippy --workspace -- -D warnings`: initially failed on style issues in
  `deslop-graph`, then passed after fixes.

Invalidated assumptions:
- None. The chosen graph is intentionally syntactic/resolution-light; semantic certainty remains
  delegated to confidence labels and the existing verify/apply gate.

Current recommendation:
- Use `deslop graph --format json` as the refactor-planning input for agents, then use
  `scan`/`propose`/`verify`/`apply` for concrete cleanup. A future pass can add stronger
  language-specific import resolution or SCIP/LSIF ingestion without changing the schema.

Blockers: none.

Signature: Codex (GPT-5), generic LLM refactor graph, 2026-07-06.
## 2026-07-06T14:49:18+02:00 — Dogfood Deslop On Deslop Graph

Objective: Use the newly installed `deslop` against the deslop codebase itself and make a scoped
cleanup where the graph/scan evidence was useful.

Target:
- `crates/deslop-graph/src/lib.rs`, the newly added graph crate.

Dogfood inputs:
- `deslop graph crates/deslop-graph/src/lib.rs --format json`: emitted `deslop.graph/1` with
  1 file, 84 symbols, 235 external symbols, and 602 edges before cleanup.
- `deslop scan crates/deslop-graph/src/lib.rs --format json`: identified three long methods,
  one redundant closure, one magic-number, and several near-duplicate signals.
- `deslop metrics crates/deslop-graph/src/lib.rs --format json`: ranked `GraphBuilder`,
  `node_kind_label`, `rust_symbol_def`, and `js_symbol_def` as graph/complexity hotspots.

Changes:
- Split `GraphBuilder::add_symbol_node` into focused symbol-node insertion, index, and
  contains-edge helpers.
- Split `GraphBuilder::finish` by extracting graph summary and agent-note construction.
- Removed repeated import/call/inheritance edge-add code in `SourceExtractor`.
- Replaced a redundant-closure-like helper with an explicit loop.
- Named signature truncation constants instead of bare numeric literals.
- Extracted test helpers for repeated graph node/edge assertions.

Results:
- Long-method findings in `deslop-graph` dropped from 3 to 0.
- Redundant-closure and magic-number findings in `deslop-graph` dropped to 0.
- Remaining `deslop scan crates/deslop-graph/src/lib.rs` findings are near-duplicate only,
  concentrated in match/table code and symmetric tests; stopped there to avoid overfitting
  false-positive-prone repetition.

Validation:
- `cargo fmt --all --check`: pass.
- `cargo test -p deslop-graph`: pass.
- `cargo clippy -p deslop-graph -- -D warnings`: pass.
- `cargo run -q -p deslop-cli -- graph crates/deslop-graph/src/lib.rs --format json`: pass,
  emitted `deslop.graph/1`, 1 file, 621 edges after cleanup.
- `cargo install --path crates/deslop-cli --features mcp --force`: pass, replaced
  `/home/christos/.cargo/bin/deslop`.
- Installed smoke: `deslop graph crates/deslop-graph/src/lib.rs --format json`: pass,
  emitted `deslop.graph/1`, 1 file, 621 edges.

Invalidated assumptions:
- None. Remaining near-duplicate signals are triage-only and should not drive further churn
  without a more semantic extraction target.

Current recommendation:
- Keep this cleanup scoped. Future graph work should improve language-specific resolution
  rather than splitting match arms or tests solely to reduce near-duplicate counts.

Blockers: none.

Signature: Codex (GPT-5), dogfood graph cleanup and reinstall, 2026-07-06.
## 2026-07-06T15:33:58+02:00 — Graph-Guided Module Split

Objective: Use `deslop graph` to separate the new graph crate into modules by functional
ownership instead of leaving one large `lib.rs`.

Graph input:
- `deslop graph crates/deslop-graph/src/lib.rs --format json` showed clear functional clusters:
  public graph schema/types, graph builder/resolution, source/CST extraction, ID/module-key
  normalization, rendering, and tests.

Changes:
- Replaced monolithic `crates/deslop-graph/src/lib.rs` with a small facade.
- Added:
  - `types.rs`: public schema structs/enums plus crate-internal extraction structs.
  - `builder.rs`: graph assembly, symbol indexing, edge resolution, summary/agent notes.
  - `extract.rs`: tree-sitter traversal, symbol extraction, import/call/inheritance labels.
  - `ids.rs`: stable node IDs, module/import keys, labels, normalization.
  - `render.rs`: JSON/DOT rendering.
- Kept public API unchanged via `pub use` reexports from `lib.rs`.

Results:
- `deslop graph crates/deslop-graph/src --format json` now reports 6 files, 106 symbols,
  658 edges, and 244 resolved edges.
- `deslop scan crates/deslop-graph/src --format json` now has no findings for
  `builder.rs`, `ids.rs`, or `types.rs`; remaining findings are near-duplicate only in
  extractor match patterns, tests, and a render/id-label symmetry.

Validation:
- `cargo check -p deslop-graph`: pass.
- `cargo fmt --all --check`: pass.
- `cargo test -p deslop-graph`: pass.
- `cargo clippy -p deslop-graph -- -D warnings`: pass.
- `cargo check -p deslop-cli -p deslop-mcp`: pass.
- `cargo test -p deslop-mcp graph_tool_returns_refactor_graph_json`: pass.
- `cargo test -p deslop-cli parses_graph_command`: pass.
- `cargo install --path crates/deslop-cli --features mcp --force`: pass; replaced
  `/home/christos/.cargo/bin/deslop`.
- Installed smoke: `deslop graph crates/deslop-graph/src --format json`: pass, reports
  6 files / 106 symbols / 658 edges / 244 resolved edges.

Invalidated assumptions:
- None. The graph module split is a structural refactor with unchanged public schema/API.

Current recommendation:
- Keep `extract.rs` table/match repetition for now; remaining near-duplicate findings are
  low-confidence and reflect language-specific grammar differences.

Blockers: none.

Signature: Codex (GPT-5), graph-guided deslop-graph module split, 2026-07-06.

## 2026-07-10 — Config-boundary analyzer landed (owner-directed, no delegation)

**Objective:** catch "dishonest wiring" (configured-but-not-wired/hardcoded/shadowed config) as a
deterministic deslop pass — motivated by the RelationExtractor knob incidents (canvas_top_k echo-only;
relation_top_k k>3→3 literal clamp).

**Built (crates/deslop-analyzer/src/boundary.rs + wiring):** repo-wide post-pass (mirrors
add_cross_file_duplication) over the config key lifecycle: TOML/YAML/JSON key inventory → structural
parse-site detection (lookup-shaped calls with key-string args) → per-occurrence classification
(echo sink / store / live) over both key strings AND convention-named or parse-bound identifiers,
aggregated repo-wide on normalized keys (kebab/snake/camel fold). Rules: config-key-unread (Minor),
config-key-unconsumed (Major, anchored keys only: artifact-declared | --flag | ENV_SHAPED | dotted),
config-key-shadowed (Major, literal-only reassignment AFTER the parse, SAME function scope, outside
guards incl. &&/or short-circuits). DetectedBy::Boundary; SafetyClass NeverAuto; [analyzer.boundary]
config (deny_unknown_fields fail-loud); suppression integrated; docs (README + deslop.toml.example);
module-doc known-limitations (prefix-constructed keys, derive configs, container round-trips).

**Precision campaign (live shakedown on RelationExtractor, 6 rounds):** 188 → 67 (anchor requirement)
→ 16 (inline-consumption + store-walk fixes) → 5 (multi-key alias attribution — found via a false flag
on MY OWN DRIVER_ALLOW_FOREIGN_GPU_MIB nested-get) → 4 (short-circuit guards) → **2** (scope-aware
shadowing). Final 2 = the known prefix-constructed-key limitation, hedged by precondition text.
Ground-truth fixtures (pre-fix incident shapes) caught at every round; 8/8 boundary tests; workspace
171/171. Notable: the analyzer's own first test run caught ME declaring-but-not-wiring its
skip_artifacts knob — the exact pathology class, self-demonstrated.

**Verification run:** cargo test --workspace (171 passed / 0 failed); release binary scan of
RelationExtractor configs+scripts+src (14s).

**Residual risk / next:** unconsumed rule currently reports 0 on post-fix RelationExtractor (expected —
incidents are fixed; fixtures carry the pre-fix shapes). P2 candidates: per-language precision packs
(serde/clap derive keys), container round-trip crediting, prefix-construction detection.

**Signature:** Claude (Fable 5), config-boundary analyzer (3 rules) landed with 6-round precision campaign, 188→2 on the motivating repo, 2026-07-10.

## 2026-07-10T20:33:39+02:00 — Tree-sitter structural readability and refactor confidence

**Objective:** add deterministic readability detection to `deslop metrics` by combining
complexity and entropy, and expose confidence for functions, methods, classes/type containers,
and other language-pack metric regions.

**Production target and output contract:** `deslop metrics` text/JSON and the MCP `metrics` tool
now emit additive `deslop.metrics/1` fields. Each region has its tree-sitter kind, normalized CST
leaf-token entropy, CST node-kind entropy, information volume (`leaf_count * raw token entropy`),
a 0-100 structural-readability score, component burdens, `measurement_confidence`, `size_support`,
and `refactor_confidence`. The report declares model `deslop-structural-readability/1` with
`calibrated=false` and ranks regions at refactor confidence >= 0.50. Existing fix/apply safety is
unchanged; readability is triage-only.

**Changes:**
- `crates/deslop-metrics/src/lib.rs`: added CST token/node entropy, information volume, bounded
  complexity/information/entropy interaction model, separate measurement/refactor confidence,
  confidence-weighted repo readability, ranked absolute refactor candidates with factor reasons,
  nested-region retention, and text/JSON rendering.
- `crates/deslop-lang/src/lib.rs`: added Python function/class metric regions and corrected Python
  tree-sitter branch/nesting/flow node kinds.
- `crates/deslop-mcp/src/lib.rs` + `spec.rs`: documented and contract-tested the additive MCP
  readability fields.
- `README.md`, `SPEC.md`, `.agents/PLAN.md`: documented the model, size semantics, calibration
  boundary, region coverage, and safety boundary.
- `crates/deslop-analyzer/src/boundary.rs`: rustfmt plus six semantics-preserving clippy repairs
  required because the clean parent change did not pass the repository's `-D warnings` gate.

**Numerical/contract evidence:**
- Focused metric matrix: 9/9 tests pass. It verifies bounded scores, complexity-only vs
  entropy-only vs combined ordering, positive interaction, size increasing evidence support,
  large-simple code remaining less suspicious, JS/Python class+method coverage, and Clojure
  nested-call suppression.
- MCP readability contract test passes and checks class/method regions plus both confidence fields.
- CLI self-smoke on `crates/deslop-metrics/src/lib.rs`: 81 regions, repo health 40.3/100,
  structural readability 83.2/100, two absolute candidates: `input_files` confidence 0.5866
  (readability 48.88) and `ast_complexity` confidence 0.5412 (readability 55.95).
- Full gate: `cargo fmt --all --check`; workspace build; slim no-default-features build;
  `cargo test --workspace` (177 passed); `cargo test -p deslop-mcp --features slim-llm`
  (18 passed); analyzer regression tests after clippy repair (49 passed); workspace clippy
  with `-D warnings`. All final gates pass.

**Invalidated assumptions / negative memory:**
- Tree-sitter is sufficient for static feature extraction; human ratings are needed only to
  calibrate a human-agreement probability, not to compute a deterministic structural score.
- A repo-relative hotspot is not automatically an absolute high-confidence refactor candidate.
  The old small bloated fixture measured only 0.2368 refactor confidence despite being a correct
  statistical outlier. This is recorded in Hindsight negative memory; the absolute gate now uses
  a genuinely large 40-branch fixture and remains separate from repo-relative hotspots.
- Stopping traversal at an outer class/impl omitted methods. Traversing every declared region fixed
  containers/members, but Clojure's broad `list_lit` declaration required semantic filtering to
  avoid treating every nested call as a region.

**Gate classification:** MECHANICS PASS, QUALITY CLOSURE NOT CLAIMED. The deterministic triage
capability is integrated and deployable; the score is not a calibrated probability of human
readability.

**Estimated distance to production ready:** 55% ready / 45% remaining for a calibrated
human-readability model. Baseline artifact: `deslop-structural-readability/1`. The three remaining
quality gates are (1) independent human-rated, cross-language region data, (2) cross-project held-out
comparison of complexity-only, entropy-only, combined, and lexical-enriched models, and (3)
calibration of coefficients/0.50 candidate threshold with reliability/error reporting. No blocker
prevents using the current explicitly uncalibrated triage score.

**Restart/rebuild:** rebuild/reinstall the CLI or MCP binary to activate the new output in an
already-installed executable. No migration, network access, or new dependency is required.

**Signature:** Codex (GPT-5), Tree-sitter structural readability implementation, 2026-07-10.

## 2026-07-10T21:14:42+02:00 — Refactor-confidence distribution normalization

**Objective:** prevent compressed raw-confidence distributions from hiding all refactor targets,
while preventing flat or tied distributions from manufacturing arbitrary outliers.

**Output contract changes (`deslop.metrics/1`, additive):**
- Top-level `refactor_confidence_distribution`: count, mean, median, population stddev, min/max,
  linearly interpolated p25/p75, `flat`, and `relative_candidate_eligible`.
- Per-region `refactor_zscore` and tie-aware empirical `refactor_percentile` alongside the existing
  absolute `refactor_confidence`.
- Candidate selection is absolute (`raw >= 0.50`) OR guarded relative (`z >= 1.0` and percentile
  >= 0.90). Relative selection requires at least 8 regions, confidence range >= 0.05, and stddev
  >= 0.01. Candidate output states whether absolute and/or relative evidence selected it.
- Text, JSON, MCP descriptions/tests, README, and SPEC expose the statistics and semantics.

**Numerical convergence test:** exact summaries verified for `[0.10, 0.20, 0.30, 0.40]`
(mean/median 0.25, population stddev 0.1118033989, p25 0.175, p75 0.325). For nine 0.10 values
plus one 0.30, the low-absolute outlier receives z=3.0 and percentile=1.0 and qualifies relatively.
Ten tied 0.20 values receive percentile=0.5, forced z=0, flat=true, and produce no relative target.

**Real repository smoke:** metrics-crate scan produced n=87, mean=0.15356, stddev=0.13849,
median=0.14869, p25=0.04057, p75=0.22215, min=0.00497, max=0.64064, flat=false, relative eligible.
Candidates expanded from 2 absolute-only to 9 guarded high-tail regions. `input_files` measured raw
0.5866, z=3.13, percentile=0.988. The normalization implementation itself ranked first at raw
0.6406, z=3.52, percentile=1.0, providing a direct dogfood target rather than suppressing the result.

**Invalidated assumption / negative memory:** identical floating inputs do not guarantee computed
stddev equals exact zero. The first flat test found microscopic roundoff generating meaningless
z-scores. Fixed by forcing z=0 for any distribution classified flat; Hindsight negative memory was
written with recheck conditions.

**Verification:** `cargo fmt --all --check`; workspace build; slim no-default-features build;
`cargo test --workspace` (178 passed); `cargo test -p deslop-mcp --features slim-llm` (18 passed);
workspace clippy with `-D warnings`. No new dependency, migration, write path, or network behavior.

**Gate:** BOUNDED-QUALITY PASS, QUALITY CLOSURE NOT CLAIMED, READINESS UNCHANGED. Baseline artifact
remains `deslop-structural-readability/1`. Estimated distance to production ready remains 55% ready /
45% remaining for human-calibrated readability; normalization improves within-repo actionability but
does not supply human labels. Remaining gates are cross-language human ratings, held-out
cross-project ablation, and calibration of absolute/relative thresholds.

**Restart/rebuild:** rebuild/reinstall an already-installed CLI or MCP binary. No other restart.

**Signature:** Codex (GPT-5), confidence-distribution normalization, 2026-07-10.

## 2026-07-10T21:31:22+02:00 — Labeled refactor-confidence JSON (`deslop.metrics/2`)

**Objective:** make confidence immediately interpretable in JSON by mapping each score to one
categorical key, while retaining a stable numeric field for arithmetic and sorting.

**Contract:** metrics output is now `deslop.metrics/2`. Both per-region readability and ranked
candidate objects serialize:

```json
"refactor_confidence": { "high": 0.70 },
"refactor_confidence_score": 0.70
```

Bands are `very_low` [0.00,0.20), `low` [0.20,0.40), `moderate` [0.40,0.60), `high`
[0.60,0.80), and `very_high` [0.80,1.00]. The nested object always has exactly one key. The
numeric companion is the same underlying score and remains the authority for distribution,
threshold, ranking, z-score, and percentile calculations. The schema was bumped because the
`refactor_confidence` JSON type changed from number to object.

**Measured JSON smoke:** the metrics crate's top candidate serialized as
`{"high": 0.6406387287}` with companion `0.6406387287`; `input_files` serialized as
`{"moderate": 0.5866305613}`. Existing z-score, percentile, distribution, and candidate reasons
remain present.

**Validation:** band-boundary test covers 0.00, 0.19, 0.20, 0.40, 0.60, 0.70, 0.80, and 1.00;
each object has one key and equals the numeric companion. CLI JSON smoke and MCP contract test pass.
Full gate: rustfmt; workspace build; slim no-default-features build; workspace tests 179 passed;
MCP slim-llm tests 18 passed; workspace clippy `-D warnings`. No dependency or runtime behavior
change outside serialization.

**Gate:** PACKAGING PASS, QUALITY CLOSURE NOT CLAIMED, READINESS UNCHANGED. Baseline artifact is
now the `deslop.metrics/2` packaging of `deslop-structural-readability/1`. Estimated calibrated-model
readiness remains 55% ready / 45% remaining; labels improve communication but do not validate
human agreement. Remaining gates remain human-rated cross-language data, held-out cross-project
ablation, and score/threshold calibration.

**Restart/rebuild:** rebuild/reinstall an existing CLI or MCP installation. `/1` consumers must
upgrade to `/2` and read `refactor_confidence_score` when they require a scalar.

**Signature:** Codex (GPT-5), labeled confidence JSON packaging, 2026-07-10.

## 2026-07-11T00:53:28+02:00 — Explicit intrinsic confidence and repo-relative context (`deslop.metrics/3`)

**Objective:** make the confidence authority explicit and prevent consumers from confusing the
stable Tree-sitter-derived score with scan-local normalization.

**Contract:** metrics output is now `deslop.metrics/3`. Region and candidate objects serialize:

```json
"refactor_confidence": { "high": 0.70 },
"refactor_confidence_score": 0.70,
"confidence_basis": "tree_intrinsic_v1",
"repo_relative": { "zscore": 1.84, "percentile": 0.94 }
```

`refactor_confidence` and its scalar companion are intrinsic to the parsed region and use the
versioned Tree-sitter feature model. `repo_relative` is computed from the current scan's confidence
distribution and is contextual, not portable across scan sets. The old flat `refactor_zscore` and
`refactor_percentile` keys were removed. The top-level distribution summary and guarded relative
candidate-selection behavior remain unchanged.

**Measured JSON smoke:** a region emitted `confidence_basis: "tree_intrinsic_v1"`, intrinsic score
0.0104468 in the `very_low` band, and nested repo-relative z=-1.053 / percentile=0.04494. The top
candidate emitted intrinsic score 0.6406387 in the `high` band and nested z=3.5189 / percentile=1.0.

**Changes:** `crates/deslop-metrics/src/lib.rs` owns the `/3` serialization and model metadata;
`crates/deslop-mcp/src/lib.rs` and `spec.rs` expose and test the exact MCP contract; `SPEC.md` and
`.agents/PLAN.md` document the authority split and migration.

**Verification:** focused metrics tests 11/11; exact MCP metrics contract; CLI JSON smoke;
`cargo fmt --all --check`; workspace build; slim no-default-features build;
`cargo test --workspace` (179 passed); `cargo test -p deslop-mcp --features slim-llm` (18 passed);
workspace clippy with `-D warnings`. No dependency, migration, write-path, or network change.

**Invalidated assumptions / negative-memory status:** no new invalidation in this packaging slice.
Existing constraints remain active: repo-relative rank is not absolute refactor evidence, and flat
floating-point distributions must force contextual z-scores to zero.

**Gate:** PACKAGING PASS, QUALITY CLOSURE NOT CLAIMED, READINESS UNCHANGED. The deterministic
structural score is usable for triage, but calibrated-model readiness remains 55% ready / 45%
remaining pending human-rated cross-language data, held-out cross-project ablation, and absolute
score/threshold calibration.

**Restart/rebuild:** rebuild/reinstall an existing CLI or MCP binary. `/2` consumers must migrate
to `/3`, use `refactor_confidence_score` for arithmetic, and read contextual normalization from
`repo_relative`.

**Signature:** Codex (GPT-5), intrinsic/repo-relative confidence contract, 2026-07-11.

## 2026-07-12T13:11:12+02:00 — Algorithm audit: graph-first per-node analysis

**Objective:** audit deslop's algorithms and determine how to make the tool effective for cleaning
both human- and AI-authored code, with Tree-sitter as a general syntax backbone and evaluation at
node/block/line granularity.

**Target:** parser lifecycle; language packs; dependency graph; analyzer and duplication algorithms;
metrics/readability/entropy/complexity; finding/work-order/slim/verify flow; evaluation coverage;
primary readability/naturalness literature.

**Changes:** added `.agents/ALGORITHM_AUDIT.md`; appended the implementation checkpoint to
`.agents/PLAN.md`; recorded the confirmed negative memory in Hindsight. No Rust, schema, config,
test, dependency, or runtime code was changed.

**Commands/checks run:** Serena project activation and memories (Rust symbolic inspection was
unavailable because Serena exposed only Python); Hindsight startup recall/search and consolidation;
targeted `rg`/numbered source reads; Context7 Tree-sitter 0.25 traversal/query API check; primary-
source web literature review; current CLI metrics/graph/propose/slop/eval probes; timed metrics and
graph runs; `cargo test -p deslop-metrics`; `cargo fmt --all --check`; `cargo test --workspace`;
`cargo clippy --workspace -- -D warnings`.

**Results:** full gate green (179 tests). Live semantic probes failed despite the green suite:
clean health `40.38` versus sloppy `46.14`; clean relative-only refactor candidates at intrinsic
`0.15–0.17`; metrics `30.50s` versus graph `0.74s` over `crates`; false resolved `compact_label`
edges; Clojure branches score zero increments; typed TypeScript falls back/skips; one Rust region
generates 11 duplicate work orders. The broader sloppy corpus generated 62 orders but 31 unique IDs.

**Failure modes/root causes:** no shared parsed IR; raw grammar strings used as cross-language
semantics; per-region reparsing and overlapping aggregates; unsound global simple-name resolution;
unvalidated heuristic weights presented as confidence/health; zero-order entropy conflated with
compression/naturalness; fixed-window `O(n² × k)` same-file duplication; finding schemas lack node
identity/evidence; work orders are per finding rather than per region.

**Invalidated assumptions:** unit-test success proves metric/graph correctness; a repo-relative
outlier is an absolute refactor candidate; Tree-sitter-derived output is necessarily graph-first;
unique global simple-name lookup is resolution proof; Shannon entropy, model cross-entropy, and
compression have the same meaning or monotonic quality direction.

**Current recommendation/checkpoint:** implement P0 contract repair, then one parse per file into an
owned syntax arena with normalized roles, exclusive per-node features, lexical/CFG/dependency
projections, durable NodeKeys, structured evidence, and region-grouped work orders. Keep readability,
structural load, anomaly, redundancy, evidence reliability, impact, and refactor safety separate.
Run one convergent human-labelled calibration experiment only after the feature substrate is stable.

**Blockers:** none for the audit. Ruflo was not callable; built-in read-only agents covered the
independent architecture, metric, and literature tracks. A real TypeScript grammar will require an
existing-stack check and likely a justified grammar dependency during implementation.

**Next actions:** start with work-order grouping and regression proof; correct graph confidence and
language adapters; remove metric gating authority; implement the shared graph IR; replace clone
matching; then run the convergent calibration benchmark. Exact design and terminal validation
outcomes are in `.agents/ALGORITHM_AUDIT.md`.

**Dependencies/restart:** none for this read-only audit. Any later implementation will require a
rebuild/reinstall of CLI, MCP, and LSP binaries; no live fix is active now.

**Negative-memory status:** written and consolidated. The current structural readability/health/
confidence and graph-resolved outputs are authority-downgraded to experimental until the recorded
recheck conditions pass.

**Signature:** Codex (GPT-5), algorithm-audit integration owner, 2026-07-12.

## 2026-07-12T13:48:15+02:00 — Ultimate generic deslop roadmap and completion ledger

**Objective:** turn the graph-first algorithm audit into an executable product plan for a generic
human/LLM refactoring tool, including branch/function/module merge/split decisions, dependency order,
readability evidence, primary research references, measurable gates, and markable completion items.

**Target:** authoritative product contract; universal per-node program-graph architecture; adapter and
capability boundary; refactoring opportunity/recipe algorithms; work-order dependency planning; LLM
protocol; verification authority; cross-language calibration; incrementality; release evidence.

**Changes:** appended the authoritative “Ultimate Generic Deslop Plan (2026-07-12)” to
`.agents/PLAN.md`, retaining older plans as history; added `.agents/TODO.md` with 159 stable checklist
items and M0.1 as the explicit next task. The plan defines one immutable project snapshot with lossless
Tree-sitter syntax, owned nodes, scope/name, CFG/PST, PDG/SDG, project dependency, clone, evidence,
candidate, transaction, and verification overlays. It separates candidate generation, semantic legality,
and behavior validation; defines S0-S4 adapter capabilities, three-valued preconditions, work-order
`Reads`/`Writes`/`Requires`/`Invalidates`, a safety lattice, a dependency DAG, and atomic rollback. No
Rust, dependency, schema, runtime, or live-process code was changed.

**Research integrated:** 21 primary references covering PDG/SDG and slicing, code property graphs,
PST/SESE control regions, extract-method legality, scope/stack graphs and binding preservation, clone
indexing, SCCs and modularization, readability/naturalness/entropy, differential preconditions, and
refactoring-engine testing. Independent agents supplied architecture sequencing, primary-source limits,
and convergent numerical release gates; `/root` owned integration and final decisions.

**Commands/checks run:** focused `rg`/`sed` repository and artifact inspection; primary-source web and
Tree-sitter documentation review; duplicate checklist-ID and checkbox-shape checks; required heading,
reference, and local-artifact assertions; trailing-whitespace scan; `git diff --check HEAD -- .agents`;
`cargo fmt --all --check`; Hindsight durable decision write and graph consolidation.

**Results:** artifact contract PASS; 159 uniquely identified checkboxes, four completed audit/planning
items, 21 primary references, no malformed checkboxes or trailing whitespace, no patch whitespace errors,
and no Rust formatting regression. The benchmark plan now has explicit minimum assets and provisional
floors for graph accuracy, opportunity precision/recall, behavior preservation, human preference, paired
LLM uplift, and incremental scale; values must be frozen before the held-out run.

**Invalidated assumptions:** no new failed implementation attempt. The plan formalizes prior invalidations:
Tree-sitter syntax is not a universal semantic oracle; topology does not identify a correct cycle-breaking
edge; a high readability/slop/clone score is not refactor legality; test success alone is not equivalence;
and fuzzy baseline identity must never authorize a write.

**Current recommendation/checkpoint:** execute M0.1 first: group all findings for one
`(snapshot, target region, recipe)` into one work order and add duplicate-ID/proposal-count regressions.
Complete M0 contract truth before the owned snapshot/graph migration so later benchmark numbers are not
anchored to known duplicate, resolution, grammar, parse-error, or metric-authority failures.

**Blockers:** none for planning. Implementation of the semantic layers will require per-language adapter
work and may require a maintained TypeScript/TSX grammar after confirming the existing stack cannot meet
the contract; that dependency is not added by this planning change.

**Dependencies/restart:** none; documentation only. No rebuild, reinstall, migration, or live-process
restart is required. The proposed capabilities are not active until their TODO gates are implemented.

**Negative-memory status:** prior algorithm-audit negative memory remains active; the authoritative
architecture and next checkpoint were written to repo Hindsight and consolidated.

**Signature:** Codex (GPT-5), roadmap integration owner, 2026-07-12.

## 2026-07-12T14:20:29+02:00 — M0.1 unique region work orders

**Objective:** begin the ultimate-deslop implementation with M0.1: make a work order one refactoring
transaction rather than one finding, eliminate duplicate IDs/overlapping rewrites, preserve all evidence,
and prove the corrected contract through CLI, LLM-consumer, verifier, and apply paths.

**Target:** `deslop.workorder/1` generation and identity cardinality; repeated/overlapping source-path
discovery; legacy work-order JSONL ingestion; duplicate patch verification/application; exact corpus and
consumer regressions. `/root` owned integration; read-only agents independently traced producer/consumer
flows, compatibility, adversarial cases, and numerical validation.

**Changes:**

- `deslop-protocol` groups non-`safe-auto` findings by authoritative `SourceFile` path and exact enclosing
  region for the sole current implicit `rewrite-region/v1` recipe, orders regions/evidence
  deterministically, retains every finding, and preserves existing region-derived IDs/schema.
- `deslop-analyzer` deduplicates repeated and overlapping scan inputs by canonical physical-file identity,
  chooses a deterministic normalized display path, and keeps byte-identical distinct files separate.
- `deslop-slim` rejects duplicate legacy work-order IDs with first/current JSONL line evidence before any
  LLM call. Its aggregate regression proves one prompt and patch per region and all evidence in the prompt.
- `deslop-verify` rejects duplicate patch IDs before verification or writes, refuses generated work-order
  ID collisions instead of silently overwriting, and proves one aggregate patch verifies/applies once.
- CLI integration regressions cover the audited Rust fixture, full sloppy corpus, overlapping roots,
  distinct identical files, and equivalent path spelling/order. SPEC wording now directs agents to address
  every compatible listed finding while making the safety contract authoritative.
- `.agents/TODO.md` marks M0.1 complete, makes M0.2 the next item, and records M0.12-M0.14 for exact-byte
  revision guards, proposal-config reconstruction, and the `NeverAuto` policy conflict.

**Measured before/after:**

- `slop_rust.rs`: 13 work-order records / 3 unique IDs / largest repetition 11 -> 3 records / 3 IDs,
  retaining all 13 findings in group sizes 1, 1, and 11.
- Entire sloppy corpus: 62 records / 31 IDs / 8 duplicated IDs -> 31 records / 31 IDs / all 62 findings /
  zero duplicated IDs; largest aggregate remains 11.
- The eleven-finding region now causes one LLM call, one patch, one verification result, and one atomic
  file write. Repeated file arguments and file-plus-parent inputs do not duplicate work; two distinct files
  with identical bytes remain distinct; equivalent path ordering/spelling produces byte-identical JSON.

**Commands/checks run:** Serena activation/instructions/memories and targeted text-symbol search; Hindsight
active-plan/negative-memory search, checkpoint and negative-memory writes, and consolidation; baseline/live
CLI `propose` probes with exact JSON aggregation; focused protocol/analyzer/slim/verify/CLI tests; dependent
report/MCP/slim/verify tests; `cargo fmt --all --check`; `cargo build --workspace`; slim no-default-features
build; `cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm`; workspace clippy with
`-D warnings`; TODO shape/ID checks; `git diff --check HEAD`.

**Verification results:** PASS. Workspace tests: 195 passed. MCP `slim-llm`: 18 passed. Workspace and slim
builds passed; formatting and warnings-denied clippy passed; exact live acceptance returned 31 orders,
31 IDs, 62 findings, largest group 11, and no duplicate IDs. No new dependency or schema version was added.

**Failure modes/root causes corrected:** the producer previously mapped each finding to an independently
serialized order while deriving identity only from its enclosing region; verifier silently overwrote equal
IDs, slim called the LLM repeatedly, and apply eventually failed on overlapping patches. Source discovery
also admitted the same physical file through repeated/overlapping roots. Both sources now converge before
rewriting, and legacy/programmatic duplicate inputs fail early.

**Invalidated assumptions / residual semantic boundary:** `/1` cannot claim the roadmap's full
`(ProjectSnapshotId, NodeKey, RecipeId)` identity because it has no snapshot/node/recipe fields. It now
correctly implements one order per exact line-region for its sole implicit recipe. Region fingerprints also
hash trimmed text rather than exact bytes, verifier reconstructs orders with default analyzer config, and
SPEC/runtime disagree about proposing `NeverAuto`; these pre-existing defects are recorded as M0.12-M0.14,
not hidden by the cardinality repair. Verification still proves patch safety, not that every finding cleared;
expected graph-delta enforcement remains M5/M7.

**Current recommendation/checkpoint:** proceed to M0.2, replacing bare-name graph resolution with scoped
unique/ambiguous/unresolved facts and exact duplicate-name regressions. Keep M0.1's generated-output and
duplicate-input gates permanent. True snapshot/recipe transactions remain M1.4/M5.1/M6.1.

**Blockers:** none for M0.1 or M0.2. Serena's configured semantic language remained Python-only, so Rust
inspection used Serena text search plus local `rg`/targeted reads; this did not block implementation.

**Dependencies/restart:** rebuild/reinstall CLI, MCP, LSP, or bundled slim binaries to activate this code.
Existing `deslop.workorder/1` IDs and shapes remain compatible; legacy JSONL containing duplicate IDs is now
rejected deliberately and must be regenerated or manually consolidated.

**Negative-memory status:** durable checkpoint and new identity/config/policy negative memory were written
to repo Hindsight and consolidated. Recheck conditions are linked to TODO M0.12-M0.14.

**Signature:** Codex (GPT-5), M0.1 integration owner, 2026-07-12.

## 2026-07-12T15:46:39+02:00 — M0.2 scope-aware graph authority

**Objective:** continue the ultimate-deslop implementation with M0.2: remove first-wins bare-name
authority from `deslop.graph/1`, distinguish unique syntactic candidates from ambiguity and unresolved
labels, and prove that duplicate definitions no longer silently redirect refactor-planning edges.

**Target:** `deslop-graph` path discovery, symbol indexing, owner/scope traversal, call/import/inheritance
edge classification, graph authority documentation, MCP consumer descriptions, and exact regressions.
`/root` owned integration and final verification; three read-only agents independently audited producer
flow, consumers/schema compatibility, and adversarial regressions. No agent edited shared files.

**Changes:**

- Replaced first-wins symbol and module maps with candidate-preserving indexes for simple names,
  qualified names, owner/name pairs, parent ownership, and module keys. Every symbol now has a
  path-qualified name, including top-level definitions.
- Added deterministic best-candidate routing through nearest lexical owners, explicit self/type owners,
  named owners, module-qualified files, qualified suffixes, and finally global syntactic fallback.
  One candidate is `Syntactic`, competing candidates are `Ambiguous`, and no candidate produces a
  syntactic unresolved placeholder. Reference edges are never promoted to `Resolved`; exact `Contains`
  ownership remains resolved.
- Fixed inheritance extraction so an edge originates at the subclass after its node exists. Python
  multiple bases now produce separate edges rather than one combined label.
- Ported canonical physical-path deduplication and deterministic display-path selection into graph
  discovery, making equivalent root order/spelling produce byte-identical JSON.
- Added 15 graph regressions covering same-file duplicate names, same-scope duplicates, remote duplicates,
  qualified duplicates, colliding module/import keys, unresolved calls, local binding shadowing, nested
  scope, self/named type calls, unique remote candidates, subclass-owned multiple inheritance, path
  determinism, and the live duplicate `compact_label` case.
- Updated `SPEC.md`, the active MCP graph description, its duplicate spec description, and MCP tests to
  state that graph/1 syntactic edges are evidence rather than resolution proof. `.agents/TODO.md` marks
  M0.2 complete and leaves M0.3 next for alias/import and full language-binding regressions.

**Measured before/after:** the audited live source had 2 `compact_label` definitions and 10 calls, with
all calls previously routed to the first definition in `builder.rs`. The corrected probe has 2 definitions,
10 calls, 10/10 syntactic calls, and every target in the caller's file. The current full graph-source probe
contains 6 files, 135 symbols, 384 unresolved/ambiguous placeholder nodes, 1,069 edges, 135 resolved
containment edges, zero ambiguous edges on this source set, and zero externally proven edges.

**Commands/checks run:** Serena project activation/memory bootstrap from the continued session, targeted
Serena text search plus local `rg`/`sed`; Hindsight startup context from the continued session, two durable
checkpoint/negative-memory writes, and `improve`; focused graph tests and clippy; MCP graph/tools tests and
the complete default MCP suite; exact CLI graph JSON plus `jq` ownership assertions; `git diff --check`;
TODO ID uniqueness; `cargo fmt --all --check`; workspace build; slim no-default-features build;
`cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm`; and workspace clippy with
`-D warnings`. One attempted Cargo invocation supplied two positional test filters and was rejected by
Cargo; the complete graph suite was rerun immediately and passed.

**Verification results:** PASS. `deslop-graph`: 15 passed. Default MCP: 16 passed. Workspace: 207 passed.
MCP `slim-llm`: 18 passed. Formatting, workspace and slim builds, patch whitespace checks, TODO identity,
and warnings-denied workspace clippy passed. No dependency or graph schema version was added.

**Invalidated assumptions / residual semantic boundary:** a unique result from a syntactic name lookup is
not proof of lexical binding, import aliasing, types, dispatch, or externality. `deslop.graph/1` therefore
retains its compatible schema but deliberately reserves `Resolved` for containment; unique best references
and unresolved placeholders are syntactic, while competing candidates are ambiguous. Graph/1 cannot retain
an ambiguity candidate list or explicit status/authority/provenance. Those facts require the M3 scope graph
and a versioned graph/2 contract. Local variable bindings and aliases are not modeled yet, so syntactic
targets must not authorize semantic refactors.

**Current recommendation/checkpoint:** proceed to M0.3 and complete alias/import, shadowing, and language-
specific binding fixtures without weakening the new authority labels. Then continue M0 adapter/parse-error
contract repairs before building the owned syntax snapshot and full scope graph in M1-M3.

**Blockers:** none for M0.2 or M0.3. Serena remains configured for Python symbols only, so non-trivial Rust
inspection used its text search plus local symbol-oriented reads; this did not block the change.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries to activate the new graph behavior. The
`deslop.graph/1` JSON shape is compatible, but consumers that treated reference `resolved` as name-binding
proof must accept `syntactic`/`ambiguous` and must not auto-refactor from those edges alone.

**Negative-memory status:** durable corrective memory records the invalidated first-wins/name-uniqueness
assumption and the graph/1 authority downgrade; repo Hindsight consolidation completed. Recheck at M0.3
alias fixtures and M3 graph/2 binding/provenance implementation.

**Signature:** Codex (GPT-5), M0.2 integration owner, 2026-07-12.

## 2026-07-12T19:35:03+02:00 — M0.3 alias and binding safety regressions

**Objective:** continue the ultimate-deslop implementation with M0.3: convert the remaining duplicate,
shadowing, alias/import, and cross-file graph cases into authoritative regressions, then remove every
observed false planning target without overstating graph/1 semantic authority.

**Target:** private graph extraction/indexing for bindings and import sources; bare, qualified, and import
reference routing; Clojure form classification; graph/1 authority documentation; CLI JSON/DOT and MCP
structured-output consumers. `/root` owned integration and full verification. Three read-only agents
audited language fixtures, implementation flow, and consumer/schema compatibility; they made no edits.

**Changes:**

- Removed project-wide bare-name fallback. A call with no scoped/import/module evidence now targets a
  syntactic unresolved placeholder even when exactly one same-named project function exists; multiple
  remote definitions likewise cannot manufacture ambiguity or a binding without visibility evidence.
- Added private, deterministic local/import binding indexes. Rust, Python, JavaScript, JS-compatible
  TypeScript, Julia, and Clojure extraction records common parameters, assignments/declarations, receiver
  names, local binding forms, and import names. Local bindings block same-named outer or module candidates;
  unsupported aliases block fallback and remain syntactic placeholders.
- Distinguished local qualifiers from imported qualifiers internally: local receivers block module-stem
  coincidence before lookup, while a syntactically exact imported module may still narrow a qualified call.
  No reference edge is promoted to `Resolved`.
- Reworked import-key derivation so Rust/Python/Julia/Clojure forms use the source module rather than an
  `as` alias; relative JavaScript/TypeScript sources retain path-based matching. Cross-language fixtures
  prove import edges point to `origin`, while alias calls cannot point to an unrelated `alias` function.
- Added Clojure `:require`, `:import`, and `:refer-clojure` to non-call forms, eliminating the observed false
  `:require` call edge.
- Expanded `deslop-graph` to 19 tests: same-scope and qualified duplicates, remote unresolved names,
  nested definitions, local/parameter/receiver shadowing, five non-Rust local-shadow adapters, six-language
  import aliases, cross-file import sources, ambiguity DOT labels, path determinism, inheritance, and the
  live `compact_label` regression.
- Added a real CLI integration test asserting graph/1 JSON placeholder semantics, no resolved references,
  agent notes, and DOT `(syntactic)` rendering. MCP now preserves the same alias placeholder in structured
  output and describes unresolved `external-symbol` targets accurately. `SPEC.md` defines reference `to`
  as a planning hint, not a proven binding.
- `.agents/TODO.md` marks M0.3 complete and advances **NEXT** to M0.4, distinct JavaScript/TypeScript/TSX
  grammar selection.

**Measured evidence:** the live graph-source probe still has 2 `compact_label` definitions and 10 calls;
10/10 calls are syntactic and every target is in the caller's file. The current source graph has 6 files,
160 symbols, 505 external-or-unresolved placeholders, 1,423 edges, 160 resolved containment edges, zero
ambiguous edges on that source set, and zero externally proven edges. The corpus probe has zero resolved
reference edges and zero false Clojure `:require`/`require` calls.

**Commands/checks run:** Serena activation/instructions and required global/repo memories; Hindsight startup
recall/search, two corrective checkpoint writes, and `improve`; targeted `rg`/`sed` flow inspection; focused
graph tests and warnings-denied clippy; CLI graph integration; default MCP graph/tools and complete suite;
exact CLI graph JSON plus `jq` source/corpus assertions; `git diff --check`; TODO ID uniqueness;
`cargo fmt --all --check`; workspace build; slim no-default-features build; `cargo test --workspace`;
`cargo test -p deslop-mcp --features slim-llm`; and workspace clippy with `-D warnings`.

**Verification results:** PASS. `deslop-graph`: 19 passed. CLI: 24 passed across unit/integration suites.
Default MCP: 17 passed. Workspace: 213 passed. MCP `slim-llm`: 19 passed. Formatting, workspace/slim builds,
patch whitespace, TODO identity, live probes, and warnings-denied workspace clippy passed. No dependency or
public schema version was added.

**Failed iterations / invalidated assumptions:** the first compile exposed incorrect reference patterns in
new token filtering and was corrected immediately. The first focused run correctly made two old remote-name
expectations stale and exposed that the shadow test only checked confidence, not its false target; expectations
were strengthened to require placeholders. A Julia fixture initially selected the signature call in the
unrelated file rather than the caller edge, so edge selection was made owner/file-specific. Julia assignment
nodes lack the expected `left` field; the adapter now conservatively uses their first named child. The
`compact_label` live count briefly rose because new code called the audited helper; identifier extraction was
made direct, preserving the exact 2-definition/10-call acceptance probe.

**Residual semantic boundary:** M0.3 blockers are intentionally owner-level and conservative; they may
over-block outside an inner lexical block and do not implement exact declaration order, destructuring,
wildcards, re-exports, visibility, package/build roots, or alias-to-symbol provenance. TypeScript cases remain
JavaScript-compatible because the current TypeScript pack still selects the JavaScript grammar. Span-accurate
bindings, candidate lists, explicit resolution status/authority/provenance, and compiler facts remain M3 and
graph/2 work; typed TypeScript/TSX correctness is M0.4/M0.5.

**Current recommendation/checkpoint:** execute M0.4 next by selecting maintained, distinct JavaScript,
TypeScript, and TSX grammars and proving the registry never silently parses typed syntax with JavaScript.
Then add typed/JSX error fixtures in M0.5 before broader adapter repairs.

**Blockers:** none for M0.3 or M0.4. Serena remains configured for Python-only symbols, so Rust inspection
used Serena text search plus local targeted reads; this did not block implementation.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries to activate the hardening. `deslop.graph/1`
shape remains compatible, but consumers must accept that previously targeted unique remote bare names now
produce syntactic unresolved placeholders and must never authorize edits from reference `to` alone.

**Negative-memory status:** corrective repo memory supersedes the weaker M0.2 project-wide bare-name
candidate allowance and records the remaining conservative scope limitation; Hindsight consolidation passed.
Recheck at M0.4/M0.5 grammar fixtures and M3 graph/2 scope/binding implementation.

**Signature:** Codex (GPT-5), M0.3 integration owner, 2026-07-12.

## 2026-07-12T20:08:05+02:00 — M0.4 path-selected TypeScript and TSX grammars

**Objective:** continue the ultimate-deslop implementation with M0.4: stop parsing TypeScript and TSX
through the JavaScript grammar, select every JavaScript-family dialect deliberately, and propagate that
selection through all parse-owning consumers without breaking existing public language schemas.

**Target:** grammar dependency resolution, `LangPack`/registry selection, `SourceFile` parsing and region
ownership, analyzer/metrics/graph/mutation/verifier/LSP/MCP consumers, configuration inheritance, and exact
positive/negative dialect regressions. `/root` owned integration and full verification; three read-only
agents audited dependency/schema compatibility, consumer flow, and a decisive syntax truth table.

**Changes:**

- Added the maintained official `tree-sitter-typescript 0.23.2` dependency. Its
  `LANGUAGE_TYPESCRIPT` and `LANGUAGE_TSX` constants both use parser ABI 14, compatible with workspace
  `tree-sitter 0.25.10` (supported 13 through 15); Cargo resolves one shared
  `tree-sitter-language 0.1.7`.
- Added `LangPack::grammar_for_path`. JavaScript/JSX continues to use `tree-sitter-javascript`;
  `.ts`, `.mts`, and `.cts` use `LANGUAGE_TYPESCRIPT`; `.tsx` uses `LANGUAGE_TSX`.
- Kept the public language family compatible: `.tsx` remains `Lang::TypeScript` and serializes as
  `"typescript"`. Grammar dialect is path-bound parser provenance, not an unversioned public enum value.
- Added `parse_source` and `source_parses_without_errors`; `SourceFile` region lookup is path-aware.
  Migrated analyzer agnostic passes, boundary analysis, token analysis, Rust pack parsing, metrics,
  graph extraction, native mutation generation, verifier candidate/mutant parse guards, and downstream
  LSP analysis to the source-aware parser. The legacy `parse_tree(Lang, text)` remains the non-TSX default
  for callers that have no path.
- Added a decisive grammar truth table: `.js/.jsx` parse JSX and reject type annotations;
  `.ts/.mts/.cts` parse type annotations and reject TSX; `.tsx` parses typed JSX. Tests assert actual
  `jsx_element` and `type_annotation` nodes rather than only the absence of `ERROR`.
- Added typed consumer regressions: analyzer and LSP inline suppression over TS/TSX, named metrics
  function regions, graph function/interface extraction without notices, and verifier TSX parse guards.
- Made mutation capability honest: the verifier now queries the registered native mutation packs, so
  JavaScript/TypeScript return no-probe/unknown rather than claiming unsupported native mutation.
- Completed MCP analyzer parity by accepting, applying, and advertising `javascript` and `typescript`
  threshold tables; TSX deliberately inherits `[analyzer.typescript]`. README, SPEC, `docs/CONFIG.md`, and
  `deslop.toml.example` document grammar selection and configuration inheritance.
- `.agents/TODO.md` marks M0.4 complete and advances **NEXT** to M0.5 typed construct, JSX/TSX region,
  and explicit error-policy fixtures.

**Commands/checks run:** Serena activation/instructions and required memories from the continued session;
Hindsight recall/search, checkpoint and negative-memory writes, and `improve`; Context7 official
tree-sitter-typescript documentation; `cargo search`/`cargo info` version checks; `cargo tree` dependency
resolution; targeted `rg`/`sed` consumer audits; focused lang/parse/analyzer/metrics/graph/mutate/verify/LSP/
MCP tests and clippy; `cargo check --workspace`; `git diff --check`; TODO ID uniqueness;
`cargo fmt --all --check`; workspace build; slim no-default-features build; `cargo test --workspace`;
`cargo test -p deslop-mcp --features slim-llm`; and workspace clippy with `-D warnings`.

**Verification results:** PASS. Workspace: 223 tests. Feature-enabled MCP: 20 tests. Focused suites include
53 analyzer, 20 graph, 12 metrics, 7 mutation, 39 verifier, 7 LSP, and 18 default MCP tests. Formatting,
workspace/slim builds, dependency unification, patch whitespace, TODO identity, and warnings-denied workspace
clippy passed. One maintained dependency was added; no public schema or serialized `Lang` value changed.

**Failed iterations / invalidated assumptions:** the first working design temporarily introduced
`Lang::Tsx`. Compatibility review invalidated it before checkpoint: grammar dialect is not a language family,
and a new serialized enum value would break strict findings/metrics/graph consumers without versioning.
The implementation was replaced with source-path-bound grammar selection and every source-aware parse caller
was migrated. A verifier migration initially referenced the nonexistent `WorkOrder.source_path` instead of
`WorkOrder.path`, and final focused clippy caught one needless borrow; both were corrected before full gates.

**Residual semantic boundary:** M0.4 proves correct grammar selection with one annotation, interface,
and JSX element plus consumer preservation. It does not yet freeze the broader typed construct matrix,
generic-arrow TSX ambiguity, decorators, overloads, type-only imports/exports, JSX fragments/spreads/member
tags, malformed-node spans, or the cross-surface partial-analysis policy. Those are M0.5 and M0.8. Explicit
grammar dialect/version provenance in serialized facts remains a versioned M2 adapter-schema concern.

**Current recommendation/checkpoint:** execute M0.5 next with frozen typed TypeScript and JSX/TSX positive,
negative, region, and error fixtures across parser, metrics, graph, analyzer, verifier, CLI, MCP, and LSP as
appropriate. Keep public `Lang::TypeScript`; expose dialect provenance only through a versioned contract.

**Blockers:** none for M0.4 or M0.5. Serena remains configured for Python-only symbols, so Rust flow
inspection used Serena text search plus local targeted reads; this did not block implementation.

**Dependencies/restart:** rebuild or reinstall binaries to activate the grammar. The new crate is compiled
into parser consumers; no migration or config key is required. Existing `[analyzer.typescript]` now applies
to both TypeScript and TSX through CLI/LSP/MCP. JavaScript/TypeScript native mutation remains honestly
unsupported and therefore cannot provide a mutation proof.

**Negative-memory status:** durable corrective memory records and supersedes the rejected public
`Lang::Tsx` approach; path-aware family/dialect separation is the active rule. Repo Hindsight consolidation
passed. Recheck only when M2 versions explicit adapter/dialect provenance.

**Signature:** Codex (GPT-5), M0.4 integration owner, 2026-07-12.

## 2026-07-12T20:30:27+02:00 — M0.5 typed TypeScript, JSX, and TSX fixtures

**Objective:** freeze the typed JavaScript-family construct, behavioral-region, and explicit parser-error
contract before expanding language coverage, so downstream refactoring facts are based on the selected
TypeScript or TSX grammar rather than incidental JavaScript recovery.

**Target:** shared physical TypeScript/TSX/JSX fixtures and their parser, analyzer, metrics, graph, protocol,
and verifier consumers. `/root` owned integration and full verification; three read-only agents audited the
fixture boundary, malformed-input contract, and consumer coverage.

**Changes:**

- Added `tests/fixtures/typescript/typed.ts` with type-only imports/exports, interfaces, generic aliases,
  overload signatures plus implementation, decorators, generic classes, private fields, methods, type
  predicates, `satisfies`, and namespaces.
- Added `component.tsx` with typed generic arrows and components, fragments, member tags, spread props,
  and generic JSX type arguments, plus a JavaScript `component.jsx` dialect fixture.
- Added unequivocally malformed `.ts` and `.tsx` fixtures. Parser regressions require root error state and
  an explicit `ERROR` or missing recovery node instead of assuming any syntactically suspicious JSX is
  rejected.
- Locked selected paths, public `Lang::TypeScript` identity, construct node kinds, and exact behavioral
  regions. The fragment representation is the grammar's nameless `jsx_element`, not an invented
  `jsx_fragment` kind.
- Migrated typed fixture coverage into metrics, graph, analyzer, protocol, and verifier tests. Metrics keep
  named callable spans instead of file fallback; graph emits typed declarations without notices; protocol
  targets the enclosing TSX component; verifier both selects TSX from the work-order path and rejects the
  malformed TS/TSX fixtures.
- Corrected the prior M0.4 report's prose: the pre-existing kebab-case serde representation of
  `Lang::TypeScript` is `"type-script"`, not `"typescript"`. M0.5 now locks that JSON value and proves no
  public `"tsx"` language value was introduced.
- Updated SPEC with the fixture boundary and the explicit limitation that parser success is not JSX tag-name
  equality validation. Updated `.agents/TODO.md` to mark M0.5 complete and advance **NEXT** to M0.6.

**Commands/checks run:** targeted fixture probes and parser/consumer tests; `cargo fmt --all --check`;
`git diff --check`; `cargo build --workspace`; `cargo build -p deslop-slim --no-default-features`;
`cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm`; and
`cargo clippy --workspace -- -D warnings`. Hindsight checkpoint and corrective memories were written and
consolidated.

**Verification results:** PASS. Workspace: 228 tests. Feature-enabled MCP: 20 tests. Formatting, patch
whitespace, workspace and no-default-features slim builds, and warnings-denied workspace clippy passed.
No dependency or public schema change was made in M0.5.

**Failed iterations / invalidated assumptions:** an initial TSX region assertion selected a line inside the
nested generic arrow and correctly resolved to that inner callable; the probe moved to the component body to
test the intended enclosing `View` region. Expected fixture end lines were corrected to the parser's exact
inclusive spans. More importantly, mismatched JSX opening/closing names were invalidated as a reliable
malformed fixture because the official grammar may accept them without `ERROR`; deslop does not claim that
tree-sitter performs JSX tag-name semantic validation.

**Residual semantic boundary:** M0.5 establishes parser error evidence and verifier rejection, but does not
yet choose whether graph, metrics, analyzer, LSP, and MCP should reject a whole malformed file or emit
explicitly partial facts. That cross-surface policy remains M0.8. Grammar-version node-shape provenance and
adapter goldens remain M2.7 concerns.

**Current recommendation/checkpoint:** execute M0.6 next by emitting Python behavioral regions and freezing
async, decorated, and nested-function ownership before expanding graph resolution semantics.

**Blockers:** none. Serena remains configured for Python-only symbols and therefore cannot symbolically
inspect this Rust workspace; targeted text and local Rust reads remain the active fallback.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries to consume the fixture-backed parser
behavior. No migration or configuration change is required.

**Negative-memory status:** corrective repo memory supersedes the M0.4 `"typescript"` wording and rejects
mismatched JSX tag names as parse-error evidence. Hindsight consolidation passed. Recheck partial malformed
analysis at M0.8 and serialized dialect provenance at M2.

**Signature:** Codex (GPT-5), M0.5 integration owner, 2026-07-12.

## 2026-07-12T20:47:33+02:00 — M0.6 Python behavioral regions

**Objective:** repair Python's adapter contract so async, decorated, method, and nested-function nodes
produce stable per-callable analysis and rewrite ownership instead of falling back to finding lines or
silently disabling long-method and behavioral-duplication analysis.

**Target:** `PythonPack` canonical roles and region ownership, analyzer behavioral segmentation and nested
long-method traversal, metric spans, protocol grouping, graph containment, shared fixture coverage, and the
corpus-level CLI work-order baseline. `/root` owned implementation/integration and all verification. One
read-only agent completed an exact pinned-grammar/CST audit; two broader read-only audits were stopped when
their orchestration overhead no longer paid for itself.

**Changes:**

- Added `tests/fixtures/python/behavioral.py`, freezing a top-level function, decorated nested async
  wrapper, class, decorated async method, and nested normalizer.
- Implemented Python `region_class`, `is_long_method_region`, `is_behavioral_container`, and
  `enclosing_region`. `function_definition` is behavioral; classes are declaration containers that expose
  contained methods; `decorated_definition` derives its semantic role from the `definition` field.
- Decorated callables are one semantic region: the name/kind comes from the wrapped definition, while the
  ownership span starts at the first decorator. The wrapped definition is excluded from duplicate
  long-method evaluation. Nearest nested callable wins for enclosing-region/work-order selection.
- Added exact parser assertions for two `decorated_definition` nodes, four `function_definition` nodes,
  two anonymous `async` tokens, and stable line/byte spans. The pinned grammar has no
  `async_function_definition` node.
- Added `LangPack::is_behavioral_container`, defaulting false. Python opts classes in so declaration
  semantics do not prune their methods from duplication analysis without changing unrelated adapters.
- Long-method traversal now continues into nested callables, so outer and inner function nodes are each
  evaluated. Decorated wrapper/inner syntax still yields only one semantic result.
- Metrics now ask the language adapter for the ownership span while retaining the declared node for metric
  evidence. Python tests lock `traced`, `wrapper`, `Service`, `process`, and `normalize` regions and prevent
  whole-file fallback.
- Protocol tests prove decorated method findings cover lines 13–18 including `@traced`, while a nested
  finding selects only lines 15–16. Graph tests lock resolved containment as
  file → `traced` → `wrapper` and file → `Service` → `process` → `normalize`, with no synthetic decorator
  symbol.
- Added analyzer regressions for Python decorated/nested long methods and callable duplication. The broader
  sloppy corpus still has 62 findings, now grouped into 28 unique work orders instead of 31 because Python
  line-level findings merge at callable ownership boundaries; the CLI integration baseline was updated.
- Updated SPEC and `.agents/TODO.md`, including correction of the existing public TypeScript serde spelling
  to `type-script`; M0.7 is **NEXT**.

**Commands/checks run:** pinned grammar `node-types.json`/`grammar.json` inspection; targeted `rg`/`sed` flow
audits; focused parse/lang/analyzer/metrics/protocol/graph tests and clippy; a measured CLI `propose` corpus
run with `jq`; `cargo fmt --all --check`; `git diff --check`; exact TODO checklist identity validation;
`cargo build --workspace`; `cargo build -p deslop-slim --no-default-features`; `cargo test --workspace`;
`cargo test -p deslop-mcp --features slim-llm`; and `cargo clippy --workspace -- -D warnings`. Hindsight
checkpoint and negative memories were written and consolidated.

**Verification results:** PASS. Workspace: 234 tests. Feature-enabled MCP: 20 tests. Formatting, patch
whitespace, workspace/slim builds, TODO identity, and warnings-denied clippy passed. Measured corpus:
28 work orders, 62 findings, 28 unique IDs, including one Python work order.

**Failed iterations / invalidated assumptions:** the first segmentation change made every declaration node
transparent. Full integration exposed that this broadened unrelated adapter behavior; it was replaced with
the explicit, default-false `is_behavioral_container` capability and Python-only class opt-in. A first TODO
uniqueness shell probe incorrectly treated references in descriptive text and the distinct `M7.3`/`M7.3a`
IDs as duplicates; the corrected probe extracts complete checklist IDs only. The initial corpus assertion
failure was not lost findings: measurement proved the same 62 findings were grouped into 28 rather than 31
regions under the repaired Python ownership contract.

**Residual semantic boundary:** nested long-method nodes are now evaluated individually, but an outer
callable's present metrics remain inclusive of nested syntax. Exclusive/inclusive per-node aggregation is
an M1 shared-snapshot concern. Graph symbol spans intentionally describe inner definitions while metric and
work-order ownership spans include decorators; versioned adapter facts in M2 must make that distinction
explicit. Stacked-decorator syntax follows the same grammar wrapper but is not yet a separate fixture.

**Current recommendation/checkpoint:** execute M0.7 next by repairing Clojure branch/decision roles and
freezing reader/macro edge cases before choosing the cross-surface partial-parse policy in M0.8.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted text/local Rust
inspection remains the active fallback.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries to activate Python grouping. No dependency,
migration, public schema, or configuration change is required.

**Negative-memory status:** durable negative memory supersedes global declaration transparency with the
adapter-scoped behavioral-container callback. Recheck at M1 canonical-role/owned-snapshot work or an
intentional JS/TS class segmentation repair. Hindsight consolidation passed.

**Signature:** Codex (GPT-5), M0.6 integration owner, 2026-07-12.

## 2026-07-12T20:59:42+02:00 — M0.7 contextual Clojure complexity roles

**Objective:** stop deriving Clojure decisions and nesting from impossible raw CST-kind matches, so
control forms contribute complexity, ordinary calls do not, and reader/macro data is not mistaken for
executed behavior.

**Target:** `LangPack` metric-role callbacks, `ClojurePack` form/context mapping, metric aggregation,
reader/macro fixtures, parser assertions, SPEC, and the durable roadmap. `/root` owned implementation,
integration, and all verification; prior M0.6 subagent work had ended and no delegation was needed.

**Changes:**

- Added default adapter callbacks `metric_branch_contribution`, `is_metric_nesting`, and
  `is_metric_flow_break`; existing adapters retain raw-kind-array behavior without central matches.
- Clojure now maps evaluated `list_lit` heads contextually. `if`/`when` variants,
  `cond`/`condp`/`case`, comprehensions, and loop forms contribute one decision and nesting level.
  Ordinary calls contribute neither. `throw` and `recur` are flow breaks; `recur` is no longer a branch.
- Added reader-context evaluation tracking. Discard, quote, var quote, reader eval, and syntax-quoted
  templates are data for complexity; unquote and unquote-splicing re-enter evaluated context.
- Reclassified `defmacro` and `defmethod` as behavioral regions and aligned declared Clojure metric and
  Halstead operator lists with the contextual callbacks.
- Added `tests/fixtures/clojure/control_edges.clj` with nested `if`/`when`, ordinary calls, a syntax-quoted
  macro template, quote/discard edges, a live form inside a macro call, and `loop`/`recur`.
- Parser tests lock the exact quote/discard/syntax-quote/unquote node counts and top-level regions. Metrics
  lock cyclomatic/cognitive/max-nesting triples: classifier `3/3/2`, ordinary calls `1/0/0`, macro
  template `1/0/0`, quoted/discarded plus one live branch `2/1/1`, and loop+if+recur `3/4/2`.
- Updated SPEC and `.agents/TODO.md`; M0.8 partial-analysis policy is **NEXT**.

**Commands/checks run:** pinned grammar file/node inspection; targeted `rg`/`sed`; measured CLI JSON
metrics with `jq`; focused lang/parse/metrics tests and clippy; `cargo fmt --all --check`;
`git diff --check`; exact TODO identity validation; workspace build; slim no-default-features build;
`cargo test --workspace`; feature-enabled MCP tests; and warnings-denied workspace clippy. Hindsight
checkpoint and constraint memories were written and consolidated.

**Verification results:** PASS. Workspace: 236 tests. Feature-enabled MCP: 20 tests. Formatting, patch
whitespace, both builds, TODO identity, and workspace clippy passed. No dependency, public schema, or
configuration change was made.

**Failed iterations / invalidated assumptions:** the audit invalidated both existing raw declarations:
Clojure branch names could never equal the grammar's `list_lit` kind, while declaring every `list_lit` as
nesting made ordinary calls inflate depth. The first manual patch placed the Clojure override methods in
`GenericPack`; a measured CLI run immediately exposed unchanged Clojure scores, and the methods were moved
to `ClojurePack` before focused/full gates.

**Residual semantic boundary:** each listed `cond`/`case` form currently contributes one decision rather
than clause-count contributions. Boolean-chain and multi-arm normalization belongs to the cross-language
construct matrix/per-node IR work. Syntax-quoted templates are not macroexpanded, and reader conditionals
are not active-dialect selected because the callback has no path/dialect provenance; do not claim expanded
or platform-exact complexity.

**Current recommendation/checkpoint:** execute M0.8 next by selecting and enforcing one explicit
parse-error/partial-analysis policy across scan, metrics, graph, LSP, MCP, and slim, including provenance
that prevents partial facts from authorizing unsafe rewrites.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust reads
remain the active fallback.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries to activate corrected Clojure metrics.
No migration or config change is required.

**Negative-memory status:** durable constraint memory rejects raw `list_lit` kind matching and records the
no-macro-expansion/no-reader-dialect boundary. Recheck under M1/M2 owned syntax facts and M0.8 uncertainty
notices. Hindsight consolidation passed.

**Signature:** Codex (GPT-5), M0.7 integration owner, 2026-07-12.

## 2026-07-13T00:50:42+02:00 — M0.8 fail-closed partial-analysis authority

**Objective:** replace silent parse-recovery fallbacks with one explicit cross-surface authority policy,
so malformed or parser-incomplete sources remain inspectable but can never authorize metrics claims,
work orders, LLM egress, code actions, verification overrides, or writes.

**Target:** shared core/parse provenance; analyzer and project passes; findings, metrics, graph, and slim
schemas; report/CLI/SARIF; protocol/fix/verify; LSP; MCP default and `slim-llm` modes; SPEC and the durable
roadmap. `/root` owned all edits, integration decisions, and verification. Ruflo was unavailable; three
read-only subagents audited core, integration, and contract/test surfaces without write ownership.

**Changes:**

- Added fail-closed `AnalysisStatus::{Unknown, Complete, Partial, Unsupported, Failed}`, structured
  diagnostics, per-file analysis records, and aggregate status helpers. Legacy reports without provenance
  deserialize to `Unknown` with `analysis-unknown`; only explicit `Complete` with no diagnostics permits
  rewrites.
- Added deterministic Tree-sitter error/missing-node collection. Shared malformed fixtures now lock exact
  evidence: `malformed.ts` lines 2–2/bytes 62–63 and `malformed.tsx` lines 1–2/bytes 0–96.
- Quarantined partial/failed files before analyzer rules or external analyzers. Registered no-grammar packs
  retain downgraded `never-auto` text evidence; project-wide duplication/config-boundary passes run only
  for a complete requested snapshot.
- Bumped public read/report contracts to `deslop.findings/2`, `deslop.metrics/4`, `deslop.graph/2`, and
  `deslop.slim/2`. Metrics retain complete-file read-only regions in mixed scans but suppress project
  candidates/hotspots and serialize aggregate scores as null. Graph retains a partial file node only,
  publishes typed provenance, and renders notices in JSON and DOT.
- Text/JSON/SARIF expose stable parse diagnostics. CLI scan/metrics/graph/slop read-only output exits 2
  when incomplete; agent/propose output is atomic and never overwrites an existing work-order file.
  Baseline write/update and deterministic safe-fix/diff refuse incomplete analysis.
- LSP stores provenance, publishes exact parse diagnostics, and offers no quick-fix or fix-all action for
  incomplete documents.
- MCP propose/fix return successful structured domain blocks with `analyses`, `blocked_files`, and zero
  work orders/prompts. Slim preflights auto-discovered and imported JSONL work orders before consent,
  credentials, or model construction; blocked runs make zero LLM calls and no writes even with allow flags.
- Protocol work-order generation rechecks current source provenance and source/report identity. Verifier
  target rediscovery cannot create an order for an incomplete target; `allow_non_removable` cannot turn
  that rejection into a write. `VerifyOptions` and MCP gained an optional analysis `scope` so rediscovery
  can use the original requested paths instead of an expensive whole-repository scan.
- Updated SPEC, README, MCP tool descriptions, and `.agents/TODO.md`; M0.9 is now **NEXT**.

**Commands/checks run:** before/after CLI scan/metrics/graph/propose probes on the shared malformed TS
fixture; targeted core/parse/analyzer/metrics/graph/protocol/report/LSP/slim/verify tests; MCP default tests;
`cargo fmt --all --check`; `git diff --check`; `cargo build --workspace`;
`cargo build -p deslop-slim --no-default-features`; `cargo test --workspace`;
`cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`; and
`cargo clippy --workspace -- -D warnings`. Hindsight checkpoint and negative memories were written and
consolidated.

**Verification results:** PASS. Workspace: 251 tests plus doc-tests. Feature-enabled MCP: 22 tests.
Formatting, whitespace, workspace build, no-default-features slim build, and warnings-denied clippy passed.
Measured malformed TS after-state: exit 2; one diagnostic; 0 findings; 0 metric regions/candidates/hotspots;
null health/readability scores; graph 1 file/0 symbols/0 edges; proposal stdout contains 0 work orders.

**Failed iterations / invalidated assumptions:** the first draft made missing serde provenance default to
`Complete`, which was fail-open; it now defaults to `Unknown`. Per-file write gating alone was invalidated
for project-derived absence/relative facts, so incomplete requested snapshots suppress those passes and all
rewrite-capable proposal output. A global verifier-root completeness gate was also invalidated: workorder/1
does not persist the original scope, and repositories intentionally contain malformed fixtures. Verification
therefore rechecks target provenance and accepts an explicit scope; persisting the complete originating
snapshot remains M0.13/M6. Killed timeout probes left five temporary fixture directories, which were removed
after confirming they were generated by this session.

**Residual semantic boundary:** M0 quarantines the whole recovered file; valid subtrees and trusted-byte
coverage wait for M1's owned syntax snapshot/M2 adapter facts. Proposal surfaces enforce global completeness
for their requested paths, while imported workorder/1 verification can only recheck its target unless the
caller supplies `scope`. Analyzer config, capability, source revision, and scope must travel in the future
work-order contract before verifier reconstruction can claim original-snapshot equivalence.

**Current recommendation/checkpoint:** execute M0.9 next: remove or relabel uncalibrated health,
readability, and refactor-confidence gates without weakening the new partial-analysis authority.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust reads
were the active fallback.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries because findings, metrics, graph, and slim
schema versions changed. No data migration or configuration change is required; MCP clients may optionally
pass `scope` for bounded verifier rediscovery.

**Negative-memory status:** durable memory rejects fail-open missing provenance, per-file-only authority for
project-derived facts, and whole-verifier-root completeness claims without persisted scope. Hindsight
consolidation passed. Recheck under M1/M2 owned syntax facts and M0.13/M6 versioned work-order context.

**Signature:** Codex (GPT-5), M0.8 integration owner, 2026-07-13.

---

## M0.9 checkpoint — remove uncalibrated metric authority

**Date/time:** 2026-07-13T14:59:34+02:00

**Objective:** complete M0.9 by replacing the uncalibrated health/readability/refactor-confidence
contract with honest, evidence-only metric output while preserving M0.8 fail-closed provenance.

**Target:** `deslop-metrics`, CLI command discovery, MCP payload/discovery contracts, README/SPEC,
durable roadmap, numerical regressions, and jj history. `/root` owned all writes and final integration;
three read-only agents audited the implementation surface, contract tests, and validation matrix.

**Changes:**

- Bumped the breaking metric wire contract to `deslop.metrics/5`. Removed `health_score`,
  `readability_score`, `readability_model`, confidence bands, the absolute `0.50` threshold,
  `refactor_candidates`, and `refactor_confidence_distribution`; no compatibility aliases remain.
- Replaced the old region container with `heuristic_burden` under model
  `deslop-heuristic-burden/1`. Machine-readable metadata is `experimental=true`,
  `human_calibrated=false`, `authority="triage_only"`, and `gating_permitted=false`.
  `measurement_support` describes token/CST sample support rather than measured correctness.
- Kept scan-local z-scores/percentiles only as `heuristic_outliers`. There is no raw-score OR gate;
  cohorts below eight regions and flat/tied cohorts cannot emit outliers. Mixed partial scans keep
  intrinsic complete-file facts but serialize the project distribution and `repo_relative` as null,
  render `n/a`, emit zero outliers/hotspots, and retain CLI exit 2.
- Corrected adjacent `/5` measurement labels: `compression_ratio` is now zero-order
  `byte_entropy_bits_per_byte` in real `0..8` units and is no longer given a universal low-is-bad
  hotspot direction; Halstead `effort` is now the conventional `lexical_effort` formula.
- Removed the CLI `health` alias. Updated live MCP tool discovery, the duplicate MCP spec source,
  MCP payload tests, README, SPEC migration guidance, and `.agents/TODO.md`.
- Preserved M0.8 as distinct parent `nxlxzzws` and reconstructed M0.9 as child `oyrxxokr`; M0.8
  history was not collapsed or rewritten by the M0.9 content.

**Commands run:** focused `cargo check/test` for metrics, MCP, and CLI; clean/sloppy/malformed CLI
JSON/text probes with `jq`; removed-alias exit probe; `deslop slop` invariance probe;
`cargo fmt --all --check`; `git diff --check`; `cargo build --workspace`;
`cargo build -p deslop-slim --no-default-features`; `cargo test --workspace`;
`cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`; and
`cargo clippy --workspace -- -D warnings`.

**Results:** PASS after the recorded corrections. Workspace: 254 tests plus doc-tests. Feature-enabled
MCP: 22 tests. Formatting, whitespace, workspace build, no-default-features slim build, and
warnings-denied clippy passed. Exact formula scores are `0.06968888888888888`,
`0.18417777777777777`, `0.37495233115468407`, and `0.5394771590413944`. The synthetic outlier
distribution `[0.10 × 9, 0.30]` remains mean `0.12`, stddev `0.06`, z `3`, percentile `1`; tied and
sub-eight cohorts emit no outlier. A four-region fixture has raw burden `0.7970946844830109` and still
emits zero outliers, proving removal of the absolute gate.

Measured corpus output preserves evidence but not authority: clean is 30 regions/3 scan-local
outliers, mean `0.038917726028306614`, stddev `0.05291790403358435`; sloppy is 38/4, mean
`0.054624429903073535`, stddev `0.07069575147135337`. Both emit `/5` and zero legacy authority keys.
The independent slop detector remains `0.819672131147541` clean versus `60.32388663967611` sloppy.
Malformed TypeScript emits `/5`, `partial`, null distribution, 0 regions/outliers/hotspots, exact
line-2 bytes 62–63 diagnostic, and exit 2. `deslop health --help` is rejected with exit 2.

**Failure modes / invalidated assumptions:** the first broad gate found one warnings-denied clippy
`let_and_return`; the byte-entropy helper now returns the expression directly and the full gate was
rerun. `cargo test -p deslop-cli --lib` was an invalid command because the package has only a binary;
the correct `--bin deslop` target passed. More importantly, clean health `40.43731597021308` versus
sloppy health `45.288553975740356`, plus three clean relative-only “refactor candidates,” invalidates
the assumption that hand-set formula burden or scan-relative unusualness can authorize health,
readability, refactor need, confidence, or safety.

**Current recommendation/checkpoint:** M0.9 is complete. Execute M0.10 next by moving the exact
clean/sloppy, performance, duplicate-order, and false-resolution live probes from
`.agents/ALGORITHM_AUDIT.md` into automated regression suites, then run M0.11's recorded full gate.

**Blockers:** none. Serena remains configured as Python-only for this Rust workspace, so targeted local
Rust inspection remains the fallback.

**Next actions:** automate the M0.10 probes without turning clean/sloppy corpus burden ordering into a
readability calibration gate; retain current numerical measurements as schema/invariance evidence only.
M8.3 still owns CFG-based complexity and estimator-label replacement, and M8 owns any future
human-readable label after held-out calibration.

**Dependencies/restart:** rebuild or reinstall CLI/MCP binaries and migrate `/4` clients explicitly to
`/5`; removed health/readability/refactor fields have no replacement. No repository data migration or
configuration change is required.

**Negative-memory status:** Hindsight now supersedes the older recommendation to preserve the absolute
`0.50` threshold. No metric threshold may regain readability/refactor authority before M8 held-out
human calibration beats frozen size/simple baselines with acceptable calibration and confidence
intervals. Search handles: `metrics/5 heuristic_burden health reversal triage_only gating_permitted`.

**Signature:** Codex (GPT-5), M0.9 integration owner, 2026-07-13.

---

## M0.10 checkpoint — automate algorithm contract probes

**Date/time:** 2026-07-13T15:17:38+02:00

**Objective:** complete M0.10 by moving the clean/sloppy, parse-performance, duplicate-order,
aggregation, and false-resolution probes from the algorithm audit into deterministic regression
suites without converting unstable wall time or corpus-derived totals into false semantic authority.

**Target:** parse instrumentation, metrics invariants, CLI corpus integration tests, graph resolution,
slim aggregation, SPEC/audit documentation, durable roadmap, and jj checkpoint. `/root` owned all
writes and final integration; three read-only agents audited contract, core, and integration surfaces.

**Changes:**

- Added an honest clean/sloppy CLI contract for `deslop.metrics/5`: complete provenance, experimental
  triage-only metadata, no removed health/readability/refactor-confidence keys, and exact independent
  slop-density snapshots (`0.819672131147541` clean, `60.32388663967611` sloppy).
- Added thread-local `parse_source` invocation instrumentation and locked the current amplification:
  the five-region Python behavioral fixture makes eight source parses (`R + 3`). Added a relational
  regression proving that adding 20 trivial helpers does not change the target region's intrinsic
  complexity, expressivity, Halstead, or heuristic-burden evidence. One parse per file remains M1.
- Strengthened workorder regressions to require 28 unique target regions and IDs while conserving all
  62 current sloppy-corpus findings. Repeated, overlapping, reordered, and equivalent path inputs
  serialize identically. The largest Rust region contains exactly 11 merged findings: one long-method,
  nine near-duplicate, and one let-and-return finding.
- Locked the false-resolution probes: `compact_label` has two definitions and ten syntactic calls, each
  targeting the same caller file; the corpus graph is 21 files, 74 symbols, and 197 edges with no
  non-containment `resolved` claims and no false `require`/`:require` calls.
- Strengthened slim aggregation so its single rewrite prompt retains all 11 rule-evidence entries; the
  existing verifier regression confirms the grouped patch verifies and applies once.
- Added an explicitly ignored self-scan probe. At this checkpoint it measured 39 metric files, 1,745
  regions, 5,715 graph nodes, and 13,392 edges; metrics took 48.217533591s and graph 1.769094036s.
  The probe gates only stable schema/structural facts and logs source-tree-dependent counts and time.
- Marked the audit's 31-unique-ID value as historical pre-M0.1 evidence, documented the current
  conservation contract in the audit/SPEC, completed M0.10 in the roadmap, and made M0.11 **NEXT**.

**Commands run:** focused parse-amplification and helper-invariance metrics tests; CLI algorithm,
workorder, and graph integration suites; slim and verify aggregation tests; the ignored crates
performance probe with `--nocapture`; `cargo fmt --all --check`; `git diff --check`; and
`cargo test --workspace`.

**Results:** PASS. All seven focused commands passed. The workspace has 259 passing tests plus one
intentional ignored performance probe and passing doc-tests. Formatting and whitespace checks passed.
The measured operation-count contract is exactly eight `parse_source` calls for five metric regions;
the current workorder and graph snapshots match the numbers above.

**Failure modes / invalidated assumptions:** an initial counter implementation used unstable
`LocalKey::update` and failed with E0658 on the repository toolchain; it was replaced by stable
`LocalKey::with` plus `Cell::get/set`. The slow probe initially froze exact self-scan counts, but adding
its own source changed those counts; source-tree totals and elapsed time are now recorded rather than
gated. Finally, treating the historical 31 unique workorder IDs as current was invalidated by the
post-grouping analyzer/region semantics, which now yield 28 unique targets while conserving all 62
findings.

**Current recommendation/checkpoint:** M0.10 is complete. Execute M0.11 next: rerun focused checks and
the complete fmt/build/test/slim-feature/clippy matrix, then record the measured before/after evidence.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust reads
remain the fallback.

**Next actions:** run the exhaustive M0.11 release gate without treating the intentionally ignored
performance probe as part of the default suite. If code or fixtures change, rerun that probe explicitly
and interpret count/time deltas rather than mechanically updating a threshold.

**Dependencies/restart:** rebuild test and CLI binaries to include the instrumentation and contract
suites. No runtime restart, data migration, public schema migration, or configuration change is needed.

**Negative-memory status:** Hindsight records that 31 unique IDs is historical pre-grouping evidence,
not a timeless gate. Current authority is 28 unique targets plus 62-finding conservation and exact
overlap/order invariance; recheck the snapshot after analyzer rules, packs, region boundaries, fixtures,
or workorder schema change. Search handles: `M0.10 sloppy corpus 62 findings 31 historical 28 current`.

**Signature:** Codex (GPT-5), M0.10 integration owner, 2026-07-13.

---

## M0.11 checkpoint — exhaustive release gate and measured after-state

**Date/time:** 2026-07-13T15:21:44+02:00

**Objective:** complete M0.11 by running focused algorithm contracts before the full build/test/feature/
clippy matrix and recording numerical before/after evidence without conflating semantic corrections,
source-tree growth, and runtime performance.

**Target:** M0.1–M0.10 integrated workspace, default and minimal-feature builds, feature-enabled MCP,
durable audit/roadmap evidence, and jj checkpoint. `/root` owned final validation and documentation.

**Changes:** no production or test code changed in M0.11. Added the dated M0 release-gate after-state
table to `.agents/ALGORITHM_AUDIT.md`, completed M0.11 in `.agents/TODO.md`, and marked M0.12 **NEXT**.

**Commands run:** `cargo test -p deslop-metrics`; CLI `algorithm_contracts`, `propose_workorders`, and
`graph_resolution` suites; focused slim/verify aggregation tests; `cargo fmt --all --check`;
`git diff --check`; `cargo build --workspace`; `cargo build -p deslop-slim --no-default-features`;
`cargo test --workspace --quiet`; `cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`;
`cargo clippy --workspace -- -D warnings`; and live JSON/JQ graph, metrics, slop, and workorder probes.

**Results:** PASS. Focused results: 20 metrics, 1 default algorithm contract with one intentional
ignored slow probe, 5 workorder, 3 graph, 1 slim aggregation, and 1 verifier aggregation test.
Workspace: 259 passing tests, one intentional ignored performance probe, and passing doc-tests.
Feature-enabled MCP: 22 passing tests. Formatting, whitespace, workspace build, minimal-feature slim
build, and warnings-denied clippy all pass.

The current live after-state is: clean/sloppy metrics `/5`, 30/38 regions, 3/4 triage-only outliers,
and no health field or gating permission; independent slop scores `0.819672131147541` and
`60.32388663967611`; 28 unique work orders/targets conserving 62 findings with a largest merged group
of 11; and a crates graph of 39 files, 2,134 symbols, and 13,392 edges with zero resolved
non-containment edges. The M0.10 ignored self-scan measured 1,745 metric regions in
48.217533591s and graph construction in 1.769094036s. The stable operation-count probe remains eight
`parse_source` calls for five behavioral regions.

**Before/after interpretation:** the original audit had 179 passing tests, 1,556 metric regions in
30.50s, 10,872 graph edges with 4,203 resolved claims in 0.74s, reversed clean/sloppy health
`40.38`/`46.14`, and duplicate workorder output. M0 removes the unsound health and graph authority and
groups workorders, but does not claim a speedup: the source/supported-language surface is larger and
metrics still reparse per region. Exact before/after values and qualitative TypeScript/Clojure fixes
are recorded in `.agents/ALGORITHM_AUDIT.md`; M1 owns one-parse performance.

**Failure modes / invalidated assumptions:** no M0.11 gate failed. The interpretation explicitly
rejects using self-scan wall time or total region/edge counts as a controlled benchmark across a
changing source tree. Passing a larger test count is evidence of coverage, not proof by itself; the
operation-count and semantic corpus contracts remain authoritative.

**Current recommendation/checkpoint:** M0.11 is complete. Execute M0.12 next by separating exact-byte
write authorization from the trimmed cross-revision baseline fingerprint, migrating identifiers
explicitly, and proving boundary-whitespace changes make stale writes fail closed.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust reads
remain the fallback.

**Next actions:** audit every `fingerprint`/`region_fingerprint` producer and consumer before editing;
define which bytes authorize writes versus which normalized identity supports cross-revision matching;
then add protocol, verifier, slim, CLI/MCP, and baseline migration regressions as required by the
user-visible contract.

**Dependencies/restart:** no runtime restart or migration is needed for M0.11 because it changes only
durable evidence. M0.12 will require an explicit schema/identifier migration decision before rollout.

**Negative-memory status:** the M0.10 historical-31 correction remains active. M0.11 adds the durable
constraint that source-tree self-scan time and totals cannot be interpreted as before/after performance
without a controlled fixture; use operation counts and a fixed benchmark corpus instead. Search handles:
`M0.11 release gate self-scan uncontrolled timing 8 parses 5 regions`.

**Signature:** Codex (GPT-5), M0.11 integration owner, 2026-07-13.

---

## M0.12 checkpoint — separate exact write guards from normalized identity

**Date/time:** 2026-07-13T15:49:42+02:00

**Objective:** complete M0.12 by preserving the existing trimmed finding/baseline identity while
introducing a distinct exact-byte revision guard for every rewrite-capable path, explicitly migrating
region/workorder IDs and wire schemas, rejecting boundary-whitespace staleness, and closing the
verify-to-write target-byte recheck gap.

**Target:** core identity APIs, analyzer/external finding producers, protocol region/workorder/patch/
characterization schemas, verifier/characterization/apply, slim import and egress, CLI, MCP live and
duplicate specs, README/SPEC, roadmap, dependency lock, and regression suites. `/root` owned all writes
and final integration; three read-only agents audited core, contract/test, and end-to-end surfaces.

**Changes:**

- Renamed the existing helper to `baseline_fingerprint` without changing its FNV64 algorithm or
  trimmed text/path/line inputs. Finding, baseline, feedback, and analyzer behavior retain the same
  best-effort cross-revision matching identity, explicitly without write authority.
- Added serde-transparent `RevisionGuard`, built as `rg1_<byte-length>_<digest>` using BLAKE3 derive-key
  domain separation over normalized path, exact line and byte range, and untrimmed UTF-8 target bytes.
  The standard library has no stable cryptographic digest; official BLAKE3 Rust docs were checked via
  Context7, and maintained `blake3 1.8.5` is the only new direct dependency.
- Migrated to WorkOrder/2 with exact `start_byte`/`end_byte`, matching-only `region_fingerprint`,
  proposal-time `revision_guard`, and explicit `wo2_` correlation IDs. Imported workorders must pass
  schema, ID, normalized fingerprint, and exact guard consistency checks.
- Migrated Patch/2 and CharacterizationTest/2 to mandatory `revision_guard` with no
  `region_fingerprint` alias/default. Legacy `/1` write artifacts are rejected with regeneration
  guidance. MCP envelopes are `deslop.workorders/2` and `deslop.fix/2`; SlimReport is `deslop.slim/3`.
- Verifier public APIs now validate schemas even for programmatic/MCP inputs, compare the submitted
  proposal guard with the newly current exact guard, retain the scan/read byte consistency check, and
  carry expected exact region bytes into `PreparedPatch`. Apply rechecks those bytes immediately before
  replacement and aborts the whole write on mismatch.
- Slim validates imported workorder identity, confines its canonical path to the configured root,
  compares serialized proposal bytes to current disk before any LLM egress, and emits only Patch/2 and
  CharacterizationTest/2. MCP schemas/prompts expose both matching identity and exact guard and require
  callers to copy the guard verbatim.
- Updated README, SPEC, both MCP schema sources, tests, and `.agents/TODO.md`; M0.13 is now **NEXT**.

**Commands run:** targeted Hindsight recall/search; official BLAKE3 Context7 resolution/docs query;
`cargo check -p deslop-protocol`; `cargo check --workspace`; repeated all-target no-run compilation;
focused core/protocol, verifier guard/legacy/characterization/pre-write, slim, CLI revision-guard,
workorder, MCP default/boundary tests; `cargo test --workspace --quiet`;
`cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`;
`cargo build --workspace`; `cargo build -p deslop-slim --no-default-features`;
`cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`; and `git diff --check`.

**Results:** PASS. Workspace: 269 passing tests plus one intentional ignored performance probe and
passing doc-tests. Feature-enabled MCP: 22 passing tests. Workspace/minimal-slim builds, formatting,
whitespace, and warnings-denied clippy pass. Six fixed-path boundary mutations—leading space/tab,
trailing space/tab, final-LF removal, and LF→CRLF—retain the normalized `region_fingerprint` and
`wo2_` ID, produce a different `revision_guard`, reject verify/characterization/apply, make zero writes,
and preserve the changed source. CLI and MCP round trips prove the same behavior; baseline/1 still
suppresses the matching finding across outer whitespace.

**Failure modes / invalidated assumptions:** the first broad protocol patch had a stale context and was
rejected atomically, so it was reapplied in reviewed increments. One focused `cargo test` invocation
incorrectly passed three filters; Cargo rejected the command and each filter was rerun separately.
The substantive invalidation is that the old post-rescan byte comparison was current-vs-current and
therefore tautological for proposal freshness. A normalized fingerprint cannot authorize writes, and a
verification result cannot be written later without rechecking the target bytes.

**Current recommendation/checkpoint:** M0.12 is complete. Execute M0.13 next: persist proposal analyzer
config, capability/provenance, requested scope, and source-revision context so verifier reconstruction
matches the originating workorder set rather than silently using defaults.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust reads
remain the fallback.

**Next actions:** design WorkOrder/3 or a versioned proposal-context envelope without collapsing M1's
future ProjectSnapshotId/NodeKey contract into M0. Persist enough canonical analyzer configuration and
scope to make current reconstruction deterministic, reject missing legacy context fail closed, and add
suppression/threshold/capability round-trip regressions before touching NeverAuto policy in M0.14.

**Dependencies/restart:** rebuild/reinstall CLI, MCP, and any protocol consumers. Workorder/1, Patch/1,
CharacterizationTest/1, MCP workorders/1/fix/1, Slim/2, and `wo_` IDs are intentionally incompatible;
regenerate outstanding proposals and patches. Baseline/1 files and finding fingerprints require no
migration.

**Negative-memory status:** Hindsight records that fuzzy/trimmed identity can never authorize a write,
current-vs-current consistency does not prove proposal freshness, and apply must recheck exact expected
bytes. Recheck under M1.4's ProjectSnapshotId/NodeKey migration and on any digest/path/range/transaction
change. Search handles: `M0.12 text.trim boundary whitespace stale revision_guard wo2 patch/2`.

**Signature:** Codex (GPT-5), M0.12 integration owner, 2026-07-13.

---

## M0.13 checkpoint — persist proposal reconstruction context

**Date/time:** 2026-07-13T16:30:39+02:00

**Objective/target:** make verify, apply, characterization, and imported slim workorders reconstruct
the exact originating proposal instead of rescanning an unrelated scope with
`AnalyzerConfig::default()`. Keep M0.14's `NeverAuto` policy and M1's owned syntax snapshot out of
scope.

**Changes:**

- Added canonical serializable effective analyzer settings, including all thresholds and language
  overrides, declarative suppression, boundary configuration, Rust/Julia external selection, and
  root-relative Julia project identity. Suppression now retains its canonical raw semantics and
  applies globs relative to the proposal root even when the scanner reads absolute paths.
- Added analyzer scan context with source text captured from the same read that produced findings,
  non-skipped boundary artifacts captured from their analysis read, and per-target external analyzer
  name/availability/covered-rule observations. Proposal emission rechecks captured bytes before
  returning.
- Added self-contained `deslop.proposal-context/1`: analyzer semantics version, canonical
  root-relative deduplicated file/directory scope, effective analyzer settings, baseline exclusions,
  all consulted source/config exact revisions plus language/provenance, external capability
  observations, and a deterministic context-free work-order-set digest. Context and set identities
  use domain-separated BLAKE3. Root escapes, noncanonical paths, scope-kind changes, tampering, and
  mixed contexts fail closed.
- Migrated to context-bound `wo3_` IDs and required WorkOrder/3, Patch/3, and
  CharacterizationTest/3 records. MCP envelopes are workorders/3 and fix/3; slim reports are /4.
  Legacy /1-/2 authority records are rejected with no alias or default-filled migration.
- Replaced verifier default rescans with context reconstruction. Runtime scope is only an equality
  assertion; it cannot override persisted scope/config. Reconstructed source membership, exact
  bytes, parser provenance, external capability, work-order set, target region, and context-bound ID
  must all match before normal verification. Loaded slim workorders perform the same reconstruction
  before consent, credentials, LLM egress, check commands, or writes.
- Wired the shared proposal path through CLI propose and `scan --format agent` (including persisted
  baseline exclusions), MCP propose/fix/verify/apply/characterization, slim auto/imported flows, and
  report rendering. Updated README, SPEC, active/duplicate MCP schemas, tests, and TODO; M0.14 is
  **NEXT**.
- Updated seven pre-existing test-only expressions for Rust 1.94 warnings-denied clippy
  (`useless_vec`, owned path comparisons, and cloned one-element slices); no production behavior
  changed in those cleanups.

**Commands/checks run:** focused analyzer/protocol/report/verifier/slim/CLI/MCP tests; CLI proposal
and revision-guard integrations; `cargo test --workspace`; `cargo test -p deslop-mcp --features
slim-llm -- --test-threads=1`; `cargo build --workspace`; `cargo build -p deslop-slim
--no-default-features`; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets
--all-features -- -D warnings`; and `git diff --check`.

**Verification results:** PASS. Workspace: 274 passing tests plus one intentional ignored performance
probe and all doc-tests. Feature-enabled MCP: 22 passing tests. Workspace build, no-default-features
slim build, formatting, whitespace, and all-feature/all-target warnings-denied clippy pass. Numerical
regressions prove non-default long-method configuration reconstructs without caller overrides;
relative baseline fingerprints survive rooted absolute scanning; peer-source mutation, source
boundary whitespace, context tampering, root escape, scope drift, mixed contexts, and legacy schemas
reject; slim stale imports make zero model calls; apply makes zero writes on expired context.

**Failure modes / invalidated assumptions:** persisting only target bytes and caller-supplied scope
was invalidated because cross-file duplication and boundary findings depend on the complete requested
input set. Persisting config alone was invalidated because clj-kondo/clippy/Julia availability can
change the work-order set. Scanning canonical absolute paths initially invalidated suppression and
baseline identity semantics; suppression is now explicitly proposal-root-relative and baseline
exclusions match their canonical root-relative fingerprints. Treating source-revision drift as a
per-patch structured rejection was invalidated: it expires the whole proposal context before lookup,
so verify/apply/characterization return a terminal operational context mismatch. One feature-enabled
MCP path initially paired an inferred root with unresolved relative paths; all inferred-root paths are
now canonicalized together. Rust 1.94 added warnings in unrelated existing test expressions; the
warnings-denied gate required the mechanical cleanups recorded above.

**Current recommendation/checkpoint:** M0.13 is complete. Execute M0.14 next: choose and enforce one
`NeverAuto` report/proposal policy across every producer and consumer, with an end-to-end regression,
without weakening proposal-context reconstruction.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust
reads remained the fallback.

**Dependencies/restart:** rebuild/reinstall CLI and MCP binaries. Outstanding workorder/2, patch/2,
characterization-test/2, MCP workorders/2/fix/2, slim/3, and `wo2_` artifacts must be regenerated.
Baseline/1 and finding fingerprints remain compatible. No new third-party package was introduced;
the protocol reuses the existing workspace BLAKE3/serde/anyhow stack.

**Negative-memory status:** the durable constraint is now implemented: no verifier default rescan,
caller scope, normalized target identity, or target-only provenance can stand in for the originating
proposal. Recheck only when M1 introduces ProjectSnapshotId/NodeKey ownership or M2 replaces the M0
capability observation schema.

**Signature:** Codex (GPT-5), M0.13 integration owner, 2026-07-13.

---

## M0.14 checkpoint — enforce `NeverAuto` as report-only

**Date/time:** 2026-07-13T16:50:27+02:00

**Objective/target:** resolve the safety-lattice contradiction in which SPEC defined `NeverAuto` as
report-only while proposal generation admitted every class except `SafeAuto`. Preserve findings on
read-only surfaces while denying all proposal, prompt, characterization, verification, and write
authority, including mixed-region and override cases.

**Changes:**

- Added the fail-closed `SafetyClass::permits_proposal` allowlist for `AnalyzerConfirmed`,
  `SafeWithPrecondition`, `RiskySuggest`, and `LlmOnly`. `SafeAuto` stays deterministic and
  `NeverAuto` stays evidence-only. Canonical rule metadata now labels `missing-reference`,
  `julia-jet`, and boundary rules report/review-only, with an invariant forbidding proposal/fix
  defaults for every `never-auto` rule.
- Proposal generation now quarantines any complete candidate rewrite region whose replacement span
  overlaps `NeverAuto` evidence. This includes nested evidence and zero-width point diagnostics;
  disjoint regions remain eligible. WorkOrder identity validation rejects empty or non-proposable
  findings, and report/slim prompt builders validate before serialization or source egress.
- Bumped proposal reconstruction semantics from `deslop-analyzer/1` to `/2` in protocol, SPEC, and
  both MCP schemas. `/1` contexts expire explicitly; wire shapes remain proposal-context/1,
  workorder/3, patch/3, characterization-test/3, MCP workorders/3/fix/3, and slim/4.
- CLI `propose` and `scan --format agent`, MCP propose/fix, slim auto/import flows, verifier/apply and
  characterization inherit the shared policy. MCP complete scans with no eligible regions return an
  explicit no-proposal next action. Slim prompts now include each eligible finding's safety class.
- Deterministic fix and LSP regressions prove that even an injected `NeverAuto` finding carrying a
  syntactically valid edit cannot write or produce a code action. Verifier regression proves
  `allow_non_removable`, characterization mode, and a check command cannot widen report-only
  authority: the command is not run and source bytes are unchanged.
- Read-only JSON continues to carry every finding. SARIF now carries result-level `safety` and
  `reportOnly`; a rule with mixed per-finding safety is labeled `per-finding` instead of silently
  taking the first observed class. README, SPEC, TODO, runtime/duplicate MCP descriptions, and tests
  were updated.

**Commands/checks run:** targeted core/protocol/CLI/MCP/slim/verifier/fix/LSP/report tests; `cargo
check -p deslop-slim -p deslop-mcp -p deslop-report -p deslop-verify -p deslop-lsp -p deslop-fix`;
`cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`;
`cargo build --workspace`; `cargo build -p deslop-slim --no-default-features`; `cargo fmt --all
-- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; and `git diff
--check`.

**Verification results:** PASS. Workspace: 288 passing tests plus one intentional ignored performance
probe and all doc-tests. Feature-enabled MCP: 23 passing tests. Workspace and no-default-features slim
builds, formatting, whitespace, and all-feature/all-target warnings-denied clippy pass. The supported
Julia `config-key-unconsumed` fixture numerically proves one `never-auto` scan finding with zero CLI
proposal/agent records, zero MCP workorders/prompts, zero slim egress regions/model calls/patches, zero
verifier check-command execution, and zero writes under widening. Protocol tests prove pure, mixed,
nested, disjoint, zero-width, mutated identity, and legacy-semantics cases.

**Failure modes / invalidated assumptions:** the first E2E used `config-key-unread` on a generic TOML
artifact; parser provenance already blocked that path before M0.14, so it could not distinguish the
bug and was replaced with a parse-complete Julia-source finding. Merely filtering `NeverAuto` out of
a mixed WorkOrder was invalidated because the patch still replaces and exports the entire enclosing
region; report-only evidence is therefore absorbing for every overlapping target. A negated denylist
was invalidated as fail-open for future safety variants and replaced by an explicit allowlist. SARIF's
first-safety-per-rule metadata was invalidated because safety is per finding. One focused Cargo command
incorrectly passed two test filters and was rejected; the full core suite then passed.

**Current recommendation/checkpoint:** M0.14 is complete. Execute M0.DoD next with one corpus-level,
numerical demonstration of duplicate-ID count, ambiguous-edge resolution count, grammar selection,
and partial/capability honesty; do not reopen proposal-policy tuning unless a new safety class or
subregion/protected-span authority model is introduced.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust
reads remained the fallback.

**Dependencies/restart:** rebuild/reinstall CLI, LSP, MCP, and library consumers. Outstanding
proposal contexts and workorders carrying `deslop-analyzer/1` must be regenerated; wire schema numbers
did not change. No new third-party dependency was introduced.

**Negative-memory status:** Hindsight should retain that filtering a report-only finding from a
region does not remove rewrite authority over its bytes; region replacement requires the safety join
of all overlapping evidence, with `NeverAuto` absorbing. Also retain that unsupported/generic
provenance fixtures cannot prove a proposal-filter regression. Recheck only when proposals gain
protected subspans or a safety class is added.

**Signature:** Codex (GPT-5), M0.14 integration owner, 2026-07-13.

---

## M0.DoD checkpoint — numerical M0 contract snapshot

**Date/time:** 2026-07-13T17:00:54+02:00

**Objective/target:** close M0 with one convergent, executable demonstration of workorder uniqueness,
graph authority, grammar selection, and partial/external-capability honesty. This checkpoint adds no
production behavior and does not claim that empirical ID uniqueness is a collision-proof identity
construction.

**Changes:** added `crates/deslop-cli/tests/m0_definition_of_done.rs`, a public-CLI integration
snapshot over the M0 corpus and focused fixtures. It measures proposal cardinality, unique IDs and
targets, grouped findings, corpus graph authority counts, a genuine duplicate-qualified-name
ambiguity, the former `compact_label` false-resolution probe, typed TS/TSX/JSX completeness,
malformed TS/TSX partial scan/metrics/graph behavior, and a deterministic empty-environment JET
capability observation. The CLI grammar proof is explicitly paired with the parser AST-sentinel
truth table covering `jsx_element`, `type_annotation`, wrong-grammar rejection, `.mts`, and `.cts`.
Updated TODO to complete M0 and select M1.1.

**Commands/checks run:** live CLI `propose`, `graph`, `scan`, `metrics`, and external-capability
probes; focused `cargo test -p deslop-cli --test m0_definition_of_done -- --nocapture`; focused
`cargo test -p deslop-parse selects_javascript_typescript_and_tsx_grammars_by_dialect -- --nocapture`;
`cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`;
`cargo build --workspace`; `cargo build -p deslop-slim --no-default-features`; `cargo fmt --all
-- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; and `git diff
--check`.

**Verification results:** PASS. The DoD snapshot reports 30 workorders, 30 unique IDs, 30 unique
targets, and 65 grouped findings. The corpus graph reports 21 files, 74 symbols, 197 edges, 123
syntactic reference edges, and zero non-containment resolved edges. A synthetic duplicate
`Alpha::ping` fixture produces exactly one ambiguous edge, an external-symbol placeholder, and zero
false resolution; the live `compact_label` probe has two definitions, ten calls, and zero resolved
calls. Three dialect fixtures are complete with zero diagnostics; two malformed typed fixtures are
partial with zero findings, zero metric regions/outliers, and zero graph symbols/edges. An isolated
JET project yields exactly one persisted capability observation with `available=false` and three T1
fallback workorders. Workspace: 289 passing tests plus one intentional ignored performance probe and
all doc-tests. Feature-enabled MCP: 23 passing tests. Builds, formatting, whitespace, and strict
all-target/all-feature clippy pass.

**Failure modes / invalidated assumptions:** counting zero resolved edges in the ordinary corpus and
the syntactic `compact_label` probe did not exercise the `ambiguous` state; that proof was invalidated
and replaced with an actual duplicate-qualified-name fixture. CLI language labels plus successful
parsing alone do not prove the selected grammar; the DoD proof therefore composes with the existing
AST-sentinel and negative-grammar truth table. An environment-dependent JET probe was made
deterministic by activating an empty project and restricting `JULIA_LOAD_PATH` to that project and
stdlib.

**Current recommendation/checkpoint:** M0 is complete. Begin M1.1 with an ADR that preserves the
distinctions already proven in M0: exact revision guards versus baseline identity, syntax ownership
versus serialized keys, complete/partial authority, per-path grammar selection, external capability
observations, and graph evidence versus binding proof.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust
reads remain the fallback.

**Dependencies/restart:** none; this checkpoint adds only an integration test and durable artifacts.
No production rebuild is required beyond normal validation.

**Negative-memory status:** a zero count on syntactic edges is not evidence that the ambiguous branch
works; DoD tests must create the authority state they claim to validate. Also, successful typed input
does not alone prove grammar selection without AST sentinels and wrong-grammar controls. Recheck if
graph confidence states, grammar dispatch, corpus membership, or workorder grouping change.

**Signature:** Codex (GPT-5), M0.DoD integration owner, 2026-07-13.

---

## M1.1 checkpoint — revision-bound ProjectAnalysis ADR

**Date/time:** 2026-07-13T17:25:52+02:00

**Objective/target:** make the M1 ownership boundary implementable before introducing the source
store. Define `ProjectAnalysis`, exact source and grammar revisions, local/wire identity domains,
invalidation, concurrency, partial-analysis authority, and every consumer migration without weakening
the completed M0 contracts.

**Changes:** added `docs/adr/0001-project-analysis.md` and completed M1.1 in `.agents/TODO.md`. The ADR
places the immutable source/syntax substrate in `deslop-parse`; centralizes repository root,
`RepositoryId`, scope, alias, and atomic `GrammarSelection` resolution; defines `SourceRevision`,
`FileRevisionKey`, `ProjectSnapshotId`, `ProjectAnalysisId`, `ProjectionId`, owner-tagged non-Serde
`NodeId`, revision-bound `NodeKey`/`RegionKey`, baseline identity, and exact `RevisionGuard`; assigns
one private parse owner and per-build `ParseLedger` per supported file revision; preserves invalid
UTF-8 and partial/error state without rewrite authority; separates raw-arena from semantic-projection
invalidation; and specifies analyzer, metrics, graph, evaluator, protocol/verifier, CLI, MCP, slim,
and LSP behavior. Wire migration is explicit rather than an in-place `/3` extension.

**Commands/checks run:** targeted Hindsight active/negative-memory searches; Serena onboarding/tool
check followed by local Rust reads because Serena indexes this workspace as Python-only; `rg`/`sed`
audits across parse/analyzer/metrics/graph/protocol/verifier/LSP/CLI/slim/evaluator; three read-only
agent audits for core ownership, integration consumers, and contract tests; `cargo test -p
deslop-cli --test m0_definition_of_done -- --nocapture`; Markdown fence/heading checks; `git diff
--check`; `jj status`; and `jj diff --stat`.

**Verification results:** PASS for the documentation checkpoint and compatibility probe. The ADR has
balanced code fences, a complete decision/consumer/invalidation/test contract, and no whitespace
errors. The unchanged M0 executable snapshot passes with 30 workorders, 30 unique IDs, 30 unique
targets, 65 grouped findings, 21 files/74 symbols/197 graph edges/123 syntactic edges/zero false
resolution, one true ambiguous edge, three complete typed grammar fixtures, two quarantined partial
fixtures, and one unavailable JET capability observation. No Rust behavior changed, so the full
workspace/build/clippy gates were not rerun at this documentation-only checkpoint.

**Failure modes / invalidated assumptions:** a naked dense `NodeId` was invalidated because it can
silently address the same slot in a different analysis; the accepted ID carries a non-serialized
owner tag. Process-wide parse instrumentation was invalidated because concurrent MCP requests/tests
would contaminate counts; the ledger is owned per build. Putting canonical roles in `NodeKey/1` was
invalidated because roles arrive in M2; `/1` uses raw grammar structure and any role-aware identity is
an explicit `/2` bump. One-snapshot-per-LSP-document was invalidated by multiple dirty overlays; the
authority unit is a workspace overlay generation while M1 preserves file-local diagnostics. Separate
slim summary/run proposal passes were invalidated because consent could describe different bytes from
provider egress; both now derive from one prepared pinned run. `jj diff --check` was attempted but
this jj version has no such option, so the whitespace check used `git diff --check` for interoperability.

**Current recommendation/checkpoint:** M1.1 is complete. Implement M1.2 in `deslop-parse` starting
with revision/newtype and `GrammarSelection` tests, then the explicit source store, centralized
snapshot builder/root resolution, immutable project snapshot, single parse owner, and per-build
ledger. Do not migrate consumer behavior until this lower-layer contract is executable.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; targeted local Rust
reads remain the fallback.

**Dependencies/restart:** none. This is a documentation and planning checkpoint; no binary rebuild,
service restart, schema migration, or new third-party dependency is required. M1.10 will require
regenerating `/3` workorders/patches as the new strict schemas rather than accepting legacy defaults.

**Negative-memory status:** record that dense indices need analysis ownership, parse ledgers must be
per build rather than process-global, grammar selection must be stored once, canonical roles cannot
silently enter `NodeKey/1`, LSP authority spans all dirty overlays, and consent/prompt egress must use
one pinned prepared batch. Recheck only if the M1 identity, concurrency, LSP, or wire boundaries are
explicitly revised.

**Signature:** Codex (GPT-5), M1.1 integration owner, 2026-07-13.

---

## M1.2 checkpoint — content-addressed source snapshot and parse ownership

**Date/time:** 2026-07-13T17:50:57+02:00

**Objective/target:** make the first executable layer of ADR 0001 real without migrating existing
consumers: exact raw-byte source revisions, reusable content storage, deterministic snapshot scope
and read ownership, one atomic grammar selection, and one private parse owner per supported file
revision with request-local numerical accounting.

**Changes:** added `deslop-parse::snapshot` and extended `deslop-lang` with an authoritative
`Registry::resolve_grammar` that returns inseparable grammar descriptor plus actual Tree-sitter
language. `SourceStore` interns `Arc<StoredSource>` values by domain-separated `sr1_` raw-byte
revision and can be shared across snapshot builders. `ProjectSnapshotBuilder` records explicit
repository authority and invocation base; distinguishes default, requested, and exact-file scopes;
preserves file/directory kind; collapses aliases/descendants; rejects root escapes and conflicting
inputs; applies overlays before disk reads; captures non-lossy Unicode logical paths, per-path read
counts, analysis inputs, and atomic grammar selections; and emits deterministic `ps1_` identities.
`ProjectAnalysis` owns the immutable snapshot, one private Tree and byte line index per source,
complete/partial/failed provenance, deterministic `pa1_` identity, and a fresh per-build ledger with
separate requested/owner/invocation/reuse counts. Invalid UTF-8 retains exact bytes/revision and one
owner with zero parser calls. Existing `SourceFile`/`parse_source` APIs remain additive and unchanged.

**Commands/checks run:** focused `cargo test -p deslop-lang -p deslop-parse`; focused strict clippy;
`cargo test --workspace`; `cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`;
`cargo fmt --all -- --check`; `cargo build --workspace`; `cargo build -p deslop-slim
--no-default-features`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`git diff --check`; `jj status`; and `jj diff --stat`. Three read-only agent audits reviewed core
Tree-sitter ownership, root/scope/integration semantics, and the acceptance matrix while the root
agent owned all edits and integration.

**Verification results:** PASS. Focused suites: one `deslop-lang` test and 26 `deslop-parse` tests.
The new matrix locks the `abc` `sr1_` vector and exact byte sensitivity; content dedup without path
identity collapse; deterministic snapshot/analysis IDs; TS/TSX atomic grammar truth; all supported
grammar package/version keys; default versus exact-empty scope; invocation-base resolution;
overlay-before-read; cross-snapshot blob pointer reuse; absolute in-root input normalization;
conflicting bytes; partial and invalid-UTF-8 owner/invocation counts; explicit and discovered symlink
escape rejection; in-root alias collapse; and `Send + Sync` ownership. Workspace: 306 passing tests
plus one intentional ignored performance probe and all doc-tests. MCP slim feature: 23 passing tests.
Workspace/minimal builds, formatting, whitespace, and strict all-target/all-feature clippy pass.

**Failure modes / invalidated assumptions:** the initial grammar metadata table and later
`grammar_for_path` call were separate authority decisions; replaced by one `deslop-lang`
`ResolvedGrammar` stored in the snapshot and consumed without reselection. A builder-local mutable
store could not reuse blobs across snapshots; replaced with an injectable thread-safe
`Arc<SourceStore>` returning inseparable revision/bytes objects. One empty vector could not safely
mean both default-root and zero changed files; replaced by typed default/requested/exact scope.
Reading disk before applying overlays could observe or fail on bytes outside the snapshot; overlays
now remove shadowed paths from the read plan. Parallel optional grammar/language fields admitted
invalid states; replaced by a private source/input enum. Lossy path hashing was invalidated in favor
of validated Unicode components and canonical slash encoding. Machine-global ignore configuration
was disabled for snapshot discovery. Early compile/clippy failures (missing `Lang` ordering,
`tempfile` dev wiring, and two style lints) were corrected before the full gate.

**Current recommendation/checkpoint:** M1.2 is complete. Implement M1.3 by copying the private Tree
into a deterministic owned arena while preserving all named/anonymous/error/missing nodes, grammar
field/order relations, raw byte/point/line spans, source slices, and token/trivia ownership. Keep
`NodeId` and serialized keys out of this pass except for an internal dense arena index needed to wire
parent/children; M1.4 owns identity authority.

**Blockers:** none. Automatic repository/root policy wrappers remain a consumer-migration concern;
the foundational builder already captures explicit authority, invocation base, and typed scope
without forcing legacy callers to migrate. Serena remains Python-symbol-only for this Rust workspace.

**Dependencies/restart:** rebuild Rust consumers to pick up the additive libraries. No service
restart, external migration, wire-schema change, or new third-party package was introduced;
`blake3`, `ignore`, and `tempfile` were already workspace dependencies.

**Negative-memory status:** retain that grammar identity and the actual parser language must be one
resolved object; overlays must shadow before disk reads; exact-empty scope is distinct from default;
parse ledgers belong to one build; source revision and bytes must be inseparable; and path hashing
must never be lossy. Recheck when M1.3 stores arena facts or M1.8 adds parse-artifact reuse.

**Signature:** Codex (GPT-5), M1.2 integration owner, 2026-07-13.

---

## M1.3 checkpoint — deterministic owned syntax arena

**Date/time:** 2026-07-13T18:13:17+02:00

**Objective/target:** copy every private Tree-sitter tree into immutable revision-bound Rust storage
without exposing borrowed nodes or prematurely creating public identity authority. Preserve raw CST
facts, exact byte ownership, recovery state, grammar provenance, deterministic order, and M1.2's one
parse owner/ledger contract.

**Changes:** added `deslop-parse::arena` with `deslop-raw-arena/1` and attached one optional owned
arena to every successfully parsed `ParsedFile`. Construction is iterative deterministic preorder
over every concrete named and anonymous child. Nodes retain visible and alias-free grammar
kind/name IDs, incoming field, exact half-open byte and raw Tree-sitter point spans, reciprocal
parent/ordered children, named/extra/error/missing/has-error flags, and exact source-slice
coordinates. Arena-level grammar provenance is copied from the snapshot's atomic
`GrammarSelection`; no grammar is reselected. Positive-width non-extra leaves are raw tokens;
non-error extra subtrees and direct-child gaps are trivia. Recovery `ERROR` nodes remain tokens even
when Tree-sitter marks them extra. Root-external leading/trailing bytes use an explicit file owner,
so every source byte is owned exactly once and every syntax-owned segment remains inside its owner
span. Zero-width missing nodes remain addressable but own no bytes. Raw slots and slicing helpers
remain crate-private; M1.4 owns analysis-tagged public `NodeId` and structured wrong-snapshot lookup.
`ProjectAnalysisId` now commits to arena schema `/1`; invalid UTF-8 still has no Tree or arena.

**Commands/checks run:** targeted Hindsight M1.3/negative-memory searches; Serena symbol attempt
(unavailable for Rust because this project is indexed as Python-only); local `rg`/`sed` inspection of
the ADR, snapshot, consumers, and pinned Tree-sitter 0.25.10 API; three read-only agent audits for
arena fidelity, downstream integration, and numerical contracts; focused `cargo test -p
deslop-parse --lib`; focused strict parse clippy; the exact M0 DoD test; `cargo test --workspace`;
`cargo test -p deslop-mcp --features slim-llm -- --test-threads=1`; `cargo build --workspace`;
`cargo build -p deslop-slim --no-default-features`; `cargo fmt --all -- --check`; `cargo clippy
--workspace --all-targets --all-features -- -D warnings`; and `git diff --check`.

**Verification results:** PASS. `deslop-parse` has 34 passing tests. The 58-byte Unicode/comment
oracle owns 22 nodes and 28 segments (14 token/26 bytes, 14 trivia/32 bytes) in exact preorder; the
35-byte missing-node oracle retains one zero-width `)` at byte 20 with 20 nodes and an exact
11-token/7-trivia partition. Pinned malformed TypeScript is 66 bytes/27 nodes/24 segments with one
ERROR; malformed TSX is 97 bytes/52 nodes/46 segments with one ERROR. A seven-byte whitespace-only
file has one zero-width syntax root and one file-owned trivia segment; empty input has one root and
no segments. Tree/arena lockstep checks every kind, ID, field, point, flag, child order, and slice;
alias and repeated-field witnesses pass; repeated arena reads leave both parse ledgers at exact
1 request/1 owner/1 invocation/0 reuse. Workspace: 314 passing tests plus one intentional ignored
performance probe and all doc-tests. The unchanged M0 numerical gate still reports 30 workorders,
30 IDs, 30 targets, 65 findings, 21 files/74 symbols/197 graph edges/123 syntactic edges/zero false
resolution, one true ambiguous edge, three complete grammar sentinels, two quarantined partial
fixtures, and one unavailable JET observation. Feature-enabled MCP has 23 passing tests. Both build
modes, formatting, whitespace, and strict all-target/all-feature clippy pass.

**Failure modes / invalidated assumptions:** requiring the grammar root to span the entire input was
invalidated by Rust files whose Tree-sitter root excludes leading whitespace; assigning those bytes
to that syntax root was also invalid because the segment then escaped its owner span. The accepted
model uses an explicit file owner for root-external trivia. Treating every `is_extra` ancestry as
trivia was invalidated because recovery `ERROR` nodes may be extra and can cover an entire malformed
TSX program; only non-error extras propagate trivia ownership. A Rust missing-brace fixture did not
actually produce a Tree-sitter missing node and was replaced by a pinned TypeScript zero-width
missing-`)` witness. Self-reciprocity alone did not prove Tree fidelity, so the suite now traverses
the private Tree and owned arena in exact lockstep. Bare public arena indices were invalidated because
same-valued slots could be mixed across files before M1.4's owner validation; they remain internal.
Tree-sitter point columns are byte columns, not Unicode or UTF-16 columns, and the Unicode oracle
locks that distinction for later LSP conversion.

**Current recommendation/checkpoint:** M1.3 is complete. Implement M1.4 with a process-local
analysis-owner tag plus dense project-global slot for non-Serde `NodeId`, structured wrong-analysis
and out-of-range errors, revision-bound serialized `NodeKey/1` using raw grammar structure, a
separate cross-revision baseline fingerprint, and exact `RevisionGuard`. Keep canonical roles out of
`NodeKey/1` and never let any fuzzy identity authorize writes.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace; local Rust tools are
the documented fallback.

**Dependencies/restart:** rebuild Rust consumers to pick up the additive internal arena. No service
restart, external schema migration, wire change, or third-party dependency was introduced. Existing
consumers still use legacy parse paths until M1.9/M1.10; this milestone intentionally adds storage,
not migration.

**Negative-memory status:** record that grammar roots need not cover leading/trailing bytes; such
trivia needs a file owner rather than a lying syntax span. Do not classify recovery ERROR subtrees as
trivia merely because `is_extra` is true. Do not expose bare dense arena slots before analysis-owner
validation. Tree parity needs a lockstep oracle, missing-node claims need an actual pinned missing
fixture, and byte point columns must be converted rather than published as UTF-16. Recheck when M1.4
adds identity or M1.5 adds containment.

**Signature:** Codex (GPT-5), M1.3 integration owner, 2026-07-13.

---

## M1.4 checkpoint — syntax identity domains and write-authority separation

**Date/time:** 2026-07-13T18:48:58+02:00

**Objective/target:** expose the owned arena through deterministic, owner-validated scan-local node
identity; add a strict revision-bound serialized key and deliberately fuzzy cross-revision comparison
fingerprint; and preserve exact write authorization without allowing syntax correlation to mint a
`RevisionGuard` or authorize a write.

**Changes:** added `deslop-parse::identity` and public `NodeId`, `NodeView`, `NodeIds`, `NodeKey`,
`NodeAnchor`, `NodeBaselineFingerprint`, `SourcePoint`, and `SyntaxSpan` surfaces. Each immutable
`ProjectAnalysis` receives a non-repeating process-local owner tag independent of deterministic
analysis content; `NodeId` combines that tag with a dense project-global preorder slot, has no Serde
implementation, and reports wrong-analysis before range errors. File ranges follow canonical
`BTreeMap` path order and map reciprocal file-local parent/children into global IDs. Structured
`deslop.node-key/1` values include exact `FileRevisionKey`, `deslop-raw-arena/1`, alias-free raw
grammar kind and numeric symbol, the root-to-node incoming-field path, fixed-width byte/point
coordinates, a bottom-up `nsa1_` raw structural digest, and a checked collision ordinal. Custom
deserialization rejects unknown fields, unsupported schemas, invalid prefixes, empty identity
fields, reversed coordinates, and non-canonical paths. `FileRevisionKey` wire paths use canonical
slash components and `%25`; raw or escaped backslashes are rejected, and snapshot admission rejects
literal Unix backslash components because legacy exact-guard normalization would alias them with
directory separators. `nb1_` baseline fingerprints hash repository, path, raw kind, field path, and
Unicode-trimmed node text while excluding revisions, coordinates, numeric grammar versions, anchors,
and collision ordinals. They are explicitly collision-prone read-only evidence and have no reanchor,
lookup, guard-construction, or write API. Existing `deslop-core` `rg1_` reconstruction remains the
sole exact write authority and is unchanged for `/3` wire compatibility.

**Commands/checks run:** targeted Hindsight startup, active-plan, and negative-memory reads; Serena
activation/instruction checks followed by local Rust reads because Serena indexes this repository as
Python-only; three read-only agent audits for the core identity boundary, consumer integration, and
contract tests; focused parse tests and strict parse clippy throughout; exact core/protocol/verifier/
CLI/slim compatibility tests; the exact M0 numerical gate; `cargo test --workspace`; `cargo test -p
deslop-mcp --features slim-llm -- --test-threads=1`; `cargo build --workspace`; `cargo build -p
deslop-slim --no-default-features`; `cargo fmt --all -- --check`; `cargo clippy --workspace
--all-targets --all-features -- -D warnings`; `git diff --check`; `jj status`; and `jj diff --stat`.

**Verification results:** PASS. `deslop-parse` has 42 passing tests. The dense three-file oracle has
36 slots and 33 child edges with roots at 0, 10, and 19; reversed overlay orders produce identical
full `(slot, path, kind, key, parent, children)` sequences while independent analyses produce
different `NodeId` owners. Wrong owners win over even `u32::MAX` range errors. Prefixing `0.rs`
shifts `a.rs` from slot 0 to 10 without changing its node keys. Key tests lock the exact eight-field
wire, strict standalone anchors, schema/source/path adversaries, collision overflow, exact-revision
expiry, ambiguous duplicate baselines, and the pinned Rust call-expression digest
`nsa1_2e71d4d3ed08b9955a5d305e4d79667b5933bdd90860055902470563646d464c`. A peer-only file edit
expires `NodeKey` while leaving a locally reconstructed target-region `rg1_` equal, proving that
correlation identity and write authority are separate. Workspace: 322 passing tests plus one
intentional ignored performance probe and all doc-tests. Feature-enabled MCP has 23 passing tests.
Workspace and minimal-slim builds, formatting, whitespace, and strict all-target/all-feature clippy
pass. The unchanged M0 numerical gate remains 30 workorders/IDs/targets, 65 grouped findings,
21 files/74 symbols/197 graph edges/123 syntactic edges/zero false resolution, one ambiguity, three
complete grammar sentinels, two quarantined partial fixtures, and one unavailable JET observation.

**Failure modes / invalidated assumptions:** a deterministic owner derived from analysis content and
a bare dense slot both allow accidental cross-analysis access; the accepted owner is process-local
and separately allocated. A fuzzy or cross-revision `NodeKey`, a span-only structural anchor, and a
first-match baseline resolver were rejected because each can silently select the wrong duplicate.
Canonical roles were again excluded because M2 owns them. Permissive `PathBuf` Serde and `%5c`
backslash decoding were rejected because their meaning changes by host platform and can alias or
traverse on Windows. Public standalone anchor deserialization now preserves the same invariants as a
nested `NodeKey`. A production node-to-guard accessor was rejected because Tree-sitter endpoint
semantics are not the canonical verifier region contract; callers cannot mint write authority from
syntax identity. Changing legacy `rg1_` coordinate hashing from native-width `usize` to fixed-width
`u64` under the same prefix was rejected as a silent wire migration: `/3` artifacts must retain
their current algorithm, and a portable `rg2_` belongs to the explicit M1.10 `/4` flag day. Removing
legacy `RevisionGuard: From<String>` has the same migration boundary; runtime reconstruction already
rejects forged values.

**Current recommendation/checkpoint:** M1.4 is complete. Implement M1.5 as immutable containment and
smallest-exclusive-region indices over the owned arena and public node IDs. Keep the index raw-CST
and revision-local; do not introduce M2 canonical roles, fuzzy reanchoring, or write authority.

**Blockers:** none for M1.5. Serena remains Python-symbol-only for this Rust workspace. The current
legacy `rg1_` hash is architecture-native; do not claim cross-architecture wire portability or alter
the existing prefix before M1.10's explicit schema migration.

**Dependencies/restart:** rebuild Rust consumers to pick up the additive parse API. No service
restart or external migration is required. `serde` and `serde_json` were already workspace
dependencies. Consumer migration, deterministic reread staleness, LSP document versions, full
peer-readset commit, multi-file atomic rollback, and the `/4` wire flag day remain assigned to
M1.9/M1.10/M6/M7 rather than being implied by `NodeKey`.

**Negative-memory status:** retain that `NodeId` ownership must be per analysis, `NodeKey` is exact
revision-bound raw syntax identity with a strict arena schema and structural digest, and baseline
fingerprints are collision-prone evidence with no lookup/write path. Literal backslashes cannot
enter logical snapshot paths while `rg1_` normalizes them. Do not silently redefine `rg1_`; add a
fixed-width successor only at the declared wire flag day. Measure M1.4's cloned per-node file keys
and field paths, allocating child views, and linear range/key scans in M1.11 before migration-scale
performance claims. Recheck when M1.5 adds indices, M1.8 adds invalidation, M1.10 migrates wire
consumers, or M1.11 measures memory.

**Signature:** Codex (GPT-5), M1.4 integration owner, 2026-07-13.

---

## M1.5 checkpoint — structural containment and exclusive syntax ownership

**Date/time:** 2026-07-13T19:16:48+02:00

**Objective/target:** make structural CST containment and the smallest exclusive raw byte owner
explicit, immutable, revision-local, and efficient enough to replace downstream Tree-sitter
`descendant_for_byte_range` calls later. Preserve anonymous, extra, ERROR, and missing nodes without
introducing M2 semantic roles or M1.6 aggregation policy.

**Changes:** added `deslop-parse::containment::ContainmentIndex` to every successfully built arena.
Construction derives and validates preorder-exclusive subtree ends, node depths, direct-child
subtree contiguity, the positive-width segment partition order, and co-minimal zero-width nodes in
byte/grammar-preorder order before the immutable analysis is published. `ProjectAnalysis` now
provides inclusive subtree and strict descendant iterators plus owner-checked structural
`node_contains`; these use project-global `NodeId`s and preserve wrong-analysis/range errors.
`ExclusiveSyntaxRegion` and exact-size whole-file/per-node iterators expose the M1.3 token/trivia
partition without inclusive descendant roll-up. `smallest_exclusive_syntax_region` binary-searches
the existing segment slice without a duplicate per-segment endpoint array. File-owned regions carry
`&FileRevisionKey`, preventing detached File owners from comparing equal across paths/revisions.
Strict positive byte-range lookup rejects reversed, empty, and out-of-bounds ranges, finds the start
and end-byte owners in O(log S), and returns their structural LCA in O(height); any root-external
endpoint returns exact File ownership rather than a lying grammar root. Equal-span parent/child
wrappers therefore select the structurally deeper raw node. A separate named helper explicitly
promotes that raw node to the nearest named ancestor. `SyntaxPointContext` treats insertion points
separately: it returns every unrelated co-minimal exact zero-width node in grammar preorder and
independent before/after byte owners, avoiding an undocumented first-match or left/right bias.

**Commands/checks run:** targeted Hindsight active/negative-memory search; local ADR, plan, arena,
consumer, and Tree-usage inspection (Serena remains Python-only for this Rust workspace); three
read-only agent audits for core index semantics, downstream range-query requirements, and numerical
contracts; repeated focused parse tests and strict parse clippy; `cargo test --workspace`; `cargo
test -p deslop-mcp --features slim-llm -- --test-threads=1`; `cargo build --workspace`; `cargo build
-p deslop-slim --no-default-features`; `cargo fmt --all -- --check`; `cargo clippy --workspace
--all-targets --all-features -- -D warnings`; `git diff --check`; `jj status`; and `jj diff --stat`.

**Verification results:** PASS. `deslop-parse` has 44 passing tests. The 62-byte nested Rust oracle
has 37 nodes; all 1,369 ordered pairs match an independent parent-chain oracle, with exactly 254
self-inclusive containment pairs and 217 strict ancestor pairs. Every subtree iterator matches the
filtered preorder oracle. All 1,953 non-empty byte ranges match an independent deepest-containing-
span oracle. Equal `36..56` statement/conditional spans select the child `if_expression`; equal
`39..43` literal/token spans select anonymous `true`, while explicit named promotion returns
`boolean_literal`. The 49-byte partition oracle has exactly 27 exclusive regions (14 token, 13
trivia), reconstructs the source, and every byte matches both linear region search and an independent
maximum-structural-depth owner/kind oracle. Boundaries lock File `0..3`, token `3..5`, parent trivia
`5..6`, and root trivia `47..49`. Missing `)` remains a zero-width structural child at `20..20`, owns
no region, and is returned separately from the function-owned `20..21` byte; empty and seven-byte
whitespace files retain zero-width roots with zero/one exclusive regions. Foreign/correct-owner
`u32::MAX`, cross-file containment, partial syntax, invalid UTF-8, absent paths, range/point bounds,
and nested zero-width TypeScript recovery all fail or resolve as declared. Workspace: 324 passing
tests plus one intentional ignored performance probe and all doc-tests. Feature-enabled MCP has 23
passing tests. Both build modes, formatting, whitespace, and strict all-target/all-feature clippy
pass; the unchanged M0 executable compatibility test passes within the workspace suite.

**Failure modes / invalidated assumptions:** span containment was rejected as structural truth
because equal-span parent/children would make containment symmetric; preorder subtree intervals are
authoritative. Ancestor/subtree lookup alone was insufficient because parse, metrics, and analyzer
consumers currently ask for smallest byte-range descendants; endpoint owner plus LCA supplies the
owned replacement without migrating consumers early. Returning one zero-width first match was
rejected because unrelated same-point nodes are structurally ambiguous and sibling boundaries have
no unbiased side; point context returns all co-minimal exact nodes and both sides separately. Named
nodes are not the raw default because anonymous punctuation can be the true smallest owner. File
ownership without a file key was rejected because owners from different files could collapse under
Eq/Hash. Root-external trivia remains File-owned and never expands the grammar root. A duplicated
`usize` endpoint array was removed because the validated segment slice already supports logarithmic
lookup. The borrowed M1.3 28-region expectation was also invalidated for the actual 49-byte fixture:
the measured truth is 27 regions, 14 token/13 trivia, with trailing newline inside the Rust root.

**Current recommendation/checkpoint:** M1.5 is complete. Implement M1.6 by consuming each direct
exclusive region once and deriving explicitly declared inclusive aggregates bottom-up over subtree
intervals. Keep nested-callable reset and metric-region selection as caller/adapter policy; do not
infer them from raw kind strings before M2.

**Blockers:** none. Serena remains Python-symbol-only for this Rust workspace. Existing consumers
continue using borrowed Tree-sitter traversal until M1.9/M1.10; this milestone supplies the complete
owned raw boundary but intentionally does not migrate or reinterpret their semantic regions.

**Dependencies/restart:** rebuild Rust consumers to pick up the additive parse API. No service
restart, external schema migration, wire change, or dependency change is required. M1.6 owns
aggregation, M1.7 query captures, M1.8 immutable invalidation, M2 canonical roles, and M1.9/M1.10
consumer migration/RegionKey semantics.

**Negative-memory status:** retain that structural containment is topology, never span inference;
preorder ancestry alone does not satisfy downstream byte-range lookup; positive ranges use exclusive
endpoint owners plus LCA; empty ranges require explicit point context; root-external bytes remain
File-owned; and unrelated same-point zero-width minima must not become hidden first-wins. Do not make
named promotion, semantic region resets, inclusive aggregation, fuzzy identity, or write authority
implicit in this raw index. Recheck for M1.6 aggregation, M1.7 captures, M1.9 consumer migration, and
M1.11 memory/latency measurement.

**Signature:** Codex (GPT-5), M1.5 integration owner, 2026-07-13.

---

## M1.6 checkpoint — exclusive local and declared inclusive aggregation

**Date/time:** 2026-07-13T19:48:19+02:00

**Objective/target:** make raw syntax evidence aggregation exact-once, revision-local, generic, and
explicit about direct-owner, full-subtree, and semantic-boundary projections. Supply the complete
owned primitive required by the later metrics/analyzer migration without guessing M2 roles or
reparsing source regions.

**Changes:** added `deslop-parse::aggregation` and public `InclusiveSyntaxPolicy`, owner/projection
context, owner-checked result views, and typed callback/lookup errors. `ProjectAnalysis` now exposes
infallible `fold_syntax_aggregates` and fallible `try_fold_syntax_aggregates`. Construction first
validates and normalizes every reset `NodeId`, then initializes the File pseudo-owner and every raw
node once in grammar preorder, including anonymous, extra, ERROR, missing, internal, and zero-width
nodes. It folds every positive-width `ExclusiveSyntaxRegion` once in byte order into only its direct
owner. One bottom-up pass always derives `full_inclusive`; a second pass derives
`declared_inclusive` only when normalized resets exist. A reset cuts only its declared edge to the
parent: its own declared value remains, nested reset values do not leak into it, and the full view is
unchanged. File local retains root-external bytes, File full-inclusive remains the total source, and
File declared-inclusive is the residual outside reset subtrees. Results carry the exact analysis and
file revision, normalized policy, dense preorder node values, and explicit foreign/wrong-file lookup
errors without Serde or write authority. Fallible callbacks preserve exact initialization owner,
fold owner/range, or merge parent/child/projection context, allowing checked arithmetic instead of
panic, wrap, or saturation. Added allocation-free `NodeView::child_count` and `is_leaf` accessors for
future structural feature initialization. After the existing linear file-range lookup, the core
algorithm is O(N + S + R) plus caller-defined clone/merge costs.

**Commands/checks run:** startup Serena/Hindsight context and roadmap/ADR/audit inspection; three
read-only agent tracks for core API/algebra, consumer migration requirements, and independent
numerical contracts; repeated focused aggregation tests; full `cargo test -p deslop-parse --lib`;
strict parse clippy; warnings-denied rustdoc; `cargo test --workspace`; `cargo test -p deslop-mcp
--features slim-llm -- --test-threads=1`; `cargo build --workspace`; `cargo build -p deslop-slim
--no-default-features`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`cargo fmt --all -- --check`; `git diff --check`; `jj status`; and `jj diff --stat`.

**Verification results:** PASS. `deslop-parse` has 47 passing tests. The 62-byte nested Rust fixture
has 37 raw nodes and 37 exclusive regions: File initializes first, all 37 nodes initialize once, all
37 region callbacks are contiguous, and every byte has visit count one. Full aggregation measures
37 regions/62 bytes, 22 token regions/43 token bytes, and 15 trivia regions/19 trivia bytes. Resetting
the function, closure, and arbitrary call deduplicates an unsorted input and performs exactly
`2N-R = 71` merges; declared values are respectively 17/34, 16/20, and 3/7 regions/bytes, while the
File/root residual is 1/1 and the four disjoint values reconstruct 37/62. Equal-span literal/token
resets conserve 37/62 without counting the anonymous four-byte token twice. Resetting every node
makes every declared value equal its local value; `ResetAt([])` is executable-equivalent to
`AllDescendants`. An independent O(N*height + S*height) parent-chain oracle matches every local,
full, declared, and reset flag. Rebuilt analyses produce identical ordered `(NodeKey, local, full,
declared, reset)` vectors. The mixed 49-byte partition locks File local at 1 region/3 bytes, root full
at 26/46, total at 27/49, and root-reset File residual at 1/3. Partial TypeScript remains queryable
at 18/35 while its missing node initializes with zero regions; empty and whitespace inputs retain
their zero-width roots; invalid UTF-8 and absent paths run zero callbacks. Foreign-analysis,
same-analysis peer-file, and correct-owner out-of-range resets fail before callback one. Fallible
initializer, fold, and checked-overflow merge failures retain their exact context. Parse ledgers do
not change. Workspace has 327 passing tests plus one intentional ignored performance probe and all
doc-tests; feature-enabled MCP has 23 passing tests. Both build modes, warnings-denied rustdoc,
formatting, whitespace, and strict all-target/all-feature clippy pass.

**Failure modes / invalidated assumptions:** a region-only fold was rejected because internal and
zero-width nodes need once-per-owner structural seeds. A single reset-aware value was rejected
because it obscures true full-subtree totals and forces refolding when consumers require both views.
The public names were made `full_inclusive`/`file_full_inclusive` after review showed that an
unqualified `inclusive` accessor could select the wrong projection. Source-ordered region callbacks
do not make collapsed inclusive aggregates source ordered: parent-local regions can surround child
subtrees, so merge is explicitly pure, associative, and commutative; ordered token/capture streams
remain M1.7/M1.9 work. Infallible-only arithmetic was rejected because overflow/domain failures must
return context instead of panic, wrap, or saturate. Raw-kind callable inference was rejected because
Python decorated regions, Clojure forms, and other adapter semantics belong to M2. Summing arbitrary
ancestor/descendant full values remains invalid because they overlap; disjoint File residual plus
declared reset-root values is the conserved semantic partition.

**Current recommendation/checkpoint:** M1.6 is complete. Implement M1.7 as owned, deterministic
query/cursor-derived captures over the one private per-revision Tree. Captures must map back to
existing `NodeId`s without fragment reparsing, preserve query/capture order and grammar provenance,
and keep borrowed Tree-sitter handles inside construction/query callbacks.

**Blockers:** none. Serena still indexes this Rust workspace as Python-only, so local Rust inspection
remains the documented fallback. Existing analyzer/metrics consumers continue their legacy parsing
until M1.9; the current Python behavioral fixture still demonstrates 364 source bytes versus 649
summed overlapping region bytes and 12 physical versus 21 summed region NLOC, which is the migration
regression M1.9 must collapse using declared reset boundaries rather than changing M1.6 semantics.

**Dependencies/restart:** no new dependency, wire schema, service restart, cache clear, or migration
is required. Rebuild Rust consumers for the additive API. M1.7 owns capture extraction, M2 owns
canonical roles/query packs, M1.9 owns analyzer/metrics migration and line-ownership policy, and
M1.11 owns measured latency/peak memory plus compaction of retained local/full/declared `T` values
and the existing O(F) file-range lookup.

**Negative-memory status:** retain that direct-region folding alone cannot seed structural node
facts; reset-aware values cannot substitute for true full-inclusive values; source-ordered local
callbacks do not authorize order-sensitive commutative roll-ups; reset policy must be explicit
`NodeId` data rather than inferred raw kinds; and fallible checked aggregation must preserve context.
Do not sum overlapping inclusive peers, attach root-external File bytes to the grammar root, promote
partial parse authority, serialize process-local aggregates/IDs, or derive write authority. Recheck
when M1.7 adds ordered captures, M1.9 declares adapter reset/line policies, or M1.11 measures storage.

**Signature:** Codex (GPT-5), M1.6 integration owner, 2026-07-13.

---

## M1.7 checkpoint — owned grammar-query matches and captures

**Date/time:** 2026-07-13T20:13:49+02:00

**Objective/target:** execute raw Tree-sitter queries against the one private Tree retained for each
exact source revision, return only owned results bound to existing `NodeId`s, preserve both grouped
match semantics and deterministic source-order dispatch, and prevent fragment reparsing or borrowed
Tree-sitter handles from crossing the public API.

**Changes:** added `deslop-parse::query` with exact `GrammarSelection`-bound `SyntaxQueryId` and
cloneable `SyntaxQuery`. A compiled query retains its exact source so public per-pattern byte ranges
remain self-describing, plus owned capture names, capture quantifiers, rooted/non-local flags,
`#set!` properties, `#is?`/`#is-not?` property predicates, and general predicate arguments. Query
source length is rejected above `u32::MAX` before Tree-sitter can narrow it. `ProjectAnalysis` now
compiles queries from a stored parser language and exposes grouped `syntax_query_matches`, preserving
Tree-sitter match discovery and capture association/order, plus intentionally association-free
`syntax_query_captures` in Tree-sitter source order. Both return owned names, pattern/capture indices,
and analysis-local `NodeId`s; no Tree-sitter node, cursor, match, or capture type is public or Serde.
Execution validates the `NodeId` owner, exact full grammar identity including dialect, private Tree
availability, and complete visible-node preorder parity between the retained Tree and arena. It then
maps each private unique `Node::id()` to the aligned existing arena slot; span/kind lookup is never
used. Fresh cursors evaluate built-in text predicates against pinned snapshot bytes. Non-filtering
`#set!` metadata is allowed, while unevaluated property/general predicates fail closed until M2
provides an evaluator. Cursor output is published only after complete exhaustion; match-limit
overflow returns a typed error and discards every partial result. Partial recovery trees remain
mechanically queryable with unchanged provenance/authority. `ParsedFile` now retains the exact
resolved language even for invalid UTF-8 so query compilation remains a grammar operation; executing
such a reusable query against a valid same-grammar revision works without parsing the invalid file.
After existing O(F) node/file lookup, each execution currently builds and validates an O(N) borrowed
preorder plus O(N) `Node::id` map before query work; M1.11 owns measurement/caching/compaction.

**Commands/checks run:** startup Serena/Hindsight context; roadmap, ADR, audit, pinned Tree-sitter
0.25.10 source, arena, identity, and consumer inspection; three read-only agent tracks for core API,
consumer requirements, and independent numerical contracts; repeated focused query tests; `cargo
test -p deslop-parse`; parse and workspace strict clippy; `cargo test --workspace`; `cargo test -p
deslop-mcp --features slim-llm -- --test-threads=1`; `cargo build --workspace --all-targets
--all-features`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
warnings-denied workspace rustdoc; `cargo fmt --all -- --check`; `git diff --check`; `jj status`; and
`jj diff --stat`.

**Verification results:** PASS. `deslop-parse` has 56 passing tests. The exact 62-byte nested Rust
oracle has 37 nodes and wildcard capture parity over all 37 existing NodeIds: 18 named, 19 anonymous,
all unique, including distinct equal-span pairs. A three-pattern query locks capture-table order,
pattern ranges `0..94`, `94..196`, and `196..220`, all 21 pattern/capture quantifiers, five grouped
matches in engine discovery order, and nine flat captures in source order; the orders deliberately
differ and duplicate identifier NodeIds remain present. Field-constrained captures lock the let,
pattern, and value nodes/fields. Rebuilding with a lexically prior ten-node file shifts global
NodeIds by ten while ordered capture `NodeKey`s remain identical. A match limit of one provably
exceeds the cursor for a six-result query, and both public shapes return only
`MatchLimitExceeded`. Missing TypeScript `)` captures as anonymous NodeId 12 at `20..20`; malformed
TS and TSX ERROR captures retain NodeIds 24/1 and spans `62..63`/`0..96`. Text predicates filter
pinned identifiers, `#set!` executes, and unsupported property/general predicates return no results.
Queries with zero captures retain matches but produce an empty flat stream; empty queries return
empty complete vectors. All reachable compile-error kinds retain exact row/column/offset/message,
JS and JSX sharing an artifact still fail exact-dialect reuse, foreign/out-of-range NodeIds are typed,
and query/source/results remain owned Send/Sync/'static. Query compilation and repeated execution
leave both the full parse ledger and legacy parse-source counter unchanged. Workspace has 332 passing
tests plus one intentional ignored performance probe; feature-enabled MCP has 23 passing tests. All
build, strict clippy, rustdoc, formatting, and whitespace gates pass.

**Failure modes / invalidated assumptions:** span/kind capture lookup was rejected because equal-span
parent/child and zero-width recovery nodes are ambiguous. A flat-only API was rejected because it
cannot retain multi-capture match association; grouped and source streams have distinct documented
contracts. Grouped matches cannot be byte-sorted because Tree-sitter match discovery is not global
source order. Metadata-only handling of `#is?` and custom directives was rejected because Tree-sitter
does not evaluate them and silent execution would overmatch. Returning cursor output before checking
the finite match limit was rejected because it canonizes partial evidence. Grammar artifact identity
alone was rejected because JS/JSX can share one artifact while dialect identity differs. Orphaned
pattern source ranges were rejected by retaining query source. Hashing full query bytes after
Tree-sitter silently narrowed an oversized length was prevented by an explicit preflight bound.

**Current recommendation/checkpoint:** M1.7 is complete. Implement M1.8 as immutable successor
analysis construction with explicit changed-range evidence and deterministic re-anchor-or-expire
behavior. Reuse Tree-sitter old-tree parsing only when exact prior/new file revisions and grammar
identity authorize it; never mutate a published analysis or treat approximate span proximity as
identity.

**Blockers:** none. Serena still indexes this Rust workspace as Python-only, so local Rust inspection
remains the documented fallback. Existing analyzer/metrics/graph consumers continue legacy parsing
until M1.9/M1.10; M1.7 supplies their raw query substrate but intentionally does not create semantic
roles, query packs, property/general directive evaluators, projection identities, or write authority.

**Dependencies/restart:** rebuild Rust consumers for the additive API. No new dependency, wire
schema, service restart, cache clear, or migration is required. M1.8 owns immutable changed-range
construction and NodeKey re-anchor/expiry; M2 owns semantic query packs and predicate/directive
evaluation; M1.9/M1.10 own consumer migration; M1.11 owns query map/result allocation measurement.

**Negative-memory status:** stored in Hindsight. Never map captures by span/kind, flatten away match
association, sort grouped matches by byte, silently ignore property/general predicates, return
finite-limit partials, reuse queries across non-identical full grammar selections, orphan pattern
ranges from their source, allow Tree-sitter length narrowing, persist private Tree-sitter IDs, expose
borrowed handles, serialize NodeId/query handles, reparse/reread source, infer M2 roles, or promote
partial-tree authority. Recheck these constraints during M1.8, M1.9, M2, and M1.11.

**Signature:** Codex (GPT-5), M1.7 integration owner, 2026-07-13.

---

## M1.8 checkpoint — immutable incremental successor and explicit node transitions

**Date/time:** 2026-07-13T20:49:49+02:00

**Objective/target:** construct a new immutable `ProjectAnalysis` from a successor snapshot, reuse
compatible parser state without mutating the published predecessor, report textual and structural
changes without conflating them, and make every predecessor node either explicitly retained,
re-anchored with exact evidence, or expired for a typed reason.

**Changes:** added `deslop-parse::incremental` with `ProjectAnalysis::successor` and
`successor_with_edits`. Exact unchanged `FileRevisionKey`s share the original `Arc<ParsedFile>` but
receive fresh analysis-local `NodeId`s; their outcome is `Retained`, not cross-revision re-anchoring.
Compatible edited files clone and sequentially edit the old private Tree, invoke the exact stored
runtime language once, rebuild all public arena/containment/key state from final bytes, and must
equal a clean rebuild. A canonical UTF-8-safe old-to-final `source_invalidation_edit` is separate
from validated sequential edits and Tree-sitter `syntax_changed_ranges`; the latter are structural,
final-new-coordinate evidence and may be empty for real byte edits. Plain `successor` derives one
coarse splice for parser reuse/invalidation but expires every node in the edited file because final
bytes cannot prove edit history. `successor_with_edits` validates each replacement in its current
intermediate coordinate space, UTF-8 and u32 bounds, and exact final reconstruction. Only that exact
history may correlate nodes, and only when the private Tree-sitter node identity survives and the
mapped span, bytes, visible and grammar kinds/ids, canonical flags, field path, and structural digest
all match. No span, proximity, baseline, collision, `has_changes`, or fallback matching exists.
Transition evidence is process-local correlation only and explicitly cannot refresh a proposal,
work order, revision guard, editor version, projection, or write authority. Removed, grammar-changed,
syntax-unavailable, and changed nodes expire distinctly. Same-grammar runtime-language disagreement
is an integrity error; repository mismatch and malformed scripts fail before construction. Cold and
incremental project parsing now both fail if Tree-sitter unexpectedly returns no Tree. The parse
ledger is fresh per successor: only zero-invocation whole-file Arc reuse records `reused=1`;
incremental old-Tree parsing records one invocation and `reused=0`.

**Commands/checks run:** startup Serena/Hindsight context; roadmap, prior checkpoint, ADR, pinned
Tree-sitter API/source, parse ownership/identity/query/consumer inspection; three read-only agent
tracks for core authority, downstream integration, and independent numerical contracts; repeated
focused incremental and full parse tests; `cargo test --workspace`; `cargo test -p deslop-mcp
--features slim-llm -- --test-threads=1`; `cargo build --workspace --all-targets --all-features`;
`cargo clippy --workspace --all-targets --all-features -- -D warnings`; warnings-denied workspace
rustdoc; `cargo fmt --all -- --check`; `git diff --check`; `jj status`; and `jj diff --stat`.

**Verification results:** PASS. `deslop-parse` has 66 passing tests, including ten focused successor
contracts. On the pinned 67-byte to 70-byte two-edit fixture, derived evidence is canonical
`34..61 => 34..64`, Tree-sitter reports structural `40..64`, and all 49 changed-file nodes expire;
the verified sequential script records `34..37 => 34..40` then intermediate `59..64 => 59..64`,
Tree-sitter reports no structural changed range, and exactly 24 nodes re-anchor while 25 expire. The
unchanged 13-byte peer shares its file Arc, retains all ten keys with fresh NodeIds, and records ledger
`1 requested / 1 owner / 0 invocation / 1 reuse`; edited files record `1/1/1/0`. Partial TypeScript
repair re-anchors 7 of 20 nodes and expires 13, including the insertion-point recovery node. Empty to
22-byte Rust expires its sole old root; valid to invalid UTF-8 rebuilds with zero invocation and
expires every syntax node; invalid no-op reuse and invalid-to-valid recovery have pinned counts.
Rename is deterministically `Added(new)` plus `Removed(old)` with no cross-path transition. Duplicate
append/prepend histories prove derived evidence never authorizes identity and every exact re-anchor
lands only in the history-consistent occurrence. Workspace tests and doc-tests pass with one
intentional ignored slow probe; feature-enabled MCP has 23 passing tests. All build, strict clippy,
rustdoc, formatting, and whitespace gates pass.

**Failure modes / invalidated assumptions:** Tree-sitter changed ranges were rejected as a byte diff
or complete invalidation set because same-shape token edits, trivia edits, and some deletions produce
empty structural ranges. An LCP/LCS-derived splice was rejected as node-identity proof because
duplicate final bytes do not reveal whether insertion/deletion occurred before or after an identical
subtree. Raw Tree-sitter identity alone was rejected because context-sensitive aliases and public
structure can change; every public invariant is rechecked. Span/kind proximity, fuzzy baselines,
collision ordinals, nearest-node matching, and `has_changes` were rejected as authority. Sequential
edit ranges cannot be unioned because each is relative to a different intermediate state; callers
must use the canonical old-to-final invalidation. Counting an incremental parse as `reused=1` was
rejected because that counter denotes whole-file zero-parser reuse, while the incremental change kind
already records old-Tree use. Publishing a no-Tree incremental result while cold construction failed
differently was rejected; both construction paths now fail.

**Current recommendation/checkpoint:** M1.8 is complete. Implement M1.9 by migrating analyzer and
metrics consumers to one shared snapshot/analysis. Rebuild edited-file projections and project-level
dependencies under the new analysis identity even when some nodes re-anchor; use declared reset
boundaries and exclusive ownership to eliminate current overlapping region parse/metric amplification.

**Blockers:** none. Serena still indexes this Rust workspace as Python-only, so local Rust inspection
remains the documented fallback. M1.8 proves correctness and bounded parser reuse, not project-scale
latency: successor assembly still rebuilds flat node ranges/keys across the project and edit-script
validation is currently O(K*B). M1.11/M9 own measurement and compaction.

**Dependencies/restart:** rebuild Rust consumers for the additive API. No new dependency, wire
schema, service restart, cache clear, or migration is required. M1.9/M1.10 own consumer projection
migration and dependency invalidation; M1.11 owns parse/reuse/latency/memory instrumentation; M2 owns
semantic adapter/query packs. Existing work orders and revision guards always remain expired across a
successor regardless of node transition outcome.

**Negative-memory status:** stored in Hindsight. Never treat structural changed ranges or a derived
old/new splice as edit provenance; never re-anchor through proximity, fuzzy fingerprints, collision
matching, raw kinds alone, or persisted Tree-sitter IDs; never union sequential intermediate ranges;
never count old-Tree incremental parsing as whole-file reuse; and never convert transition-local
correlation into projection or write authority. Recheck only if the edit-history authority or pinned
Tree-sitter contract changes.

**Signature:** Codex (GPT-5), M1.8 integration owner, 2026-07-13.

---

## M1.9 partial checkpoint — owned adapter facts and primary metrics analysis

**Date/time:** 2026-07-13T21:04:12+02:00

**Objective/target:** begin migrating analyzer and metrics from repeated legacy parsing to one shared
`ProjectAnalysis`, without exposing borrowed Tree-sitter nodes or duplicating `LangPack` semantics.

**Changes:** added parse-owned `SyntaxAdapterFacts` projection. It evaluates all existing node-based
language-pack hooks inside `deslop-parse` against the retained private Tree, selects the pack from the
stored `GrammarSelection` language, validates full Tree/arena cardinality, and returns only owned facts
aligned to existing `NodeId`s. Added primary `metrics_analysis(&ProjectAnalysis, MetricsConfig)` and an
owned `MetricFile` context using pinned text, `NodeView` traversal, and one bulk adapter-fact map. Region
discovery and AST complexity have owned implementations with no path read or parser call. The M1.9
execution plan now records the shared-analysis boundary, terminal validation, ownership requirements,
and read/external/discovery constraints. Legacy `metrics_paths`/`metrics_source` remain temporarily in
place and M1.9 is not marked complete.

**Commands/checks run:** targeted analyzer/metrics/parse/source/roadmap inspection; Hindsight M1.9
search; three read-only audit tracks; focused `cargo check`; `cargo test -p deslop-metrics`; combined
`cargo test -p deslop-parse -p deslop-metrics`; formatting and whitespace checks; `jj status`; and
`jj diff --stat`.

**Results:** PARTIAL PASS. Parse has 66 passing tests and metrics has 20. The new behavioral-Python
primary-path test builds one snapshot/analysis, obtains five regions twice with byte-identical JSON,
records exactly one parser invocation, leaves the parse ledger unchanged across both projections, and
leaves the legacy parse counter at zero. All focused formatting and whitespace gates pass.

**Invalidated assumptions / negative memory:** parse elimination alone does not complete the metrics
migration. The owned implementation still slices full nested spans and recursively walks full nested
subtrees, so the 364-byte/12-NLOC Python fixture still expands to the invalid 649 summed bytes/21 NLOC.
The required next step is reset-aware ownership at semantic enclosing owners plus an explicit
single-owner physical-line policy. Naively feeding legacy directory scopes to the snapshot builder is
also invalid because legacy walkers honor ignore files while current snapshot discovery does not.
Live-path external analyzer results cannot enter a revision-bound projection; inputs must be mirrored
and pinned or capability must be unavailable. Projection identity must bind `ProjectAnalysisId` plus
config/adapter/external capability. These constraints are stored provisionally in Hindsight.

**Current recommendation/next actions:** finish metrics exclusive ownership and its exact 364-byte,
12-NLOC conservation oracle; add centralized/versioned root and discovery/read-plan construction;
then implement primary analyzer context/projection and migrate every agnostic/token/Rust/boundary/
suppression traversal. Only after path compatibility adapters delegate through one snapshot and all
legacy consumer parser calls are gone should M1.9 be checked.

**Blockers:** no external blocker. The remaining boundary is substantial and intentionally not hidden:
external analyzers and config/build artifacts need a snapshot-pinned read manifest before their
results can retain authority.

**Signature:** Codex (GPT-5), M1.9 integration owner, partial checkpoint, 2026-07-13.

---

## M1.9 metrics projection checkpoint — exclusive ownership and exact adapters

**Date/time:** 2026-07-13T21:33:44+02:00

**Objective/target:** complete the primary metrics half of M1.9 on one immutable shared analysis,
with deterministic projection identity, exact stored language adapters, reset-aware evidence, and
terminal numerical/error contracts before beginning analyzer rule migration.

**Changes:** `SnapshotEntry::Source` now retains the exact selected `LangPack` plus its versioned
name/schema identity; `ProjectSnapshotBuilder` accepts an injected registry and uses it for overlay
validation, discovery, and grammar resolution. `SyntaxAdapterFacts` uses that stored adapter and
validates every private Tree node against its owned arena slot across visible/grammar kinds and IDs,
byte/point spans, fields, and recovery flags. `ProjectAnalysis::derive_projection_id` binds the
analysis, canonical policy bytes, capability bytes, and sorted per-path adapter identities.
`MetricsProjection` owns the `Arc<ProjectAnalysis>`, config, ID, and unchanged `/5` report.

Metrics now resolves semantic reset collisions, folds exclusive ranges once, tokenizes each pinned
file once with absolute offsets, assigns every token to one reset/File owner, assigns each physical
line once, excludes nested reset subtrees from outer AST evidence, and retains exact File residual.
The line policy selects the earliest semantic metric owner occurring on a nonblank line and otherwise
uses File residual; it preserves prefixed TypeScript/TSX callable NLOC while same-line nested Rust is
charged only to the outer callable. Legacy source/path APIs remain temporarily present; analyzer and
planner migration are still required before M1.9 completion.

**Commands/checks run:** `cargo test -p deslop-lang -p deslop-parse -p deslop-metrics`; repeated
focused metrics and adapter tests; `cargo check -p deslop-parse -p deslop-metrics`; `cargo fmt --all`;
and `cargo clippy -p deslop-metrics --all-targets -- -D warnings`.

**Results:** PARTIAL PASS. Language tests pass 1/1, parse passes 67/67 after the decisive same-`Lang`
two-adapter regression test, and metrics passes 27/27. The Python fixture conserves 364 bytes and 12
NLOC across File plus five semantic owners. Cold parse ledger is exactly `1/1/1/0`, repeated
projections leave it unchanged, legacy parsing remains zero, independent analyses with reversed input
order produce identical projection/report identities, sigma changes projection identity, malformed
TS/TSX plus valid Rust yields a clean Partial report with no project-relative claims, and strict
metrics clippy passes.

**Invalidated assumptions / negative memory:** selecting semantic hooks via `pack_for_lang` is
invalid because multiple packs may share one `Lang`; exact snapshot-selected adapter identity is now
the authority. Tree/arena cardinality alone is insufficient; full slot parity is required. Tokenizing
exclusive ranges independently is invalid because operator/comment state crosses arena boundaries;
tokenize once source-wide, then attribute by absolute token start. Semantic enclosing spans are not
one-to-one (nested Clojure callables collide), so reset candidates require explicit one-to-one owner
resolution. Post-hoc display-path rebasing remains invalid because paths affect ordering, ranks,
fingerprints, messages, and suppression.

**Current recommendation/next actions:** implement `PreparedAnalyzerAnalysis`, `AnalyzerFile`, and an
owned `AnalyzerProjection`; migrate provenance, agnostic traversals, token/range logic, Rust rules,
suppression, duplication, boundary, and external capability handling without `parse_source`, path
reads, or `pack_for_lang`. Then implement the shared root/discovery/read/presentation planner and make
both metrics/analyzer path APIs one-snapshot adapters.

**Blockers:** none. Serena remains unavailable for Rust symbols, so local Rust inspection is the
documented fallback. A bare analysis cannot prove complete boundary/external input capture; the
prepared analyzer manifest must make unavailable/incomplete coverage explicit. The no-grammar custom
text-pack analyzer contract also needs an explicit snapshot text-source representation or a documented
M2 invalidation; a hidden legacy fallback is not acceptable.

**Dependencies/restart:** rebuild Rust consumers for the additive adapter identity, registry builder,
projection ID, and metrics projection APIs. No new dependency, service restart, cache clear, or data
migration is required.

**Negative-memory status:** recorded locally in this report; Hindsight consolidation follows this
checkpoint. Never reintroduce per-range tokenization, `pack_for_lang` reconstruction, count-only
Tree/arena pairing, collided reset ownership, or post-hoc path rebasing.

**Signature:** Codex (GPT-5), M1.9 integration owner, metrics projection checkpoint, 2026-07-13.

---

## M1.9 analyzer checkpoint — owned source-only projection

**Date/time:** 2026-07-13T21:49:33+02:00

**Objective/target:** migrate the analyzer's file-local and duplication passes onto one immutable
`ProjectAnalysis`, prove exact stored-adapter dispatch and projection identity, and establish an
explicit authority boundary for not-yet-pinned config/external inputs.

**Changes:** added `AnalyzerFile` over a retained `ParsedFile`, exact stored `LangPack`, owned
`SyntaxAdapterFacts`, and `NodeId` lookup. Added `AnalyzerProjection` and `scan_analysis`, binding the
effective config and owned analyzer capability schema through `ProjectionId`. Primary dispatch uses
the exact stored adapter name, never path or `Lang` reselection. The projection canonicalizes legacy
suppression match roots away because primary paths are logical. Enabled boundary analysis is rejected
until a complete pinned manifest exists; optional externals are recorded unavailable and never run
against live paths.

Agnostic string/comment/constant masking, long-method regions, tail-return ancestry, comment policy,
and token duplication now use NodeViews/facts. Duplication derives masks and behavioral segments from
the owned tree, including the Rust pure-path-mapping exclusion, and cross-file duplication reuses the
same contexts. Rust redundant-closure/needless-clone field traversal is owned. Python, JS/TS, Clojure,
and Julia text rules consume a pinned compatibility view. Inline directives recurse only through
owned comment nodes, so marker strings cannot suppress findings. Partial syntax is quarantined before
rule execution.

**Commands/checks run:** focused matrix/mask/cross-file/dispatch tests; `cargo fmt --all -- --check`;
`cargo test -p deslop-analyzer -p deslop-parse`; and
`cargo clippy -p deslop-analyzer -p deslop-parse --all-targets -- -D warnings`.

**Results:** PARTIAL PASS. Analyzer passes 62/62 and parse passes 68/68; strict all-target clippy and
format checks pass. The terminal matrix owns five revisions at exact cold ledger `1/1/1/0`, produces
nine pinned findings across valid Rust/Python and malformed TS, keeps TSX clean, is byte-deterministic
across repeated projections, invalidates identity and the threshold-equality finding when Python NLOC
policy changes, leaves the ledger unchanged, and records zero legacy parser calls. Additional tests
pin Rust NodeView idioms, masks plus positive/negative inline suppression, complete cross-file
duplication, adapter-only ProjectionId invalidation, canonical suppression-path identity, and exact
Python/JS/TSX/Clojure/Julia stored-adapter dispatch.

**Invalidated assumptions / negative memory:** a bare `Arc<ProjectAnalysis>` cannot authorize
repository-negative config-boundary claims, so enabled boundary analysis must fail until a complete
manifest is supplied. Live external paths cannot enter an immutable projection; unavailable is the
only current authoritative result. `Lang` dispatch and private suppression match roots are invalid
projection inputs. Worker-thread legacy parse counters alone are insufficient evidence, so source
guards and ledger oracles remain required. Silently dropping same-analysis Node lookup failures was
rejected; owned IDs are invariants and now use explicit expectations in the agnostic/inline path.

**Current recommendation/next actions:** add `PreparedAnalyzerAnalysis` with complete boundary input
coverage, presentation map, and revision-bound external plans; migrate boundary parsing and external
execution to pinned bytes/mirrors. Then implement the shared planner, cut over analyzer/metrics path
APIs, remove or privatize the reparsable `SourceFile` bridge, and enforce static no-read/no-parse/
no-reselection guards on the primary surface.

**Blockers:** no external blocker. Boundary artifacts and external build environments are not yet
represented as a complete pinned manifest. The registered no-grammar text-pack contract still needs
an explicit snapshot text-source representation or documented M2 invalidation.

**Dependencies/restart:** `deslop-analyzer` now uses the workspace's existing `serde_json` dependency
for canonical effective-config bytes. Rebuild Rust consumers. No service restart, cache clear, or data
migration is required.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never reintroduce
per-pass parsing, live external/boundary reads, Lang/path pack reconstruction, marker-string inline
suppression, hidden boundary completeness, or suppression-root-dependent primary identity.

**Signature:** Codex (GPT-5), M1.9 integration owner, owned analyzer checkpoint, 2026-07-13.

---

## M1.9 planner and prepared-analyzer checkpoint

**Date/time:** 2026-07-13T22:13:36+02:00

**Objective/target:** centralize compatibility path planning for metrics/analyzer, pin boundary inputs,
and make the default analyzer path a one-snapshot owned projection without live rereads, reparses, or
forgeable project-level completeness.

**Changes:** added `ProjectSnapshotPlanner` with explicit/auto root and repository authority,
requested/exact-logical scopes, canonical versus legacy-ignore discovery, source/analysis overlays,
deduplicated one-read multi-role entries, and presentation mapping. Auto root rejects multi-repository
scope and uses VCS identity from normalized remote/root commits when available, falling back to a
path-bound local identity only without VCS evidence. Metrics and analyzer path APIs now build one
planner snapshot and one `ProjectAnalysis` before delegating.

Added an opaque `PreparedAnalyzerAnalysis` with a private input manifest and boundary completeness
witness. Boundary discovery pins all TOML/YAML/JSON candidate bytes as analysis inputs; policy filters
well-known tool artifacts later. Boundary code evidence was ported from borrowed Tree nodes to
`NodeId`/`NodeView` parents, ordered children, spans, kinds, and pinned text. The pass has no
`parse_source`, `SourceFile::read`, or live artifact read. A single cached `AnalyzerFile` vector now
serves local rules, cross-file duplication, and boundary analysis. Presentation paths enter projection
policy before finding construction, suppression, fingerprints, messages, and sorting. Optional
externals remain explicitly unavailable unless a future revision-isolated plan is prepared; no live
process is run.

**Commands/checks run:** focused planner, boundary, presentation, partial, invalid-UTF-8, and
revision-pinning tests; `cargo test -p deslop-parse -p deslop-metrics -p deslop-analyzer`;
`cargo clippy -p deslop-analyzer -p deslop-parse -p deslop-metrics --all-targets --all-features -- -D warnings`;
`cargo fmt --all -- --check`; and a static `rg` guard proving `boundary.rs` contains neither
`parse_source` nor `SourceFile::read`.

**Results:** PARTIAL PASS. Parse passes 72/72, metrics 27/27, and analyzer 66/66; strict clippy,
formatting, and the boundary static guard pass. Planner tests prove repository-bound root selection,
cross-repository rejection, exact-logical overlays, canonical/legacy discovery, one disk read across
compatible roles, presentation preference, and VCS identity normalization. Prepared boundary tests
pin exactly `config-key-unread`, `config-key-unconsumed`, and `config-key-shadowed`, keep a live key
quiet, retain a cold `1/1/1/0` parse ledger, record zero legacy parses, remain deterministic after disk
mutation, and change projection identity only after rebuilding changed bytes. Partial syntax and
invalid UTF-8 boundary artifacts produce explicit downgrade reports and zero boundary claims.

**Invalidated assumptions / negative memory:** a public `Complete` enum is not a completeness proof;
the witness is now private and planner-produced. Presentation cannot be post-hoc or omitted from
projection identity because it changes paths, fingerprints, peer messages, suppression, and ordering.
Project-negative boundary findings cannot ignore partial source files. Reconstructing an analyzer
view for each pass violates the one-adapter-projection intent even without reparsing, so views are now
cached once. Silently skipping unreadable/invalid config bytes is not authoritative; invalid UTF-8 is
an explicit failed analysis input.

**Current recommendation/next actions:** replace the internal `SourceFile` compatibility member with
a non-reparsable pinned text view and add static no-read/no-parse/no-pack-reselection guards for both
primary path projections. Resolve the registered no-grammar text-pack snapshot contract explicitly
or record a scoped M2 invalidation. Then run workspace-wide gates and check M1.9 only if every stated
acceptance condition holds.

**Blockers:** none external. The internal analyzer text bridge is still reparsable and the custom
no-grammar test pack cannot yet enter `ProjectAnalysis`; both are deliberate remaining M1.9 work.

**Dependencies/restart:** no new dependency, service restart, cache clear, or data migration. Rebuild
Rust consumers for the new planner and prepared projection behavior.

**Negative-memory status:** recorded locally and ready for Hindsight consolidation. Never expose a
forgeable completeness flag, rebase display paths after projection, run boundary on incomplete
projects, reconstruct per-pass adapter facts, or analyze live artifact bytes.

**Signature:** Codex (GPT-5), M1.9 integration owner, planner/prepared checkpoint, 2026-07-13.

---

## M1.9 terminal checkpoint — analyzer and metrics snapshot migration complete

**Date/time:** 2026-07-13T22:46:19+02:00

**Objective/target:** finish the analyzer/metrics migration with no reparsable consumer bridge,
prove compatibility entry points delegate through one owned snapshot, resolve the grammarless test
adapter honestly, and pass workspace-wide acceptance gates.

**Changes:** replaced analyzer-held `SourceFile` values with non-reparsable `AnalyzerText` views and
removed the obsolete `AnalysisPack`/rule shim plus every legacy analyzer/token/Rust/metrics parse
pipeline. Analyzer and metrics `SourceFile` compatibility APIs now build an exact single-source
overlay snapshot, construct one `ProjectAnalysis`, and delegate to owned consumers. Added a planner
helper for virtual sources with zero disk reads and exact caller presentation, including the path
preservation needed by suppression globs. Added production-wide static guards against `parse_source`,
live reads, and pack reselection, plus deterministic zero-legacy-counter compatibility tests.

Removed the test-only grammarless `.testpack` analyzer shim. Snapshot publication now rejects a
registered adapter without a grammar artifact with an exact diagnostic. Honest grammarless text
analysis is scoped to M2.1's versioned capability contract; it cannot bypass `ProjectAnalysis`.
Updated proposal corpus contracts to encode that live, unpinned clj-kondo output is unavailable:
five capability entries remain visible, while the two live `unused-namespace` claims and one live
`redundant-do` claim no longer enter work orders.

**Commands/checks run:** focused single-overlay/no-grammar/static-guard/source-compatibility tests;
`cargo test -p deslop-parse -p deslop-analyzer -p deslop-metrics`; `cargo test --workspace`;
`cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`cargo build --workspace --all-targets --all-features`; `cargo doc --workspace --no-deps`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. Parse passes 74/74, analyzer 67/67, and metrics 28/28. The full workspace passes
with one deliberately ignored slow performance probe; strict clippy, build, rustdoc, formatting, and
whitespace checks pass. Cold owned parse ledgers remain `1/1/1/0`, repeated consumers do not change
them, compatibility adapters record zero legacy parse invocations, and all primary production files
pass the static snapshot-bypass guard.

**Invalidated assumptions / negative memory:** installed external tools are not revision evidence;
workspace goldens must not depend on live clj-kondo availability. A grammarless `LangPack` does not
have sufficient identity or syntax authority to enter `ProjectAnalysis`. Keeping dead legacy rules
behind a public pack trait still preserves a reparsing route and is not an acceptable migration.
Presentation candidates cannot choose lexicographically over an explicit source API display path,
because that changes suppression and proposal identity.

**Current recommendation/next actions:** begin M1.10 by inventorying graph, evaluator, LSP,
MCP/protocol, and slim parse/read/reselection surfaces, then migrate them through the same planner and
owned projection boundary. Keep external execution unavailable until a revision-isolated prepared
plan exists. M2.1 must define `TextSource` capability semantics before grammarless adapters return.

**Blockers:** none for M1.9. No service restart, cache clear, or data migration is required; Rust
consumers require a rebuild, already covered by the workspace build gate.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never restore legacy
consumer parsing, live external findings, grammarless generic-grammar fallback, or post-hoc display
path rebasing.

**Signature:** Codex (GPT-5), M1.9 integration owner, terminal checkpoint, 2026-07-13.

---

## M1.10 graph checkpoint — owned downstream projection

**Date/time:** 2026-07-13T22:55:32+02:00

**Objective/target:** migrate graph construction from per-file reads, pack reselection, legacy parses,
and borrowed Tree-sitter nodes to the shared immutable project analysis with exact output parity.

**Changes:** added `GraphProjection` and `graph_analysis(Arc<ProjectAnalysis>, GraphConfig)`, binding
config and presentation to a derived projection ID. Rebuilt `graph_paths` as a shared-planner adapter.
Introduced graph-local `GraphFile`/`OwnedNode` facades over pinned text, `ParsedFile`, `NodeId`, and
`NodeView`; all extraction, symbol, binding, import, inheritance, and call traversal now uses them.
Exact stored grammar language drives module identity. Removed graph's direct `deslop-lang`, `ignore`,
and `tree-sitter` dependencies. Tightened `NodeView::raw_kind`'s returned lifetime to the retained
analysis, allowing downstream views to borrow stored kind strings without exposing parser nodes.

**Commands/checks run:** `cargo test -p deslop-graph`; graph owned-ledger/static-guard tests;
`cargo clippy -p deslop-graph -p deslop-parse --all-targets -- -D warnings`;
`cargo test -p deslop-cli --test graph_resolution`; and
`cargo test -p deslop-cli --test m0_definition_of_done`.

**Results:** PASS. Graph passes 24/24 and strict clippy. The CLI ambiguity/import probes and exact M0
21-file/74-symbol/197-edge/123-syntactic vector pass. Repeated graph projections over two files keep
identical IDs/JSON, unchanged cold ledgers of `1/1/1/0` per revision, and zero legacy parser calls.
Static production guards reject legacy parse/read/reselection and `tree_sitter::Node`.

**Invalidated assumptions / negative memory:** retaining a graph-specific borrowed-node extractor is
not harmless merely because the output is owned; it reparses and loses the snapshot's exact grammar
authority. Rediscovering supported files with graph's own walker also creates a second scope/read
policy. Graph raw-kind logic remains a future M2 adapter concern, but its execution authority now
comes only from the stored analysis.

**Current recommendation/next actions:** retain analysis and presentation in analyzer `ScanContext`,
then make protocol proposal grouping use pinned text and owned enclosing-region facts rather than
`SourceFile::read`, `analysis_provenance_or_failed`, or `enclosing_region_for_span`.

**Blockers/dependencies/restart:** none. Cargo lock removed three now-unused graph dependencies. No
service restart or migration is required; rebuild Rust consumers.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never restore a graph
walker/parser, borrowed syntax nodes, or display-path adapter selection.

**Signature:** Codex (GPT-5), M1.10 integration owner, graph checkpoint, 2026-07-13.

---

## M1.10 protocol/evaluator checkpoint — pinned proposal consumers

**Date/time:** 2026-07-13T23:01:57+02:00

**Objective/target:** eliminate protocol's post-analysis source reread/reparse and evaluator's
per-case compatibility scans while preserving proposal identities, corpus scoring, MCP, and slim.

**Changes:** retained `Arc<ProjectAnalysis>` and `SnapshotPresentationMap` in analyzer `ScanContext`
and `AnalyzerProjection`. Protocol now builds proposal text views from `ScanContext::input_contents`,
inverts the retained presentation map to logical paths, finds the smallest owned containing node,
and uses its stored `SyntaxAdapterFacts::enclosing_region`. Proposal source revision guards derive
from pinned bytes. Removed `SourceFile::read`, post-scan `read_to_string`, provenance parsing, and
production `enclosing_region_for_span` use. Evaluator now sends all manifest cases through one
`scan_paths_with_config` call and scores retained reports, instead of one `scan_file` snapshot per
case. MCP and slim continue to delegate through these migrated surfaces.

**Commands/checks run:** analyzer/protocol/evaluator unit suites; proposal static guard and repeated
zero-legacy-counter test; evaluator baseline zero-legacy test; strict analyzer/protocol/evaluator
clippy; MCP and slim suites; CLI proposal and M0 definition-of-done tests; format and whitespace.

**Results:** PASS. Analyzer 67/67, protocol 18/18, evaluator 3/3, MCP 20/20, slim 22/22, proposal
CLI 6/6, and M0 numeric contract pass. Repeated proposals are byte-identical and record zero legacy
parser calls. Existing grouping, nested callable, TSX, stale/tampered context, and baseline behavior
remain unchanged.

**Invalidated assumptions / negative memory:** rereading after analysis is not a stronger proposal
contract; it mixes a second live revision with snapshot findings. The snapshot bytes are proposal
authority, and later revision guards reject stale apply/reconstruction. `SourceFile` may remain a
text/line helper only when constructed from pinned contents; its parse-backed region method is not
an acceptable production consumer. Evaluator batching is required to prove one project parse ledger.

**Current recommendation/next actions:** make LSP `DocumentState` retain its analysis/logical path and
build successor analyses on document changes, then run the final M1.10 cross-consumer guard and full
workspace gates.

**Blockers/dependencies/restart:** none. `deslop-eval` adds only a test dependency on existing
`deslop-parse` for the legacy-counter assertion. Rebuild Rust consumers; no service restart.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never reintroduce
post-scan proposal reads, provenance reparsing, or one-snapshot-per-eval-case loops.

**Signature:** Codex (GPT-5), M1.10 integration owner, protocol/evaluator checkpoint, 2026-07-13.

---

## M1.10 terminal checkpoint — downstream snapshot consumers complete

**Date/time:** 2026-07-13T23:18:00+02:00

**Objective/target:** finish downstream shared-analysis migration by making LSP document state own
immutable analyses and successors, then prove every named consumer avoids production rereads,
reparses, and adapter reselection without changing graph, proposal, evaluator, MCP, slim, or LSP
contracts.

**Changes:** added analyzer's presentation-aware owned entry point for in-memory clients. LSP
`DocumentState` now retains `Arc<ProjectAnalysis>`, `SnapshotPresentationMap`, and its logical path.
Open constructs one source-overlay analysis; change constructs a successor from the retained
predecessor; save with replacement text uses the same successor route; and save without text reruns
the analyzer projection over the retained revision. The document-only policy explicitly disables
config-boundary claims because no complete project artifact manifest exists. Handler failures now
propagate instead of publishing results from a failed replacement analysis. Added lifecycle and
production static-guard tests.

The terminal consumer audit confirms graph uses its owned projection, protocol groups from retained
analysis and pinned bytes, evaluator batches its manifest, and MCP/slim delegate to the migrated
planner/proposal surfaces. Remaining MCP/slim reads are explicit config, JSONL, provider response,
apply, or stale-state recheck I/O rather than analysis input reconstruction.

**Commands/checks run:** `cargo test -p deslop-lsp`; strict LSP/analyzer clippy; analyzer tests;
cross-consumer `rg` audit; `cargo test --workspace --all-features`;
`cargo build --workspace --all-targets --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features`;
`cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. LSP passes 10/10, including UTF-16 edits, malformed-source quarantine, TSX grammar
selection, JSON-RPC diagnostics/actions, the production ownership guard, and the new revision
lifecycle oracle. Both cold and changed revisions have exact `1/1/1/0` ledgers; the predecessor
remains immutable; save without text preserves analysis identity and counts; and the legacy parser
counter stays zero. The all-feature workspace suite passes with one deliberately ignored slow probe.
Build, warnings-denied rustdoc, strict all-target/all-feature clippy, formatting, and whitespace pass.
The existing exact M0 graph/proposal numerical contracts pass inside the workspace suite.

**Invalidated assumptions / negative memory:** the old single-source analyzer adapter silently
disabled boundary analysis; direct owned callers must express that document-only policy explicitly.
Reopening a document from scratch on every change produces correct findings but discards incremental
ownership and predecessor evidence. Save without content is a policy refresh over pinned bytes, not
authority to reread or reparse the file. Production guards plus owned ledgers remain necessary because
test-only compatibility helpers intentionally still exercise legacy APIs.

**Current recommendation/next actions:** execute M1.11 with one instrumented cold/repeated/incremental
matrix. Add missing retained-memory and lookup-allocation counters at the `ProjectAnalysis` boundary,
lock structural invariants in ordinary tests, and keep noisy latency measurement in an explicit probe.
Use measured decomposition to decide compaction rather than starting serial micro-optimizations.

**Blockers/dependencies/restart:** none. No new dependency, service restart, cache clear, or data
migration is required. Rebuild LSP/analyzer consumers; the workspace build already verified this.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never restore
per-change fresh LSP analysis, document-level boundary authority, save-time source rereads, or
consumer-specific parsing/pack selection.

**Signature:** Codex (GPT-5), M1.10 integration owner, terminal checkpoint, 2026-07-13.

---

## M1.10 corrective checkpoint — one LSP workspace overlay generation

**Date/time:** 2026-07-13T23:22:06+02:00

**Objective/target:** correct the terminal LSP migration after the M1.11 memory audit surfaced M1.1's
active constraint that separate dirty-document snapshots mix workspace revision authority.

**Changes:** moved retained `ProjectAnalysis` and presentation ownership from each `DocumentState` to
`LspState`. Every open document is now an exact-logical overlay in one planner-built workspace
snapshot. Open, change, and close build immutable successors atomically; unchanged documents are
reused and changed documents parse once. Save without replacement text reruns analyzer policy over
the same retained generation. Reports are applied back to every open document, and open/change/save/
close publish diagnostics for all buffers because cross-file findings can change. URI map keys are
stable strings rather than the interior-mutable protocol URI type. Added a two-dirty-document oracle.

**Commands/checks run:** focused LSP tests; strict LSP/analyzer all-target/all-feature clippy;
`cargo test --workspace --all-features`; `cargo build --workspace --all-targets --all-features`;
warnings-denied all-feature workspace rustdoc; strict workspace all-target/all-feature clippy;
formatting; and whitespace checks.

**Results:** PASS. LSP passes 11/11. Adding a second dirty document produces one two-file successor
with one newly parsed file and one reused file. Editing one of two documents produces one parser
invocation plus one reuse in the same generation; both logical paths resolve in that analysis, both
predecessors remain immutable, and the legacy parse counter stays zero. The all-feature workspace
suite passes with one intentionally ignored slow probe; build, rustdoc, clippy, format, and whitespace
also pass.

**Invalidated assumptions / negative memory:** preserving old file-local LSP behavior was not enough
to satisfy snapshot ownership. Per-document `ProjectAnalysis` values are invalid when multiple dirty
buffers may participate in cross-file analysis. Publishing only the changed buffer is also invalid
once analyzer reports share a workspace generation. The earlier M1.10 memory statement that
`DocumentState` should retain its own analysis is superseded by this checkpoint.

**Current recommendation/next actions:** resume M1.11 inventory against the workspace-corrected
analysis boundary and build the single cold/repeated/incremental instrumentation matrix.

**Blockers/dependencies/restart:** none. No dependency, service restart, cache clear, or migration.
Rebuild the LSP binary; the workspace build already verified it.

**Negative-memory status:** corrective negative memory recorded locally; Hindsight correction follows.
Never restore one snapshot per dirty document or publish only one buffer after a workspace generation
changes.

**Signature:** Codex (GPT-5), M1.10 integration owner, workspace correction, 2026-07-13.

---

## M1.11 terminal checkpoint — ownership instrumentation and measured compaction

**Date/time:** 2026-07-13T23:55:49+02:00

**Objective/target:** instrument parse ownership, deterministic traversal, latency, visible retained
memory, query/aggregation costs, and incremental update work on one convergent cold/repeated/
incremental matrix; compact only measured costs and declare the owned traversal surface migration-ready.

**Changes:** added identity-neutral `ProjectAnalysis` parse/structure/memory reports with a pinned node
order digest and deterministic lower-bound byte accounting. Added query source/metadata/result,
aggregation callback/value, allocation-free point-context, and successor edit/rebuild/transition
reports. `NodeKey` now shares one `Arc<FileRevisionKey>` per file and interns exact field paths while
preserving its wire schema. Compact digest/index entries replace linear key lookup; file range and node
range lookup are binary/partition searches. `NodeView::children` and exact zero-width point results are
allocation-free exact-size iterators. Query execution reuses a validated retained Tree-sitter-id index
instead of rebuilding a preorder vector and hash map, and capture results share query-owned names.
All-descendant aggregation aliases its full projection instead of retaining a duplicate declared
projection. Query-index construction failures propagate as typed build errors rather than panics.

**Commands/checks run:** focused parse/query/incremental tests and strict parse clippy throughout;
the ignored M1.11 probe once before compaction, after each representation change, and five times at
the terminal checkpoint; `cargo test --workspace --all-features`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`;
`cargo build --workspace --all-features --all-targets`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. The ordinary deterministic oracle locks 3 files, 188 source bytes, 94 nodes, 91
child edges, exact parse ownership, and digest
`pao1_437c1bdc53a43224fde0a0c23fcebbca531996848a87585944f60fe5759c55ed`.
Node-key storage falls from 75,873 to 36,195 bytes: shared file payload is 552 bytes and interned field
paths are 7,986 bytes. The final visible retained lower bound is 61,900 bytes versus 98,234 before
compaction, 36,334 bytes (37.0%) lower while including new 1,880-byte key and 1,504-byte query indices.
The query probe retains 415 visible metadata bytes and 202 bytes for four capture results. The exact
one-file update visits 94 predecessor/94 successor nodes, rebuilds 33 edited-file nodes, retains 61,
reanchors 16, expires 17, stores 2,256 transition bytes, and bounds sequential validation at 132
bytes. Five terminal timing samples span cold 3,848–7,065 us, repeated 1,944–3,789 us, and incremental
3,070–6,146 us; these noisy values are reported but never asserted. All workspace gates pass.

**Invalidated assumptions / negative memory:** right-sizing the point-result vector was not meaningful
compaction; borrowing the retained containment slice removes the allocation. Rebuilding a Tree-sitter
preorder vector/hash map for every query was unnecessary, but retaining borrowed nodes would violate
the owned boundary; a process-local numeric-id index plus cursor traversal preserves it. Source length
alone is not a memory measure, wall time is not correctness evidence, instrumentation must not enter
identity, and 128-bit key digests require exact-key collision checks before lookup succeeds.

**Current recommendation/next actions:** run M1.DoD over the gold scan/propose matrix and lock the
parse-ledger, borrowed-node, and exclusive-region non-overlap contracts. Begin M2 only after that
terminal M1 proof passes.

**Blockers/dependencies/restart:** none. No dependency, service restart, cache clear, or migration is
required. Rebuild Rust consumers; workspace build already verifies the iterator API migration.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never restore
per-execution query maps, allocating child/point views, per-node revision payloads, redundant
all-descendant projections, timing assertions, or instrumentation-derived identity.

**Signature:** Codex (GPT-5), M1.11 integration owner, terminal checkpoint, 2026-07-13.

---

## M1 definition-of-done terminal checkpoint — joined owned-analysis proof

**Date/time:** 2026-07-14T00:08:24+02:00

**Objective/target:** close M1 with one executable multi-language proof that scan/propose workflows
own each file revision once, repeated projections share immutable analysis, warm reuse invokes no
parser, metric byte/line ownership is exclusive despite nested spans, and no borrowed Tree-sitter or
serializable process-local node identity crosses the public API.

**Changes:** added the joined CLI integration contract over fixed Rust, Python, TSX, Clojure, and
Julia fixtures. It runs the path scanner, analyzer, metrics, and graph twice over one retained
analysis, checks exact projection identity/result stability, validates every public exclusive region
as a gap-free byte partition, builds an unchanged successor, and executes proposal production.
`ProposalBatch` now retains the exact `Arc<ProjectAnalysis>` used to produce its reports/work orders,
making proposal ledger evidence inspectable without a global counter. Added a metrics-private gold
oracle that enumerates every declared reset-owner range and physical nonblank line. Added a public-
surface guard for borrowed Tree-sitter node/cursor signatures and a compile-fail `NodeId: Serialize`
test. The CLI test gains only a direct dev dependency on the existing workspace parse crate.

**Commands/checks run:** focused joined M1, metric ownership, proposal ownership, parse public-surface,
and compile-fail doc tests; strict affected-crate clippy; the unchanged M0 definition-of-done test;
`cargo test --workspace --all-features`; `cargo clippy --workspace --all-features --all-targets --
-D warnings`; `cargo build --workspace --all-features --all-targets`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. The joined oracle locks 5 files, 1,651 source bytes, 746 nodes, 700 gap-free
exclusive syntax regions, 21 analyzer findings, 17 metric regions, a 45-node/49-edge graph, and 9
work orders grouping 17 findings. Cold parse ownership totals are exact `requested/owners/invoked/
reused = 5/5/5/0`; the unchanged successor is `5/5/0/5`, retains all 746 transitions, and preserves
analysis identity. Each disk source is read once. Analyzer, metrics, and graph retain the identical
analysis pointer and repeat deterministically. Proposal's independent cold analysis has the same
exact five-file ownership invariant. The metric oracle assigns all 1,651 bytes and all 67 nonblank
lines once across 17 semantic owners and 700 ranges. The legacy parser counter remains zero. The M0
snapshot remains 28 work orders / 28 IDs / 28 targets / 62 grouped findings. All workspace gates pass.

**Invalidated assumptions / negative memory:** overlapping callable spans are expected and cannot
serve as evidence of metric double counting; acceptance must enumerate the reset-aware exclusive
ranges and lines. A process/thread-global parser counter cannot prove request-local ownership;
`ParseLedger` is authoritative. Protocol work orders alone did not expose their construction ledger;
the non-serialized batch must retain the producing analysis. `NodeView` borrowing `ProjectAnalysis`
is valid, while borrowing a Tree-sitter `Node`/cursor or serializing `NodeId` is not. MCP and slim are
delegated proposal consumers, not independent parser implementations; verifier rereads are stale-
state/write guards, not scan/propose reconstruction.

**Current recommendation/next actions:** begin M2.1 with a versioned S0-S4 adapter/capability schema.
Keep the M0 and M1 joined numerical tests as compatibility gates for every M2 change.

**Blockers/dependencies/restart:** none. No external dependency, service restart, cache clear, or data
migration is required. Rebuild Rust consumers; the workspace build already verifies the additive
`ProposalBatch.analysis` field and test-only CLI dependency.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never replace local
ledger proof with a global counter, infer exclusivity from nested spans, expose borrowed Tree-sitter
handles, serialize `NodeId`, or split delegated proposal consumers into new parser paths.

**Signature:** Codex (GPT-5), M1 definition-of-done integration owner, terminal checkpoint,
2026-07-14.

---

## M2.1 terminal checkpoint — versioned total adapter capabilities

**Date/time:** 2026-07-14T00:27:24+02:00

**Objective/target:** version one honest S0-S4 capability contract, make every adapter declaration
total and machine-validatable, and bind exact capabilities into derived identity without changing raw
source/snapshot analysis identity.

**Changes:** added `deslop.language-adapter-capabilities/1`, ordered `SemanticTier` and 23-member
`AdapterCapability` catalogs, explicit provided/unsupported/unknown support, four evidence-authority
classes, total declarations, validation, and derived highest-complete-tier logic in `deslop-lang`.
`LangPack` now supplies a capability manifest. Snapshot adapter identities retain the exact validated
manifest, reject adapter-schema mismatches, and include its stable wire values in derived projection
identity. Production syntax packs declare only their existing raw syntax, token/comment, region,
metric, normalization, and recipe surfaces; canonical roles remain unknown. Added an exact JSON vector,
tier truth table, malformed-manifest rejection, complete registry matrix, strict legacy identity
rejection, and capability-only identity invalidation test. Boxed the enlarged stored adapter enum arm
to keep the snapshot entry representation balanced under strict clippy.

**Commands/checks run:** focused `deslop-lang` and parse adapter tests; affected strict clippy;
`cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. The wire catalog has 23 entries with exact S0-S4 counts `6/4/6/5/2`. All seven
registry packs validate with all 23 declarations and honestly derive no complete tier until M2.2
provides canonical roles. Missing and reordered declarations, a provided fact without authority,
authority on an unavailable fact, wrong manifest schema, and mismatched adapter schema fail closed.
A custom capability-only change from unknown to adapter-provided canonical roles advances the complete
tier through S1 because all existing S1 facts are provided, leaves raw analysis identity unchanged,
and changes the derived projection identity. Exact JSON and all workspace gates pass.

**Invalidated assumptions / negative memory:** a pack name and adapter schema alone are insufficient
derived identity once capabilities can change. A default manifest must not silently upgrade test or
third-party packs. Existing syntax and region hooks do not imply canonical roles, existing syntactic
graph output is not S2/S3 semantic authority, TSX remains stored grammar provenance rather than a new
public language, and canonical roles must not enter the raw `NodeKey/1` identity.

**Current recommendation/next actions:** implement M2.2 as a versioned canonical-role view alongside
raw grammar kind and field data. Require fixture-backed total mappings before changing production
packs from unknown to provided canonical-role support.

**Blockers/dependencies/restart:** none. No new external dependency, service restart, cache clear, or
migration is required. Rust consumers must rebuild for the required `LangPack::capability_manifest`
method and expanded serialized adapter identity; workspace build already verifies internal consumers.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never infer a complete
tier from partial syntax, omit unavailable declarations, decouple capabilities from projection
identity, add `Lang::Tsx`, or put canonical roles into `NodeKey/1`.

**Signature:** Codex (GPT-5), M2.1 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.2 terminal checkpoint — canonical roles beside raw grammar facts

**Date/time:** 2026-07-14T00:37:29+02:00

**Objective/target:** define a small stable and composable canonical-role vocabulary without erasing
raw Tree-sitter evidence, changing raw analysis identity, or prematurely claiming production adapter
coverage assigned to M2.6-M2.10.

**Changes:** added `deslop.canonical-roles/1` with 23 ordered roles and a compact set that serializes
in canonical catalog order and strictly rejects wrong schemas, duplicates, reordering, and unknown
fields. `LangPack` gains a default-empty role callback. Added capability-gated
`deslop.canonical-role-projection/1`: it retains the exact `Arc<ProjectAnalysis>` and emits one owned
fact per raw node with `NodeId`, visible kind/id, raw grammar kind/id, parent field, and composed role
set. Unknown or unsupported stored capability returns a typed error rather than an empty confirmed
projection. Refactored the private Tree/raw-arena validation walk so legacy syntax-hook facts and role
facts share the exact same node/span/field/flag mismatch guard. Public role/raw/projection types and
schema constants are re-exported from `deslop-parse`.

**Commands/checks run:** focused canonical role catalog and parse projection tests; affected strict
clippy; `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. The exact role catalog contains 23 roles. The custom retained Rust-grammar fixture
projects 32 nodes, preserves 11 raw parent fields, and emits 22 role assignments. Every projected raw
fact equals its arena `NodeView`; the oracle specifically retains visible `type_identifier` versus
raw grammar `identifier` with field `name`, and composes declaration+callable on a function. Repeated
projection has identical identity/facts and retains the same analysis pointer. Production Rust still
declares canonical roles unknown and fails with typed `CapabilityUnavailable`. Raw analysis identity
and all `deslop.node-key/1` values remain unchanged. All workspace gates pass.

**Invalidated assumptions / negative memory:** canonical roles are not a replacement for grammar
kinds, aliases, numeric IDs, or fields. A default-empty callback is not evidence of support; the
stored capability manifest gates projection. Role policy is derived adapter state and must not enter
`ProjectAnalysisId` or `NodeKey/1`. M2.2 defines the common contract only; claiming per-language
coverage before focused query/mapping fixtures would be false authority.

**Current recommendation/next actions:** implement M2.3 as versioned query-pack declarations for the
six required capture families, compiled through the existing owned Tree-sitter query substrate.
Distinguish syntactic captures from S2 name resolution/control-flow authority and keep production
packs unknown until their language milestones install fixture-backed packs.

**Blockers/dependencies/restart:** none. No dependency, service restart, cache clear, or migration is
required. Rust consumers rebuild for the additive `LangPack::canonical_roles` method and new public
projection types; workspace build already verifies internal consumers.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never erase raw
grammar evidence, authorize an empty set under unavailable capability, mutate raw identity for roles,
or treat syntactic role/query captures as semantic resolution or control-flow proof.

**Signature:** Codex (GPT-5), M2.2 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.3 terminal checkpoint — total versioned query packs

**Date/time:** 2026-07-14T00:46:45+02:00

**Objective/target:** define exact adapter query packs for declarations, references, scopes, control,
comments, and opaque/generated code while preserving unavailable families and preventing syntactic
captures from masquerading as higher-tier semantic proof.

**Changes:** added `deslop.language-query-pack/1` with six ordered families, total declarations,
provided/unsupported/unknown support, authority, exact Tree-sitter source, unique canonical capture
names, and per-capture canonical role sets. `LangPack` now returns a query pack, defaulting all six to
unknown. Snapshot construction validates and stores the exact pack in `LanguageAdapterIdentity`,
rejects adapter-schema mismatch, and length-frames every semantic identity component including
variable capture/role lists. Added `deslop.language-query-projection/1`, which retains its exact
analysis, exposes the total stored pack, compiles provided entries only against the retained grammar,
and requires declared capture order to equal Tree-sitter's compiled catalog. Public schema,
declaration, compiled-family, projection, and error types are re-exported.

**Commands/checks run:** focused query-pack wire/malformed tests; custom adapter compile, execution,
identity, mismatch, and capture-drift tests; all existing owned query tests; affected strict clippy;
`cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. The custom Rust-grammar pack provides all six families and executes exact capture
counts `[1,1,2,1,1,2]` for declarations/references/scopes/control/comments/opaque-generated, eight
owned captures total. Every compiled capture catalog exactly matches its declaration. Execution does
not change the ledger and the sole file remains one parser invocation. Query-only stored policy
changes preserve raw `ProjectAnalysisId` and change `LanguageQueryProjection` identity. Missing,
reordered, payload-incomplete, duplicate-capture, adapter-schema-mismatched, and compiled-capture-
drift inputs fail closed. All seven production registry packs publish total six-entry unknown packs.
All workspace gates pass.

**Invalidated assumptions / negative memory:** capture-family names do not confer name resolution,
scope correctness, CFG edges, or generated-code provenance. Unavailable families must remain visible
rather than become empty successful results. Pack name/schema and capability manifest alone are not
enough derived identity once exact queries vary. NUL concatenation is insufficient for variable
capture/role lists; identity components are length-framed.

**Current recommendation/next actions:** implement M2.4 as an exact token/operator classification and
language lexical-policy contract over owned token regions. Treat the current per-pack Halstead token
arrays as partial seed evidence only, not as a complete classification.

**Blockers/dependencies/restart:** none. No external dependency, service restart, cache clear, or data
migration is required. Rust `LangPack` implementers gain a default query-pack method; serialized
adapter identity now strictly requires `queries`. Workspace build verifies internal consumers.

**Negative-memory status:** recorded locally; Hindsight consolidation follows. Never suppress
unavailable query families, compile against a reselected grammar, omit exact query packs from derived
identity, infer semantic absence from no capture, or promote syntax captures to S2/S3 authority.

**Signature:** Codex (GPT-5), M2.3 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.4 active checkpoint — declarative lexical policy schema

**Date/time:** 2026-07-14T00:52:06+02:00

**Objective/target:** replace text-scanner assumptions with a versioned language policy that can
classify exact raw grammar leaves and operators while keeping trivia gaps and higher semantic claims
out of scope.

**Changes:** added the initial `deslop.language-lexical-policy/1` contract: nine token classes, eight
operator classes, identifier case and Unicode policy, line/block comment delimiters, exact ordered
raw-kind/optional-text rules, structurally valid token/operator pairs, and a required terminal wildcard
for total provided classification. `LangPack` defaults to an all-unknown policy. Snapshot adapter
identity now validates, stores, exposes, and hashes the policy; public types are re-exported. Added a
focused policy oracle covering identifiers, a multi-character comparison operator, comments, fallback,
round-trip, missing fallback, and malformed operator classification. Registry adapters remain unknown.

**Commands/checks run:** `cargo check -p deslop-lang`; focused lexical-policy tests; existing parse
adapter tests; `cargo clippy -p deslop-lang -p deslop-parse --all-features --all-targets -- -D warnings`;
`cargo fmt --all`; and `git diff --check`.

**Results:** ACTIVE / WORKSPACE-WIDE UNVERIFIED. The implemented schema and affected crates pass all
focused checks. M2.4 is deliberately not checked: no analysis-owning leaf projection or numerical
language fixture exists yet, stable enum-string framing must replace temporary debug-formatted lexical
identity components, policy-only invalidation/mismatch tests remain, and full workspace gates have not
run for this active change.

**Invalidated assumptions / negative memory:** Halstead operator arrays are partial metric inputs, not
a token-classification contract. Trivia gaps are byte ownership rather than tokens. Comment substring
search and independent two-character tokenization cannot be lexical authority when the retained grammar
already owns exact leaves.

**Current recommendation/next actions:** add an analysis-retaining lexical projection over positive-
width raw leaves, classify only through the exact stored policy, pin class/operator/comment/Unicode
counts and no-reparse behavior, stabilize identity encoding, then run affected and workspace gates.

**Blockers/dependencies/restart:** none. Work is incomplete by design at this checkpoint; no service
restart or migration applies.

**Negative-memory status:** recorded locally. Never mark M2.4 complete from schema tests alone, reuse
the metrics text tokenizer as authority, classify trivia gaps as tokens, or infer effects/precedence
from lexical operator classes.

**Signature:** Codex (GPT-5), M2.4 integration owner, active checkpoint, 2026-07-14.

---

## M2.4 terminal checkpoint — declarative lexical classification

**Date/time:** 2026-07-14T01:03:19+02:00

**Objective/target:** complete a strict language-owned token/operator policy and an analysis-owned,
parse-once projection without promoting metrics tokenization, trivia gaps, or lexical classes into
semantic authority.

**Changes:** completed `deslop.language-lexical-policy/1` with stable wire/identity strings, explicit
unsupported and unknown states, ordered raw-kind/optional-text matching, same-kind shadow rejection,
and terminal wildcard totality. Completed `deslop.lexical-token-projection/1`: explicitly classified
composite CST nodes own their exact spans and suppress descendants; all other composites traverse to
positive-width leaves. The projection retains its `ProjectAnalysis`, raw syntax facts, exact source
text, stored policy, and framed derived identity. Added exact serialization and malformed-policy
oracles, adapter-schema mismatch rejection, policy-only derived invalidation, Unicode identifier,
full line/block comment, literal, multi-character operator, non-overlap, deterministic repeat, and
no-reparse checks. Production adapters remain explicitly unknown pending M2.6-M2.10.

**Commands/checks run:** `cargo fmt --all`; `cargo test -p deslop-lang`; `cargo test -p deslop-parse
adapter::tests`; affected strict clippy; `git diff --check`; then `cargo test --workspace
--all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc
--workspace --all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D
warnings`; `cargo fmt --all -- --check`; and final `git diff --check`.

**Results:** PASS. The numerical fixture emits 26 non-overlapping token owners: 2 comments, 6
delimiters, 5 identifiers, 3 keywords, 3 literals, 4 operators, 1 other, and 2 punctuation tokens;
operator subclasses are one each arithmetic, assignment, comparison, and logical. Both full comments
are preserved and each source revision has exactly one parser invocation. All workspace gates pass;
only the repository's two explicitly ignored instrumentation/performance probes remain ignored.

**Invalidated assumptions / negative memory:** leaf-only projection is not a valid grammar-token
boundary because comments and other token-like constructs may be composite CST nodes. The failed
attempt emitted only `//`, `/*`, and `*/`, losing comment bodies. Required alternative: select only
adapter-explicit composite rules, claim their full spans, suppress descendants, and otherwise fall
through to leaves. Never invent substring/comment recovery or classify overlapping parent/child spans.
Halstead arrays remain metric seeds only; lexical operator classes do not assert precedence, effects,
or evaluation order.

**Current recommendation/next actions:** start M2.5 by freezing explicit support/authority and policy
contracts for parse errors, unsupported constructs, macros, generated code, and dialects; retain the
same adapter-schema, identity, honest-unknown, and analysis-ownership boundaries.

**Blockers/dependencies/restart:** none. No rebuild, reload, migration, or service restart is needed.

**Negative-memory status:** recorded locally and ready for Hindsight consolidation. Search handles:
`M2.4 leaf-only composite comments`, `lexical token owner descendant suppression`, `Halstead not
lexical authority`. Status: resolved; recheck if a grammar exposes an explicitly classified composite
whose descendants escape its span or source order.

**Signature:** Codex (GPT-5), M2.4 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.5 active checkpoint — construct, recovery, and dialect policy

**Date/time:** 2026-07-14T01:05:46+02:00

**Objective/target:** define machine-readable, adapter-owned policy for parse recovery, unsupported
constructs, macros, generated markers, and exact grammar dialect variants, then expose those claims as
analysis-owned facts without reparsing or path-based grammar reconstruction.

**Changes:** planning only. Selected one versioned aggregate whose five families each preserve explicit
support and authority. Parse facts will derive only from retained error/missing flags; ordered construct
rules will match raw kind plus optional exact text; provided dialect declarations must bind the stored
dialect, grammar id, and grammar version exactly. Production adapters remain unknown until their M2.6-
M2.10 golden matrices establish real claims.

**Commands/checks run:** M2.5-targeted Hindsight negative-memory search; repository-wide `rg` inventory
of error flags, macro/generated roles and queries, dialect storage/dispatch, and unsupported surfaces;
targeted reads of the adapter/capability contracts and M2 architectural plan.

**Results:** ACTIVE / UNVERIFIED. No implementation or capability claim exists in this change yet.

**Invalidated assumptions / negative memory:** no new failed experiment. Existing constraints remain:
query capture labels are not generated provenance, macro syntax is not macro expansion, a path suffix
is not stored dialect authority, and parse recovery must remain visible rather than being suppressed.

**Current recommendation/next actions:** implement the strict schema and identity storage first, then
the retained projection and fixed malformed/dialect fixture, followed by affected and workspace gates.

**Blockers/dependencies/restart:** Serena's active project exposes Python rather than Rust symbols, so
Rust code work continues through targeted local reads and compiler/test oracles. No functional blocker,
restart, migration, or new dependency applies.

**Negative-memory status:** no new M2.5 failure recorded; task-targeted Hindsight search returned no
relevant prior M2.5 invalidation. Local constraints are authoritative for this checkpoint.

**Signature:** Codex (GPT-5), M2.5 integration owner, active checkpoint, 2026-07-14.

---

## M2.5 terminal checkpoint — construct, recovery, and dialect policy

**Date/time:** 2026-07-14T01:14:19+02:00

**Objective/target:** finish explicit policy and retained facts for parse errors, unsupported
constructs, macros, generated code, and exact dialect variants without upgrading syntax labels into
expansion, provenance, or semantic authority.

**Changes:** added strict `deslop.language-construct-policy/1`: parse recovery has explicit
support/authority/handling; unsupported, macro, and generated sections are total and ordered with
exact raw-kind/optional-text rules and opaque/surface handling; dialect declarations bind dialect,
grammar id, and grammar version. Duplicate, shadowed, wildcard, payload-retaining unavailable, missing,
and reordered contracts fail closed. `LangPack` defaults to all unknown. Snapshot adapter identity now
validates, stores, exposes, and stably frames the exact policy. Added
`deslop.construct-policy-projection/1`, which retains `ProjectAnalysis`, stored policy and dialect,
raw facts, exact text, authority, and handling; grammar flags alone produce error/missing facts and
adapter rules alone produce construct facts. Provided dialect drift is typed failure. Public re-exports,
adapter-schema mismatch, missing legacy field, deterministic repeat, policy-only derived identity, and
no-reparse checks are included. Production packs remain unknown for M2.6-M2.10.

**Commands/checks run:** `cargo test -p deslop-lang`; `cargo test -p deslop-parse`; affected strict
clippy; format and whitespace checks; then `cargo test --workspace --all-features`; `cargo build
--workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`;
and `git diff --check`.

**Results:** PASS. The malformed custom fixture locks four facts in source order: generated
`#[generated]` `attribute_item` with surface-syntax handling, opaque `unsafe_block`, opaque
`macro_invocation`, and syntax-authority `ERROR` text `=` with file-incomplete handling. Exact stored
dialect is `same-lang/tree-sitter-rust/test`; claimed mismatch fails. The all-unknown policy produces
zero construct/recovery facts and explicit unknown dialect support. Policy changes preserve raw
analysis identity, change derived identity, and each source revision invokes its parser once. Every
workspace gate passes; only the two repository-designated slow instrumentation probes are ignored.

**Invalidated assumptions / negative memory:** no implementation attempt failed. Durable constraint:
absence of a matching rule is not proof that a construct is semantically absent; unknown and
unsupported remain explicit. Query-generated labels do not establish generated origin, macro syntax
does not establish expansion, and paths cannot override stored dialect identity. M2.4 token-owner
descendant suppression does not apply to construct regions, which may legitimately nest.

**Current recommendation/next actions:** implement M2.6 by declaring only Rust capabilities supported
by exact golden fixtures, then populate its canonical roles, queries, lexical policy, recovery,
unsupported/macro/generated rules, and dialect declaration without weakening the frozen schemas.

**Blockers/dependencies/restart:** none. No new dependency, migration, rebuild activation, or service
restart applies.

**Negative-memory status:** no new failed path required Hindsight negative memory. The terminal policy
constraints are ready for durable checkpoint memory; search handles: `M2.5 construct policy`, `dialect
identity exact`, `macro syntax not expansion`, `unknown rule absence`.

**Signature:** Codex (GPT-5), M2.5 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.6 active checkpoint — Rust production adapter and goldens

**Date/time:** 2026-07-14T01:16:00+02:00

**Objective/target:** make the production Rust adapter implement the frozen M2.1-M2.5 contracts and
prove its exact supported boundary with valid and malformed golden fixtures.

**Changes:** planning only. Audited the current Rust grammar descriptor, region/metric/tail hooks,
analyzer fixtures, capability tiers, and newly frozen projection contracts. Selected a complete-S0
target while retaining every S2-S4 capability as unknown.

**Commands/checks run:** task-targeted Hindsight search; targeted reads of `RustPack`, capability tier
catalog, Rust analyzer pack, existing Rust clean/sloppy fixtures, and M2 terminal constraints; fixture
inventory with `rg`.

**Results:** ACTIVE / UNVERIFIED. Existing Rust production policy remains default unknown for canonical
roles, queries, lexical classification, construct/recovery, and dialect declarations. A first broad
implementation hunk was fully removed after target-audit; `cargo check -p deslop-lang` passes and no
production source change remains in the active diff.

**Invalidated assumptions / negative memory:** a patch anchored only on the repeated
`fn capability_manifest(&self)` signature selected `ClojurePack`, and a relocation hunk without an
explicit implementation anchor then selected `JuliaPack`. Both were detected by searching for the
Rust-specific assertion string, removed with `apply_patch`, and the exact parent behavior restored.
Authority downgrade: no Rust policy implementation exists yet. Preferred alternative: every patch
must explicitly anchor on `impl LangPack for RustPack`, use small unique method insertions, and compile
after each method group. Search handles: `M2.6 repeated LangPack patch wrong implementation`, `RustPack
unique hunk anchor`. Recheck whenever mechanically editing a method shared by all language packs.
Existing graph/analyzer Rust support is not evidence of S2 name resolution, CFG, effects, macro
expansion, or compiler authority. Generated query labels cannot substitute for exact marker evidence.

**Current recommendation/next actions:** implement the Rust policy methods and two golden fixtures,
then measure all projection vectors before pinning counts and running integration gates.

**Blockers/dependencies/restart:** Serena Rust symbols remain unavailable; targeted local edits plus
compiler/query/runtime oracles are the fallback. No functional blocker or dependency change applies.

**Negative-memory status:** new provisional negative memory recorded locally; Hindsight consolidation
required before resuming implementation.

**Signature:** Codex (GPT-5), M2.6 integration owner, active checkpoint, 2026-07-14.

---

## M2.6 terminal checkpoint — Rust production adapter and goldens

**Date/time:** 2026-07-14T01:32:50+02:00

**Objective/target:** complete the production Rust implementation of every frozen M2.1-M2.5 adapter
contract and prove its exact authority boundary through valid and malformed golden fixtures.

**Changes:** `RustPack` now provides canonical raw-kind roles, all six syntax query families, total
lexical classification, file-incomplete recovery, opaque macro definitions/invocations and unsafe
blocks, exact generated-marker facts, and exact Rust grammar dialect provenance. Canonical generated
roles require `#[generated]` or `#[automatically_derived]`; the opaque/generated query family captures
macros only and does not fabricate generated provenance. The capability manifest derives S1; every
S2-S4 capability stays unknown. Added valid and malformed Rust fixtures and a production integration
oracle running role, query, lexical, and construct projections from the same retained analysis. Fixed
the M2.4 lexical sentinel collision: raw kind `*` with exact text classifies multiplication, while only
raw kind `*` with no text is the terminal wildcard. The former Rust-unknown rejection test now uses
Python, which remains unknown.

**Commands/checks run:** focused `cargo test -p deslop-lang`; focused and complete `cargo test -p
deslop-parse`; affected strict clippy, format, and whitespace checks; then `cargo test --workspace
--all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc
--workspace --all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D
warnings`; `cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. Valid golden: 161 raw nodes; 110 non-overlapping token owners with class counts
comment 2, delimiter 36, identifier 22, keyword 11, literal 4, operator 13, other 8, punctuation 14;
78 role assignments across 17 categories; query captures `[5,2,5,1,2,3]`; construct counts generated
2, macro 3, unsupported 1. Unicode `π`, full line/block comments, exact arithmetic `*`, both generated
markers, macro definition/invocations, and the unsafe region are pinned. Malformed golden: exactly one
syntax-authority file-incomplete `ERROR` fact with text `=`. Each file invokes its parser once. All
workspace gates pass; only the two repository-designated slow probes remain ignored.

**Invalidated assumptions / negative memory:** (1) repeated LangPack signatures are unsafe patch
anchors; explicit `impl LangPack for RustPack` context and small compile-checked groups are required.
(2) raw kind `*` is a real Rust multiplication token, so treating every `*` raw-kind rule as the
wildcard made truthful classification impossible. Correct contract: an exact-text `*` rule matches
the literal raw kind; only `text: null` is wildcard. (3) completing canonical roles also completes S1,
not merely S0, because the manifest already truthfully provides all S1 syntactic capabilities. Macro
and generated labels still do not grant expansion, name resolution, CFG, effects, or compiler facts.

**Current recommendation/next actions:** apply the same fixture-gated production upgrade to the
JavaScript, TypeScript, and TSX dialect family in M2.7, retaining dialect-specific identities and no
S2+ promotion.

**Blockers/dependencies/restart:** none. No dependency, migration, service restart, or live-state
activation applies.

**Negative-memory status:** repeated-hunk failure is recorded locally and in Hindsight; the literal-
star sentinel correction is recorded locally and requires durable Hindsight consolidation. Search
handles: `M2.6 Rust golden`, `lexical exact star wildcard`, `Rust S1 derived`.

**Signature:** Codex (GPT-5), M2.6 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.7 active checkpoint — JavaScript, TypeScript, and TSX dialect goldens

**Date/time:** 2026-07-14T01:34:31+02:00

**Objective/target:** complete production adapter policy for JavaScript plus the TypeScript/TSX
grammar family and prove every stored dialect independently through golden projections.

**Changes:** audited the JavaScript and TypeScript packs, the separate TypeScript and TSX grammar
selections from M0.4, existing typed/TSX/JSX valid and malformed fixtures, and the prior negative
memory forbidding JavaScript grammar fallback and public `Lang::Tsx`. Added one shared composable
canonical-role mapper for program/module, declarations/callables/types, imports/exports, parameters,
blocks/statements/control, calls/read/write/literals, JSX, comments/errors, opaque `with`, and exact
`@generated` decorators. Both production manifests now provide canonical roles and derive S1 while
S2+ remains unknown.

**Commands/checks run:** targeted Hindsight search; targeted reads of both production packs and all
existing TypeScript/TSX/JSX fixture sources; `cargo fmt --all`; `cargo check -p deslop-lang`; and
post-edit target-string audit confirming both production implementations were wired.

**Results:** ACTIVE / WORKSPACE-WIDE UNVERIFIED. Canonical roles compile and manifests now derive S1;
query, lexical, construct/dialect policies and numerical golden coverage remain incomplete.

**Invalidated assumptions / negative memory:** no new failed experiment. Existing constraint remains
authoritative: TypeScript/TSX must use their stored official grammars, never JavaScript fallback; typed
CST nodes do not establish compiler/type facts.

**Current recommendation/next actions:** implement shared role/lexical/construct helpers with separate
query/dialect builders, then run the three-dialect numerical matrix before pinning counts.

**Blockers/dependencies/restart:** none. Serena Rust symbols remain unavailable; local compiler/query
oracles are the fallback. No dependency or restart applies.

**Negative-memory status:** no new failure; M0.4 grammar-fallback and Lang::Tsx prohibitions loaded.

**Signature:** Codex (GPT-5), M2.7 integration owner, active checkpoint, 2026-07-14.

---

## M2.7 terminal checkpoint — JavaScript, TypeScript, and TSX dialect goldens

**Date/time:** 2026-07-14T01:44:25+02:00

**Objective/target:** complete the production adapter implementation for JavaScript and the public
TypeScript family, compile every stored query against the exact JavaScript/TypeScript/TSX grammar,
and prove the supported boundary with numerical valid and malformed goldens.

**Changes:** added shared ECMAScript canonical-role, total lexical, file-incomplete recovery,
unsupported-construct, generated-marker, and dialect-policy helpers. JavaScript and TypeScript each
derive S1 from the completed syntactic capabilities while S2-S4 remain unknown. Query packs remain
grammar-aware: JavaScript declarations use the JavaScript catalog; TypeScript adds typed declaration
kinds; all six families compile independently against JavaScript, TypeScript, and TSX. JavaScript
declares javascript/jsx over `tree-sitter-javascript` 0.25.0; TypeScript declares typescript/tsx over
the distinct `tree-sitter-typescript` grammar ids at 0.23.2. Macros are explicitly unsupported;
`with_statement` is opaque; only exact `/* @generated */` comments and `@generated` decorators emit
generated facts. Added `.js`, `.ts`, and `.tsx` production goldens and a retained-analysis integration
oracle covering all four M2 projections plus the existing malformed typed fixtures. Generator
declarations/functions and typed abstract classes were included during the terminal semantic audit.

**Commands/checks run:** repeated focused `cargo check/test -p deslop-lang`; focused diagnostic and
pinned `cargo test -p deslop-parse ecmascript_production_adapter_golden_matrix_is_owned_and_parse_once`;
complete affected `cargo test -p deslop-lang -p deslop-parse`; affected strict clippy, format, and
whitespace checks; then `cargo test --workspace --all-features`; `cargo build --workspace
--all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo
clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; and
`git diff --check`.

**Results:** PASS. JavaScript locks 61 CST role facts with 45 assignments across 15 categories, 42
token owners, query captures `[1,1,3,0,2,1]`, one exact generated fact, and one opaque unsupported
`with_statement`. TypeScript locks 143 role facts with 71 assignments across 15 categories, 90 token
owners, query captures `[4,2,3,0,1,0]`, and exact generated comment/decorator facts. TSX locks 107
role facts with 45 assignments across 14 categories, 68 token owners, query captures
`[3,0,2,0,1,0]`, JSX expression evidence, and no fabricated construct facts. Unicode `π`, exact
arithmetic `*`, and full comment owners are pinned. Malformed TypeScript emits exactly one `ERROR`
fact for `.`, while malformed TSX emits one exact file-incomplete `ERROR` region. Every file records
one parser invocation. All workspace gates pass; only the two repository-designated slow probes are
ignored.

**Invalidated assumptions / negative memory:** no grammar catalog split was needed for the shared
scope/control/comment/opaque families, but query compilation alone does not prove capture-role
consistency. The terminal audit found generator declarations/functions present in declaration query
authority but absent from the canonical role mapper; this was corrected, along with typed abstract
class query coverage, before full gates. Exact stored grammar identity remains authoritative:
TypeScript/TSX never fall back to JavaScript, TSX remains a dialect rather than a public `Lang`, and
typed/JSX syntax does not grant name resolution, CFG, effects, expansion, or compiler/type authority.

**Current recommendation/next actions:** execute M2.8 using the same projection-gated method for the
Python production adapter and fixed valid/malformed goldens.

**Blockers/dependencies/restart:** none. No dependency, migration, service restart, or live-state
activation applies.

**Negative-memory status:** terminal semantic-audit lesson requires durable Hindsight storage. Search
handles: `M2.7 ECMAScript golden`, `query capture canonical role consistency`, `TypeScript exact
grammar no fallback`.

**Signature:** Codex (GPT-5), M2.7 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.8 active checkpoint — Python production policy and goldens

**Date/time:** 2026-07-14T01:47:47+02:00

**Objective/target:** implement every frozen M2.1-M2.5 production policy surface for Python and prove
its exact authority boundary through fixed valid and malformed golden fixtures.

**Changes:** planning/audit only. Inspected `PythonPack`, its exact
`python/tree-sitter-python/0.25.0` descriptor, existing decorated/async/nested behavioral fixture and
tests, and the installed grammar node catalog. Defined a convergent matrix covering all four retained
projections, all six query families, numerical counts, exact syntax evidence, and one parse per file.

**Commands/checks run:** targeted Hindsight active-plan and negative-memory search; targeted `rg` and
source reads for the Python pack, fixture inventory, parser tests, dependency version, and installed
grammar node catalog.

**Results:** ACTIVE / UNVERIFIED. Python still exposes the default unknown role, query, lexical,
construct/recovery, and dialect policy. The grammar catalog explicitly contains decorated, match,
type-alias, legacy `exec_statement`, and legacy `print_statement` nodes, enabling a real unsupported
boundary rather than a fabricated detector.

**Invalidated assumptions / negative memory:** the shared Hindsight MCP does not accept search mode
`keyword`; two targeted calls failed with `unknown search mode` and returned no memory. Omitting mode
correctly selected graph traversal. This tooling correction is stored durably. Existing behavioral
region support is not evidence of S1 completion, and Python annotations/patterns cannot promote S2+
authority.

**Current recommendation/next actions:** add the Python policy methods using an implementation-specific
anchor, compile after each method group, then build and numerically pin valid/malformed fixtures.

**Blockers/dependencies/restart:** Serena remains unable to expose Rust symbols; targeted local reads,
compiler checks, real query compilation, and runtime projections are the fallback. No functional
blocker or dependency change applies.

**Negative-memory status:** invalid Hindsight mode recorded in Hindsight and locally; existing
query/role and exact-star constraints carried forward. Search handles: `M2.8 Python golden`,
`Hindsight unknown search mode keyword`, `query capture canonical role consistency`.

**Signature:** Codex (GPT-5), M2.8 integration owner, active checkpoint, 2026-07-14.

---

## M2.8 terminal checkpoint — Python production policy and goldens

**Date/time:** 2026-07-14T01:58:48+02:00

**Objective/target:** complete every frozen M2.1-M2.5 production adapter surface for Python and prove
the exact supported boundary with valid/malformed numerical goldens and actual query-role agreement.

**Changes:** `PythonPack` now provides raw-kind canonical roles for modules, decorated/function/class
definitions, parameters, scopes, statements/control/pattern matching, calls/reads/writes/literals,
comments, recovery, generated, and opaque legacy syntax. Added all six syntax query families, total
case-sensitive Unicode lexical classification, file-incomplete recovery, opaque Python 2
`exec_statement`/`print_statement` rules, explicitly unsupported macros, exact `# @generated` and
`@generated` markers, and exact `python/tree-sitter-python/0.25.0` dialect provenance. The manifest
derives S1 and leaves S2-S4 unknown. Added valid/malformed fixtures and a retained-analysis oracle that
also executes every query capture against its declared canonical roles. Moved the production-unknown
rejection oracle from Python to Julia, which remains unknown pending M2.10.

**Commands/checks run:** focused schema/check/test cycles; diagnostic then pinned Python golden test;
complete `cargo test -p deslop-lang -p deslop-parse`; production adapter golden matrix filter; affected
strict clippy, format, and whitespace checks; then `cargo test --workspace --all-features`; `cargo
build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features
--no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all
-- --check`; and `git diff --check`.

**Results:** PASS. The valid golden contains 127 CST role facts and 108 role assignments across 21
categories; 75 non-overlapping lexical owners with counts comment 2, delimiter 6, identifier 26,
keyword 13, literal 5, operator 8, other 3, punctuation 12; query captures `[4,1,8,3,2,2]`; and exact
construct counts generated 2 / unsupported 2. Unicode `π`, arithmetic `*`, member-access `.`, both
full comments, exact generated comment/decorator, and exact opaque `print total` / `exec \"legacy =
True\"` are pinned. The member call proves operands below `await` remain independent token owners. The
malformed golden emits exactly one file-incomplete `ERROR` for `return value +`. Every query capture
has every role declared by its query contract, every file invokes the parser once, and all workspace
gates pass; only the two repository-designated slow probes are ignored.

**Invalidated assumptions / negative memory:** a raw-kind keyword rule is unsafe when Tree-sitter
uses the same kind for a named composite and its anonymous keyword leaf. Initial Python rules for
`await`, `lambda`, `type`, and `yield` caused lexical projection to claim whole composite subtrees as
single keywords; the member-call probe showed `await helpers.normalize(total)` suppressing all
operands. Corrected policy uses exact-text rules for these four keywords, raising truthful ownership
from 66 to 75 facts. Preferred audit: compare installed named and anonymous node catalogs for raw-kind
collisions and probe at least one operand-bearing composite. Existing constraints also hold: query
compilation alone is insufficient, exact `*` must not collide with wildcard, annotations/patterns are
syntactic only, and Python S1 grants no resolution/CFG/effect/compiler authority.

**Current recommendation/next actions:** execute M2.9 with the same numerical and query-role gates for
the Clojure production adapter, paying special attention to reader/macro forms and composite token
ownership.

**Blockers/dependencies/restart:** none. No dependency, migration, service restart, or live-state
activation applies.

**Negative-memory status:** Python composite/keyword collision requires durable Hindsight storage;
the invalid Hindsight `keyword` search mode was already recorded. Search handles: `M2.8 Python
golden`, `Python await composite lexical suppression`, `named anonymous raw-kind collision`.

**Signature:** Codex (GPT-5), M2.8 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.9 active checkpoint — Clojure reader/macro policy and goldens

**Date/time:** 2026-07-14T02:00:00+02:00

**Objective/target:** complete the Clojure production adapter without treating uniform list nodes or
quoted macro templates as semantic runtime evidence.

**Changes:** planning/audit only. Inspected `ClojurePack`, exact grammar descriptor, existing
reader/macro/control golden, parser assertions, installed grammar node catalog, and durable negative
memory for evaluated versus quoted list forms. Defined a convergent predicate-query and numerical
projection matrix.

**Commands/checks run:** targeted Hindsight retrieval; targeted source/fixture/parser reads; complete
installed Clojure node-type catalog inspection with `jq`; dependency/version audit.

**Results:** ACTIVE / UNVERIFIED. Clojure remains default unknown for all M2 policy surfaces. Its
grammar exposes `source`, uniform `list_lit`, literal/symbol nodes, and explicit reader forms; it does
not provide declaration/call/control node kinds. Contextual list-head classification and evaluated-
reader ancestry are therefore mandatory.

**Invalidated assumptions / negative memory:** existing durable constraint remains authoritative:
raw `list_lit` membership cannot prove branch/nesting/flow; syntax-quoted macro templates are data,
and reader conditionals lack selected-platform provenance. No raw-kind-only S1 implementation is
acceptable. Clojure external analyzer support and head-based metrics do not themselves complete the
frozen role/query/lexical/construct contracts.

**Current recommendation/next actions:** add contextual canonical roles and predicate queries first,
compile them against the real grammar, then implement lexical/reader/dialect policy and measure the
fixed golden matrix.

**Blockers/dependencies/restart:** Serena Rust symbols remain unavailable; targeted local tools and
compiler/query/runtime oracles are the fallback. No functional blocker or dependency change applies.

**Negative-memory status:** existing Clojure quoted/list-head constraint loaded from Hindsight and
carried locally. Search handles: `M2.9 Clojure golden`, `Clojure list_lit reader conditional macro
expansion`, `quoted form runtime role leakage`.

**Signature:** Codex (GPT-5), M2.9 integration owner, active checkpoint, 2026-07-14.

---

## M2.9 terminal checkpoint — Clojure reader/macro policy and goldens

**Date/time:** 2026-07-14T02:09:27+02:00

**Objective/target:** complete the truthful Clojure production adapter while preventing quoted,
discarded, syntax-quoted, and reader-conditional data from gaining runtime control authority.

**Changes:** `ClojurePack` now derives S1 and provides evaluated-ancestry/list-head canonical roles,
total Unicode symbol/operator lexical classification, file-incomplete recovery, opaque unsupported
`#=` evaluation, explicit opaque reader-macro facts, exact generated comment/metadata markers, and
exact `clojure/tree-sitter-clojure/0.1.0` provenance. Added safe provided query families for module
scope, comments, and raw reader/opaque forms; declaration, reference, and control queries remain
unknown rather than capturing quoted forms. Added valid/malformed goldens with query-role checks,
quoted-control non-leakage, exact numerical projections, and one parse per file. Replaced Clojure’s
text-prefix list-head parser with grammar `value/name` field extraction across canonical roles and
existing metric/region callbacks, so metadata-prefixed definitions retain their true head.

**Commands/checks run:** focused language schema/test cycles; diagnostic then pinned Clojure golden;
complete affected `cargo test -p deslop-lang -p deslop-parse -p deslop-metrics`; affected strict
clippy, format, and whitespace checks; then `cargo test --workspace --all-features`; `cargo build
--workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features
--no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all
-- --check`; and `git diff --check`.

**Results:** PASS. Valid golden: 160 CST role facts, 183 assignments across 14 role categories; 90
token owners with comment 2, delimiter 36, identifier 30, literal 10, operator 3, other 1,
punctuation 8; safe query captures `[0,0,1,0,2,7]`; exact construct counts generated 2 / macro 6 /
unsupported 1. Unicode `π`, exact symbol operator `*`, newline-owning full comments, generated comment
and metadata, quote/discard/read-conditional/syntax-quote/unquotes, and opaque `#=` are pinned. The
live `if` carries Branch; the quoted `if` carries neither Branch nor Call. Malformed input emits one
whole-file `ERROR`. Every provided query capture has its declared role, both files parse once, and all
workspace gates pass; only the two repository-designated slow probes are ignored.

**Invalidated assumptions / negative memory:** (1) the frozen Tree-sitter query source cannot express
“no quoted ancestor” across arbitrary nesting. Head-predicate queries would therefore return quoted
declarations/references/control as runtime structure. Authority downgrade: those three families remain
unknown; module scope, comments, and raw opaque readers are provided. Preferred alternative is an
ancestry-aware filter in a versioned query schema, assessed in M2.11—not a leaky approximation. (2)
Clojure metadata is included in a `list_lit` span, so prefix scanning sees `^` instead of `defn`.
Grammar `value/name` fields are the authoritative head and now drive canonical plus metric/region
hooks. (3) comment nodes own their trailing newline; the exact generated policy explicitly declares
LF and EOF variants. Macro facts grant no macroexpansion or active reader-platform authority.

**Current recommendation/next actions:** execute M2.10 for Julia, then make the cross-adapter M2.11
matrix explicitly test Clojure’s three unknown contextual query families against leakage.

**Blockers/dependencies/restart:** no immediate blocker. Deferred contextual query support requires a
versioned contract change if pursued; current output is complete and honest without it. No dependency,
migration, service restart, or live-state activation applies.

**Negative-memory status:** contextual-query and metadata-head invalidations require durable Hindsight
storage. Search handles: `M2.9 Clojure golden`, `Clojure quoted ancestor query leakage`, `Clojure
metadata list head value name fields`.

**Signature:** Codex (GPT-5), M2.9 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.10 active checkpoint — Julia macro/quote policy and goldens

**Date/time:** 2026-07-14T02:10:50+02:00

**Objective/target:** complete the Julia production adapter over its direct typed grammar while keeping
macros, quoted ASTs, and external analyzer evidence outside unsupported authority.

**Changes:** planning/audit only. Inspected `JuliaPack`, exact grammar/version, existing region and
StaticLint/JET integration, installed node catalog, and relevant cross-adapter negative memories.
Defined a convergent direct-query and numerical golden matrix.

**Commands/checks run:** targeted Hindsight retrieval; targeted Julia pack/analyzer/metrics/fixture
inventory reads; complete node-kind catalog extraction with `jq`; dependency/version audit.

**Results:** ACTIVE / UNVERIFIED. Julia remains default unknown for all frozen M2 policy surfaces. The
grammar exposes direct definition, statement, call, control, macro, quote, comment, literal, and
operator nodes, so every query family should be expressible without contextual post-filtering.

**Invalidated assumptions / negative memory:** external StaticLint/JET capability is separate,
configuration-gated evidence and cannot promote this syntax adapter. Interpolated string composites
must not be classified as one literal if that would suppress embedded expressions. Existing
query-role, exact-operator, and implementation-anchor constraints carry forward.

**Current recommendation/next actions:** implement Julia canonical/query/lexical/construct policy in
small compile-checked groups, then create and measure the valid/malformed golden matrix.

**Blockers/dependencies/restart:** Serena Rust symbols remain unavailable; local compiler/query/runtime
oracles are the fallback. No functional blocker or dependency change applies.

**Negative-memory status:** existing constraints loaded and carried locally; no new failure yet.
Search handles: `M2.10 Julia golden`, `Julia interpolation composite lexical`, `Julia macro quote
authority`.

**Signature:** Codex (GPT-5), M2.10 integration owner, active checkpoint, 2026-07-14.

---

## M2.10 terminal checkpoint — Julia macro/quote policy and goldens

**Date/time:** 2026-07-14T02:22:20+02:00

**Objective/target:** complete all frozen production adapter policy surfaces for Julia while keeping
macros, quoted ASTs, interpolation, and external analyzers inside their exact authority boundaries.

**Changes:** `JuliaPack` now provides source/module/function/type/import/export/parameter/scope/control/
call/write/read/literal/comment/error canonical roles; all six direct grammar query families; total
case-sensitive Unicode lexical classification with exact named operators; file-incomplete recovery;
opaque quote expressions/statements; opaque macro definitions/calls; exact generated comment/macro
markers; and `julia/tree-sitter-julia/0.23.1` dialect provenance. The manifest derives S1 and leaves
S2-S4 unknown. Added valid/malformed fixtures and a retained-analysis oracle with numerical projections,
query-role checks, interpolation ownership, exact construct facts, and parse-once assertions. The
production-unknown rejection oracle now uses its dedicated custom test pack rather than migrating
between production languages.

**Commands/checks run:** focused language schema/check/test cycles; diagnostic then pinned Julia
golden; complete affected `cargo test -p deslop-lang -p deslop-parse -p deslop-metrics`; affected
strict clippy, format, and whitespace checks; then `cargo test --workspace --all-features`; `cargo
build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features
--no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all
-- --check`; and `git diff --check`.

**Results:** PASS. Valid golden: 95 CST role facts, 94 assignments across 18 roles; 61 token owners
with comment 3, delimiter 8, identifier 24, keyword 12, literal 3, operator 6, other 1, punctuation 4;
query captures `[2,4,2,2,3,3]`; exact constructs generated 2 / macro 2 / unsupported quote 1.
Unicode `π`, arithmetic `*`, assignment `+=`, full line/block comments, standalone `@generated`,
`@time`, quoted AST, and embedded `$total` identifier ownership are pinned. The whole interpolated
string is not collapsed into one literal owner. Malformed input emits one whole-file `ERROR`. Every
query capture carries its declared role, every file parses once, and all workspace gates pass; only
the two repository-designated slow probes are ignored.

**Invalidated assumptions / negative memory:** (1) `argument_list` is shared by function signatures
and ordinary calls; raw-kind classification labeled every call argument list as Parameter. Correct
boundary: traverse ancestry and require `signature`/macro-definition before a body boundary. (2) Julia
assignment spellings are named `operator` nodes in this grammar, so anonymous raw `=` rules alone left
`=`, `+=`, and peers as Other. Exact named-operator rules now classify them as Assignment. (3) string
literal composites remain unclassified intentionally so interpolation identifiers survive. Macro and
quote facts grant no expansion, resolution, CFG, effects, compiler, StaticLint, or JET authority.

**Current recommendation/next actions:** execute M2.11 as one cross-adapter construct/support matrix,
including explicit Clojure contextual-query unknowns and S2-S4 leakage rejection.

**Blockers/dependencies/restart:** none. No dependency, migration, service restart, or live-state
activation applies.

**Negative-memory status:** Julia signature/call and named-operator distinctions require durable
Hindsight storage. Search handles: `M2.10 Julia golden`, `Julia argument_list signature parameter`,
`Julia named operator assignment fallback`.

**Signature:** Codex (GPT-5), M2.10 integration owner, terminal checkpoint, 2026-07-14.

---

## M2.11 active checkpoint — cross-adapter construct and capability leakage

**Date/time:** 2026-07-14T02:30:00+02:00

**Objective/target:** prove in one registry-wide oracle that production dialect, construct, query,
recovery, and semantic-tier authority cannot migrate between Rust, JavaScript, TypeScript, TSX,
Python, Clojure, and Julia.

**Changes:** planning/audit only. Opened a clean `jj` change, persisted the M2.10 terminal checkpoint,
loaded relevant leakage and dialect negative memories, and inventoried all production goldens plus the
public manifest/construct/query projection surfaces. Defined one convergent table-driven validation
matrix rather than another sequence of language-specific probes.

**Commands/checks run:** targeted Hindsight search/remember; `jj status`/log/new; targeted TODO, plan,
report, adapter-test, capability-manifest, construct-policy, and projection API reads.

**Results:** ACTIVE / UNVERIFIED. All seven production dialects already expose frozen local goldens.
M2.11 can therefore be implemented as a retained-analysis integration oracle without expanding any
adapter's authority.

**Invalidated assumptions / negative memory:** a uniform cross-language table must not force uniform
support: ECMAScript/Python macros are Unsupported, Clojure contextual declaration/reference/control
queries are Unknown, and Rust/Clojure/Julia macro syntax is Provided but opaque. Public TypeScript
language identity also does not erase TypeScript-versus-TSX grammar provenance. Generated markers are
exact-text facts, and malformed syntax cannot create construct or semantic authority.

**Current recommendation/next actions:** implement the table-driven production matrix, run its focused
test, correct only demonstrated leakage, then execute full workspace gates. Audit M2.DoD separately.

**Blockers/dependencies/restart:** Serena cannot expose Rust symbols in this project, so targeted local
reads/compiler/runtime checks remain the documented fallback. No functional blocker, dependency,
migration, restart, or cache action applies.

**Negative-memory status:** existing dialect, query-role, Clojure contextual-query, exact-marker, and
Julia syntax-boundary memories are loaded. No new failure is established yet. Search handles: `M2.11
cross adapter leakage`, `construct support matrix`, `Clojure quoted query unknown`, `TypeScript TSX
dialect provenance`.

**Signature:** Codex (GPT-5), M2.11 integration owner, active checkpoint, 2026-07-14.

---

## M2.11 terminal checkpoint — cross-adapter construct and capability leakage

**Date/time:** 2026-07-14T02:31:00+02:00

**Objective/target:** prove that production adapter dialect, construct, query, recovery, generated,
and semantic-tier authority remains exact across every registered production grammar variant.

**Changes:** added one table-driven retained-analysis integration oracle over 21 sources: valid,
malformed, and near-marker inputs for Rust, JavaScript, TypeScript, TSX, Python, Clojure, and Julia.
The matrix locks dialect triples; adapter-schema ownership; full manifest support/authority; exact
query and construct section support plus unavailable-payload hygiene; construct counts, handling, and
generated texts; malformed recovery facts; quoted Clojure control non-leakage; near-marker rejection;
and parse-once preservation. Added the JavaScript malformed fixture and seven language/dialect-specific
near-marker fixtures. No production policy or dependency changed.

**Commands/checks run:** focused matrix test (after correcting an over-specific test-name filter);
`cargo test -p deslop-lang -p deslop-parse`; affected all-feature/all-target strict clippy; format and
diff checks; then `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace
--all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. The convergent oracle covers 21 files and preserves exactly one parser invocation
per file before and after all projections. Every adapter provides all S0/S1 capabilities and reports
every S2-S4 capability Unknown with no authority. All six query families are Provided except Clojure
declarations/references/control, which remain payload-free Unknown. Macro policy is payload-free
Unsupported for JavaScript/TypeScript/TSX/Python and Provided opaque for Rust/Clojure/Julia. Exact
valid construct vectors `[unsupported, macro, generated]` are Rust `[1,3,2]`, JavaScript `[1,0,1]`,
TypeScript `[0,0,2]`, TSX `[0,0,0]`, Python `[2,0,2]`, Clojure `[1,6,2]`, and Julia `[1,2,2]`.
All seven malformed fixtures emit one exact syntax-authority `ERROR` with file-incomplete handling and
no construct authority; all seven near-marker fixtures emit zero generated facts. All workspace gates
pass; only the two repository-designated slow probes are ignored.

**Invalidated assumptions / negative memory:** uniform matrix shape does not imply uniform support.
Forcing all query/macro families to Provided would leak authority; the oracle instead pins each honest
Unknown/Unsupported boundary and requires unavailable declarations to carry no source, captures,
rules, or authority. TypeScript and TSX remain one public language family but distinct grammar/dialect
provenance. Parse errors and spelling-near generated annotations remain syntax only.

**Current recommendation/next actions:** audit and close M2.DoD only if every emitted adapter fact has
version/capability/provenance and no confirmed consumer output exceeds the stored manifest tier.

**Blockers/dependencies/restart:** none. No dependency, migration, service restart, cache clear, or
live-state activation applies.

**Negative-memory status:** no new production failure was found. Existing dialect/query/marker
constraints are now executable in the cross-adapter matrix. Search handles: `M2.11 terminal`,
`production adapter leakage matrix`, `21 source construct query authority`.

**Signature:** Codex (GPT-5), M2.11 integration owner, terminal checkpoint, 2026-07-14.

---

## M2 definition-of-done active checkpoint — joined provenance and tier ceiling

**Date/time:** 2026-07-14T02:35:00+02:00

**Objective/target:** close M2 only with an executable joined proof that every emitted adapter fact
has an auditable retained adapter/version/capability/provenance chain and that downstream confirmed
outputs do not exceed the source adapter's S1 ceiling.

**Changes:** planning/audit only. Opened a clean M2.DoD `jj` change, loaded relevant authority and
partial-analysis memories, inspected the M0/M1 DoD integration style and all public adapter projection
surfaces, and defined a seven-dialect joined oracle. No implementation or production policy changed.

**Commands/checks run:** targeted Hindsight retrieval; TODO/plan/report and public projection/identity
API reads; M0/M1 DoD test inspection; live seven-fixture CLI graph and scan JSON probes.

**Results:** ACTIVE / UNVERIFIED. Current live graph is complete with 7 files, 15 symbols, 42 edges,
15 structurally resolved containment edges, 27 syntactic edges, and zero resolved non-containment
edges. Current scan is complete with four findings and zero AnalyzerConfirmed findings. Public
projections retain analysis/path/schema/policy or query pack, and stored entries retain exact adapter,
manifest, and grammar identity; the DoD test must join and verify that chain for every fact.

**Invalidated assumptions / negative memory:** a projection ID hash is identity binding, not by itself
human-auditable provenance. The closure proof must traverse projection → analysis/path → stored adapter
identity/grammar → capability or policy declaration. Conversely, duplicating strings into every fact
would add redundant payload without additional authority. Resolved containment is S1 structural
ownership; resolved calls/imports/inheritance would require currently Unknown higher-tier semantics.

**Current recommendation/next actions:** implement the joined M2 DoD integration test, run it first,
then the unchanged M0/M1 gates and complete workspace verification.

**Blockers/dependencies/restart:** Serena Rust symbols remain unavailable; targeted local APIs and
compiler/runtime checks are the documented fallback. No functional blocker, dependency, migration,
restart, or cache action applies.

**Negative-memory status:** no new production failure yet. The projection-hash-versus-auditable-chain
distinction is carried into the executable test. Search handles: `M2 DoD provenance chain`, `resolved
non containment tier ceiling`, `adapter fact stored identity`.

**Signature:** Codex (GPT-5), M2.DoD integration owner, active checkpoint, 2026-07-14.

---

## M2 definition-of-done terminal checkpoint — joined provenance and tier ceiling

**Date/time:** 2026-07-14T02:40:16+02:00

**Objective/target:** close the complete M2 language-adapter contract with an executable proof that
every emitted fact is joined to exact adapter/version/capability/provenance and no confirmed consumer
output exceeds the stored adapter tier.

**Changes:** added `m2_definition_of_done.rs`, a seven-dialect retained-analysis integration gate that
walks every canonical-role, lexical-token, construct-policy, and query-capture fact through its exact
node/raw grammar evidence, projection schema/id/owner/path, stored adapter identity, grammar dialect
and version, and capability/policy declaration. The first run found Rust call references captured as
`scoped_identifier`/`field_expression` and TypeScript references captured as `member_expression` with
no declared Expression/Read roles. Fixed the root mapping only for nodes occupying the exact
`call_expression.function` field, updated local golden counts, and bumped the shared adapter contract
from `/1` to `/2`. The gate also runs analyzer, metrics, and graph over the same analysis and enforces
the S1 ceiling. Updated M2.6 evidence to the corrected Rust 90-role-assignment total.

**Commands/checks run:** live seven-fixture scan/graph/metrics JSON probes; repeated focused M2 DoD
localization; affected language/parse tests; unchanged M0/M1/M2 DoD integration gates; affected
all-feature/all-target strict clippy, format, and diff checks; then `cargo test --workspace
--all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc
--workspace --all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D
warnings`; `cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. Seven exact dialects produce 854 canonical facts with 640 role assignments, 536
lexical facts, 28 construct facts, and 88 query captures; every raw fact agrees with the retained node
and every capture satisfies its declared roles. All manifests derive S1 and keep S2-S4 Unknown. The
same analysis produces four findings with zero AnalyzerConfirmed claims, 15 metric regions, and a
44-node graph containing 15 symbols and 42 edges; all 27 non-containment edges are non-resolved.
Every file has exactly one parser invocation before and after all projections/consumers. M0 and M1
numerical gates remain unchanged. All workspace gates pass; only the two repository-designated slow
probes are ignored.

**Invalidated assumptions / negative memory:** local language goldens and query compilation did not
prove global query-capture/canonical-role consistency. The missing callee-composite roles survived
M2.6/M2.7 because those goldens pinned each projection separately. Authority downgrade: a provided
query family is not coherent until every emitted capture resolves to all declared canonical roles.
Preferred alternative implemented: one joined DoD oracle plus context-specific callee roles; do not
globally label scoped/member composites as reads because they also occur in type/non-call contexts.
The behavior change requires adapter schema `/2`, so cached `/1` derived identities must be rebuilt.

**Current recommendation/next actions:** begin M3.1 with the scope/name-resolution ADR. Preserve the
M2 gate as the hard ceiling: no resolved non-containment output until M3 supplies explicit S2/S3
capability and provenance.

**Blockers/dependencies/restart:** no functional blocker or dependency change. Existing serialized or
cached adapter-derived identities using `deslop-lang-adapter/1` must be regenerated; no service restart
or migration applies to in-memory/current analyses.

**Negative-memory status:** durable storage required for the separately-green-projections fallacy and
context-specific callee-role fix. Search handles: `M2 DoD query role mismatch`, `scoped_identifier
field_expression member_expression call function`, `adapter schema 2`.

**Signature:** Codex (GPT-5), M2.DoD integration owner, terminal checkpoint, 2026-07-14.

---

## M3.1 active checkpoint — scope and resolution authority ADR

**Date/time:** 2026-07-14T02:43:00+02:00

**Objective/target:** freeze the scope/name-resolution model and authority boundary before implementing
any S2/S3 binding, import/export, call, or impact claims.

**Changes:** planning/audit only. Opened a clean M3.1 change, loaded graph-resolution negative memory,
inspected ADR 0001 and the complete M3 roadmap, and audited graph/2 candidate routing, shadow blocking,
module-key heuristics, ambiguity collapse, confidence labels, and agent notes.

**Commands/checks run:** targeted Hindsight search; TODO/plan/ADR inventory; full ADR 0001 read; targeted
graph builder/extractor/identity/test reads.

**Results:** ACTIVE / UNVERIFIED. Current graph/2 correctly withholds resolved authority from calls,
imports, and inheritance, but it is not an M3 substrate: ambiguous endpoints are collapsed to one
placeholder, paths are not retained, module/import keys are heuristic, namespaces/build targets are
not modeled, and candidate uniqueness remains syntax-only.

**Invalidated assumptions / negative memory:** repository-global or heuristic-key uniqueness is not
lexical/name-resolution proof. Sorting candidates is determinism, not precedence. Runtime observation
and compiler/static resolution have different domains and cannot be merged by a single last-writer-wins
authority rank.

**Current recommendation/next actions:** write accepted ADR 0002 with schemas, path/outcome and authority
tables, build-context binding, invalidation, verification, rejected alternatives, and M3 rollout.

**Blockers/dependencies/restart:** no blocker or dependency change. Serena Rust symbols remain
unavailable; targeted local reads are the documented fallback. No restart or live-state action applies.

**Negative-memory status:** graph/1 false-resolution and M2 authority memories are loaded. Search
handles: `M3.1 ADR scope resolution`, `bare name uniqueness`, `all candidate paths authority`.

**Signature:** Codex (GPT-5), M3.1 integration owner, active checkpoint, 2026-07-14.

---

## M3.1 terminal checkpoint — accepted scope and resolution authority ADR

**Date/time:** 2026-07-14T02:46:28+02:00

**Objective/target:** establish the normative scope/name-resolution and authority contract before any
M3 implementation promotes S2/S3 output.

**Changes:** added accepted `docs/adr/0002-scope-and-name-resolution.md`. It defines versioned scope
graph and resolution projections; exact analysis/adapter/grammar/build-context identity; scope,
declaration, definition, binding, reference, import/export, module, and dynamic-boundary facts;
language-declared namespaces and structured precedence; complete viable/rejected resolution paths;
coverage and outcome tables; separate lookup/evidence authority; compiler/LSP/runtime conflicts;
import/alias/glob/re-export/module stitching; incremental invalidation; consumer gating; executable
verification; consequences, rejected alternatives, and M3.2-M3.DoD rollout.

**Commands/checks run:** targeted memory and graph audits; ADR 0001 and M3 roadmap review; structural
heading/term assertions; whitespace and size checks; then `cargo test --workspace --all-features`;
`cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace
--all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`;
`cargo fmt --all -- --check`; and `git diff --check`.

**Results:** PASS. ADR 0002 is 355 lines / 2,736 words and covers every M3.1 decision branch. Complete
coverage is required for Unique/Ambiguous/Unresolved; incomplete cases are Unknown, and provider
disagreement is Conflict. Every candidate path and rejection is retained. Global bare-name uniqueness,
sorted first-wins, inferred externality, and graph/2 semantic promotion are rejected. All workspace
gates pass; only the two repository-designated slow probes are ignored.

**Invalidated assumptions / negative memory:** lookup precedence and evidence authority cannot share
one rank. Runtime observation is orthogonal to static binding. A single candidate under incomplete
coverage is Unknown, not Unique; zero candidates are Unknown unless coverage is complete. Deterministic
sorting does not resolve semantic ties.

**Current recommendation/next actions:** implement M3.2 fact schemas and retained ownership exactly
against ADR 0002, without beginning resolution rules or promoting capability manifests prematurely.

**Blockers/dependencies/restart:** none. No dependency, migration, restart, cache clear, or live-state
activation applies.

**Negative-memory status:** the accepted ADR is the durable corrective authority; graph/1 false-name
resolution memory remains active. Search handles: `ADR 0002 scope graph`, `coverage outcome authority`,
`all resolution paths`.

**Signature:** Codex (GPT-5), M3.1 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.2 active checkpoint — owned scope-graph fact model

**Date/time:** 2026-07-14T02:52:02+02:00

**Objective/target:** implement the foundational `deslop.scope-graph/1` facts and identities without
beginning language resolution rules or promoting S2/S3 authority.

**Changes:** planning/audit only. Reloaded Serena and Hindsight startup context, inspected ADR 0002's
fact/identity/coverage contract, audited `ProjectAnalysis`, `ProjectionId`, `NodeId`, strict `NodeKey`
wire identity, stored grammar/adapter identities, capability manifests, and M2 owned projections. The
selected boundary is a new `deslop-parse` module because it owns every required immutable identity and
already has Serde and hashing dependencies.

**Commands/checks run:** Serena activation/onboarding/memory reads; Hindsight global/repo startup
recall and targeted active/negative searches; `jj status`; targeted `rg`/`sed` audits of the roadmap,
ADR, parse identities, adapters, Cargo manifests, and language capability catalog.

**Results/failure modes:** working copy is clean at `kxlpnnwt f0ec75b3`. Serena identifies the project
as Python and cannot expose Rust symbols, so targeted local reads remain the documented fallback.
Hindsight searches succeed when mode is omitted. Explicit `semantic` mode failed as unsupported and
was recorded as negative memory. No code or verification result exists yet.

**Invalidated assumptions:** M3.2 cannot put portable fact identity in `deslop-lang` because exact
`NodeId`/`NodeKey` ownership lives in `deslop-parse`. A serialized path/range/spelling is not a fact ID.
Production extraction cannot be smuggled into this task because it requires the M3.3 total rule packs.

**Current recommendation/checkpoint:** implement a strict validating builder with dense local handles,
revision/build-context-bound keys, retained analysis ownership, complete structural fact variants,
and adversarial schema/link tests. Keep all current capability manifests unchanged.

**Blockers/dependencies/restart:** none. No new dependency, migration, restart, or live-state action is
required.

**Negative-memory status:** active constraints cover graph/1 bare-name false resolution, query/fact
coherence, invalid Hindsight search modes, and no premature S2/S3 authority. Search handles: `M3.2
scope facts identity`, `NodeId NodeKey ownership`, `unknown search mode semantic`.

**Signature:** Codex (GPT-5), M3.2 integration owner, active checkpoint, 2026-07-14.

---

## M3.2 implementation checkpoint — complete owned fact catalog

**Date/time:** 2026-07-14T03:08:20+02:00

**Objective/target:** establish the full structural fact and identity substrate before the terminal
workspace verification cycle.

**Changes:** added `crates/deslop-parse/src/scope_graph.rs` and exported its public contract. It defines
versioned build-context/fact-policy/projection schemas; dense analysis-owned non-Serde fact IDs;
payload-bound revision keys; portable namespaces/scope kinds/visibility/binding timing/reference roles;
scope, declaration, definition, binding, reference, import, export, build-module, dynamic-boundary, and
shadowing records; explicit Complete/Partial/Unsupported/Failed evidence with mandatory incomplete
reasons; a validating builder; retained `Arc<ProjectAnalysis>` projection; and strict wire documents.
Promoted the already-workspace-pinned `serde_json` dependency from test-only to runtime for canonical
payload hashing; no new package was added. Added the new module to the borrowed Tree-sitter surface
guard.

**Commands/checks run:** repeated focused `cargo test -p deslop-parse scope_graph`; full `cargo test -p
deslop-parse`; `cargo clippy -p deslop-parse --all-features --all-targets -- -D warnings`;
`RUSTDOCFLAGS='-D warnings' cargo doc -p deslop-parse --all-features --no-deps`; `cargo fmt --all`; and
`git diff --check`.

**Results/failure modes:** PASS. Six scope-graph tests pass, the parse crate is 99 passed/1 designated
slow probe ignored, both `NodeId` and `ScopeFactId` compile-fail Serde doctests pass, focused clippy and
rustdoc are warning-free, and whitespace is clean. An initial fixture expected four parsed `x` nodes;
Rust format-string text is not a parsed identifier, so the measured grammar count is three. The fixture
was corrected to the actual syntax inventory and no implementation path depended on the false count.

**Invalidated assumptions:** a prefix-valid fact key is insufficient—the key must hash its complete
evidence and fact payload so corrupted roles, coverage, links, or contents cannot survive strict
deserialization. Coverage status alone is insufficient; every non-Complete fact now requires an exact
reason. Canonical roles cannot be accepted on trust; the builder checks them against the owned M2
projection.

**Current recommendation/checkpoint:** run the full all-feature workspace test/build/doc/clippy/fmt/
diff gates. If green, verify manifests remain S1 with S2/S3 Unknown, mark M3.2 complete, and checkpoint
Hindsight. M3.3 remains the sole owner of executable language rule packs and capability promotion.

**Blockers/dependencies/restart:** none. The schema is new and not live-consumed, so no migration,
restart, reload, or cache clear applies.

**Negative-memory status:** the failed four-identifier fixture assumption is corrected locally and is
not a reusable implementation strategy. Durable constraints remain: no forged M2 evidence, bare-name
resolution, NodeId serialization, or premature semantic authority. Search handles: `M3.2 payload-bound
fact key`, `coverage reason`, `Rust format string identifier count`.

**Signature:** Codex (GPT-5), M3.2 integration owner, implementation checkpoint, 2026-07-14.

---

## M3.2 terminal checkpoint — owned scope-graph facts and identities

**Date/time:** 2026-07-14T03:10:07+02:00

**Objective/target:** finish the foundational scope/name fact layer while preserving the M2 authority
ceiling and reserving all executable resolution semantics for M3.3.

**Changes:** completed and exported `deslop.scope-graph/1`, `deslop.build-context/1`, and
`deslop.scope-fact-policy/1` from `deslop-parse`. The 2,659-line module supplies ten fact variants,
dense non-Serde handles, complete-payload `sf1_` keys, strict wire documents, retained analysis
ownership, build-context/policy-sensitive projection identity, copied NodeKey/raw/canonical/grammar/
adapter/capability evidence, explicit coverage reasons, typed builders, and cross-fact validation.
Updated the M3.2 TODO and durable plan. No language manifest, adapter behavior, graph code, analyzer,
protocol, CLI, or live process changed.

**Commands/checks run:** focused and full parse tests; focused all-feature/all-target clippy and parse
rustdoc; then `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace
--all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`; and final
`jj status`/diff/authority audits.

**Results:** PASS. The 14-fact fixture covers Scope, Declaration, Definition, Binding, Reference,
Import, Export, BuildModule, DynamicBoundary, and Shadowing. Six focused tests cover deterministic
round-trip ownership; build-context/policy identity isolation; foreign/wrong-kind handles; forged M2
roles; capability/coverage alignment; unknown fields and schema drift; corrupt source order/payload
keys; invalid namespaces; cycles; and dangling links. The parse crate reports 99 passed/1 designated
slow probe ignored plus two compile-fail doctests. Exact M0, M1, and M2 definition-of-done tests pass.
The production no-semantic-authority-leak test passes, and the change surface contains no
`deslop-lang` or `deslop-graph` edit. All full gates are green; only the two repository-designated slow
probes are ignored.

**Invalidated assumptions / negative memory:** recorded during implementation: format-string contents
do not create Rust identifier syntax nodes; prefix-valid keys do not prove payload identity; caller-
supplied canonical roles require owned-M2 verification; and incomplete status without a reason is not
sufficient provenance. None is left as a fallback path.

**Current recommendation/next actions:** begin M3.3 by defining total versioned adapter rule packs and
the shared path engine against this builder. Do not populate production facts, mark Complete, or promote
LexicalScopes/NameResolution/ImportsExports until exact dialect rules and supported/unsupported sections
are executable and tested.

**Blockers/dependencies/restart:** none. This is a new unused schema substrate; no migration, rebuild of
cached user data, restart, reload, or cache clear is required.

**Negative-memory status:** durable constraints are active and satisfied. Search handles: `M3.2
complete payload sf1`, `canonical role coherence`, `coverage reason`, `no S2 promotion`. The corrected
fixture and authority boundary will be stored in Hindsight at this checkpoint.

**Signature:** Codex (GPT-5), M3.2 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.3 active checkpoint — language resolution rule packs

**Date/time:** 2026-07-14T03:14:01+02:00

**Objective/target:** implement total per-dialect resolution-rule metadata and the shared traversal
mechanism without crossing into M3.4 result/outcome storage.

**Changes:** planning/audit only in fresh jj change `xupxwnxm`. Loaded M3/graph negative memory,
reviewed ADR 0002's complete rule-pack and precedence contract, audited `LangPack`, capability and query
schemas, production adapter hooks, seven exact dialect identities, and the new M3.2 fact surface.

**Commands/checks run:** Hindsight targeted active/negative searches; targeted `sed`/`rg` over ADR,
TODO, `deslop-lang` schemas, query packs, capability manifests, and every production adapter; `jj new`
and `jj status`.

**Results/failure modes:** clean child change created over terminal M3.2. All production S2/S3
capabilities remain Unknown. Rust, JavaScript, TypeScript/TSX, Python, and Julia have provided syntax
query seeds for declarations/references/scopes; Clojure declarations/references/control remain Unknown.
Therefore a rule schema may describe known language semantics, but no adapter can claim complete name
resolution merely because metadata or query captures exist.

The first schema patch failed atomically before any code edit because its top-of-file anchor expected a
different Tree-sitter import shape. No partial module, trait hook, or adapter edit landed; final `jj
status` shows planning artifacts only. The broad patch path is invalidated. Next implementation must
split module creation, uniquely anchored `mod`/re-export insertion, trait-hook insertion, and each
adapter override into separately compiled patches.

**Invalidated assumptions:** M3.3 must not collapse into a universal “locals before imports” function,
and it cannot mark terminal outcomes—that would skip M3.4's complete candidate-path contract. Static
rule metadata and executable extraction coverage are distinct; capability manifests stay unchanged
until the latter is complete.

**Current recommendation/checkpoint:** implement the strict total schema first, bind it into stored
adapter identity, then populate each dialect with explicit Provided/Unsupported/Unknown sections before
building the transient shared traversal engine.

**Blockers/dependencies/restart:** none. No new dependency or live-state action is expected.

**Negative-memory status:** active constraints include graph/1 bare-name false resolution, query/fact
coherence, and repeated `LangPack` patch-anchor mistakes. Search handles: `M3.3 rule pack`, `structured
precedence`, `Clojure query unknown`, `no first wins`.

**Signature:** Codex (GPT-5), M3.3 integration owner, active checkpoint, 2026-07-14.

---

## M3.3 schema checkpoint — strict resolution-rule identity

**Date/time:** 2026-07-14T03:28:32+02:00

**Objective/target:** complete the strict rule-pack substrate and bind it into immutable adapter and
analysis identity before adding production language metadata.

**Changes:** added `crates/deslop-lang/src/resolution.rs` with `deslop.resolution-rules/1`, ten ordered
rule sections, typed declarative instructions, exact dialect triples, namespace closure, strict
Provided/Unknown/Unsupported payload rules, and structured precedence. Added the unknown-by-default
`LangPack::resolution_rule_pack` hook. Extended stored `LanguageAdapterIdentity` to retain, expose, and
hash the rule pack, and snapshot construction to validate adapter-schema and selected-dialect matches.

**Commands/checks run:** `cargo fmt --all`; `cargo check -p deslop-lang`; focused resolution tests;
focused grammar-selection parse test; `cargo clippy -p deslop-lang -p deslop-parse --all-targets -- -D
warnings`; `git diff --check`.

**Results:** PASS. Three schema tests prove strict round-trip, total payload-free unknown catalogs,
wrong-section rejection, duplicate precedence rejection, undeclared namespace rejection, and unknown-
field rejection. Focused snapshot selection remains green. Production adapters still use the default
all-Unknown pack and no capability manifest changed.

**Invalidated assumptions / failure modes:** the earlier broad patch was replaced by separately
compiled module, trait, and identity patches with live anchors. A rule pack is now an identity input,
not an untracked side table. Provided metadata without an exact selected dialect is rejected at
snapshot publication.

**Current recommendation/next actions:** add each production pack with a unique implementation anchor
and exact dialect catalog. Keep extraction sections Unknown wherever the existing M2 syntax inventory
cannot prove a total extractor. Compile after each language family, then implement shared traversal.

**Blockers/dependencies/restart:** none. No dependency, migration, or live-state action applies.

**Negative-memory status:** the atomic broad-patch failure is superseded by the verified staged path;
the repeated-LangPack anchor constraint remains active for per-language overrides. Search handles:
`resolution-rules/1`, `stored rule identity`, `provided dialect gate`.

**Signature:** Codex (GPT-5), M3.3 integration owner, schema checkpoint, 2026-07-14.

---

## M3.3 rule-pack checkpoint — exact production metadata

**Date/time:** 2026-07-14T03:35:11+02:00

**Objective/target:** attach honest, exact per-dialect language semantics to the strict rule-pack
substrate before implementing shared traversal.

**Changes:** added unique production `LangPack` overrides for Clojure, Julia, Python, JavaScript/JSX,
TypeScript/TSX, and Rust. Their total catalogs cover declared namespaces and unification/transitions,
known scopes, timing, shadowing/duplicates, qualification/member access, import/export behavior,
module prerequisites, dynamic boundaries, and structured precedence. Precedence terms now carry an
explicit `lower-first` or `higher-first` direction. Extraction remains Unknown and payload-free for
all families, and other unsupported-by-current-evidence sections remain Unknown. No capability
manifest was promoted.

**Commands/checks run:** `cargo fmt --all`; `cargo test -p deslop-lang`; `cargo test -p deslop-parse`;
`cargo clippy -p deslop-lang -p deslop-parse --all-targets -- -D warnings`; targeted `rg` audits.

**Results:** PASS. The language suite reports 12 passed. The parse suite reports 99 passed, one
designated slow probe ignored, and two compile-fail doctests passed. Focused clippy is warning-free.
The exact built-in matrix proves all seven dialects, family-specific namespace and Provided-section
counts, distinct serialized packs, exact/partial dialect matching, and payload-free Unknown extraction.
Schema tests reject unknown payloads, wrong-section operations, undeclared namespaces, duplicate
precedence dimensions, and unknown fields.

**Invalidated assumptions / failure modes:** an ordered list of precedence dimensions alone was
underspecified because it did not say whether later source order or smaller lexical distance wins.
That representation is superseded by directional `PrecedenceTerm` values. Static language metadata
still does not prove fact extraction or name-resolution capability.

**Current recommendation/next actions:** build the parse-owned transient traversal over M3.2 facts.
It must restrict traversal to reachable scope/import relations, retain every candidate and structured
precedence component, expose deferred/opaque boundaries, and never assign or persist a terminal result.

**Blockers/dependencies/restart:** none. These immutable identity inputs take effect on the next
analysis construction; there is no long-lived process, cache migration, reload, or restart in scope.

**Negative-memory status:** the nondirectional precedence representation is invalidated and must not
return. Active constraints remain: no global bare-name lookup, no first-wins sorting, no terminal M3.4
outcome, and no capability promotion from metadata alone. Search handles: `directional precedence`,
`production resolution packs`, `extraction Unknown`.

**Signature:** Codex (GPT-5), M3.3 integration owner, rule-pack checkpoint, 2026-07-14.

---

## M3.3 traversal checkpoint — reachable attempts without outcomes

**Date/time:** 2026-07-14T03:43:55+02:00

**Objective/target:** implement the shared execution layer for stored language rules while reserving
serialized candidate paths and terminal resolution status for M3.4.

**Changes:** added the parse-owned `ResolutionTraversalEngine` and public transient result types. The
engine indexes M3.2 facts, begins at a reference's exact scope/namespace/first path segment, follows only
lexical parent edges, and retains every same-key declaration attached to those scopes. Each attempt
carries definition/binding links, lexical distance, pack-declared namespace reachability, visibility,
binding-timing observations, explicit shadowing facts, adapter-schema coherence, and the ordered
directional precedence components. Relevant alias/selective/glob/module imports and affected dynamic
boundaries are returned as deferred observations. Unknown rule sections are explicit gaps. Added a
stored-identity test proving that changing only the resolution-rule payload changes adapter identity
bytes, and re-exported the complete rule vocabulary through `deslop-parse`.

**Commands/checks run:** `cargo fmt --all`; focused traversal tests with `--nocapture`; `cargo test -p
deslop-lang`; `cargo test -p deslop-parse`; focused adapter-identity test; `cargo clippy -p deslop-lang
-p deslop-parse --all-targets -- -D warnings`; `RUSTDOCFLAGS='-D warnings' cargo doc -p deslop-lang -p
deslop-parse --no-deps`; `git diff --check`; targeted terminal-vocabulary and capability audits.

**Results:** PASS. The adversarial fixture retains exactly three reachable candidates: the outer value,
the inner declared-later value, and the inner wrong-namespace type. The same-key sibling declaration in
an unrelated callable is excluded. Timing marks the outer binding visible and the inner binding later;
namespace rules reject the type attempt; explicit shadowing remains attached; one alias import and one
dynamic boundary are deferred. The Rust pack exposes exactly one rule gap, Unknown extraction. The parse
suite reports 102 passed/one designated slow probe ignored plus three compile-fail doctests; the added
doctest proves traversal results are not serializable. Focused clippy, rustdoc, fmt, and diff checks pass.

**Invalidated assumptions / failure modes:** repository-global name indexes are unnecessary for lexical
traversal and remain prohibited as candidate authority. Stable fact order is used only as a declared
precedence component, never to select a candidate. Import syntax without module/export traversal is a
deferred observation, not an endpoint. A result struct with Serde or Unique/Ambiguous/Unresolved would
cross the frozen milestone boundary and is explicitly absent.

**Current recommendation/next actions:** run the complete workspace all-feature gates and unchanged
M0/M1/M2 authority tests. If green, check only M3.3, write the terminal checkpoint, and store the durable
rule/traversal and negative-memory conclusions before starting M3.4 in a fresh jj change.

**Blockers/dependencies/restart:** none. Traversal operates on immutable in-memory projections; no live
service, schema migration, persistent cache, reload, or restart is involved.

**Negative-memory status:** active and satisfied. Search handles: `M3.3 no global lookup`, `transient
traversal no outcome`, `deferred import endpoint`, `directional precedence`. No fallback path was added.

**Signature:** Codex (GPT-5), M3.3 integration owner, traversal checkpoint, 2026-07-14.

---

## M3.3 terminal checkpoint — language rule packs and shared traversal

**Date/time:** 2026-07-14T03:45:52+02:00

**Objective/target:** finish per-language declarative resolution rules and the shared reachable-fact
traversal without claiming extraction completeness, promoting S2/S3 capabilities, or implementing
M3.4's retained candidate/outcome schema.

**Changes:** completed `deslop.resolution-rules/1` and wired it through `LangPack`, all six production
families/seven selected dialects, immutable `LanguageAdapterIdentity`, and projection identity bytes.
The strict 1,514-line rule module has ten total ordered sections with exact dialects, support/authority,
typed namespaces/scopes/timing/shadowing/qualification/import/module/dynamic relations, and directional
structured precedence. The 1,303-line parse traversal indexes M3.2 facts, walks only exact lexical
ancestors, retains all attempted declarations and provenance-bearing observations, and defers imports,
qualification remainder, and dynamic boundaries without choosing or serializing an outcome. Checked
M3.3 in `.agents/TODO.md`. No dependency, graph/analyzer/protocol/CLI behavior, or live process changed.

**Commands/checks run:** focused schema, identity, traversal, language, and parse tests; focused
all-target clippy and rustdoc; then `cargo test --workspace --all-features`; `cargo build --workspace
--all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo
clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff
--check`; final `jj status`/diff/TODO/capability/terminal-vocabulary audits.

**Results:** PASS. All workspace tests pass; only the two repository-designated slow probes are ignored.
The parse crate reports 103 passed/one ignored and three compile-fail doctests. The language crate reports
12 passed. M0, M1, and M2 exact definition-of-done gates pass unchanged. The production no-semantic-
authority-leak test passes. The built-in matrix covers Clojure, Julia, Python, JavaScript, JSX,
TypeScript, TSX, and Rust dialect selection (seven distinct triples across six families), strict
totality, namespace/section counts, and distinct serialization. The traversal fixture numerically
retains three reachable attempts and excludes the unrelated sibling; timing, namespace, shadowing,
alias-import, dynamic-boundary, directional-precedence, wrong-handle, and non-Serde boundaries pass.

**Invalidated assumptions / negative memory:** the initial broad multi-surface patch anchor was
invalidated and replaced with staged edits. Nondirectional precedence dimensions were invalidated and
replaced with explicit lower/higher-first terms. Repository-global same-name lookup, stable-order winner
selection, import syntax as an endpoint, metadata as extraction proof, and terminal status in M3.3 remain
prohibited. These constraints are implemented rather than documented as fallbacks.

**Current recommendation/next actions:** begin M3.4 in a fresh jj child. Define a strict
`deslop.resolution/1` projection that retains every traversal edge, endpoint attempt, structured
precedence key, rejection reason, visibility/namespace/timing/build checks, source facts, authority,
coverage, and dynamic observations before deriving coverage-bounded Unique/Ambiguous/Unresolved/Unknown
status. Preserve the M3.3 transient engine as non-selecting input and keep lower-precedence paths.

**Blockers/dependencies/restart:** none. Rule metadata is used by newly built analyses. There is no
persistent schema consumer yet, so no migration, cache clear, service reload, or restart is required.

**Negative-memory status:** terminal durable constraints will be stored in Hindsight. Search handles:
`M3.3 exact rule packs`, `directional precedence`, `transient reachable traversal`, `no S2 promotion`,
`no M3.4 outcome`. The broad-patch and nondirectional-precedence failures are superseded by the verified
staged/directional implementation and remain negative constraints.

**Signature:** Codex (GPT-5), M3.3 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.4 active checkpoint — retained resolution paths and outcomes

**Date/time:** 2026-07-14T03:48:55+02:00

**Objective/target:** implement strict complete candidate-path storage and coverage-bounded resolution
status on top of M3.3's non-selecting traversal.

**Changes:** planning only. Opened clean jj child `oyrvxomo` over terminal M3.3 `xupxwnxm` and recorded
the local/UI execution plan. The design boundary keeps M3.3 transient and assigns all Serde, path
retention, endpoint equivalence, coverage, authority, rejection evidence, and terminal status to M3.4.

**Commands/checks run:** terminal M3.3 `jj status`; `jj new -m 'Store complete resolution paths'`;
Hindsight checkpoint/negative-memory writes and consolidation (1,217 entities, 3,203 relations, 3,394
observations); execution-plan update.

**Results/failure modes:** clean child created with no inherited working-copy changes. M3.3 remains fully
verified and described. No M3.4 source/schema edit exists yet. The primary failure modes to exclude are
first/sorted winner selection, endpoint-only deduplication that loses paths, status from candidate count
without coverage, and treating deferred import/module syntax as a resolved endpoint.

**Invalidated assumptions:** lookup precedence and evidence authority are separate; a maximum candidate
under an incomplete rule/build/provider surface cannot yield a terminal authoritative status. Stable
ordering is serialization determinism only. One endpoint reached by multiple paths is not ambiguity, but
the paths must still remain distinct.

**Current recommendation/next actions:** audit ADR 0002 and current fact/traversal fields, then define the
strict path/result/coverage/authority schemas and identity inputs before implementing derivation.

**Blockers/dependencies/restart:** none. No live state or dependency change applies.

**Negative-memory status:** active M3.3 constraints were consolidated successfully. M3.4 search handles:
`complete candidate paths`, `coverage before status`, `same endpoint multiple paths`, `no stable winner`.

**Signature:** Codex (GPT-5), M3.4 integration owner, active checkpoint, 2026-07-14.

---

## M3.4 schema/derivation checkpoint — complete paths before status

**Date/time:** 2026-07-14T04:06:33+02:00

**Objective/target:** make the frozen ADR path, coverage, precedence, authority, and status contract
executable without implementing later module stitching or external-provider ingestion.

**Changes:** added the 2,563-line `crates/deslop-parse/src/resolution.rs` and exported its public API.
`deslop.resolution/1` retains its `Arc<ScopeGraphProjection>`, exact analysis/scope-graph/build/fact/rule
policy identities, strict result/path documents, non-Serde dense result handles, payload-bound `rr1_` and
`rp1_` keys, reference evidence (including grammar/adapter/rules/capability/authority), every traversed
edge, endpoint, structured directional precedence component, independent namespace/visibility/timing/
condition/adapter check, rejection reason, exact source facts, dynamic boundaries, coverage reasons,
diagnostics, and status. Deferred imports are stored as unknown endpoint-free paths. Cross-field
validation requires every edge/check/boundary key to be retained and Complete coverage to have explicit
authority. Production manifests and the M3.3 traversal are unchanged.

**Commands/checks run:** repeated `cargo fmt --all`; `cargo check -p deslop-parse`; eight focused M3.4
tests with and without `--nocapture`; full `cargo test -p deslop-parse`; `cargo clippy -p deslop-parse
--all-targets -- -D warnings`; `RUSTDOCFLAGS='-D warnings' cargo doc -p deslop-parse --no-deps`; `git
diff --check`; targeted ADR/source/public-vocabulary audits.

**Results:** PASS. Complete fixtures produce Unique for one distinct maximum endpoint, Ambiguous for two
equal maximum endpoints, and Unresolved for zero viable endpoints. Partial zero-candidate, dynamic, and
deferred-import cases produce Unknown. Two distinct retained paths converging on one endpoint remain
Unique; reversing ambiguous path order cannot choose a winner. Lower-precedence, wrong-namespace,
not-visible, and declared-later paths remain stored with exact reasons. An unrelated same-name sibling
never enters a path. Strict documents reject status contradictions, corrupt payload keys, and unknown
fields. Policy changes alter projection identity and foreign dense IDs fail. The parse crate reports 111
passed/one designated slow probe ignored and four compile-fail doctests; focused clippy/rustdoc are clean.

**Invalidated assumptions / failure modes:** a missing precedence relation cannot fail the entire
projection or default to viable; it is now an explicit unknown check. Edge endpoints alone were not
sufficient provenance; validation now requires every edge endpoint/source, check source, and dynamic
boundary to occur in the retained source-fact closure. Candidate order is never consulted for status.

**Current recommendation/next actions:** audit wire validation and coverage dominance against all ADR
terminal combinations, run full workspace all-feature gates, and check M3.4 only if unchanged authority
gates and the no-bare-name boundary remain green.

**Blockers/dependencies/restart:** none. No dependency, live process, migration, reload, or cache action
applies; this is the first unused resolution document schema.

**Negative-memory status:** active constraints are implemented. Search handles: `rp1 complete payload`,
`coverage before status`, `same endpoint multiple paths`, `missing precedence unknown`, `source fact
closure`. No fallback or authority promotion was introduced.

**Signature:** Codex (GPT-5), M3.4 integration owner, schema/derivation checkpoint, 2026-07-14.

---

## M3.4 terminal checkpoint — complete retained resolution paths

**Date/time:** 2026-07-14T04:25:20+02:00

**Objective/target:** store every candidate resolution path with sufficient provenance to derive
coverage-bounded terminal outcomes without bare-name or stable-order fallbacks.

**Changes:** added and exported the 3,225-line `crates/deslop-parse/src/resolution.rs`. The strict
`deslop.resolution/1` projection retains the exact scope graph, build/fact/rule/policy identities,
reference evidence, all viable/rejected/unknown paths, edges, checks, directional precedence components,
endpoint equivalence, source-fact closure, dynamic boundaries, diagnostics, per-path/result coverage and
authority, payload-bound `rp1_`/`rr1_` keys, policy-bound projection identity, and non-Serde owner-checked
dense result handles. Status is derived from distinct viable maximum endpoints only after coverage:
Unique, Ambiguous, Unresolved, Unknown, or Conflict. Duplicate rules execute per language; deferred
imports and unresolved qualification tails remain endpoint-free Unknown. Strict validation now also
rejects uppercase/noncanonical digests and a Complete result containing any incomplete path.

**Commands/checks run:** repeated `cargo fmt --all`; 12 focused `resolution::tests`; `cargo test -p
deslop-parse --all-features`; two terminal runs of `cargo test --workspace --all-features`, `cargo build
--workspace --all-features`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`,
`cargo clippy --workspace --all-features --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and
`git diff --check`; targeted no-global/no-first-winner and capability-manifest diffs; `jj status` and
`jj diff --stat`.

**Results:** PASS. All 12 focused cases pass. The parse crate reports 115 passed, one designated slow
probe ignored, and four compile-fail doctests passed. Every all-feature workspace gate passes, including
unchanged M0/M1/M2 definition-of-done and graph false-resolution probes. Complete fixtures distinguish
one maximum endpoint, multiple equal endpoints, multiple paths to one endpoint, and no viable endpoints.
Incomplete dynamic/import/qualification/precedence cases remain Unknown. Lower-precedence and rejected
paths retain exact reasons. Reversing path order cannot choose a winner. Production adapters remain S1
and name-resolution capability remains Unknown. Disk state contains the schema/API only; no live process,
migration, rebuild deployment, reload, cache clear, dependency, or restart is required.

**Invalidated assumptions / failure modes:** missing precedence cannot fail construction or become viable;
it is explicit Unknown. Candidate count alone cannot assign status without result and per-path coverage.
Projection identity must bind exact result keys, not only graph and policy. A qualified root match cannot
promote an unresolved tail. Equal latest-visible positions remain tied; adapter duplicate rejection
cannot fall through outward. During validation, an explicit-shadowing edge initially omitted the
shadowing declaration from its retained source-fact closure; strict validation caught it and the path now
retains that fact. Path authority/coverage cannot be implicit behind result-level evidence. Hex-looking
but uppercase identities are noncanonical and rejected.

**Current recommendation/next actions:** open a fresh M3.5 jj child and design module/package/build-target
stitching around exact existing fact and build-context identities. The implementation must append module,
import/export/re-export/package edges to retained paths, preserve file-local invalidation, and never add a
repository-global bare-name lookup surface.

**Blockers/dependencies/restart:** none.

**Negative-memory status:** terminal constraints are ready for Hindsight consolidation. Search handles:
`M3.4 complete retained paths`, `coverage dominance`, `explicit shadowing source closure`, `no first
winner`, `qualification tail unknown`, `projection identity result keys`, `canonical resolution ids`,
`duplicate rejection no fallthrough`.

**Signature:** Codex (GPT-5), M3.4 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.5 active checkpoint — identity before incremental stitching

**Date/time:** 2026-07-14T04:37:00+02:00

**Objective/target:** stitch declared file/module/package/build-target names and preserve exact clean/
incremental parity with bounded reverse-dependency invalidation.

**Changes:** planning/audit only. Opened clean jj child `yxxzorzk` over terminal M3.4 `oyrvxomo`. Audited
ADR 0002, `ScopeFactData::{Import,Export,BuildModule}`, language import/module rules, deferred traversal,
resolution path construction, and `ProjectAnalysis::successor` change evidence.

**Commands/checks run:** `jj new/status`; targeted `rg`/`sed` over ADR 0002 and the scope graph,
resolution, traversal, language-rule, snapshot, and incremental modules; M3.4 Hindsight consolidation
(1,226 entities, 3,236 relations, 3,429 observations).

**Results/failure modes:** the fact model already carries exact import form/alias/selection/conditions,
export local target/name/re-export path/visibility/conditions, build module package/target/source-root/path/
file constituents, and opaque exact build-context identity. Traversal correctly defers only imports that
can supply the lookup root. The blocking identity defect is that `derive_fact_key` hashes the global
`ProjectAnalysisId` and builder index in addition to the complete revision payload. Therefore any
unrelated file edit or earlier fact insertion churns every fact, path, and result key, making the ADR's
unrelated-result reuse and byte-equivalent incremental contract unattainable.

**Invalidated assumptions / negative memory:** a payload-bound key is not incrementally stable merely
because it includes every field; adding whole-project and positional fields creates false dependencies.
Dense owner/index identity and wire revision identity must stay separate. The projection/document remains
analysis-bound; an unchanged fact's wire key should be bound to its exact file/node revision, build
context, policy, evidence, and data, not unrelated repository state. Module lookup must be constrained by
declared `BuildModule` constituents and exact package/target/path, never inferred from file names.

**Current recommendation/next actions:** first remove global analysis/index inputs from `sf1_` derivation
and add successor identity tests. Then construct the module stitch index, retained import/export/re-export
paths, and reverse-dependency successor API. Use one multi-file/two-target fixture to prove clean parity,
wrong-target rejection, export invalidation, and unrelated result reuse numerically.

**Blockers/dependencies/restart:** no external blocker or dependency. This is a schema identity correction
inside the unreleased M3 projection; no live process, migration, reload, cache clear, or restart applies.

**Negative-memory status:** provisional and active. Search handles: `M3.5 global analysis fact-key churn`,
`scope fact positional identity`, `module constrained import`, `incremental clean parity`, `no path stem
module inference`. Recheck after the fact-key successor test; do not implement caching atop unstable keys.

**Signature:** Codex (GPT-5), M3.5 integration owner, active identity checkpoint, 2026-07-14.

### M3.5 identity correction checkpoint

**Date/time:** 2026-07-14T04:45:00+02:00

**Changes:** removed whole-project `analysis_id` and positional builder `index` from `sf1_` derivation
and strict document recomputation. Keys still bind schema, exact build context, fact policy, complete
revision-bearing node/grammar/adapter/capability evidence, and fact payload. Added a two-file test that
changes only a peer source and reverses fact insertion order.

**Commands/results:** `cargo fmt --all`; focused successor identity test; all seven scope-graph tests;
`git diff --check`. PASS. The stable file retains the exact fact key. The edited peer fact key,
`ProjectAnalysisId`, and scope projection ID change. Dense process-local owner/index behavior and
build-context/policy key sensitivity remain intact.

**Invalidated assumption:** revision binding does not require a whole-project revision when exact file/
node revision evidence is already in the fact payload. Global and positional inputs were false
dependencies, not additional safety.

**Next action:** build the exact module index from declared `BuildModule` facts and extend deferred import
paths through module/export edges before adding successor-level reuse.

**Negative-memory status:** the provisional `M3.5 global analysis fact-key churn` failure is fixed at the
fact boundary and remains a regression constraint until clean/incremental resolution parity passes.

**Signature:** Codex (GPT-5), M3.5 integration owner, identity correction checkpoint, 2026-07-14.

---

## M3.5 module and invalidation checkpoint

**Date/time:** 2026-07-14T04:50:00+02:00

**Objective/target:** stitch exact declared module/package/build-target facts into retained resolution
paths and implement bounded successor invalidation with byte-identical clean-build parity.

**Changes:** added explicit `BuildModule.export_coverage` and aligned Complete coverage with declared
imports/exports adapter authority. Added a module stitch index constrained by exact build context,
package, target, module path, source-root ownership, and constituent file scopes. Alias, selective, glob,
export, and re-export edges now extend stored resolution paths; wrong-target candidates remain rejected;
cycle-aware traversal leaves pure cycles Unknown. Added `ResolutionProjection::successor` with explicit
added/removed fact sets, invalidation reasons, and reused/rebuilt/added/removed result reporting. Corrected
`ScopeFactKey` identity so unrelated project revisions and builder position are not false dependencies.

**Commands/checks run:** repeated `cargo fmt --all`; seven scope-graph tests; 17 focused resolution tests;
`cargo test -p deslop-parse --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc -p deslop-parse
--all-features --no-deps`; `cargo clippy -p deslop-parse --all-features --all-targets -- -D warnings`;
`git diff --check`.

**Results:** PASS at the package checkpoint. The parse package reports 121 passed, zero failed, one
designated slow instrumentation probe ignored, and four compile-fail doctests passed. Successor versus
clean strict JSON is byte-identical in all measured fixtures. Unrelated peer edit reuses five exact result
keys and rebuilds zero. Adding an export reuses one independent result and rebuilds five reverse
dependents. Adding a newly matching module rebuilds the one formerly unresolved import and reuses zero.
Focused rustdoc, clippy, formatting, and diff checks are clean.

**Invalidated assumptions / failure modes:** an exact export map does not prove that the export set is
complete; explicit export coverage is required before a missing or unique export can be terminal. A
whole-project analysis ID and builder position strengthen neither revision safety nor determinism when
exact fact evidence is already revision-bound; they only destroy reuse. Re-export traversal cannot use a
first-visited winner, and a pure cycle cannot fabricate a terminal endpoint. New facts require special
matching-module invalidation because a formerly unresolved path has no old module dependency to follow.

**Current recommendation/next actions:** run every all-feature workspace terminal gate, then audit the
diff for global bare-name/file-stem inference, first-winner/order fallback, and accidental production
capability promotion. Close M3.5 only if those checks and the unchanged earlier definition-of-done probes
remain green.

**Blockers/dependencies/restart:** none. No new dependency, process, migration, reload, cache clear, or
restart applies. Workspace-wide validation remains pending at this checkpoint.

**Negative-memory status:** provisional constraints remain active. Search handles: `M3.5 global analysis
fact-key churn`, `explicit export set coverage`, `new matching module invalidation`, `clean incremental
resolution parity`, `re-export pure cycle unknown`, `no path stem module inference`.

**Signature:** Codex (GPT-5), M3.5 integration owner, module/invalidation checkpoint, 2026-07-14.

---

## M3.5 terminal checkpoint — exact module stitching and bounded invalidation

**Date/time:** 2026-07-14T04:57:12+02:00

**Objective/target:** complete file/module/package/build-target stitching in the exact build context and
prove incremental successors preserve the clean strict resolution document.

**Changes:** terminalized the package checkpoint implementation. Exact `BuildModule` facts own package,
target, module path, source root, constituent file scopes, and explicit export-set coverage. Module alias,
selective, glob, export, and re-export edges are retained with source facts and checks; wrong-target
candidates remain rejected and pure cycles remain Unknown. Stable scope-fact wire keys exclude unrelated
analysis and builder-position inputs while retaining exact revision evidence. `ResolutionProjection::
successor` exposes additions, removals, reasons, reuse, and rebuilds from reverse dependencies.

**Commands/checks run:** `cargo test -p deslop-parse --all-features`; focused parse rustdoc and clippy;
`cargo test --workspace --all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D
warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace --all-features
--all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`; targeted `rg`, source
inspection, and parent diff audits for file-stem/global bare-name inference, order winners, and production
capability changes; `jj status/diff`.

**Results:** PASS. The parse package reports 121 passed, zero failed, one designated instrumentation
probe ignored, and four compile-fail doctests passed. Every all-feature workspace test/build/rustdoc/
clippy/fmt/diff gate passes, including unchanged M0/M1/M2 definition-of-done locks and graph false-
resolution probes. Clean/successor strict JSON is byte-identical. Measured invalidation is unrelated peer
edit: five reused/zero rebuilt; export addition: one independent reused/five reverse dependents rebuilt;
newly matching module: zero reused/one formerly unresolved rebuilt. Production adapter capability and
rule declarations are unchanged.

**Invalidated assumptions / failure modes:** exact export entries do not prove export-set completeness;
terminal outcomes require separate coverage evidence. Global analysis identity and builder position are
false dependencies for already revision-bound fact keys. A newly available module requires explicit
matching invalidation because an old unresolved result cannot record a dependency on a nonexistent fact.
Re-export cycles, missing coverage, wrong targets, and unevaluated conditions cannot be promoted by
deterministic order.

**Current recommendation/next actions:** open a clean M3.6 child and audit optional compiler/language-
server provider identity, build/artifact versioning, evidence precedence, and conflict retention before
adding any higher-authority fact. Preserve M3.5's strict paths and successor parity as regression gates.

**Blockers/dependencies/restart:** none. This changes only immutable Rust schemas/projections and tests;
no live process, dependency, migration, reload, cache clear, or restart is required.

**Negative-memory status:** terminal correction ready for Hindsight consolidation. Search handles:
`M3.5 stable scope fact key`, `explicit export set coverage`, `module constrained import`, `new matching
module invalidation`, `clean incremental parity`, `re-export cycle unknown`, `no path stem inference`.

**Signature:** Codex (GPT-5), M3.5 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.6 active checkpoint — provider evidence boundary audit

**Date/time:** 2026-07-14T05:12:00+02:00

**Objective/target:** add optional compiler/language-server resolution facts with exact artifact authority,
retained disagreement, and incremental clean-build parity.

**Changes:** planning/audit only. Opened clean jj child `rmxquklo` over terminal M3.5 `yxxzorzk`. Audited
ADR 0002, the language capability authority catalog, strict scope/resolution documents, resolution result
derivation/validation, M3.5 successor invalidation, and the existing external analyzer subsystem.

**Commands/checks run:** M3.5 `git diff --check` and `jj status`; Hindsight M3.5 positive/negative memory
write plus consolidation (1,232 entities, 3,277 relations, 3,473 observations); `jj new/log/status`;
targeted Hindsight searches; targeted `rg`/`sed` over ADR 0002, deslop-lang, deslop-parse resolution/scope
graphs, deslop-external, analyzer/protocol capability records, and public exports.

**Results/failure modes:** `CapabilityAuthority` has Syntax, Adapter, Compiler, and RuntimeVerification but
no LanguageServer despite the ADR requiring a distinct authority. `ResolutionResult.authority` is strictly
equal to its adapter reference evidence, so it cannot represent a preferred compiler/LSP conclusion.
`ProviderConflict` exists as a rejection reason/status guard but is exercised only by manually mutating a
test path. Resolution paths retain only scope fact keys, and no strict provider/configuration/project-model/
result-artifact fact set exists. Existing external analyzers return lint findings and availability notices;
they are not binding providers and cannot be safely reused as semantic evidence.

**Invalidated assumptions / negative memory:** the existing `Compiler` enum variant and `ProviderConflict`
label do not constitute provider ingestion. Lint-provider availability is not project-model completeness,
and a provider name/version without exact configuration/model/result artifact identity cannot authorize a
binding. Provider authority must join completed conclusions after language lookup; it must not enter or
overwrite adapter lookup precedence.

**Current recommendation/next actions:** first add a strict semantic-resolution fact projection bound to
the exact scope graph/build context and extend the catalog with LanguageServer. Then join immutable adapter
and provider conclusions, retain conflicts, and add semantic dependency invalidation. Use one convergent
two-reference fixture for stale identity, rank, equal/lower disagreement, and clean successor parity.

**Blockers/dependencies/restart:** none. No dependency or external provider execution is needed; synthetic
pinned facts are sufficient to make and verify the contract. No live process, migration, reload, cache
clear, or restart applies.

**Negative-memory status:** provisional and active. Search handles: `M3.6 provider evidence boundary`,
`LSP distinct authority`, `pinned semantic artifact`, `provider conclusion join`, `no provider overwrite`,
`equal authority no winner`. Recheck after strict semantic-fact adversarial tests.

**Signature:** Codex (GPT-5), M3.6 integration owner, provider-boundary checkpoint, 2026-07-14.

---

## M3.6 schema/join/invalidation checkpoint

**Date/time:** 2026-07-14T17:42:07+02:00

**Objective/target:** ingest optional compiler/language-server name-resolution conclusions with exact
artifact identity, evidence precedence, retained conflicts, and bounded incremental invalidation.

**Changes:** versioned the capability authority catalog to `deslop.language-adapter-capabilities/2` and
added distinct LanguageServer authority. Added the 827-line strict
`deslop.semantic-resolution-facts/1` projection and public builder/drafts/IDs. Extended resolution paths,
results, and documents with semantic fact provenance, explicit provider conclusions, a preferred diagnostic
conclusion, provider identity/artifact/model/authority checks, and strict cross-document validation. Added
`build_with_semantic_facts`, `successor_with_semantic_facts`, and `SemanticFactChanged` invalidation. The
adapter-only APIs remain compatible; the old successor fails closed rather than silently dropping facts.

**Commands/checks run:** repeated `cargo fmt --all`; authority catalog test and full 12-test deslop-lang
suite; repeated focused resolution tests (24 current); `cargo check -p deslop-parse`; `cargo test -p
deslop-parse --all-features`; focused parse/lang rustdoc; focused all-target all-feature clippy; `git diff
--check`; targeted schema/authority/source/diff audits; `jj status/diff/wc`.

**Results:** PASS at the focused checkpoint. Parse reports 128 passed, zero failed, one designated slow
instrumentation probe ignored, and four compile-fail doctests passed. Lang reports 12 passed. Complete LSP
agreement outranks but does not erase adapter evidence. Compiler/LSP agreement against the adapter yields
Conflict with a retained compiler preferred conclusion. Equal compiler disagreement is Conflict with no
winner and order-invariant JSON. Incomplete LSP facts remain Unknown provider paths without conflict or
authority. A pinned compiler may retain a positive external endpoint, but adapter disagreement still
blocks through Conflict. Strict fact/resolution documents reject forged or stale evidence.

**Numerical incremental results:** one result-artifact revision rebuilds exactly one of five results and
reuses four. One provider configuration revision changes both facts carrying that provider, rebuilds two,
and reuses three. Both successor documents exactly equal clean builds and every rebuild reports
`SemanticFactChanged`.

**Invalidated assumptions / failure modes:** an enum Compiler variant and a ProviderConflict label were not
provider ingestion. Provider output presence does not prove project-model coverage. Evidence authority is
not lookup precedence. A higher-authority preferred diagnostic does not remove lower disagreement or make a
conflicting result safe. Equal-authority agreement may share a conclusion; disagreement has no first/sorted
winner. Dropping semantic facts through the legacy successor would be unsound and now fails closed.

**Current recommendation/next actions:** run all workspace all-feature test/build/rustdoc/clippy/fmt/diff
gates; audit production manifests for unchanged S1/S2 authority, ensure RuntimeVerification never enters
the static join, and inspect the diff for provider absence/externality or order fallbacks. Close M3.6 only
if those checks and earlier M0-M3.5 gates remain green.

**Blockers/dependencies/restart:** none. No external provider process or dependency was added. These are
immutable schemas, ingestion APIs, joins, and fixtures; no live process, migration, reload, cache clear, or
restart applies.

**Negative-memory status:** provisional constraints implemented and awaiting terminal consolidation.
Search handles: `M3.6 pinned semantic facts`, `LSP distinct authority`, `provider conclusion join`, `equal
compiler no winner`, `semantic artifact invalidation`, `legacy successor fail closed`.

**Signature:** Codex (GPT-5), M3.6 integration owner, schema/join/invalidation checkpoint, 2026-07-14.

---

## M3.6 terminal checkpoint — pinned provider facts and conflict-preserving authority

**Date/time:** 2026-07-14T17:46:54+02:00

**Objective/target:** complete optional compiler/language-server semantic fact ingestion with higher static
authority, retained conflicts, exact artifact invalidation, and no adapter or runtime authority leakage.

**Changes:** finalized `deslop.language-adapter-capabilities/2`, distinct LanguageServer authority, strict
`deslop.semantic-resolution-facts/1`, public fact/provider/artifact builders and IDs, semantic conclusions
and paths inside `deslop.resolution/1`, preferred diagnostic conclusions, strict fact-to-path/result cross-
validation, `build_with_semantic_facts`, explicit semantic successor input, and `SemanticFactChanged`.
RuntimeVerification is rejected from static conclusions and Syntax cannot assert a terminal binding.

**Commands/checks run:** repeated focused schema/join/invalidation tests; full 12-test deslop-lang suite;
`cargo test -p deslop-parse --all-features`; focused rustdoc/clippy; two full terminal cycles including
`cargo test --workspace --all-features`, `cargo build --workspace --all-features`, `RUSTDOCFLAGS='-D
warnings' cargo doc --workspace --all-features --no-deps`, `cargo clippy --workspace --all-features
--all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`; targeted source/diff
audits for production capability promotion, runtime/static mixing, provider-absence externality, and
first/sorted winner behavior; `jj status/diff`.

**Results:** PASS. All 24 focused resolution tests pass. Parse reports 128 passed, zero failed, one
designated instrumentation probe ignored, and four compile-fail doctests passed. Lang reports 12 passed.
Every workspace all-feature gate passes, including unchanged M0/M1/M2 locks and graph false-resolution
probes. Complete LSP agreement can be preferred without removing adapter evidence. Complete compiler
evidence is preferred over LSP/adapter, but any complete disagreement remains Conflict. Equal compiler
disagreement is order-invariant Conflict with no preferred source. Incomplete facts remain Unknown and do
not conflict. Explicit pinned external endpoints are retained, but disagreement still blocks.

**Numerical incremental results:** changing one result artifact rebuilds exactly one of five references and
reuses four; changing a shared provider configuration rebuilds exactly its two references and reuses three.
Both successor documents are byte-identical to clean builds. The legacy successor fails closed if semantic
facts would otherwise be dropped.

**Invalidated assumptions / failure modes:** provider enum labels are not evidence ingestion; provider
output is not project-model completeness; lookup precedence and evidence authority are separate domains;
higher authority does not erase disagreement; equal authority cannot first-win; runtime observation cannot
enter the static join; provider absence cannot manufacture externality; and a semantic projection cannot be
silently omitted from a successor.

**Current recommendation/next actions:** open M3.7 and freeze joined adversarial fixtures for duplicate
names, nested scopes, wildcard/alias imports, re-exports, dynamic and unresolved cases, reusing M3.2-M3.6
fixtures where their complete paths/status/reasons already cover the ADR and adding missing cross-products.

**Blockers/dependencies/restart:** none.

**Negative-memory status:** terminal constraints ready for Hindsight consolidation. Search handles:
`M3.6 pinned semantic facts`, `LSP distinct authority`, `provider conclusion join`, `lower disagreement
retained`, `equal compiler no winner`, `runtime static rejection`, `semantic artifact invalidation`,
`legacy successor fail closed`.

**Signature:** Codex (GPT-5), M3.6 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.7 active checkpoint — joined adversarial gold audit

**Date/time:** 2026-07-14T17:51:32+02:00

**Objective/target:** freeze executable adversarial resolution fixtures for duplicate names, nested scopes,
alias/wildcard imports, re-exports, dynamic boundaries, and unresolved/unknown absence.

**Changes:** planning and fixture audit only. Opened clean jj child `nzytmqrp` over terminal M3.6
`rmxquklo`. Audited ADR 0002 acceptance dimensions, the existing nested lexical fixture, the multi-file/
multi-target module fixture, duplicate-definition policies, dynamic/deferred/qualified/missing cases, and
the exact retained `ResolutionResult`/`ResolutionPath` inspection surface.

**Commands/checks run:** `jj status`; targeted `rg`/`sed` over scope fact accessors, resolution fixtures and
tests, ADR 0002, TODO/plan/session artifacts; targeted Hindsight searches for the M3.7 area during session
continuation.

**Results/failure modes:** every named behavior has executable coverage, but the assertions are distributed
and often check only one property. There is no single frozen, hand-labelled corpus that locks status,
coverage, path viability, endpoint kind, traversal edges, rejection reasons, check states, and dynamic
boundaries together. Opaque fact keys are revision-bound and unsuitable as semantic gold labels; public
fact/path accessors are sufficient to label the meaningful structure without weakening provenance.

**Invalidated assumptions / negative memory:** isolated behavior tests are not yet the M3.7 frozen corpus.
Adding another production resolver path would be unjustified; the gap is joined executable evidence. Gold
serialization must not bake in opaque keys or use sorted order as a winner, and incomplete/dynamic cases
must remain represented as expected Unknown rather than disappearing from the matrix.

**Current recommendation/next actions:** add test-only semantic result/path summaries and one exact joined
matrix over existing executable fixtures. Run that focused matrix first; change production code only if a
measured retained-path mismatch exposes a real contract defect. Then run parse and workspace terminal gates.

**Blockers/dependencies/restart:** none. No dependency, live process, migration, reload, cache clear, or
restart applies.

**Negative-memory status:** provisional and active. Search handles: `M3.7 joined adversarial gold`,
`semantic labels not opaque keys`, `dynamic unknown denominator`, `no isolated-test relabel`, `no stable
winner`.

**Signature:** Codex (GPT-5), M3.7 integration owner, joined-gold audit checkpoint, 2026-07-14.

---

## M3.7 joined-gold implementation checkpoint

**Date/time:** 2026-07-14T17:58:43+02:00

**Objective/target:** make the named adversarial fixture breadth one exact executable contract while
preserving all retained resolution evidence.

**Changes:** added test-only semantic result/path summaries in `resolution.rs`; generalized the peer-module
fixture to construct a local binding/reference for an `imported` spelling when present; added a 16-case
joined test; and added versioned `tests/fixtures/resolution_m3_7_adversarial_gold.json`. Semantic endpoint
labels use endpoint kind, namespace/name or exact package/target/module path, source path, and source span;
opaque fact/result keys are intentionally excluded from the labels.

**Commands/checks run:** two instrumentation passes to measure the complete summaries; exact focused gold
test; `cargo test -p deslop-parse --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc -p deslop-parse
--all-features --no-deps`; `cargo clippy -p deslop-parse --all-features --all-targets -- -D warnings`;
`cargo fmt --all -- --check`; `git diff --check`; `jj status/diff --stat`.

**Results:** PASS. The frozen matrix matches all 16 labels. It contains 36 retained paths: 13 viable, 18
rejected, and 5 unknown; two complete zero/fully-rejected cases are Unresolved, while six incomplete,
dynamic, deferred, qualified, or cyclic cases are Unknown. Same-spelled `imported` references in importer
and peer files resolve only through their declared module/export and local lexical paths respectively.
Parse reports 129 passed, zero failed, one designated instrumentation probe ignored, and four compile-fail
doctests passed. Focused rustdoc/clippy/fmt/diff gates pass.

**Invalidated assumptions / failure modes:** isolated assertions were insufficient as a frozen corpus. The
measured gold also shows why endpoint-only snapshots are insufficient: several Unique results retain both
viable explicit and glob paths to one endpoint plus rejected wrong-target attempts. Opaque revision keys
would make unrelated edits churn the gold and remain excluded. No production resolver defect was found, so
no resolver behavior or capability authority changed.

**Current recommendation/next actions:** run the workspace all-feature terminal gates; audit exact case/path
counts from the artifact and ensure previous resolution/provider and M0-M2 gates remain green. Close M3.7
only after terminal success, then open M3.8 to score this frozen corpus rather than re-labelling it.

**Blockers/dependencies/restart:** none. This is test-only fixture evidence; no live-state transition,
dependency, migration, reload, cache clear, or restart applies.

**Negative-memory status:** provisional constraints implemented and awaiting terminal consolidation.
Search handles: `M3.7 16-case gold`, `36 retained paths`, `unrelated imported peer`, `semantic labels not
opaque keys`, `dynamic unknown denominator`.

**Signature:** Codex (GPT-5), M3.7 integration owner, joined-gold implementation checkpoint, 2026-07-14.

---

## M3.7 terminal checkpoint — frozen adversarial resolution corpus

**Date/time:** 2026-07-14T18:01:30+02:00

**Objective/target:** complete M3.7 with executable, versioned adversarial fixtures spanning every named
resolution boundary and retaining exact evidence rather than endpoint-only snapshots.

**Changes:** finalized the 16-case `deslop.resolution-adversarial-gold/1` fixture and joined executable test.
The test derives stable semantic endpoint labels from declaration namespace/name/source span or exact
package/target/module identity, then locks result status/coverage/authority/dynamic boundaries and every
path's endpoint, viability, ordered edges, rejection reasons, check states, source-fact kinds, provider-fact
count, authority, and coverage. The peer fixture can now create a same-spelled local `imported` binding to
prove repository-global uniqueness never leaks across file scopes. M3.7 is checked in `.agents/TODO.md`.

**Commands/checks run:** exact focused M3.7 gold test; `jq` denominator/status/path measurement; `cargo test
-p deslop-parse --all-features`; focused parse rustdoc/clippy/fmt/diff; full `cargo test --workspace
--all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace
--all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo
fmt --all -- --check`; `git diff --check`; targeted `jj diff` and `rg` audits.

**Results:** PASS. The artifact has exactly 16 cases and 36 retained paths: 13 viable, 18 rejected, and 5
unknown. Status counts are 7 Unique, 1 Ambiguous, 2 Unresolved, and 6 Unknown. Parse reports 129 passed,
zero failed, one designated instrumentation probe ignored, and four compile-fail doctests passed. Every
workspace all-feature terminal gate passes, including unchanged M0/M1/M2 numerical/authority locks and all
three graph false-resolution probes.

**Invalidated assumptions / failure modes:** separate behavioral tests were not a frozen gold corpus;
endpoint-only gold would have dropped explicit/glob alternate paths and wrong-target rejection evidence;
opaque keys would have made fixtures revision-churn-sensitive; and a same-spelled peer declaration must not
be omitted when proving absence of global-name fallback. These constraints are now executable. No production
defect was found and no production behavior was changed.

**Current recommendation/next actions:** open M3.8 from this frozen checkpoint. Score exact case/status/path/
endpoint agreement and publish confusion matrices plus unsupported/Unknown denominators and incremental
isolation counts. M3.8 must consume the corpus without re-labelling it.

**Blockers/dependencies/restart:** none. No dependency, runtime process, migration, reload, cache clear, or
restart applies.

**Negative-memory status:** terminal constraints ready for Hindsight consolidation. Search handles: `M3.7
16-case gold`, `36 retained paths`, `13 viable 18 rejected 5 unknown`, `unrelated imported peer`, `semantic
labels not opaque keys`, `M3.8 frozen denominator`.

**Signature:** Codex (GPT-5), M3.7 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.8 active checkpoint — measurement contract audit

**Date/time:** 2026-07-14T18:05:22+02:00

**Objective/target:** measure frozen-corpus resolution precision/recall and incremental file isolation with
raw denominators, full status confusion, and clean-build parity.

**Changes:** planning/audit only. Opened clean jj child `msxmkvpo` over terminal M3.7 `nzytmqrp`. Audited
ADR 0002 verification/report requirements, the frozen 16-case/36-path gold, `ResolutionUpdate` accessors,
and existing unrelated-file, export-cone, module-appearance, and semantic-artifact successor tests.

**Commands/checks run:** M3.7 Hindsight positive/negative writes and consolidation (1,250 entities, 3,334
relations, 3,534 observations); `jj new/log/status`; targeted Hindsight M3.8 search; targeted `sed`/`rg` over
ADR/TODO/update surfaces.

**Results/failure modes:** the frozen corpus has ten Complete supported cases and six expected Unknown cases.
A single overall accuracy would hide Unknown omissions and the distinction among Ambiguous, Unresolved, and
Conflict. Existing tests prove several successor counts independently but do not publish one exact isolation
table, and there is no explicit incremental Unique-to-Ambiguous reachable-duplicate scenario. M3.8 therefore
needs a scorer derived from the gold plus joined clean-parity scenarios, not hand-copied percentages.

**Invalidated assumptions / negative memory:** M3.7 exact equality is not itself the required M3.8 report.
Percentages without counts, excluding Unknown from the denominator, set-deduplicating alternate paths, or
claiming isolation from reuse counts without clean equality are invalid. The M3.7 labels are immutable input;
measurement cannot revise them.

**Current recommendation/next actions:** deserialize and score the gold against one reusable actual-corpus
builder, then add four exact incremental scenarios including unrelated same-spelled addition and reachable
equal-precedence transition. Publish the measured report only after focused tests pass.

**Blockers/dependencies/restart:** none. No dependency or live process is needed.

**Negative-memory status:** provisional and active. Search handles: `M3.8 full status confusion`, `Unknown
denominator`, `path multiset precision recall`, `clean parity isolation`, `reachable duplicate transition`,
`no corpus relabel`.

**Signature:** Codex (GPT-5), M3.8 integration owner, measurement-contract checkpoint, 2026-07-14.

---

## M3.8 scorer/isolation implementation checkpoint

**Date/time:** 2026-07-14T18:10:10+02:00

**Objective/target:** compute exact status/path/endpoint agreement from frozen gold and measure clean-parity
incremental reverse cones without omitting Unknown cases.

**Changes:** made the M3.7 gold types strict-deserializable; factored one actual-corpus builder; added exact
multiset scoring, five-status confusion, supported/Unknown segment measurements, and four incremental
isolation scenarios. Added `.agents/M3_8_RESOLUTION_REPORT.md` with raw counts, ratios, matrix, scenario
table, commands/failures, and scope limits. No production resolver/schema/adapter code changed.

**Commands/checks run:** exact focused corpus scorer; focused two-test `m3_8_` run; repeated `cargo fmt`.

**Results:** PASS at focused checkpoint. Confusion is diagonal `[7,1,2,6,0]`; 16/16 exact cases; 36/36
exact paths. Supported: 10/10 statuses, path precision/recall 27/27, endpoint precision/recall 18/18.
Expected Unknown: 6/6 statuses, 9/9 paths, 5/5 non-null endpoints. Incremental: unrelated addition 5 reused/
0 rebuilt/1 added; reachable duplicate 0/1 and Unique→Ambiguous; export addition 1/5 and Unknown→Unique;
module appearance 0/1 and Unknown→Unknown. Every successor equals clean JSON.

**Invalidated assumptions / failure modes:** the first module-appearance expected table treated invalidation
reasons as exclusive and expected only MatchingModuleAdded. The measured rebuild also correctly retains
ReachableScopeChanged. The assertion was corrected to preserve both evidence causes; no resolver behavior was
changed. Percent-only reporting, Unknown omission, alternate-path deduplication, and reuse without clean
parity remain invalid.

**Current recommendation/next actions:** run parse and full workspace all-feature terminal gates, update the
published report with final commands, audit that all changes remain test/report-only and prior gates remain
green, then close M3.8.

**Blockers/dependencies/restart:** none.

**Negative-memory status:** provisional negative constraint added: invalidation reason counts are
non-exclusive evidence dimensions. Search handles: `M3.8 module appearance two reasons`, `status matrix
7 1 2 6 0`, `Unknown denominator 6`, `27 path 18 endpoint`, `clean parity isolation`.

**Signature:** Codex (GPT-5), M3.8 integration owner, scorer/isolation checkpoint, 2026-07-14.

---

## M3.8 terminal checkpoint — exact resolution measurement and isolation

**Date/time:** 2026-07-14T18:12:59+02:00

**Objective/target:** finish M3.8 with an executable scorer and published report containing exact raw counts,
full status confusion, supported/Unknown path and endpoint agreement, and clean-parity isolation behavior.

**Changes:** finalized the reusable M3.7 corpus loader/actual builder, exact path and endpoint multiset scorer,
five-status confusion matrix, Complete and expected-Unknown segments, and four incremental scenarios. Updated
`.agents/M3_8_RESOLUTION_REPORT.md` to terminal status and checked M3.8 in `.agents/TODO.md`. All source
changes are inside the `resolution.rs` test module; production code is unchanged.

**Commands/checks run:** exact corpus scorer; focused `m3_8_` tests; `cargo test -p deslop-parse
--all-features`; focused parse rustdoc/clippy/fmt/diff; full `cargo test --workspace --all-features`; `cargo
build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features
--no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all --
--check`; `git diff --check`; targeted `jj status/diff` and report-state audits.

**Results:** PASS. Full status confusion is diagonal `[7,1,2,6,0]`; 16/16 cases and 36/36 paths match.
Complete supported cases: 10/10 statuses, path precision/recall 27/27, endpoint precision/recall 18/18.
Expected Unknown: 6/6 statuses, 9/9 paths, 5/5 endpoints. Isolation: unrelated same-spelled addition 5
reused/0 rebuilt/1 added; reachable duplicate 0/1 and Unique→Ambiguous; export cone 1/5 and Unknown→Unique;
module appearance 0/1 and Unknown→Unknown. Every successor is clean-document equal. Parse reports 131
passed, zero failed, one designated ignored probe, and four passing compile-fail doctests. Every workspace
gate, M0/M1/M2 lock, and graph false-resolution probe passes.

**Invalidated assumptions / failure modes:** invalidation reasons are non-exclusive; module appearance
retains both ReachableScopeChanged and MatchingModuleAdded for one rebuilt result. This was the only failed
first assertion and was corrected in the expected measurement without changing resolution behavior. A 1.0
ratio without raw counts, excluding Unknown, deduplicating alternate paths, or treating test-adapter results
as production capability remains invalid.

**Current recommendation/next actions:** open M3.DoD and join the frozen resolution gate to semantic-recipe
eligibility. Prove every non-Unique/incomplete/dynamic/conflicting result blocks consumers and that no graph/2
fallback authorizes a recipe.

**Blockers/dependencies/restart:** none. No dependency or live-state transition applies.

**Negative-memory status:** terminal constraints ready for Hindsight consolidation. Search handles: `M3.8
terminal matrix 7 1 2 6 0`, `27 path 18 endpoint`, `Unknown denominator 6`, `clean parity isolation`,
`invalidation reasons non-exclusive`, `production authority unchanged`.

**Signature:** Codex (GPT-5), M3.8 integration owner, terminal checkpoint, 2026-07-14.

---

## M3.DoD active checkpoint — semantic consumer boundary audit

**Date/time:** 2026-07-14T18:23:21+02:00

**Objective/target:** meet M3 definition of done by preserving the frozen resolution gate and blocking every
semantic unique-binding consumer when status, coverage, authority, capability, dynamic, or dependency
evidence is insufficient.

**Changes:** planning/audit only. Opened clean jj child `nmqkqpms` over terminal M3.8 `msxmkvpo`. Audited ADR
0002 consumer authority, all non-test uses of `ResolutionProjection`/`ResolutionStatus`, graph/2/MCP
descriptions, protocol work orders, adapter manifests, result provenance, and TODO ownership for M4/M5.

**Commands/checks run:** M3.8 Hindsight positive/negative writes plus consolidation (1,259 entities, 3,364
relations, 3,566 observations); `jj new/log/status`; targeted Hindsight M3.DoD search; targeted `rg`/`sed`
over parse/lang/protocol/graph/MCP/ADR/TODO surfaces.

**Results/failure modes:** resolution results currently have no production consumer outside `deslop-parse`.
Transformation recipes do not exist until M5.1; current WorkOrders are syntax/finding rewrite requests and
must not be silently reclassified as semantic recipes. Graph/2 remains explicitly syntactic and its MCP
description already denies resolution proof. The missing M3 boundary is therefore a reusable fail-closed
eligibility contract. Exact adapter manifests are retained in result reference evidence, so capability
checks need no live registry lookup. Reverse-dependency evidence must be projection-bound and downgrade-only.

**Invalidated assumptions / negative memory:** wiring M3 resolution into every current WorkOrder would be a
contract error, not completeness. An endpoint or higher-authority Preferred conclusion cannot authorize a
Conflict. Syntax and RuntimeVerification are not acceptable static semantic authority. A caller-supplied
boolean cannot prove dependency completeness, and first-failure decisions would hide concurrent blockers.

**Current recommendation/next actions:** add a versioned unique-binding consumer gate in deslop-parse with
strict requirement construction, projection-bound dependency evidence, all-reasons decisions, and no graph
input. Join it to all 16 frozen cases plus provider/capability/dependency adversarial cases.

**Blockers/dependencies/restart:** none. No dependency or live process is needed.

**Negative-memory status:** provisional and active. Search handles: `M3.DoD unique binding gate`, `no
WorkOrder semantic conflation`, `projection-bound dependencies`, `Preferred Conflict blocks`, `no graph2
fallback`, `all block reasons`.

**Signature:** Codex (GPT-5), M3.DoD integration owner, consumer-boundary checkpoint, 2026-07-14.

---

## M3.DoD gate implementation checkpoint

**Date/time:** 2026-07-14T18:28:33+02:00

**Objective/target:** provide the semantic boundary future recipes can consume while guaranteeing incomplete,
ambiguous, dynamic, conflicting, under-capability, under-authority, and incomplete-dependency results block.

**Changes:** added `resolution_gate.rs` and public exports. `ResolutionCapabilityRequirement` rejects Syntax
and RuntimeVerification minima; `ResolutionConsumerRequirement::unique_binding` requires canonical distinct
capabilities including NameResolution; `ResolutionDependencyEvidence::from_projection` binds exact
projection/result and is downgrade-only. `evaluate_unique_binding` returns a versioned all-reasons decision
with complete provenance and no graph input/fallback. Added three focused adversarial tests in the existing
resolution fixture module.

**Commands/checks run:** `cargo check -p deslop-parse`; repeated `cargo fmt`; focused `cargo test -p
deslop-parse 'resolution::tests::m3_dod_' -- --nocapture`.

**Results:** PASS. Exactly seven frozen cases are eligible: nested explicit shadowing's inner binding,
selective import, alias import, re-export, wildcard import, and the importer/peer same-spelled local results.
All other nine cases block. Ambiguous retains status plus two-endpoint cardinality blocks; dynamic retains
status, partial coverage, dynamic-boundary, and incomplete-dependency blocks; complete zero-candidate retains
Unresolved plus zero-endpoint blocks. Compiler minimum blocks adapter evidence; CallGraph requirement blocks
Unknown production/test manifest capability; downgraded dependencies block; foreign evidence errors. A
compiler-preferred Conflict still blocks and exposes no endpoint.

**Invalidated assumptions / failure modes:** the first test compile compared a borrowed `Vec<String>` to a
string array; this was a test expression type mismatch, corrected without contract change. More importantly,
one global authority minimum would have incorrectly demanded compiler authority for unrelated CallGraph
capability; requirements are capability-specific, and NameResolution uses joined result authority while
additional capabilities use the exact stored adapter manifest.

**Current recommendation/next actions:** run parse tests/doctests/rustdoc/clippy, audit the new public surface
and absence of graph/protocol coupling, then run full workspace all-feature terminal gates. Close M3.DoD only
if frozen M3.7/M3.8, M0-M2, graph false-resolution, and production capability gates stay green.

**Blockers/dependencies/restart:** none. No new dependency or live-state transition applies.

**Negative-memory status:** provisional constraints implemented. Search handles: `M3.DoD seven eligible`,
`capability-specific authority`, `dependency downgrade only`, `compiler Preferred Conflict blocks`, `no
graph input`, `all reasons decision`.

**Signature:** Codex (GPT-5), M3.DoD integration owner, gate implementation checkpoint, 2026-07-14.

---

## M3.DoD terminal checkpoint — complete

**Date/time:** 2026-07-14T18:33:25+02:00

**Objective/target:** close M3 with a public semantic-consumer boundary that cannot authorize incomplete,
ambiguous, dynamic, conflicting, under-capability, under-authority, or dependency-incomplete resolution.

**Changes:** finalized and documented `deslop.resolution-consumer-gate/1`, its exact-provenance decision,
capability-specific static authority checks, projection-bound downgrade-only dependency evidence, and
endpoint suppression on every block. Checked M3.DoD in the durable TODO.

**Commands/checks run:** focused M3.DoD tests; parse crate tests and compile-fail doctests; `cargo test
--workspace --all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc
--workspace --all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D
warnings`; `cargo fmt --all -- --check`; `git diff --check`; targeted source/API and cargo-tree audits.

**Results:** PASS. Exactly 7/16 frozen Complete Unique cases are eligible and all other nine block. Parse has
134 passing tests, one designated ignored test, and four passing compile-fail doctests. Full workspace gates,
M0-M2 regressions, graph false-resolution probes, provider Conflict, authority/capability/dependency, and
foreign-evidence adversarial cases pass. Production adapter capability declarations remain unchanged.

**Invalidated assumptions / failure modes:** current WorkOrders are syntactic rewrite requests, not semantic
recipes. Authority requirements must be capability-specific rather than global. A Preferred provider in a
Conflict cannot authorize a consumer. Syntax and RuntimeVerification are not static semantic authority.
Complete dependency evidence cannot be caller-forged, endpoint presence cannot bypass result status, and
graph/2 cannot serve as a resolution fallback.

**Current recommendation/next actions:** snapshot the verified M3 boundary, consolidate its durable memory,
then begin M4.1 with a versioned control-flow edge schema and explicit authority/capability semantics.

**Blockers/dependencies/restart:** none. No dependency, migration, rebuild, or live-process restart applies.

**Negative-memory status:** terminal constraints ready for durable consolidation. Search handles: `M3.DoD
consumer gate complete`, `WorkOrder is not semantic recipe`, `capability-specific authority`, `Preferred
Conflict blocks`, `projection-bound dependencies`, `no graph2 fallback`.

**Signature:** Codex (GPT-5), M3.DoD integration owner, terminal checkpoint, 2026-07-14.

---

## M4.1 active checkpoint — control-flow schema audit

**Date/time:** 2026-07-14T18:38:00+02:00

**Objective/target:** define the versioned control-flow edge contract before any adapter-specific CFG lowering.

**Commands/checks run:** targeted Hindsight active-plan/negative-memory search; `rg`/`sed` audit across the M4
roadmap, ADRs 0001/0002, `deslop-parse` identity/scope/resolution overlays, `deslop-lang` capability tiers and
production manifests, and `deslop-graph` projection/types/tests.

**Results:** `AdapterCapability::ControlFlow` already exists at S2, while every production adapter honestly
reports it Unknown. `deslop.graph/2` is explicitly a syntactic project dependency view and cannot own CFG
semantics. The shared immutable `ProjectAnalysis`, revision-bound `NodeKey`, stored grammar/adapter identity,
and projection-ID machinery provide the correct substrate. No control-flow schema exists yet.

**Invalidated assumptions / failure modes:** query `@control` captures and canonical branch/loop roles are
syntax seeds, not control-flow transitions. Enumerating edges does not prove complete CFG coverage. Runtime
observations cannot serve as exhaustive static authority. Entry/exit virtual points cannot be inferred from
source spans, and exceptional, abrupt, or suspension transitions cannot be flattened into normal flow.

**Current recommendation/next actions:** add ADR 0003 and strict `deslop.control-flow/1` types in
`deslop-parse`; bind graph/point/edge keys to payloads, retain exact capability/coverage/uncertainty evidence,
and validate every boundary/topology invariant. Exercise all eight edge families in one complete synthetic
fixture and add a corruption matrix. Leave production adapter lowering and capability promotion to M4.2.

**Blockers/dependencies/restart:** none. No new dependency is expected.

**Negative-memory status:** provisional and active. Search handles: `M4.1 CFG schema`, `control capture not CFG`,
`no graph2 CFG`, `virtual entry exit`, `static ControlFlow authority`, `coverage not edge enumeration`.

**Signature:** Codex (GPT-5), M4.1 integration owner, schema-audit checkpoint, 2026-07-14.

---

## M4.1 implementation checkpoint — strict control-flow schema

**Date/time:** 2026-07-14T18:52:55+02:00

**Objective/target:** implement the M4.1 schema boundary without prematurely lowering production adapters.

**Changes:** added accepted ADR 0003 and public `deslop.control-flow/1` types/builders in `deslop-parse`.
Graphs retain exact analysis/projection/policy, executable owner, grammar, adapter manifest, ControlFlow
support/authority, coverage reasons, one virtual entry/exit, owner-contained syntax/synthetic points, and
payload-bound graph/point/edge keys. Added typed entry, exit, normal, branch, loop, exceptional, abrupt, and
suspension families with 35 exercised portable sub-kind instances. Complete coverage rejects non-static,
unknown/unsupported, conservative, recovered, cross-file, outside-owner, or uncertain evidence.

**Commands/checks run:** repeated focused `cargo test -p deslop-parse 'control_flow::tests::m4_1_'`; `cargo
check -p deslop-parse --all-features`; `cargo test -p deslop-parse --all-features`; parse all-target clippy and
rustdoc with warnings denied; `cargo fmt --all`; `git diff --check`; cargo-tree and source audits for graph/2
independence and production capability non-promotion.

**Results:** PASS. Five focused schema/adversarial suites pass. Full parse result is 139 passed, zero failed,
one designated ignored latency/memory probe, and four passing compile-fail doctests. Parse check, clippy,
rustdoc, fmt, and diff checks pass. `deslop-parse` has no `deslop-graph` dependency. All six production language
packs still declare ControlFlow Unknown with no authority and cannot build Complete CFG evidence.

**Invalidated assumptions / failure modes:** an initial review found point/edge keys were revision/policy-bound
but not adapter-manifest-bound; the exact adapter identity is now part of both key payloads. Same-file evidence
alone was also too broad; every non-virtual point, edge source, and predicate must fall inside the executable
owner region. Clippy rejected two complex test tuple types; named aliases corrected the test structure without
changing the contract.

**Current recommendation/next actions:** run full workspace all-feature test/build/rustdoc/clippy/fmt/diff
gates and unchanged M0-M3 regressions. If green, check M4.1, consolidate negative memory, and snapshot before
beginning M4.2 adapter lowering.

**Blockers/dependencies/restart:** none. No new dependency or live-state transition applies.

**Negative-memory status:** provisional constraints implemented. Search handles: `M4.1 strict CFG schema`,
`adapter-bound control keys`, `owner-contained CFG evidence`, `35 edge sub-kinds`, `production ControlFlow
Unknown`, `no graph2 dependency`.

**Signature:** Codex (GPT-5), M4.1 integration owner, schema implementation checkpoint, 2026-07-14.

---

## M4.1 terminal checkpoint — complete

**Date/time:** 2026-07-14T18:52:55+02:00

**Objective/target:** close the control-flow schema milestone only after full workspace regression gates.

**Changes:** checked M4.1 in the durable TODO and finalized the active plan. The delivered boundary consists of
accepted ADR 0003, public strict `deslop.control-flow/1` types/builders/exports, adapter- and revision-bound
graph/point/edge identities, virtual boundaries, typed flow families, explicit coverage/precision, and
fail-closed topology/authority validation. Production lowering and capability promotion remain deferred to
their explicit M4.2 owner.

**Commands/checks run:** focused five-suite M4.1 matrix; full parse tests/check/clippy/rustdoc; cargo-tree/source
authority audits; `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace
--all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Five M4.1 suites cover eight families, 35 portable sub-kind instances, stable strict wire
round-trip, corruption, topology, precision, owner containment, and production non-promotion. Parse reports
139 passed, zero failed, one designated ignored instrumentation probe, and four passing compile-fail doctests.
All workspace terminal gates and unchanged M0/M1/M2 plus graph false-resolution probes pass. No new dependency
was introduced, `deslop-parse` does not depend on `deslop-graph`, and all six production adapters remain
ControlFlow Unknown with no authority.

**Invalidated assumptions / failure modes:** query captures/canonical roles are lowering seeds, not CFG edges;
same-file evidence is insufficient without owner containment; point/edge identity must bind the adapter
manifest; enumerated edges do not prove complete coverage; RuntimeVerification is not exhaustive static CFG
authority; exceptional/abrupt/suspension flow cannot collapse into normal; deterministic ordering does not
resolve uncertainty. Two test-only complex tuple types were replaced with aliases after clippy rejection.

**Current recommendation/next actions:** snapshot M4.1, consolidate its positive/negative memory, then begin
M4.2 by defining versioned adapter lowering rules and capability declarations before promoting any production
language.

**Blockers/dependencies/restart:** none. No runtime restart, migration, or cache clear applies.

**Negative-memory status:** terminal constraints ready for durable consolidation. Search handles: `M4.1
complete control-flow schema`, `control capture not CFG`, `adapter-bound point edge keys`, `owner containment`,
`coverage not enumeration`, `no graph2 CFG`, `production ControlFlow Unknown`.

**Signature:** Codex (GPT-5), M4.1 integration owner, terminal checkpoint, 2026-07-14.

---

## M4.2 active checkpoint — adapter lowering audit

**Date/time:** 2026-07-14T19:00:00+02:00

**Objective/target:** establish the identity and authority prerequisites for real per-adapter CFG lowering.

**Commands/checks run:** Hindsight active-plan/negative-memory search; targeted `rg`/`sed` audit of all six
production capability manifests, canonical control roles, Control query packs, construct policies, raw grammar
kinds, metrics flow-break catalogs, snapshot adapter identity construction, and adapter schema consumers.

**Results:** every production adapter still declares ControlFlow Unknown. Julia, Python, JavaScript,
TypeScript/TSX, and Rust expose Provided syntax-level Control captures; Clojure explicitly leaves Control query
support Unknown because reader/macro context is not a total query. These captures identify candidate constructs
but contain no evaluation-order, successor, exception, abrupt-target, or suspension semantics. No versioned
control-flow lowering rule pack exists in stored adapter identity. Adding it changes semantic identity and
therefore requires an adapter schema bump and exact capability/pack/dialect coherence checks.

**Invalidated assumptions / failure modes:** canonical roles plus source order are not a portable lowering
algorithm. A Provided Control query is not a Provided ControlFlow capability. Rust calls may unwind and macros/
unsafe constructs are opaque; those owners cannot be Complete until explicitly modeled. Clojure core/macro
forms cannot be treated uniformly without reader/macro authority. Omitting an Unknown adapter from a project
projection would hide a coverage gap.

**Current recommendation/next actions:** first add strict `deslop.language-control-flow-rules/1`, store it in
`LanguageAdapterIdentity`, bump adapter schema, and reject capability/rule/dialect contradictions. Then build a
fixture-backed Rust lowering slice with explicit Partial boundaries before considering further promotions.

**Blockers/dependencies/restart:** none. Existing tree-sitter and owned-arena APIs are sufficient; no new
dependency is justified.

**Negative-memory status:** provisional and active. Search handles: `M4.2 lowering rules identity`, `control
query not ControlFlow`, `Rust unwind partial`, `Clojure macro CFG unknown`, `all-pack dispatch gap`.

**Signature:** Codex (GPT-5), M4.2 integration owner, lowering-audit checkpoint, 2026-07-14.

---

## M4.2 checkpoint — lowering-rule identity substrate

**Date/time:** 2026-07-14T19:06:04+02:00

**Objective/target:** make adapter CFG behavior versioned, stored, capability-aligned, and dialect-applicable
before implementing the lowering engine.

**Changes:** added strict public `deslop.language-control-flow-rules/1` with payload-free Unknown/Unsupported
states and canonical Provided packs. Provided payloads retain exact dialect artifacts, static authority,
evaluation order, owner/body selectors, and typed sequence, branch, match, loop, abrupt, exceptional,
suspension, opaque, and adapter-defined actions. Stored the pack in `LanguageAdapterIdentity`, included its
canonical wire bytes in identity hashing, bumped adapter schema `/2`→`/3`, and made snapshot publication reject
pack/schema, capability support/authority, and selected-dialect contradictions. Updated M2's pinned schema.

**Commands/checks run:** focused rule-pack tests; focused snapshot alignment/identity test; all deslop-lang
tests; all parse tests/doctests; focused M4.1 tests; M2 DoD; deslop-lang/deslop-parse all-target clippy and
rustdoc with warnings denied; fmt and diff checks.

**Results:** PASS. Two rule-pack strictness/corruption tests, one snapshot identity/alignment test, 14/14
deslop-lang tests, 140 active parse tests plus one designated ignored probe and four compile-fail doctests,
five M4.1 suites, and M2 DoD pass. Clippy, rustdoc, fmt, and diff checks pass. All production packs have
payload-free Unknown ControlFlow rule packs matching their Unknown capability, so their highest complete tier
remains S1.

**Invalidated assumptions / failure modes:** behavior implemented only in Rust code would not invalidate
stored adapter identity. Capability and rule support must be checked together; either one alone can lie.
Provided rules for a different grammar version cannot lower the selected snapshot. A test-only M4.1 adapter
that declared Provided ControlFlow needed an explicit matching rule pack once alignment became executable.
Clippy rejected one complex mutation-vector type; a named alias fixed only test structure.

**Current recommendation/next actions:** implement shared owned-arena lowering against the exact stored pack.
Use a test-only Provided Rust catalog to establish sequence/branch/loop/abrupt edge correctness and explicit
Partial gaps; promote the production Rust pack only after the full rule catalog and fixture gate are complete.

**Blockers/dependencies/restart:** none. The adapter schema bump expires prior derived projection identities;
fresh analyses rebuild automatically. No runtime restart or migration applies.

**Negative-memory status:** provisional constraints enforced. Search handles: `M4.2 rule pack identity`,
`adapter schema 3`, `capability rule alignment`, `dialect-applicable CFG rules`, `production remains S1`.

**Signature:** Codex (GPT-5), M4.2 integration owner, rule-identity checkpoint, 2026-07-14.

---

## M4.2 checkpoint — production Rust lowering and all-pack dispatch

**Date/time:** 2026-07-14T19:30:48+02:00

**Objective/target:** implement auditable owned-arena CFG lowering at each production adapter's declared tier,
promoting only a fixture-backed adapter and retaining explicit gaps everywhere else.

**Changes:** implemented public `lower_control_flow`, `ControlFlowLoweringResult`, and
`ControlFlowLoweringGap` over the exact stored rule pack. Added compositional owner lowering for sequence,
if/else, loop/while/for, return/break/continue/terminate, opaque boundaries, entry/exit dispatch, coverage, and
edge precision. Rust now declares Provided ControlFlow at Adapter authority with 17 versioned rules covering
function/closure/const/static owners and explicit Partial boundaries for match, macro, unsafe, call unwind,
question-mark propagation, await, and yield. Added declared label-kind handling so labeled break/continue is
matched to the correct loop. Fixed abrupt-only sequence and branch reachability so unreachable suffixes cannot
fabricate normal exits. The other five production packs remain Unknown and emit canonical per-file gaps.

**Commands/checks run:** focused M4.2 tests throughout; full `cargo test -p deslop-parse --all-features`;
`cargo test -p deslop-lang --all-features`; parse/lang all-feature all-target clippy with `-D warnings`;
parse/lang rustdoc with `-D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS at the crate checkpoint. Eight M4.2 tests cover a test-only Provided pack, exact production
Rust sequence/branch/loop/abrupt lowering, labeled outer breaks, while/for/continue, simple values,
abrupt-only reachability, match and nested-control Partial boundaries, call/macro Partial boundaries, one
Unknown adapter, and all six production adapters in one dispatch matrix. Deslop-parse reports 148 passed, zero
failed, one designated ignored probe, and four passing compile-fail doctests. Deslop-lang reports 14/14. Focused
clippy, rustdoc, fmt, and diff checks pass.

**Invalidated assumptions/failure modes:** the first broad parse run failed because an adapter matrix assumed
every S2 capability remained Unknown and resolution-only test adapters inherited Rust's new Provided
ControlFlow declaration without its stored rule pack. The matrix now recognizes only Rust's promotion; the
resolution fixtures explicitly restore Unknown. A first sequence traversal also carried unreachable suffix
exits after `return`; it now advances only from a reachable normal predecessor. Nearest-loop consumption is
invalid for labeled transfers; target labels are retained and matched. Simple return values do not by
themselves imply uncertainty, while nested control in values/predicates does.

**Current recommendation/next actions:** run full workspace all-feature test/build/rustdoc/clippy/fmt/diff plus
frozen M0-M4.1 regression gates. If green, update the completion ledger, consolidate durable positive and
negative memory, and snapshot M4.2 before beginning dominance/post-dominance work.

**Blockers/dependencies/restart:** none. Adapter schema `/3` invalidates old derived identities; fresh snapshot
construction rebuilds them. No service restart, migration, or cache clear applies.

**Negative-memory status:** provisional and enforced. Search handles: `M4.2 Rust CFG promotion`, `unreachable
suffix normal flow`, `labeled break nearest loop invalid`, `resolution test adapter capability inheritance`,
`nested predicate control Partial`, `all-pack lowering gaps`.

**Signature:** Codex (GPT-5), M4.2 integration owner, production-lowering checkpoint, 2026-07-14.

---

## M4.2 terminal checkpoint — complete and verified

**Date/time:** 2026-07-14T19:35:57+02:00

**Objective/target:** close adapter-tier CFG lowering only after production promotion, explicit gaps, identity,
coverage, reachability, cross-crate consumers, and frozen regressions all agree.

**Final changes:** stored and identity-bound strict `deslop.language-control-flow-rules/1`; bumped adapter schema
to `/3`; enforced capability/rule/authority/dialect coherence; added shared owned-arena lowering and explicit
gap results; promoted Rust alone to Provided/Adapter with 17 fixture-backed rules; retained Unknown for
Clojure, Julia, Python, JavaScript, and TypeScript; integrated the new per-capability truth into adapter, M2,
and resolution-only fixture matrices. Checked M4.2 complete in `.agents/TODO.md`.

**Commands run:** `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`;
`git diff --check`; focused parse/lang/M4.1/M4.2/M2 gates during implementation; Hindsight remember/improve in
`repo:/home/christos/code/deslop`.

**Results:** PASS. Every workspace test target passes except the repository's two designated ignored probes;
workspace build, rustdoc, clippy, fmt, and diff checks pass. M0/M1/M2, graph false-resolution, M3 frozen and
adversarial resolution, M4.1 schema, and M4.2 lowering/dispatch regressions pass. Parse reports 148 active
tests and four compile-fail doctests; language reports 14 tests. Hindsight consolidation reports 1,284
entities, 3,493 relations, and 3,701 observations.

**Invalidated assumptions/failure modes:** source-order/control captures are not CFG authority; behavior cannot
live outside stored identity; sequence enumeration cannot imply reachability; labeled transfers cannot default
to the nearest loop; a capability expectation cannot be generalized from semantic tier alone; test adapters
cannot inherit manifests independently of rule payloads. These constraints are now code-, fixture-, and
memory-enforced.

**Current recommendation/next actions:** begin M4.3 from the frozen `deslop.control-flow/1` projection. Define
dominance/post-dominance and SESE/PST identity, disconnected/nonterminating handling, and Partial-coverage
propagation before implementing algorithms.

**Blockers/dependencies/restart:** none. Fresh snapshot construction naturally expires adapter `/2` identities;
no service restart, migration, or cache clear is required.

**Negative-memory status:** durable and consolidated. Search handles: `M4.2 terminal Rust CFG`, `unreachable
suffix normal flow`, `labeled break nearest loop invalid`, `test adapter capability inheritance`, `nested
control Partial`, `all-pack dispatch gaps`.

**Signature:** Codex (GPT-5), M4.2 integration owner, terminal checkpoint, 2026-07-14.

---

## M4.3 active checkpoint — dominance and region contract audit

**Date/time:** 2026-07-14T19:37:43+02:00

**Objective/target:** define the semantic boundary for dominance, post-dominance, and hierarchical SESE/PST
regions over the frozen M4.2 control-flow projection.

**Commands/checks run:** targeted Hindsight active-plan and negative-memory search; local audit of
`ControlFlowProjection`, graph/point/edge identity, coverage, virtual boundaries, and lowering behavior for
unreachable suffixes and nonterminating owners.

**Results:** the CFG schema intentionally retains all syntax points, including unreachable suffixes, and forces
at least one explicit virtual-exit edge. Dominance must therefore be restricted to entry-reachable points;
post-dominance must be restricted independently to points with a path to the virtual exit. Their intersection
is the terminating core on which point-hammock SESE regions can be claimed. Partial CFG coverage and reachable
exit-unreachable points must downgrade the region projection rather than disappear.

**Invalidated assumptions/failure modes:** initializing dominators for every retained point assigns facts to
dead code. Initializing post-dominators for every point makes disconnected/nonterminating regions look as if
they terminate. A stable-order choice among overlapping candidate regions is not hierarchy. Syntax nesting is
not a substitute for graph boundaries, and the project dependency graph remains out of scope.

**Current recommendation/next actions:** accept ADR 0004; add strict projection identity and per-point full/
immediate relations; compute forward/reverse reachability before fixed points; form only boundary-valid,
laminar nontrivial regions; retain overlaps for M4.4; prove with one numerical corpus before workspace gates.

**Blockers/dependencies/restart:** none. Existing immutable CFG keys and standard-library ordered collections
are sufficient; no dependency is justified.

**Negative-memory status:** provisional constraints active. Search handles: `M4.3 unreachable dominance`,
`postdom exit reachability`, `nonterminating virtual exit`, `SESE overlap no stable winner`, `Partial CFG
region coverage`.

**Signature:** Codex (GPT-5), M4.3 integration owner, contract-audit checkpoint, 2026-07-14.

---

## M4.3 checkpoint — dual fixed points and laminar SESE regions

**Date/time:** 2026-07-14T19:50:53+02:00

**Objective/target:** implement the accepted reachability-bounded dominance/post-dominance and hierarchical
structured-region contract without granting facts to dead or nonterminating points.

**Changes:** accepted ADR 0004; added strict public `deslop.control-regions/1`, region-policy and payload key
types, coverage, per-point full/immediate relations and depths, structured root/branch/loop regions, residuals,
document/projection/error APIs, and `derive_control_regions`. Implemented forward/reverse reachability,
independent fixed points, unique immediate-parent derivation, terminating-core hammock candidates, two-way SESE
boundary validation, equal/overlap rejection, smallest-container parents, reciprocal children, identity
binding, and strict deserialization validation. Re-exported the complete surface from `deslop-parse`.

**Commands/checks run:** six focused M4.3 numerical/corruption tests; full
`cargo test -p deslop-parse --all-features`; parse all-feature all-target clippy with `-D warnings`; parse
rustdoc with `-D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS at the parse checkpoint. Numerical vectors include a four-point linear graph with dual depths
`[0,1,2,3]`; three nested root/branch regions; two loop regions; normal/abrupt branch post-dominance at the
virtual exit; one unreachable suffix with empty relations; and a nonterminating graph with three entry-
reachable, two exit-reachable, and zero terminating-core points. Parse reports 154 passed, zero failed, one
designated ignored probe, and four passing compile-fail doctests. Focused clippy/rustdoc/fmt/diff pass.

**Invalidated assumptions/failure modes:** Rust's eager `bool::then_some` evaluated a depth subtraction even for
an out-of-domain point and underflowed; lazy `then` now preserves empty relations. Equal candidate point sets
cannot be treated as nested merely because each is a subset of the other; both now become residual. A
structured forest without a valid root is not a PST and is demoted to residual evidence.

**Current recommendation/next actions:** expand the corruption matrix for source graph/projection/policy links
and hierarchy closure, audit an explicit non-laminar graph, then run full workspace and frozen regression gates.
Keep M4.3 open until those pass.

**Blockers/dependencies/restart:** none. The projection uses standard-library ordered collections and the
existing immutable CFG; no dependency, restart, migration, or cache clear applies.

**Negative-memory status:** provisional and enforced. Search handles: `then_some eager dominance underflow`,
`equal region point sets not nested`, `structured forest no root`, `entry reachability domain`, `exit
post-dominance domain`.

**Signature:** Codex (GPT-5), M4.3 integration owner, implementation checkpoint, 2026-07-14.

---

## M4.3 terminal checkpoint — complete and verified

**Date/time:** 2026-07-14T19:58:37+02:00

**Objective/target:** close dominance, post-dominance, and hierarchical SESE/PST regions only after independent
reachability domains, structure, uncertainty, identity, corruption, and all prior regressions agree.

**Final changes:** accepted ADR 0004; implemented and exported strict `deslop.control-regions/1`, dual fixed
points, full/immediate relation facts and depths, structured root/branch/loop hammocks, residuals, coverage and
payload identity. Added policy/source-CFG identity matrices, hierarchy/relation corruption cases, and a mixed
terminating/infinite branch proving a Complete source CFG can yield Partial region coverage. Checked M4.3
complete in `.agents/TODO.md`.

**Commands run:** `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`;
`git diff --check`; eight focused M4.3 suites and focused parse gates during implementation; Hindsight
remember/improve in `repo:/home/christos/code/deslop`.

**Results:** PASS. Every workspace target passes except the two designated ignored probes. Parse reports 156
active passing tests plus four compile-fail doctests. Exact numerical results include linear dual depths
`[0,1,2,3]`, three nested diamond regions, two loop regions, one dead suffix, zero terminating-core points for
an infinite owner, and one residual root for mixed terminating/nonterminating control. Hindsight consolidation
reports 1,298 entities, 3,525 relations, and 3,735 observations.

**Invalidated assumptions/failure modes:** eager `then_some` can execute invalid out-of-domain arithmetic;
serialized points do not share one dominance universe; virtual exit existence does not imply termination;
equal point sets are not strict nesting; stable order cannot resolve crossing regions; a forest without one
root is not a PST; deterministic fixed points cannot upgrade source coverage. All are now code-, fixture-, and
memory-enforced.

**Current recommendation/next actions:** begin M4.4 by versioning irreducible/non-structured classification on
top of existing residuals. Distinguish crossing entry/exit, multi-entry SCC, exit-unreachable/nonterminating,
and invalid boundary causes without reclassifying them as structured.

**Blockers/dependencies/restart:** none. No restart, migration, cache clear, or dependency applies.

**Negative-memory status:** durable and consolidated. Search handles: `M4.3 terminal dominance postdominance`,
`then_some eager dominance underflow`, `unreachable dominance`, `postdom exit reachability`, `nonterminating
virtual exit`, `equal region sets not nested`, `structured forest no root`.

**Signature:** Codex (GPT-5), M4.3 integration owner, terminal checkpoint, 2026-07-14.

### M4.4 start checkpoint — explicit non-structured control regions

**Date/time:** 2026-07-14, Europe/Madrid.

**Objective:** Preserve irreducible, nonterminating, and rejected SESE candidates as typed non-structured
control facts without weakening or version-mutating M4.3's structured PST.

**Target:** a separate strict `deslop.non-structured-control-regions/1` overlay over the exact M4.3 projection
and source CFG. Deterministic SCC analysis will distinguish multi-entry irreducibility from ordinary reducible
cycles and from exit-unreachable nontermination. Existing residuals will retain exact typed provenance.

**Commands run:** `jj status`; targeted `rg`/`sed` audit of control-region residual derivation, control-flow
builder/validation, public exports, M4 plan/TODO, and ADR 0004. Serena is active but registered for Python only
and cannot symbolically inspect the Rust module, so targeted local Rust reads are the explicit fallback.

**Results:** the M4.4 working change is clean and based on verified M4.3. M4.3's `/1` residual payload has
free-text reasons and cannot be extended with typed fields without a wire break. The source CFG public API is
sufficient for a separate overlay and synthetic adversarial graphs. No dependency or runtime restart applies.

**Invalidated assumptions / negative-memory constraint:** adding typed fields directly to
`deslop.control-regions/1` would silently change a frozen schema. A cyclic SCC alone does not prove
irreducibility; multiple distinct external entry points are required. Virtual exit existence does not prove
that a cycle terminates.

**Current recommendation / next actions:** accept ADR 0005, implement the overlay and SCC classifier, add the
convergent gold/corruption suites, then run focused and full gates.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.4 integration owner, start checkpoint, 2026-07-14.

#### M4.4 implementation checkpoint — focused gates pass

**Date/time:** 2026-07-14, Europe/Madrid.

**Objective/target:** implement ADR 0005 as a strict source-bound overlay that preserves irreducible,
nonterminating, rejected SESE, and incomplete-flow evidence without modifying M4.3's structured schema.

**Changes:** added and exported `deslop.non-structured-control-regions/1`; added policy, projection, graph, and
fact identities; implemented iterative SCC discovery over entry-reachable points; retained canonical external
entry/exit boundaries; classified multi-entry irreducibility and exit-unreachable nontermination independently;
typed every known M4.3 residual cause with exact residual provenance; emitted explicit unknown facts for
non-Complete source CFGs; accepted ADR 0005; added eight numerical, adversarial, identity, and corruption tests.

**Commands run:** focused `cargo test -p deslop-parse --all-features m4_4_ -- --nocapture`; full
`cargo test -p deslop-parse --all-features`; parse all-target clippy with warnings denied; parse rustdoc with
warnings denied; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Parse reports 164 active passing tests, one designated ignored probe, and four passing
compile-fail doctests. Numerical fixtures retain one 3-point irreducible SCC with two entry targets and one
exit origin; zero facts for a Complete one-entry reducible loop; one nonterminating production cycle; two
independent facts for a multi-entry nonterminating SCC; exact residual point/boundary provenance; and one
unknown fact for Partial macro lowering.

**Invalidated assumptions/failure modes:** changing M4.3 `/1` is a wire break; cyclic does not imply
irreducible; a nonterminating SCC can have an outgoing edge to another exit-unreachable component, so an empty
external exit boundary is not required; deterministic SCC output does not upgrade source coverage; absence of
facts on a Partial source CFG cannot prove reducibility. The implementation and ADR encode each correction.

**Current recommendation/next actions:** run all workspace all-feature terminal gates and explicit M4.1-M4.3
regressions, update TODO/report, consolidate Hindsight, and snapshot M4.4 if clean.

**Blockers/dependencies/restart:** none. No dependency, migration, rebuild outside normal compilation, or
runtime restart applies.

**Negative-memory status:** local checkpoint recorded; durable Hindsight consolidation remains for terminal
close. Search handles: `M4.4 multi-entry SCC`, `cycle not irreducible`, `nonterminating SCC external exit`,
`Partial CFG explicit unknown fact`, `do not mutate control-regions/1`.

**Signature:** Codex (GPT-5), M4.4 integration owner, focused checkpoint, 2026-07-14.

#### M4.4 terminal checkpoint — complete and verified

**Date/time:** 2026-07-14, Europe/Madrid.

**Objective/target:** close M4.4 with a deployable, strict representation for irreducible, nonterminating,
rejected structured-candidate, and incomplete-flow control evidence while preserving M4.3's PST truth.

**Final changes:** accepted ADR 0005; implemented and exported strict
`deslop.non-structured-control-regions/1`; added exact M4.1/M4.3 projection and policy links, classification
policy, graph/fact identities, iterative entry-reachable SCC analysis, canonical component boundaries, typed
multi-entry and nontermination facts, fail-closed residual classification, and explicit incomplete-CFG unknown
facts. Checked M4.4 complete in `.agents/TODO.md`.

**Commands run:** `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`;
`git diff --check`; focused M4.4 and full parse gates during implementation.

**Results:** PASS. All workspace targets pass except the two designated ignored probes. Parse reports 164
active passing tests plus four compile-fail doctests. Exact M4.4 results are one three-point SCC with two entry
targets and one exit origin; no facts for a Complete one-entry reducible loop; one exit-unreachable production
cycle; two independent facts for a multi-entry nonterminating SCC; one typed source-bound residual; and one
owner-level unknown fact for Partial macro lowering.

**Invalidated assumptions/failure modes:** a frozen M4.3 schema cannot be extended in place; cyclic does not
mean irreducible; virtual exit existence does not mean a cycle terminates; a nonterminating SCC may exit to
another nonterminating component; stable order cannot place irreducible/crossing facts into a PST; deterministic
SCC computation cannot upgrade Partial source coverage; no emitted fact on incomplete input cannot prove
reducibility. ADR, code, fixtures, and durable memory enforce these boundaries.

**Current recommendation/next actions:** begin M4.5 with versioned def/use, reaching-definition, liveness,
parameter/output, and conservative-effect facts layered on authoritative CFG/region evidence. Consumers must
block or explicitly handle intersecting M4.4 facts.

**Blockers/dependencies/restart:** none. No restart, migration, cache clear, or dependency applies.

**Negative-memory status:** durable and consolidated under `repo:/home/christos/code/deslop`; Hindsight reports
1,305 entities, 3,549 relations, and 3,760 observations. Search handles: `M4.4 terminal irreducible SCC`,
`cycle not irreducible`, `nonterminating SCC external exit`, `Partial CFG explicit unknown`,
`control-regions/1 frozen`.

**Signature:** Codex (GPT-5), M4.4 integration owner, terminal checkpoint, 2026-07-14.

### M4.5 start checkpoint — resolved dataflow and conservative effects

**Date/time:** 2026-07-14, Europe/Madrid.

**Objective:** advance the active M4 milestone by implementing M4.5 def/use, reaching definitions, liveness,
parameter/output, and conservative effect facts as the prerequisite for M4.6 PDGs.

**Target:** accepted ADR 0006 and strict `deslop.data-flow/1`, joining exact M3 scope/resolution evidence to
exact M4 CFG points and retaining independent DefUse/Effects capability truth.

**Commands run:** `jj status`; targeted Hindsight recall/search; targeted `rg`/`sed` audit of the M4 roadmap,
adapter capability catalog, scope-fact/reference/binding model, resolution endpoints/results, CFG/region APIs,
and current production manifests. Serena remains active but is registered for Python only and cannot
symbolically inspect the Rust implementation, so local Rust symbol reads are the explicit fallback.

**Results:** M4.5 starts from clean change `e31c1e47`. The capability catalog already separates DefUse and
Effects at S2, while every production adapter still declares both Unknown. Scope facts retain exact
declaration/definition/binding/reference evidence and resolution retains typed endpoints/coverage/authority.
These are sufficient for a source-validated builder and hand-labelled Provided-capability gold corpus; they do
not authorize name-based production inference.

**Invalidated assumptions / negative-memory constraint:** canonical Read/Write roles and same-spelled names do
not identify symbols; deterministic equations do not upgrade extraction coverage; liveness at exit does not
prove a return or mutation output; a virtual exit does not terminate an SCC.

**Current recommendation / next actions:** implement the strict dataflow wire/builder surface, then add the
reaching-definition/liveness engine and convergent fixtures before changing any production capability.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.5 integration owner, start checkpoint, 2026-07-14.

#### M4.5 implementation checkpoint — ordered fixed points compile and pass

**Date/time:** 2026-07-14, Europe/Madrid.

**Changes:** added and exported `data_flow.rs` with strict `deslop.data-flow/1` document/projection/graph,
policy and payload identities, normalized symbols, ordered definitions/accesses, explicit parameter/output
boundaries, conservative effects, per-point reaching/liveness sets, source-validated drafts, capability-aware
coverage, strict deserialization and corruption-resistant payload/closure validation. Implemented forward
reaching definitions and backward liveness over entry-reachable CFG points.

**Commands run:** repeated `cargo check -p deslop-parse --all-features`; `cargo fmt --all`; focused
`cargo test -p deslop-parse --all-features m4_5_ -- --nocapture`; parse all-target clippy with warnings denied.

**Results:** PASS for the current five numerical suites. Linear transfer kills the prior same-symbol
definition; a branch join retains both reaching definitions; loop liveness converges without an exit path;
dead points have empty execution relations; and a same-point access sees only definitions with a lower event
ordinal. The public module compiles warning-free under clippy.

**Invalidated assumptions/failure modes:** a point-level unordered set is insufficient because compound writes
read before defining; write accesses and their definition occurrence are one shared event ordinal rather than
two arbitrarily ordered events. Provided Effects cannot call omitted reachable points pure. These corrections
are encoded in the builder and tests.

**Current recommendation/next actions:** build the end-to-end Provided-capability fixture joining real scope,
resolution, CFG, and regions; assert parameter/return/mutation/effect facts; add ambiguity, Partial coverage,
identity, and wire-corruption cases; then run full M4.5 gates.

**Blockers/dependencies/restart:** none. Production adapters remain honestly Unknown for DefUse/Effects and
will not be promoted by the fixture.

**Negative-memory status:** provisional local record. Search handles: `M4.5 ordered dataflow events`,
`compound write reads before definition`, `Provided Effects total reachable points`, `unreachable dataflow`.

**Signature:** Codex (GPT-5), M4.5 integration owner, ordered-engine checkpoint, 2026-07-14.

#### M4.5 terminal checkpoint — complete and verified

**Date/time:** 2026-07-14 21:49:03 CEST (Europe/Madrid).

**Objective/target:** close M4.5 with a strict, deployable local dataflow projection that joins exact M3
symbols/resolution to exact M4 CFG/region points and exposes honest def/use, liveness, boundary, and effect
evidence for M4.6 PDGs.

**Changes:** accepted ADR 0006; added and exported strict `deslop.data-flow/1` and policy/key families;
implemented source-validated graph drafts, normalized resolved symbols, ordered definition/access events,
entry-reachable reaching definitions and liveness, explicit parameter/return/mutation/exception/suspension
boundaries, conservative per-point effects, coverage/capability propagation, strict deserialization, payload
identity, and closure validation. Source facts must be inside the CFG owner. Parameter inputs require a
Parameter binding and its definition at virtual entry; symbol-bearing boundaries must normalize to the cited
symbol. Production adapters remain honestly Unknown for DefUse and Effects.

**Commands run:** focused `cargo test -p deslop-parse --all-features m4_5_ -- --nocapture`; focused
`cargo clippy -p deslop-parse --all-features --all-targets -- -D warnings`; `cargo test --workspace
--all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace
--all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt
--all -- --check`; `git diff --check`.

**Results:** PASS. Eight focused M4.5 tests pass. The numerical gold sets cover linear same-symbol kill/gen,
two-arm joins, nonterminating cyclic liveness, unreachable-point isolation, same-point read-before-write, and
resolved-shadow isolation. Integration fixtures prove Complete scope/resolution/CFG joins, parameter/return/
mutation/effect preservation, ambiguity remaining unknown and Partial, missing reachable effect evidence
forcing Partial, malformed boundaries and owner-foreign sources failing closed, deterministic repeat identity,
policy-sensitive identity, strict wire round-trip, and schema/unknown-field/payload corruption rejection. All
workspace targets pass except the two designated ignored instrumentation probes; parse reports 172 active
passing tests, one designated ignored probe, and four passing compile-fail doctests.

**Invalidated assumptions/failure modes:** declarations without Binding facts are not visible resolver
candidates, so a duplicate-declaration ambiguity fixture initially produced Unknown; explicit bindings are
required to test ambiguity. Same spelling and canonical roles never identify a symbol. Compound writes read
before their paired definition. Liveness does not infer outputs. Empty effects are pure only under complete
Effects coverage. Unreachable points carry no execution relations. Deterministic equations and manual
fixtures do not promote production adapter authority.

**Current recommendation/next actions:** begin M4.6 by deriving local PDGs from M4.3 post-dominance/control
dependence and M4.5 reaching-definition/access links, while retaining M4.4 non-structured and all M4.5 gaps as
consumer blockers.

**Blockers/dependencies/restart:** none. No dependency, migration, cache clear, or runtime restart applies.

**Negative-memory status:** local durable lesson recorded; Hindsight consolidation follows this snapshot.
Search handles: `M4.5 declarations need bindings`, `ambiguous resolution fixture Unknown`, `ordered compound
write`, `Provided Effects reachable totality`, `parameter definition at entry`, `dataflow owner containment`.

**Signature:** Codex (GPT-5), M4.5 integration owner, terminal checkpoint, 2026-07-14.

### M4.6 start checkpoint — strict local program dependence graphs

**Date/time:** 2026-07-14 21:52:21 CEST (Europe/Madrid).

**Objective/target:** implement M4.6 local PDGs from exact M4.3 control-dependence evidence and exact M4.5
reaching-definition links while retaining M4.4 non-structured facts and all upstream/local gaps.

**Changes:** accepted ADR 0007 and wrote the active convergent plan. The v1 boundary uses exact CFG points as
PDG nodes, flow data edges from retained definition/access pairs, direct control edges from CFG-edge witnesses
and complete post-dominator chains, and typed gaps for unresolved access or unavailable post-dominance. Anti-
and output-dependence remain outside v1 because M4.5 does not authorize their memory/write-order semantics.

**Commands run:** `jj new -m 'Build local program dependence graphs'`; targeted `rg`/`sed` audit of the M4
roadmap, ADRs 0003-0006, M4.1 CFG edges/points, M4.3 point/post-dominator relations, M4.4 typed facts, and M4.5
definition/access/point APIs. M4.5 Hindsight consolidation completed with 1,309 entities, 3,573 relations, and
3,786 observations.

**Results:** the required source APIs are present and versioned. M4.3 deliberately leaves post-dominance empty
for exit-unreachable points, so the control-dependence algorithm must fail individual witnesses into typed
gaps rather than force a virtual exit. M4.5 accesses already retain exact reaching-definition keys, allowing
flow data edges without new name or order inference.

**Invalidated assumptions / negative-memory constraint:** CFG adjacency is not control dependence; stable
iteration cannot repair a missing post-dominator chain; liveness does not authorize anti/output dependence;
M4.4 facts cannot be dropped merely because some PDG edges are computable.

**Current recommendation / next actions:** implement the strict wire/projection surface, pure control/data
derivers, and numerical fixtures before the combined integration/corruption suite.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.6 integration owner, start checkpoint, 2026-07-14.

#### M4.6 implementation checkpoint — combined control/data PDG passes

**Date/time:** 2026-07-14 22:08:55 CEST (Europe/Madrid).

**Changes:** added and exported strict `deslop.program-dependence/1`; implemented source-projection/policy
binding, point nodes, flow edges from exact reaching definitions, control edges from exact CFG-edge witnesses
and complete immediate-post-dominator chains, typed unresolved/post-dominance gaps, M4.4 fact retention,
coverage propagation, payload identities, and strict document validation. Expanded the capability-provided
integration source to a real conditional with a two-definition dataflow join.

**Commands run:** repeated focused `cargo test -p deslop-parse --all-features m4_6_ -- --nocapture`; focused
M4.5/M4.6 and all-M4 parse suites; `cargo clippy -p deslop-parse --all-features --all-targets -- -D warnings`;
`cargo fmt --all`; `git diff --check`.

**Results:** PASS. Eight M4.6 tests lock diamond and nested control dependence, loop-header self-dependence,
nonterminating post-dominance gaps, unreachable-edge isolation, two-definition flow dependence, unresolved
access gaps, a Complete combined conditional PDG with one control and five flow edges, and a Partial ambiguous/
nonterminating PDG retaining one M4.4 NonTerminatingCycle fact plus three typed gaps. Repeat identity, changed
policy identity, strict round-trip, and corruption/schema/unknown-field rejection pass. Parse clippy is clean.

**Invalidated assumptions/failure modes:** `ResolutionProjection` exposes its policy through the retained
document, not a direct getter. A nonterminating CFG still requires an explicit incoming virtual-exit edge, but
that edge may originate in an unreachable exit-dispatch component. Entry reachability and exit reachability
are independent M4.3 domains: a point can be unreachable from entry yet reach virtual exit, so PDG validation
must not impose `exit_reachable => reachable`. Execution dependence edges still require entry-reachable nodes.

**Current recommendation/next actions:** perform the terminal source-closure/coverage audit and run all six
workspace gates; if clean, close M4.6 and advance to M4.7 summaries/SDG.

**Blockers/dependencies/restart:** none.

**Negative-memory status:** local durable lesson updated. Search handles: `M4.6 reachability domains
independent`, `unreachable exit dispatch exit reachable`, `do not force virtual exit`, `CFG adjacency not
control dependence`.

**Signature:** Codex (GPT-5), M4.6 integration owner, combined-engine checkpoint, 2026-07-14.

#### M4.6 terminal checkpoint — complete and verified

**Date/time:** 2026-07-14 22:13:55 CEST (Europe/Madrid).

**Objective/target:** close M4.6 with strict local PDGs whose control and flow edges are derived only from
retained M4.1-M4.5 authority and whose unavailable evidence remains typed and consumer-visible.

**Final changes:** accepted ADR 0007; added and exported strict `deslop.program-dependence/1`; bound exact
analysis, CFG, control-region, non-structured-control, resolution, and dataflow projections/policies; added
point nodes, canonical source catalogs, direct control edges with inducing CFG witnesses, flow edges with exact
symbol/definition/access evidence, typed unresolved-access and post-dominance gaps, M4.4 fact retention,
coverage propagation, and payload identities. Strict validation enforces source closure, canonical nonempty
reasons, gap/reason alignment, Complete-without-gaps, reachable execution endpoints, and exit-reachable control
evidence.

**Commands run:** focused M4.6, combined M4.5/M4.6, and all-M4 parse tests; focused parse clippy; `cargo test
--workspace --all-features`; `cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc
--workspace --all-features --no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`;
`cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Eight focused M4.6 suites lock diamond/nested/loop control dependence, unreachable and
nonterminating behavior, multi-definition flow, unresolved gaps, a Complete combined conditional PDG with one
control plus five flow edges, and a Partial ambiguous/nonterminating PDG retaining its M4.4 fact and three
typed gaps. Deterministic/policy identity and strict round-trip/corruption matrices pass. All workspace targets
pass except the two designated ignored probes; parse reports 178 active passing tests, one designated ignored
probe, and four passing compile-fail doctests.

**Invalidated assumptions/failure modes:** CFG adjacency is not control dependence; an incomplete
post-dominator walk cannot emit partial guessed edges; liveness does not authorize anti/output dependence; an
unreachable exit-dispatch may be exit-reachable; nonterminating CFGs still require explicit virtual-exit
topology without connecting the reachable cycle to it; unknown resolution never becomes a data edge.

**Current recommendation/next actions:** begin M4.7 with versioned local summaries and SDG edges that attach
to exact M4.5 parameter/output/effect boundaries and M4.6 nodes, and emit no interprocedural edge without exact
M3 call resolution and compatible callee authority.

**Blockers/dependencies/restart:** none. No dependency, migration, cache clear, or runtime restart applies.

**Negative-memory status:** local lessons recorded; Hindsight consolidation follows this snapshot. Search
handles: `M4.6 reachability domains independent`, `CFG adjacency not control dependence`, `do not force virtual
exit`, `typed PDG gaps`, `exact reaching definition only`.

**Signature:** Codex (GPT-5), M4.6 integration owner, terminal checkpoint, 2026-07-14.

### M4.7 start checkpoint — explicit summaries and system dependence

**Date/time:** 2026-07-14 22:23:12 CEST (Europe/Madrid).

**Objective/target:** implement M4.7 call/parameter/return/global summaries and SDG edges only where exact
resolution, local-PDG, explicit binding, and S3 capability authority permit.

**Changes:** corrected the M4.6 coverage prerequisite so Complete local PDGs bind `LocalPdg: Provided` and its
authority; the Provided fixture now declares it while production adapters remain Unknown. Accepted ADR 0008
and wrote the convergent M4.7 plan around explicit callable-summary and call-site binding drafts.

**Commands run:** targeted `rg`/`sed` audit of the S2/S3 capability catalog, M3 preferred endpoints, M4.5 call
accesses/boundaries/effects, M4.6 nodes/edges/gaps, scope kinds, and the milestone contract; focused M4.6 tests
and parse all-target clippy after the capability correction. M4.6 Hindsight consolidation completed with 1,312
entities, 3,588 relations, and 3,803 observations.

**Results:** M3/M4 facts can map a Complete Unique Declaration/Definition endpoint to a local CFG only when its
retained node exactly equals one owner. They cannot encode actual/formal order, return destination, default/
variadic/receiver mapping, or exact global identity from an effect flag. M4.7 will require explicit drafts and
Provided CallGraph/Sdg capability for Complete edges.

**Invalidated assumptions / negative-memory constraint:** call syntax order is not parameter binding; call
resolution is not SDG parameter/return authority; `GlobalState` does not identify a global declaration;
LocalPdg output coverage must bind its explicit capability rather than only prerequisite fact coverage.

**Current recommendation / next actions:** implement strict summary/call-site wire and draft types, then exact
local-callee mapping, before adding any interprocedural edge.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.7 integration owner, start checkpoint, 2026-07-14.

#### M4.7 terminal checkpoint — complete and verified

**Date/time:** 2026-07-14 22:50:06 CEST (Europe/Madrid).

**Objective/target:** close M4.7 with strict callable/global summaries and interprocedural dependence edges
that require exact local resolution, explicit bindings, complete local PDG evidence, and graph-specific S3
capability authority.

**Changes:** accepted ADR 0008; added and exported `deslop.system-dependence/1` plus its policy and stable
summary/call/edge/gap identities; required one callable summary per local PDG; retained ordered formal inputs,
typed outputs, and source-validated global reads/writes/mutation outputs; mapped callees only from Complete
Unique preferred Declaration/Definition endpoints at exact local CFG owners; emitted call, parameter-in,
return, and parameter-out edges only from explicit drafts; retained unresolved/non-local, missing-binding,
unsupported-output, and participant-specific capability gaps. Corrected M4.6 Complete coverage to bind
`LocalPdg` support and authority. M4.7 now retains per-graph CallGraph/Sdg support and authority and strictly
checks edge/gap semantics against their call-site evidence during deserialization.

**Commands run:** repeated focused `cargo test -p deslop-parse --all-features m4_7_ -- --nocapture`; focused
parse all-target clippy; `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace
--all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Eight M4.7 suites cover policy identity, malformed digest rejection, fail-closed coverage,
support/authority alignment, participant-specific capability-gap identity, one-to-one bindings, directional
edge identity, and a two-callable end-to-end corpus. The corpus numerically locks two summaries, one call,
one explicit global summary, and four edges (one each call/parameter-in/return/parameter-out); missing
parameter and output drafts produce exact typed gaps without guessed edges. Full workspace tests pass with
186 active parse tests and one designated ignored instrumentation probe; all build, rustdoc, clippy, format,
and diff gates pass.

**Invalidated assumptions/failure modes:** declaration spelling or containment cannot identify a callee;
declarations without binding-timing evidence are not Complete Unique resolution; CallGraph support does not
authorize parameter/return dependence without Sdg; aggregate coverage cannot replace graph-specific support
and authority; `GlobalState` effects do not identify a global; missing one binding does not invalidate another
independently explicit binding, but coverage remains incomplete; wire payload hashes alone do not prove
call-site membership or graph direction.

**Current recommendation/next actions:** start M4.8 with a frozen advanced-semantics fixture matrix for
exceptional exits, async/yield suspension, closures/captures, mutation/alias uncertainty, and early exits.
Reuse the strict M4.1-M4.7 schemas; add facts only where an existing contract cannot express the fixture,
and require every unavailable semantic to remain a typed coverage or dependence gap.

**Blockers/dependencies/restart:** none. No dependency, migration, cache clear, or runtime restart applies.

**Negative-memory status:** durable checkpoint pending Hindsight write. Search handles: `M4.7 exact owner
callee`, `binding timing required for unique resolution`, `participant capability authority`, `no global
effect identity`, `strict edge gap call-site semantics`.

**Signature:** Codex (GPT-5), M4.7 integration owner, terminal checkpoint, 2026-07-14.

### M4.8 start checkpoint — advanced semantic uncertainty matrix

**Date/time:** 2026-07-14 22:50:06 CEST (Europe/Madrid).

**Objective/target:** add frozen exception, async/yield, closure, mutation, alias-uncertainty, and early-exit
fixtures that numerically verify M4.1-M4.7 preserve typed semantics and never promote unavailable evidence.

**Changes:** created a new jj change and wrote the convergent M4.8 plan. The initial contract audit confirms
the existing schemas expose exceptional, abrupt, and suspension control edges; exceptional/suspension/
mutation output boundaries; Capture/Borrow/ReadWrite accesses; Throws/Suspends/Captures effects; and typed
PDG/SDG gaps. The implementation will begin as a fixture/contract exercise and change production behavior
only if a measured case loses or overclaims evidence.

**Commands run:** `jj new -m 'Add advanced semantic uncertainty fixtures'`; targeted `rg`/`sed` inventory of
M4.1 control edge/point kinds, M4.5 access/boundary/effect kinds, and existing closure/await/yield/early-exit
coverage.

**Results:** the public vocabulary appears sufficient for the requested matrix, but cross-layer validation and
coverage behavior are not yet frozen. This is a start checkpoint, not a completion claim.

**Invalidated assumptions / negative-memory constraint:** advanced fixtures are not permission to translate
exceptional or suspension flow into Normal edges, infer capture/alias targets, identify globals from effects,
or connect early exits to later syntax.

**Current recommendation / next actions:** build one hand-labelled manifest and pure numerical CFG/dataflow
fixtures first; add cross-layer PDG/SDG integration only for cases where the lower-layer vector passes.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.8 integration owner, start checkpoint, 2026-07-14.

#### M4.8 terminal checkpoint — advanced uncertainty matrix verified

**Date/time:** 2026-07-14 23:00:18 CEST (Europe/Madrid).

**Objective/target:** close M4.8 with numerical fixtures for exception, async/yield, closure, mutation,
alias uncertainty, and early exits across the M4.1-M4.7 contracts.

**Changes:** added eight focused suites. The hand-labelled CFG fixture preserves two exceptional, two
suspension, and two abrupt edges as distinct families and remains fully entry/exit reachable under M4.3 dual
dominance. Production Rust await/yield with a nested closure produces two owners, exact Partial reasons, and
no fabricated suspension edge. Early return has no Normal or incoming fallthrough path to following syntax.
M4.5 locks Capture/Borrow as read-like, ReadWrite as read+write, all advanced output/effect wire variants, and
an ambiguous Capture with no symbol/reaching definitions. That capture reaches M4.6 only as an unresolved
access gap with no flow edge. The M4.7 integration retains mutation/exceptional/suspension outputs and
ReadsMemory/WritesMemory/Throws/Suspends/Captures effects while preserving exactly four explicit
interprocedural edges and no effect-derived additions.

**Commands run:** focused `cargo test -p deslop-parse --all-features m4_8_ -- --nocapture`; M4.7 regression
and aggregate M4 suites; focused parse clippy and diff check; `cargo test --workspace --all-features`; `cargo
build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`;
`cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git
diff --check`.

**Results:** PASS. M4.8 is 8/8, aggregate M4 is 57/57, parse is 192 active passing tests with one designated
ignored instrumentation probe, and all build/rustdoc/clippy/format/diff gates pass. Measured production await/
yield remains Partial rather than incorrectly gaining typed suspension edges; the explicit fixture proves the
schema and region engine can retain those edge families when authoritative input exists.

**Invalidated assumptions/failure modes:** a declared lowering rule is not evidence that the shared traversal
implemented it; closure ownership does not imply capture resolution; Capture/Borrow are not writes; effect
flags are not identified output/global dependencies; exceptional/suspension outputs do not automatically
authorize M4.7 return/parameter-out edges; later syntax after return is not reachable by source order.

**Current recommendation/next actions:** begin M4.9 by externalizing frozen graph/region expectations into a
strict gold schema, compare computed CFG/PST/PDG vectors numerically, and add compiler-backed facts only where
an available compiler artifact has explicit authority and dependency identity.

**Blockers/dependencies/restart:** none. No dependency, migration, cache clear, or runtime restart applies.

**Negative-memory status:** durable checkpoint pending Hindsight write. Search handles: `M4.8 declared rule
not lowered`, `await yield honest Partial`, `ambiguous Capture PDG gap`, `early return no fallthrough`, `effect
flags no SDG edge`.

**Signature:** Codex (GPT-5), M4.8 integration owner, terminal checkpoint, 2026-07-14.

### M4.9 start checkpoint — frozen graph gold and compiler evidence

**Date/time:** 2026-07-14 23:00:18 CEST (Europe/Madrid).

**Objective/target:** compare normalized CFG/PST/PDG outputs with external hand-labelled gold and with compiler
facts only where a compiler-authoritative graph artifact actually exists.

**Changes:** created a new jj change and wrote the convergent M4.9 plan. The comparison will exclude
content-addressed projection/node/edge IDs and retain typed endpoints/evidence under fixture-local ordinals.

**Commands run:** `jj new -m 'Freeze M4 graph gold comparisons'`; targeted audit of the existing M3 frozen
resolution-gold pattern and compiler semantic-provider tests.

**Results:** M3 has compiler-backed resolution facts, but the current M4 stack has no compiler CFG/PST/PDG
artifact/provider contract. Those resolution facts cannot be promoted into graph oracle authority. M4.9 will
therefore freeze hand gold and an explicit compiler-graph-unavailable matrix unless the source audit finds a
versioned graph artifact.

**Invalidated assumptions / negative-memory constraint:** compiler-backed name resolution is not compiler-
backed control/region/dependence evidence; content-addressed identities are not stable semantic oracle labels.

**Current recommendation / next actions:** implement strict external gold plus normalization and mutation
tests; audit production `CompilerTypeEvidence` and semantic provider capabilities before finalizing the
compiler-availability result.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.9 integration owner, start checkpoint, 2026-07-14.

#### M4.9 terminal checkpoint — exact gold agreement and honest compiler availability

**Date/time:** 2026-07-14 23:13:06 CEST (Europe/Madrid).

**Objective/target:** close M4.9 with exact normalized CFG/PST/PDG comparison against external hand gold and
compiler graph comparison only when explicit compiler-authoritative evidence exists.

**Changes:** added strict external `tests/fixtures/m4_graph_gold.json` under
`deslop.m4-graph-gold/1`; normalized M4 point, edge, predicate/source, dominance/post-dominance, structured
region, control-dependence witness, flow-dependence, and unresolved-gap evidence into fixture-local labels;
excluded source/policy-dependent content hashes; added strict schema/oracle/label closure validation, exact
comparison, three-layer semantic mutation tests, numerical vector assertions, and a six-adapter compiler-
evidence availability audit.

**Commands run:** repeated focused `cargo test -p deslop-parse --all-features m4_9_ -- --nocapture`; aggregate
M4 suite; focused parse clippy/diff; `cargo test --workspace --all-features`; `cargo build --workspace
--all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy
--workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Four M4.9 suites compare 50 exact semantic vectors: `[11 CFG points, 14 CFG edges, 11 PST
point facts, 2 regions, 0 residuals, 9 PDG control edges, 2 PDG flow edges, 1 unresolved gap]`. Wrong schema,
unknown fields, dangling labels, and CFG/PST/PDG mutations fail. Aggregate M4 is 61/61; parse is 196 active
passing tests plus one designated ignored instrumentation probe; all workspace gates pass.

**Invalidated assumptions/failure modes:** content-addressed IDs are not semantic gold labels; algorithm map
order by hashed keys is not oracle order and was replaced with fixture-local semantic sorting; compiler-backed
resolution is not compiler graph evidence; no production adapter currently provides a version/configuration/
dependency-bound compiler graph artifact, so compiler graph comparison is unavailable rather than inferred.

**Current recommendation/next actions:** implement M4.DoD as one frozen consumer-eligibility gate that joins
CFG, PST/non-structured, dataflow, PDG, and SDG coverage/capability/gap evidence and proves every missing or
uncertain fact blocks graph-dependent recipe eligibility.

**Blockers/dependencies/restart:** none. No dependency, migration, cache clear, or runtime restart applies.

**Negative-memory status:** durable checkpoint pending Hindsight write. Search handles: `M4.9 semantic labels
not hashes`, `compiler resolution not graph oracle`, `compiler graph unavailable`, `50 exact gold vectors`,
`semantic mutation comparator`.

**Signature:** Codex (GPT-5), M4.9 integration owner, terminal checkpoint, 2026-07-14.

### M4.DoD start checkpoint — graph-recipe eligibility propagation

**Date/time:** 2026-07-14 23:13:06 CEST (Europe/Madrid).

**Objective/target:** complete M4 by joining frozen CFG/PST/PDG gold with a public consumer eligibility gate
that propagates every missing or uncertain required semantic fact.

**Changes:** created the final M4 jj change and wrote the convergent DoD plan. The gate will operate on the
exact retained PDG source chain and optional SDG, not caller-supplied summary booleans.

**Commands run:** `jj new -m 'Gate recipes on complete M4 evidence'`; targeted API audit of PDG upstream
projection ownership, non-structured facts, dataflow capability evidence, and SDG capability/gap getters.

**Results:** all required evidence is retained and publicly traversable from `ProgramDependenceProjection`,
with `SystemDependenceProjection` binding its exact PDG. Existing Complete and ambiguous fixtures can be
exposed test-only without duplicating their builders.

**Invalidated assumptions / negative-memory constraint:** projection existence is not eligibility; Complete
non-structured coverage can still carry facts that block a structured recipe; support without authority is
not usable capability evidence; matching analysis IDs cannot replace exact projection-chain identity.

**Current recommendation / next actions:** add typed layer requirements and canonical blocks, expose the two
fixture constructors, and freeze positive/negative eligibility matrices before the final terminal run.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M4.DoD integration owner, start checkpoint, 2026-07-14.

#### M4.DoD terminal checkpoint — milestone complete and verified

**Date/time:** 2026-07-14 23:41:04 CEST (Europe/Madrid).

**Objective/target:** close M4 only after the 50-vector frozen CFG/PST/PDG oracle and every retained missing or
uncertain graph fact jointly determine downstream recipe eligibility without fallback inference.

**Changes:** added and exported strict `deslop.graph-recipe-eligibility/1` with dependency-closed local and
interprocedural requirements, typed evidence layers, canonical eligibility blocks, and content-addressed
`gre1_` decisions. The evaluator starts from one exact `ProgramDependenceProjection`, follows its retained
dataflow/control-region/CFG and non-structured sources, and accepts an optional SDG only when its exact PDG ID
matches. It propagates incomplete coverage with every canonical reason; unavailable capability support and
authority; conservative CFG edges; control-region residuals; every non-structured fact; dataflow access and
effect uncertainty; PDG gaps; SDG call uncertainty and gaps; missing required layers; and foreign-source
mismatches. Strict deserialization rejects malformed/stale IDs, wrong schemas, unknown fields, contradictory
eligibility, noncanonical order, unrequired-layer blocks, invalid reasons/precision/authority, and payload
mutation. Existing complete, missing-binding, and ambiguous-capture fixtures are reusable test-only builders;
the M4.9 gold comparator exposes a test-only exact vector count without weakening its oracle.

**Commands run:** repeated focused `cargo test -p deslop-parse --all-features m4_dod_ -- --nocapture`; focused
parse all-target clippy and `git diff --check`; aggregate `cargo test -p deslop-parse --all-features m4_ --
--nocapture`; `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace
--all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Seven DoD suites prove Complete local and interprocedural chains are eligible; an ambiguous
capture/nonterminating chain emits exactly nine blocks `[4 coverage, 1 non-structured, 1 access, 3 PDG gaps]`;
the comprehensive incomplete chain emits exactly 25 blocks across coverage, five capability gaps, one
conservative edge, one residual, three non-structured facts, access/effect/call uncertainty, four PDG gaps,
and one SDG gap. Missing SDG emits one block, missing parameter emits two, and missing outputs emit three;
foreign SDG source identity is rejected. The terminal test joins eligibility with all 50 frozen M4 gold
vectors. Aggregate M4 is 68/68. Full workspace tests pass; `deslop-parse` has 203 active passing tests, one
designated ignored instrumentation probe, and four passing compile-fail doctests. Build, warning-denied
rustdoc, all-target clippy, format, and diff gates pass.

**Invalidated assumptions/failure modes:** projection existence is not readiness; Complete coverage cannot
erase non-structured facts; a typed gap does not replace its coverage reason; support does not imply authority;
matching analysis/project identity cannot join a foreign SDG; a boolean without its exact blocks is not a
portable decision; syntactically valid but stale serialized blocks are rejected by the content-bound decision
ID.

**Current recommendation/next actions:** M4 is terminal. Begin M5.1 from the eligibility contract by versioning
`TransformationRecipe` and `TransformationCandidate`; do not reconstruct M4 readiness from statuses or bypass
the public gate.

**Blockers/dependencies/restart:** none. No dependency, migration, rebuild outside the verified workspace,
cache clear, or runtime restart remains.

**Negative-memory status:** durable terminal checkpoint pending Hindsight write. Search handles: `M4 complete
graph eligibility`, `gre1 content-bound decision`, `25-block incomplete matrix`, `exact PDG SDG source join`,
`50-vector M4 DoD`.

**Signature:** Codex (GPT-5), M4 integration owner, terminal checkpoint, 2026-07-14.

### M5.1-M5.4 start checkpoint — recipe contract and unreachable-code vertical slice

**Date/time:** 2026-07-15 CEST (Europe/Madrid).

**Objective/target:** implement M5.1-M5.4 completely, then one M5.25 graph-grounded recipe with honest
recipe-specific B2/B7 evaluation evidence.

**Changes:** created jj change `ussqqmsp` and wrote the convergent implementation plan. The contract audit found
no existing `TransformationRecipe`, `TransformationCandidate`, or `RecipeId`; current `deslop.workorder/3`
groups analyzer findings by rewrite region and belongs to the later M6 migration. The new semantic boundary
will therefore be a dedicated recipe layer consuming M4 projections and producing strict candidates that M6
can later schedule. Chosen first M5.25 family: removal of an exact CFG/PST-unreachable syntax statement with
explicit abstention for recovered, uncertain, referenced, effectful, or insufficient-authority cases.

**Commands run:** Hindsight/Serena startup context; `jj status`/`jj log`; targeted `rg` and source inspection of
the M5 ledger, workspace crates, `deslop-protocol`, `deslop-eval`, core safety types, and M4 eligibility surface;
`jj new -m 'Version transformation recipes and remove unreachable code'`.

**Results:** M4 provides the required content-bound graph evidence and fail-closed eligibility decision. Existing
evaluation reports only point precision/recall/F1 and lacks hard-negative FPR bounds, confidence intervals,
calibration, abstention, and frozen candidate-corpus identity. These must be added for the B7 slice rather than
inferred from the legacy analyzer corpus.

**Invalidated assumptions / negative-memory constraint:** the existing analyzer `Finding` or region-grouped
`WorkOrder` is not a transformation recipe/candidate; source order after an exit is not reachability evidence;
unreachable runtime code may still affect names/types/builds; one recipe family cannot complete global B2/B7.

**Current recommendation / next actions:** implement the strict recipe/candidate contract and fixture roles in
a dedicated recipe crate, then bind the unreachable-statement detector directly to retained M4 evidence.

**Blockers/dependencies/restart:** none.

**Signature:** Codex (GPT-5), M5 integration owner, start checkpoint, 2026-07-15.

### M5.1-M5.4 terminal and first M5.25 recipe-specific B2/B7 evidence

**Date/time:** 2026-07-15 CEST (Europe/Madrid).

**Objective/target:** complete M5.1-M5.4, then deliver one graph-grounded M5.25 recipe with actual-detector
recipe-specific B2/B7 evidence, without claiming the global multi-family/multi-language gates.

**Changes:** added workspace crate `deslop-recipes`. It exports strict content-bound
`deslop.transformation-recipe/1` and `deslop.transformation-candidate/1` contracts; required/forbidden
`Proven`/`Disproven`/`Unknown` results; exact condition evidence; M4 graph eligibility; safety/disposition;
bounded directional impact cones; typed expected graph changes; revision-guarded edits; validation and exact
rollback plans; and the canonical four fixture roles. Added the `rust-remove-unreachable-literal-statement`
recipe, which emits `SafeAuto` only for an exact Rust literal expression statement proven entry-unreachable by
complete CFG/control-region/non-structured evidence. It rejects recovered or conservative control and every
declaration, identifier, call, macro, operator, aggregate, and other potentially referential/effectful form.
The positive fixture executes guarded deletion, reparsing, expected old-entity removal, and byte-exact rollback.

Added strict recipe-evaluation corpus/report schemas, deterministic case expansion, content hashes, cluster-
aware confusion accounting, Wilson 95% intervals, ECE, coverage/action/abstention metrics, fail-closed
threshold evaluation, and mutation tests. The frozen Rust artifact contains 200 positive and 200 hard-negative
design clusters with five variants each: 1,000 labelled opportunities and 1,000 hard negatives. Five variants
within a cluster are explicitly not counted as independent confidence samples. Frozen assets are
`crates/deslop-recipes/corpus/unreachable_literal_rust_v1.json` and
`crates/deslop-recipes/evaluation/unreachable_literal_rust_v1_report.json`.

**Commands run:** repeated focused `cargo fmt --all`, `cargo check -p deslop-recipes`, `cargo test -p
deslop-recipes`, and warnings-denied focused clippy; explicit optimized evidence gate `cargo test --release -p
deslop-recipes unreachable::tests::frozen_b2_slice_runs_once_and_meets_recipe_specific_b7_gates -- --exact
--ignored --nocapture`; `cargo test --workspace --all-features`; `cargo build --workspace --all-features`;
`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`; `cargo clippy --workspace
--all-features --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check`.

**Results:** PASS. Focused crate suite has 11 active tests plus the explicit release evidence gate. The release
gate processed every frozen case in 7.821 seconds under the fixed 60-second budget and emitted exactly 1,000
candidates: raw TP=1000, FP=0, TN=1000, FN=0; cluster TP=200, FP=0, TN=200, FN=0. Precision and recall lower
95% bounds are `0.981154673623`; hard-negative FPR upper 95% is `0.018845326377`; ECE is `0`; opportunity
coverage and hard-negative abstention are both `1`. Report ID is
`b7r1_b0c5b717c0ce2ecb102d51f4a33aa43c84b50ce2ce7247420a37ea9d62f16365`. Full workspace tests, build,
warning-denied rustdoc, warnings-denied all-target clippy, format, and whitespace gates pass.

**Invalidated assumptions/failure modes:** one 2,000-function debug projection exceeded three minutes and was
interrupted; an optimized monolithic mixed-label projection then completed quickly but correctly emitted zero
candidates because intentionally incomplete hard-negative graphs contaminated the projection-wide eligibility
decision. Therefore monolithic cross-cluster batching is invalid evidence. The final evaluator runs all 400
five-case design clusters independently in one command and aggregates exact frozen case IDs. Broader
unreachable syntax cannot inherit literal-only safety: runtime unreachability does not erase compile/name/type
effects, and production def/use/effect authority remains insufficient for declarations, calls, macros,
operators, or composites.

**Current recommendation/checkpoint:** M5.1-M5.4 are terminal. The requested single M5.25 recipe and its
recipe-specific B2/B7 evidence are terminal. Keep M5.25, B2, and B7 globally unchecked; the next dependency-
ordered roadmap item is M5.5, while broader dead-code recipes wait for exact production def/use/effect proof.

**Blockers/dependencies/restart:** none for the delivered library recipe. No migration, runtime restart, cache
clear, or external service change is required. Wider recipe coverage depends on language-authoritative semantic
facts and is outside this completed slice.

**Negative-memory status:** durable and consolidated. The monolithic corpus approach is superseded by the
passing cluster-isolated run. Search handles: `M5.25 2000 case monolithic projection`,
`B2 B7 timeout`, `projection-wide eligibility contamination`, `cluster-isolated unreachable corpus`.

**Signature:** Codex (GPT-5), M5 integration owner, terminal checkpoint, 2026-07-15.

### Recipe production canary terminal checkpoint — closed disabled

**Date/time:** 2026-07-15 09:39:29 CEST (Europe/Madrid).

**Objective/target:** close `.agents/GOAL.md` only after the existing unreachable-literal recipe has a real
CLI delivery path (`detect -> preview -> strict shared work order -> guarded apply -> verify -> rollback`) and
the production enablement decision is backed by pinned real-repository, resource, mutation, and rollback
evidence.

**Changes:** added retained Rust project recipe construction and target-scoped graph eligibility; fixed
control-region residual closure so a rejected SESE candidate retains its proposed entry and exit; added
machine-readable detection reports with exact abstentions; added `deslop recipes detect` candidate,
work-order, report, and unified-diff formats with recipe/path filters. Added strict content-bound
`deslop.recipe-workorder/1`, distinct from legacy analyzer-region work orders, with exact candidate authority,
read/write/require/invalidate resources, patch budget, and validation/rollback contracts. Added staged and
live guarded apply with immediate exact source guards, parse and graph-delta checks, declared build/test
commands, protected-file checks, exact rollback, rollback identity reconstruction, and rerun rollback checks.
Disk and live/rebuilt states are separate. Automatic writes are disabled by default; only explicit `--canary`
authority can exercise controlled writes, without bypassing verification. Added CLI and library success,
staleness, duplicate, foreign-field, mutation, race, protected-resource, failure, and rollback tests.

**Real-repository evidence:** pinned supervisor-mcp revision
`f241a64f5c2bb9fadac2476e2395c7fdcd488e56` (MIT) analyzed all 7 Rust files with zero candidates and zero
abstentions in 4.14 seconds/61,816 KiB RSS. A separate controlled compiled opportunity on that pinned source
emitted one audited candidate/work order; staged and live `cargo check --locked` and `cargo test --locked`
passed, with zero protected-resource violations, in 93.04 seconds/606,608 KiB RSS. A live-only test failure
restored exact bytes and passed rollback parse, graph, build, and test checks in 50.98 seconds/601,660 KiB RSS.
Pinned deslop revision `cd153f7466e84341936e627bf4172157900051f2` (MIT), 83 Rust files/92,316 Rust LOC,
exceeded the frozen 75-second detection budget at 75.26 seconds/994,728 KiB RSS and emitted no terminal
report. Exact hashes and commands are in `.agents/RECIPE_CANARY_EVIDENCE.json` and the audit narrative is in
`.agents/RECIPE_CANARY_REPORT.md`.

**Commands run:** focused parse control-region and graph-eligibility tests; focused recipe, protocol,
verification, and CLI integration tests; optimized ignored 2,000-case recipe evidence gate; pinned natural
release scans under `/usr/bin/time -v`; controlled pinned supervisor detect/diff/work-order/apply and live-only
fault injection with declared Cargo build/test commands; final `cargo test --workspace --all-features`;
`cargo build --workspace --all-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features
--no-deps`; `cargo clippy --workspace --all-features --all-targets -- -D warnings`; `cargo fmt --all --
--check`; `git diff --check`.

**Results:** implementation and all terminal workspace gates PASS. Controlled apply is 1/1 and exact rollback
is 1/1, but its Wilson lower 95% recall bound is only `0.206549314377`. Natural positive opportunities are
zero; actionable real precision, recall, hard-negative FPR, and ECE therefore cannot be established. The
synthetic 1,000-positive/1,000-negative evidence remains separate and was not pooled. Production outcome is
terminal **DISABLED**, satisfying the goal's evidence-backed disabled branch.

**Invalidated assumptions/failure modes:** projection-wide eligibility contaminated unrelated targets and was
replaced with exact per-program-graph eligibility plus target-only candidate reconstruction; per-file full
graph construction removed contamination but took multiple minutes and was replaced by one combined discovery
projection with target-only rebuilds; invalid residual candidates could violate their own wire closure until
entry/exit normalization fixed the graph invariant; zero candidates on one small repository cannot establish
precision or recall; a timed-out repository is a resource abstention, not a clean zero-candidate result;
controlled positives cannot be counted as natural evidence.

**Current recommendation/checkpoint:** keep automatic recipe application disabled. Reopen only after a <=100k
Rust LOC retained-graph scan finishes in <=75 seconds and <=1 GiB RSS and a frozen real corpus has at least 30
natural positives plus 300 hard negatives meeting precision/recall lower 95% >=0.95, FPR upper 95% <=0.01,
ECE <=0.05, abstention <=1%, and all existing safety/rollback gates.

**Blockers/dependencies/restart:** no implementation blocker remains for read-only detection or controlled
canary transactions. Automatic enablement is intentionally blocked by measured runtime and insufficient
natural labels. No dependency installation, migration, cache clear, or live service restart is required; the
CLI must be rebuilt before disk changes are available through an older installed binary.

**Negative-memory status:** resource and statistical failures are retained locally and durably consolidated in
Hindsight bank `repo:/home/christos/code/deslop`. Search handles: `recipe production canary disabled`, `deslop 75 second timeout`,
`target-scoped graph eligibility`, `control-region residual closure`, `one controlled positive insufficient`.

**Signature:** Codex (GPT-5), recipe canary integration owner, terminal disabled checkpoint, 2026-07-15.
## 2026-07-15T10:53:01+02:00 — M5.5 equivalent branch factoring terminal checkpoint

**Objective:** proceed with the next dependency-ordered roadmap batch after the guarded recipe canary.

**Target:** M5.5 equivalent-arm/common-prefix/common-suffix detection and the M5.10 evidence boundary required
for this branch family. M5.6-M5.9 and global M5.10 remain outside this batch.

**Changes:** added `rust-factor-equivalent-branch-fragments` in `deslop-recipes`. The Rust detector associates
exact arm fragments with one unrecovered branch dispatch and two exact outgoing edges; proposes deterministic
condition-order-preserving equivalent-arm, prefix, or suffix replacements; abstains on comments, attributes,
let conditions/chains, non-block alternatives, recovered points, and conservative edges; and emits canonical
before entities, expected after graph changes, and forbidden-condition counter-evidence. Added an exact-node
replacement constructor, project-wide detector composition, CLI recipe selection, four-role fixtures, strict
wire/rebuild tests, and a CLI transaction test. Updated `.agents/TODO.md` and `.agents/PLAN.md`.

**Authority and safety result:** production Rust supplies authoritative CFG evidence but reports DefUse,
Effects, and LocalPdg as `Unknown`. Every M5.5 candidate is therefore `SafeWithPrecondition` and
`ReviewRequired`, with explicit Unknown dependency/effect/lifetime/drop evidence. The apply transaction rejects
the generated work order as non-automatic even when `--canary` is present. Automatic recipe application remains
disabled.

**Commands run and results:**

- `cargo test -p deslop-recipes branch::tests --all-features -- --nocapture` — 3 passed after correcting the
  positive fixture's canonical expectation from `ReviewRequired` to generic `Candidate`.
- `cargo test -p deslop-recipes --all-features` — 17 passed, 1 designated release-only evidence test ignored.
- `cargo test -p deslop-protocol --all-features recipe` — 3 passed.
- `cargo test -p deslop-cli --all-features --test recipes` — 3 passed.
- focused all-target clippy and `git diff --check` — passed.
- `cargo test --workspace --all-features` — passed with no failures; three pre-existing explicitly ignored
  performance/evidence probes remained ignored.
- `cargo build --workspace --all-features` — passed.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps` — passed.
- `cargo clippy --workspace --all-features --all-targets -- -D warnings` — passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- `cargo install --path crates/deslop-cli --all-features --force` — replaced
  `/home/christos/.cargo/bin/deslop` at 2026-07-15 10:52:45 +02:00.
- installed smoke on `crates/deslop-recipes/src/branch.rs` with the new recipe selector — one file analyzed,
  zero candidates, zero abstentions; `deslop-cli 0.1.0`.

**Failure mode / invalidated assumption:** a positive canonical fixture cannot declare `ReviewRequired`; the
four-role contract uses generic `Candidate` for the positive role and represents review authority on the emitted
candidate. More broadly, exact CST equality plus exact CFG does not prove dependency, borrow, lifetime,
destruction, exception, or suspension preservation. Empty synthetic effect vectors do not upgrade the production
adapter's explicit Unknown Effects declaration.

**Current recommendation/checkpoint:** M5.5 is terminal. M5.10 is terminal for M5.5 candidates only and remains
globally open. Proceed to M5.6 with a dedicated short-circuit/exception-order contract; do not reuse M5.5's
structural proof as semantic authority.

**Blockers/dependencies/restart:** no code blocker. The user-scoped CLI binary contains the new selector now.
Any already-running `deslop mcp` process still holds its old executable image and requires restart before this
replacement is live there. No migration or cache clear is required.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/branch.rs`, recipe contract/lib/project integration, CLI selector, and CLI recipe tests.

Signature: Codex `/root` — M5.5 integration and verification owner
## 2026-07-15T11:54:10+02:00 — M5.6 adjacent-condition merge terminal checkpoint

**Objective:** proceed from terminal M5.5 to the next dependency-ordered recipe batch.

**Target:** M5.6 safe adjacent-condition detection plus the M5.10 evidence boundary for this family. M5.7-M5.9
and global M5.10 remain outside this batch.

**Changes:** added `rust-merge-adjacent-conditions` in `deslop-recipes`. It recognizes exactly three Rust truth-
table forms: nested no-fallback `&&`, nested shared-fallback `&&`, and `else if` shared-success `||`. Detection
binds the outer and inner unrecovered dispatches, requires two exact outgoing edges from each, recursively rejects
let conditions/chains, preserves exact outcome block bytes, retains left-to-right conditional evaluation count,
and abstains on comments, recovery, conservative edges, and mismatched outcomes. The expected delta modifies the
outer dispatch and removes the nested dispatch representation. Added project discovery composition, CLI selector,
four-role and near-miss fixtures, strict wire/rebuild tests, and CLI apply-rejection coverage. Updated
`.agents/TODO.md` and `.agents/PLAN.md`; shared M5.10 evidence helpers remain in the branch recipe layer.

**Authority and safety result:** truth-table equivalence, body identity, and short-circuit evaluation count/order
are Proven from exact CST/CFG evidence. Production Rust Effects and LocalPdg remain `Unknown`, so panic,
exception, abrupt-exit, suspension, and hidden-effect obligations remain explicit Unknown. Every candidate is
`SafeWithPrecondition` and `ReviewRequired`; apply rejects it as non-automatic even with `--canary`. Automatic
recipe application remains disabled.

**Commands run and results:**

- focused `condition_merge::tests` — 4 passed after the two implementation corrections below.
- `cargo test -p deslop-recipes --all-features` — 21 passed, 1 designated release-only evidence test ignored.
- `cargo test -p deslop-protocol --all-features recipe` — 3 passed.
- `cargo test -p deslop-cli --all-features --test recipes` — 4 passed.
- focused recipe/CLI all-target clippy, fmt, and diff checks — passed.
- `cargo test --workspace --all-features` — passed with no failures; the same three explicitly ignored
  performance/evidence probes remained ignored.
- `cargo build --workspace --all-features` — passed.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps` — passed.
- `cargo clippy --workspace --all-features --all-targets -- -D warnings` — passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- `cargo install --path crates/deslop-cli --all-features --force` — replaced
  `/home/christos/.cargo/bin/deslop` at 2026-07-15 11:53:57 +02:00.
- installed selector smoke on `crates/deslop-recipes/src/condition_merge.rs` — one file analyzed, zero candidates,
  zero abstentions; `deslop-cli 0.1.0`.

**Failure modes / invalidated assumptions:** the first compile retained fallback text through a temporary
`NodeView`; owning the exact bytes before rendering fixed the lifetime without weakening identity. The first
detector matrix found only the `else if` case because Rust wraps a nested `if` used as a block statement in a
single `expression_statement`; the detector now unwraps exactly that one-child grammar form and does not broaden
to descendant guessing. Exact Rust `&&`/`||` evaluation semantics still do not upgrade the adapter's explicit
Unknown Effects declaration or authorize automatic application.

**Current recommendation/checkpoint:** M5.6 is terminal. M5.10 is terminal for M5.5-M5.6 candidates and remains
globally open. Proceed to M5.7 only from explicit dependence-slice independence; syntactic branch adjacency is
not dependence proof.

**Blockers/dependencies/restart:** no code blocker. The user-scoped CLI contains the M5.6 selector now. Any
already-running `deslop mcp` process requires restart to load the replacement. No migration or cache clear is
required.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/condition_merge.rs`, shared branch helpers, recipe lib/project integration, CLI
selector, and CLI recipe integration tests.

Signature: Codex `/root` — M5.6 integration and verification owner
## 2026-07-15T12:52:07+02:00 — M5.7 dependence-slice branch split terminal checkpoint

**Objective/target:** proceed from terminal M5.6 through M5.7 and its M5.10 evidence boundary; leave M5.8-M5.9
and global M5.10 open.

**Changes:** added `rust-split-independent-branch-actions`. The bounded Rust shape is a no-`else` branch with two
to eight direct call statements. Each action roots a bidirectional PDG Flow closure. Slice overlap or a crossing
Flow edge suppresses the proposal. The replacement stores the predicate once in a scoped boolean and retains
action order in per-action branches. Declarations, let conditions, comments, alternate arms, recovery, and
conservative control abstain. Added public slice evidence, expected removal/preservation delta, project/CLI
integration, four-role/rebuild/wire tests, and CLI apply rejection. Updated plan and TODO.

**Authority/safety:** exact CST actions, predicate evaluation count, source order, and CFG control are Proven.
Production DefUse, Effects, and LocalPdg are Unknown, so no crossing retained is explicitly Unknown rather than
independence proof; scope/borrow/lifetime/drop/effect obligations also remain Unknown. Candidates are
`SafeWithPrecondition`/`ReviewRequired`; apply rejects them even with `--canary`.

**Verification:** focused recipes 24 passed/1 designated ignored; protocol recipe tests 3 passed; CLI recipe
tests 5 passed; focused clippy/fmt/diff passed. Full `cargo test --workspace --all-features` passed with no
failures and the same three designated ignores. Workspace build, rustdoc with `-D warnings`, all-target clippy
with `-D warnings`, fmt check, and diff check passed. `cargo install --path crates/deslop-cli --all-features
--force` replaced `/home/christos/.cargo/bin/deslop` at 12:51:56 +02:00. Installed selector smoke analyzed one
file with zero candidates and zero abstentions.

**Failure modes/invalidations:** the initial multi-file patch was rejected atomically by stale formatted import
context and wrote nothing; module and integration patches were then separated. A test attempted to manufacture
opaque PDG node keys through a nonexistent public constructor and was removed rather than weakening key
identity. One clippy-only collapsible-if finding was corrected. Most importantly, an empty PDG edge set under
Unknown authority is not independence evidence.

**Recommendation/checkpoint:** M5.7 is terminal; M5.10 covers M5.5-M5.7 and remains globally open. Proceed to
M5.8 only from exact PST exit facts. No migration/cache clear is required. Existing `deslop mcp` processes need
restart to load the replaced binary.

Signature: Codex `/root` — M5.7 integration and verification owner

## 2026-07-15T13:13:42+02:00 — M5.8 PST/exit-backed guard-clause terminal checkpoint

**Objective/target:** proceed from terminal M5.7 through M5.8 and its M5.10 evidence slice; leave M5.9 and the
global M5.10 item open.

**Changes:** added `rust-invert-guard-clause`. The bounded Rust shape is a statement-position `if` with an
explicit block `else`, one direct `return` in exactly one arm, and one to eight semicolon-terminated continuation
statements. A returning `then` arm loses its `else`; a returning `else` arm is selected by `!(condition)` and the
continuation is flattened after the guard. Detection requires exact true/false dispatch, a single exact return
edge to the callable abrupt-exit dispatch, the exact abrupt virtual-exit outcome, an all-exact modeled path to
the branch merge, and retained PST reachability/exit-reachability/post-dominance facts. Added public exit/PST
evidence, expected graph delta, project/CLI integration, four-role/rebuild/wire tests, and CLI apply rejection.
Updated plan and TODO.

**Authority/safety:** complete PST coverage makes the continuation-boundary condition Proven; partial coverage
remains Unknown. Any conservative edge on a selected path suppresses the proposal. Exact CST shape, predicate
count/polarity, direct return, modeled merge reachability, and abrupt virtual-exit outcome are retained. Production
DefUse and Effects remain Unknown, so binding visibility, borrow/lifetime, temporary/drop order, effect, panic,
exception, and suspension obligations remain explicit Unknown. Candidates are `SafeWithPrecondition` and
`ReviewRequired`; automatic apply rejects them even under `--canary`.

**Commands/results:** focused `cargo test -p deslop-recipes --all-features` passed 28 tests with the designated
release corpus gate ignored; `cargo test -p deslop-protocol --all-features recipe` passed 3; focused CLI recipe
coverage passed. Strict focused clippy/fmt/diff passed. Terminal `cargo test --workspace --all-features` passed
without failures and retained the same three explicit slow/release ignores. Workspace build, rustdoc with
`-D warnings`, all-target clippy with `-D warnings`, fmt check, and diff check passed. `cargo install --path
crates/deslop-cli --all-features --force` replaced `/home/christos/.cargo/bin/deslop` at 13:13:30 +02:00.
Installed selector smoke analyzed one file with zero candidates and zero abstentions.

**Failure modes/invalidations:** requiring globally Complete PST coverage initially suppressed otherwise useful
graphs because Rust call unwind behavior intentionally makes CFG/PST coverage partial. The implementation now
retains partial PST as Unknown but never Proven. The same call-bearing fixture still abstains when the actual
selected continuation path contains a conservative edge. An early existential path walk could have ignored
conservative sibling edges; it was tightened to reject any conservative edge on every visited pre-merge point.
Direct abrupt and exit-dispatch points now require exactly one outgoing exact edge. A candidate-wire test first
mutated a Proven condition to Unknown, which remained a valid review candidate; the strict negative was corrected
to an illegal Automatic disposition.

**Recommendation/checkpoint:** M5.8 is terminal; M5.10 covers M5.5-M5.8 and remains open only for M5.9. Proceed
to M5.9 from exact reachability and exhaustiveness evidence. No migration or cache clear is required. Existing
`deslop mcp` processes need restart to load the replaced binary.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/guard_clause.rs`, recipe lib/project integration, CLI selector, and CLI recipe tests.

Signature: Codex `/root` — M5.8 integration and verification owner

## 2026-07-15T13:42:14+02:00 — M5.9 terminal branches and M5.10 evidence closure

**Objective/target:** proceed from terminal M5.8 through M5.9, close the shared M5.10 before/after/counter-
evidence boundary, and leave M5.11 as the next item.

**Changes:** added `rust-remove-literal-dead-arm` and `rust-convert-exhaustive-chain-to-match`. Literal dead-arm
detection accepts exact Rust `true`/`false` predicates with two explicit block arms and no comment, attribute, or
macro boundary; the selected block replaces the full expression. Exhaustive-chain detection accepts two through
six distinct literal/qualified-path `==` cases over one identifier plus an explicit block fallback; it emits one
match-table expression with a unique final `_` arm. Added public graph evidence, expected graph deltas, project/
CLI integration, four-role/rebuild/wire tests, and apply rejection for both selectors.

**Cross-layer completion:** the retained adapter schema already declared `ControlFlowAction::Match`, but the
shared traversal previously emitted one conservative leaf. Implemented exact unguarded match lowering with one
branch dispatch, typed case/default edges, merge joining, and abrupt-exit propagation. A unique final wildcard is
the only syntax-level exact exhaustiveness proof. Missing wildcard coverage receives a conservative default edge;
guarded arms remain conservative. Case labels escape control characters before strict wire validation. Generated
chain replacements now rebuild into the same graph contract they claim.

**Authority/safety:** literal predicate outcome, selected bytes, shared subject/cases, explicit fallback, generated
final-wildcard shape, exact current dispatches, and retained PST points are recorded. Complete PST is Proven and
partial PST stays Unknown. Production type, DefUse, and Effects remain Unknown, so compile-time effects in deleted
syntax, overloaded `PartialEq` versus pattern semantics, one-time subject move/borrow/drop behavior, and comparison
effects/panic/exception/suspension remain explicit Unknown. Both recipes are `SafeWithPrecondition` and
`ReviewRequired`; automatic apply rejects them even with `--canary`. No array/hash index-table rewrite is emitted:
without type, indexing, and fallback authority it would be weaker than the exact final-wildcard match table.

**Commands/results:** focused parse control-flow tests passed 17; focused recipes passed 33 with one designated
release corpus ignore; protocol recipe tests passed 3; all 7 CLI recipe tests passed; strict focused clippy/fmt/
diff passed. Terminal `cargo test --workspace --all-features` passed without failures and retained the same three
explicit slow/release ignores. Workspace build, rustdoc with `-D warnings`, all-target clippy with `-D warnings`,
fmt check, and diff check passed. `cargo install --path crates/deslop-cli --all-features --force` replaced
`/home/christos/.cargo/bin/deslop` at 13:41:43 +02:00. Installed smokes for both selectors each analyzed one file
with zero candidates and zero abstentions.

**Failure modes/invalidations:** proposing match before adding exact match CFG lowering was invalid because the
rewrite could not satisfy its own graph-delta validator; the existing retained Match action was implemented first.
The old match test correctly failed when the authority reason changed from “unimplemented” to explicit missing-
wildcard exhaustiveness and was updated with exact wildcard plus guarded/non-wildcard numerical cases. A sort-key
fallback used `u32::MAX` for a `usize` byte offset and was corrected. Focused clippy rejected an owned `PathBuf`
comparison in a test; it now compares borrowed paths. Raw case text can contain control characters, so labels are
escaped rather than allowing valid multiline patterns to violate strict evidence text.

**Recommendation/checkpoint:** M5.9 and M5.10 are terminal. Proceed to M5.11 extract-method candidates only from
SESE regions and complete computation/object-state slices. No migration or cache clear is required. Existing
`deslop mcp` processes need restart to load the replaced binary.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/control_flow.rs`, `crates/deslop-recipes/src/branch_terminal.rs`, recipe lib/project
integration, CLI selectors, and CLI recipe tests.

Signature: Codex `/root` — M5.9/M5.10 integration and verification owner

## 2026-07-15T14:03:30+02:00 — M5.11 focused extraction checkpoint

**Objective/target:** implement a real extract-method candidate from retained SESE and slice evidence without
pretending the still-open M5.12 full signature matrix already exists.

**Changes:** added `rust-extract-sese-branch-method` in the FunctionExpression family. The accepted Rust boundary
is an exact direct function-body `if` statement whose CFG entry belongs to a retained branch SESE region. The
transaction replaces the complete callable node with a uniquely byte-derived private sibling helper plus the
original callable containing an exact helper call. It accepts only free, non-generic synchronous functions with
simple primitive/reference parameters, no prior local declaration, at least two selected actions, and no abrupt,
try, suspension, macro, attribute, unsafe, closure, recovery, outer-attribute, or helper-collision boundary. Owned
parameters and nested branches abstain. Generated helper functions are not recursively re-extracted.

**Graph/slice evidence:** each candidate retains the exact ControlRegions entity; the PDG computation closure from
all region points under bidirectional Flow edges; object-state DataFlow boundaries/effects inside the region; every
touching retained flow edge; expected modify/preserve graph changes; and capability-tagged counter-authority.
Slice completeness becomes Proven only when DataFlow and LocalPdg coverage are Complete, typed PDG gaps are empty,
and DefUse, Effects, and LocalPdg are Provided with authority. Current production Rust projection remains Partial,
so its exact compiling proposals are SafeWithPrecondition review work orders and cannot enter automatic apply.

**Commands/results:** focused recipe tests passed 38 with the existing designated release corpus ignore; protocol
recipe tests passed 3; all 8 CLI recipe tests passed. The new five-test recipe set measured one positive candidate,
one strict edit, at least five preserved slice entities, stable isolated rebuild identity, seven near-miss counts of
zero, and a generated `rustc --crate-type lib --edition 2024` compile. Focused all-target all-feature clippy with
`-D warnings`, formatting, and diff checks passed. Full workspace terminal gates have not yet run.

**Failure modes/invalidations:** an evidence-only proposal is invalid because the transformation contract requires
an exact edit. A placeholder call or missing signature was therefore rejected in favor of a bounded compiling
transaction. The first candidate wire incorrectly attached ControlRegions and DataFlow entities to the
ProgramDependence-layer slice condition; strict validation rejected it, and slice-condition evidence now contains
only PDG nodes/flow edges while region and object-state evidence remain in their exact layers. Hindsight search mode
`semantic` is unsupported on the shared server; omit mode and accept graph traversal. Production absence of retained
flow edges is not treated as a complete slice.

**Recommendation/checkpoint:** run the full workspace terminal gates. If they pass, mark M5.11 terminal, replace the
installed CLI, and proceed to M5.12 exact inputs/outputs/mutations/exits/exceptions/captures/async/ownership.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/extract_method.rs`, recipe lib/project integration, CLI selector, and CLI recipe tests.

Signature: Codex `/root` — M5.11 integration and verification owner

## 2026-07-15T14:06:30+02:00 — M5.11 terminal extraction candidate

**Objective/target:** close M5.11 with terminal workspace evidence and install the resulting production selector.

**Terminal result:** `cargo test --workspace --all-features` passed without failures and retained the same three
explicit ignores: the CLI slow self-scan probe, the parse latency/memory probe, and the 2,000-case release recipe
corpus. Workspace build, rustdoc with `-D warnings`, all-feature/all-target clippy with `-D warnings`, formatting,
and diff checks passed. `cargo install --path crates/deslop-cli --all-features --force` replaced
`/home/christos/.cargo/bin/deslop` at 14:06:30 +02:00. The installed `rust-extract-sese-branch-method` smoke selected
and analyzed `crates/deslop-recipes/src/extract_method.rs` with zero candidates and zero abstentions.

**Current authority:** M5.11 is terminal for the bounded compiling Rust transaction. Complete retained
DefUse/Effects/LocalPdg can prove its computation/object-state slice; current production Rust capability gaps remain
explicit Unknown and keep emitted candidates review-only. M5.12 remains responsible for the full exact signature
surface and must not reinterpret M5.11's parameter-frontier signature as minimal used-input inference.

**Recommendation/checkpoint:** proceed to M5.12 exact extraction inputs, outputs, mutations, exits, exceptions,
captures, and async/ownership constraints. No migration or cache clear is required. Existing `deslop mcp` processes
must restart to load the replaced executable.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/extract_method.rs`, recipe lib/project integration, CLI selector, and CLI recipe tests.

Signature: Codex `/root` — M5.11 terminal owner

## 2026-07-15T14:36:45+02:00 — M5.12 focused exact-signature checkpoint

**Objective/target:** infer an explicit, compiling extraction signature for the bounded M5.11 Rust transaction,
retain every requested boundary dimension in candidate evidence, and validate the rewrite numerically before the
terminal workspace gate.

**Changes:** upgraded `rust-extract-sese-branch-method` to recipe version 2.0.0. The transaction now selects only
used primitive/reference parameters, accepts explicitly typed prior primitive/reference locals as inputs, records
copy/shared-borrow/mutable-reborrow ownership modes and direct syntactic mutation, and supports either a unit
statement extraction or one directly typed primitive `let` initializer returned by the helper. Added public input,
output, ownership, and full signature evidence. Candidate wires now contain separate exact-input, output, mutation,
exit, exception, capture, and async/ownership conditions. Current partial production DefUse/Effects authority keeps
mutation and exception completeness Unknown and the transaction review-only; it does not infer absence from syntax.

**Cross-layer completion:** added an additive `NestedValue` control-flow action and Rust `let_declaration` rule so a
typed initializer's nested branch receives exact CFG/SESE lowering. Rust let-else remains a conservative leaf with
an explicit reason. Internal bindings, receiver/generic/owned-value boundaries, named lifetimes, abrupt control,
try/macro/closure/unsafe/async/recovered syntax, and non-primitive or untyped outputs abstain.

**Commands/results:** language tests passed 14; focused parse control-flow tests passed 18; recipe tests passed 39
with the existing release-corpus ignore; protocol recipe tests passed 3; all 8 CLI recipe tests passed. The measured
before/after executable matrix passed four cases exactly: `false:-8:-36:-12`, `false:0:-12:-4`, `true:0:12:4`, and
`true:11:45:15`. Sixteen unsafe/unsupported near misses each produced zero candidates. Focused all-feature/all-target
clippy with `-D warnings`, formatting, and diff checks passed after replacing two `filter_map(bool::then)` chains
with explicit filter/map pipelines. Full workspace terminal gates have not yet run.

**Failure modes/invalidations:** the first typed-output fixture produced no candidate because Rust let declarations
were intentionally CFG leaves; treating the initializer as an exact nested value only when no let-else alternative
exists fixes the semantic layer rather than special-casing the recipe. Passing every callable parameter is not an
exact input frontier, so unused parameters are now omitted and typed prior locals cross explicitly. Syntax-only
absence of hidden mutation or exceptions is invalid authority; those dimensions remain Unknown until retained
DefUse/Effects coverage is complete. Broad-context test insertion briefly placed new CLI assertions in unrelated
recipe tests; the assertions were relocated to the extract-method test and all CLI tests now pass.

**Recommendation/checkpoint:** run all terminal workspace gates. If they pass, mark M5.12 terminal, replace the
installed CLI, persist the authority/failure lessons, and proceed to M5.13 dependence-cohesion callable splits.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-lang/src/control_flow.rs`, `crates/deslop-lang/src/lib.rs`,
`crates/deslop-parse/src/control_flow.rs`, `crates/deslop-recipes/src/extract_method.rs`,
`crates/deslop-recipes/src/lib.rs`, and `crates/deslop-cli/tests/recipes.rs`.

Signature: Codex `/root` — M5.12 integration and verification owner

## 2026-07-15T14:41:41+02:00 — M5.12 terminal exact-signature evidence

**Objective/target:** close M5.12 with terminal workspace evidence, replace the installed executable, and leave
the dependence-cohesion callable-split detector as the next bounded milestone.

**Terminal result:** `cargo test --workspace --all-features` passed without failures and retained the same three
explicit ignores: the CLI slow self-scan probe, parse latency/memory probe, and 2,000-case release recipe corpus.
Workspace build, rustdoc with `-D warnings`, all-feature/all-target clippy with `-D warnings`, formatting, and diff
checks passed. `cargo install --path crates/deslop-cli --all-features --force` replaced
`/home/christos/.cargo/bin/deslop`. The installed `rust-extract-sese-branch-method` smoke analyzed the production
extraction module successfully and wrote the exact empty work-order array `[]` (three bytes).

**Current authority:** the exact supported Rust frontier now covers used typed parameters and prior locals,
unit/typed primitive output, explicit ownership modes, direct mutation evidence, and retained exit/exception/
capture/suspension dimensions. Production mutation and exception completeness remain Unknown because retained
DefUse/Effects coverage is partial; the recipe remains `SafeWithPrecondition` and `ReviewRequired`. No restart is
needed for ordinary CLI calls. Existing `deslop mcp` processes must restart to load the replaced executable.

**Failure modes/invalidations:** the first installed-smoke shell wrapper used zsh's read-only `status` parameter and
exited after the detector completed; rerunning the identical detector with `rc` completed successfully. This was a
wrapper-only failure and did not invalidate detector output or modify repository state.

**Recommendation/checkpoint:** proceed to M5.13 multi-responsibility callable splits using retained dependence
cohesion and action clusters. Do not treat syntactic action adjacency or size alone as responsibility evidence.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`, language/parser control-flow
schema and lowering, extract-method recipe/public evidence, and CLI recipe integration tests.

Signature: Codex `/root` — M5.12 terminal owner

## 2026-07-15T15:00:18+02:00 — M5.13 focused dependence-cohesive split checkpoint

**Objective/target:** detect a multi-responsibility callable only from retained non-trivial action clusters, and
emit one exact atomic multi-helper transaction rather than overlapping per-branch suggestions.

**Changes:** added `rust-split-dependence-cohesive-callable`. The bounded Rust shape is one free callable with two
through four direct-body branch SESE regions, each satisfying the M5.12 exact signature frontier and containing at
least four action statements. A cluster action core must contain at least three PDG nodes joined as one connected
component by retained internal Control/Flow edges. Full computation frontiers must be disjoint; any overlap or
retained crossing Flow edge suppresses the proposal. One callable replacement prepends one byte-unique private
helper per cluster and replaces all selected sites in descending byte order. Inputs, outputs, ownership modes,
object-state evidence, cluster edges, full frontiers, and graph deltas are retained. Incomplete production DefUse/
LocalPdg keeps cross-cluster independence Unknown and the recipe review-only.

**Commands/results:** focused responsibility tests passed 2. The positive produced one edit, two helpers, two graph
modifications, Proven action-core membership/signatures, and Unknown frontier independence. Its compiled four-case
before/after matrix matched exactly: `false:false:-2:-5:8`, `false:true:0:-3:6`, `true:false:3:6:13`, and
`true:true:5:8:36`. Five non-cluster shapes each produced zero candidates. Strict candidate round-trip and illegal
automatic-promotion rejection passed; the rewritten source did not recursively re-split. The full recipe crate
passed 41 tests with one existing release-corpus ignore; all 9 CLI recipe tests passed; focused all-target clippy
with `-D warnings`, formatting, and diff checks passed. Full workspace terminal gates have not yet run.

**Failure modes/invalidations:** requiring every SESE region point to belong to one connected PDG component falsely
rejected valid clusters because the exact merge boundary is intentionally outside the control-dependent action
core. Cohesion now requires the non-trivial edge-induced action core to be connected while the full frontier remains
separately disjoint/crossing-checked. Flattening signatures from multiple clusters initially duplicated identical
input evidence and violated canonical candidate ordering; evidence is now sorted and deduplicated. A minimum-action
guard was briefly inserted in the single-branch M5.12 detector by broad patch context; it was moved to the M5.13
detector before verification. Direct call-only branches remain conservative CFG and do not fabricate clusters.

**Recommendation/checkpoint:** run terminal workspace gates. If they pass, mark M5.13 terminal, replace the
installed CLI, persist the action-core versus merge-boundary lesson, and proceed to M5.14 safe single-use helper
merge/inline detection.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/SESSION_REPORT.md`, extract-method/responsibility recipe and public
evidence, production project dispatch, CLI selector allowlist, and CLI recipe tests.

Signature: Codex `/root` — M5.13 integration and verification owner

## 2026-07-15T15:03:33+02:00 — M5.13 terminal dependence-cohesive callable split

**Objective/target:** close the bounded callable-split detector with terminal workspace evidence and install the
new production selector.

**Terminal result:** `cargo test --workspace --all-features` passed without failures and retained the same three
explicit ignores: the CLI slow self-scan probe, parse latency/memory probe, and 2,000-case release recipe corpus.
Workspace build, rustdoc with `-D warnings`, all-feature/all-target clippy with `-D warnings`, formatting, and diff
checks passed. `cargo install --path crates/deslop-cli --all-features --force` replaced
`/home/christos/.cargo/bin/deslop`. The installed `rust-split-dependence-cohesive-callable` positive smoke emitted
exactly one candidate containing one atomic edit and two private helpers.

**Current authority:** retained Control dependence proves each bounded branch action core is non-trivial and
connected. Exact CST and M5.12 signature inference prove the helpers compile for the accepted frontier. Full
computation frontiers are observed disjoint with no retained crossing Flow, but production DefUse/LocalPdg gaps
mean absence of an unretained crossing remains Unknown. The selector is `SafeWithPrecondition`, `ReviewRequired`,
and cannot enter automatic apply. Existing `deslop mcp` processes must restart to load the replaced executable.

**Recommendation/checkpoint:** proceed to M5.14 safe merge/inline of over-fragmented single-use helpers. Require
exact call binding, one use, compatible visibility/signature, and preserved evaluation/drop/exit/effect behavior;
do not infer a callee from spelling or inline merely because a helper is small.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`, extract-method/responsibility
recipe and public evidence, production project dispatch, CLI selector allowlist, and CLI recipe tests.

Signature: Codex `/root` — M5.13 terminal owner

## 2026-07-15T15:35:35+02:00 — M5.14 focused exact single-use inline checkpoint

**Objective/target:** implement the first safe merge/inline transaction for an over-fragmented local Rust helper
without deriving callee identity or use counts from spelling.

**Changes:** added `rust-inline-exact-single-use-helper` and a SystemDependence detector. The accepted frontier is
one private, synchronous, zero-parameter, implicit-unit helper in the same file as its sole caller. Its body is one
to four direct expression statements with no retained definitions, accesses, boundaries, non-return effects, or
forbidden CST. Complete Unique resolution must map the direct zero-argument call to the exact helper owner; complete
resolution, data-flow, CallGraph, and SDG enumeration must show exactly one call and no function-value reference.
The atomic transaction replaces the call statement with the exact helper block and deletes the helper, retaining a
nested block so temporary/drop scope is not flattened. Call-frame, panic-location, and backtrace observations remain
explicit review obligations, so candidates are `SafeWithPrecondition` and `ReviewRequired`.

**Production boundary:** the current Rust project builder does not retain Call accesses or authoritative resolution,
CallGraph, or SDG facts. It now constructs the honest partial SystemDependence projection and invokes the detector;
the detector returns zero candidates instead of guessing from a same-spelled identifier. The CLI selector is wired
and accepts the recipe name.

**Verification/results:** the complete authoritative fixture emitted exactly one two-edit candidate. Applying those
edits and compiling/running both programs produced identical `ok` output. A second exact call, a function-value
reference, and a public-helper boundary each emitted zero candidates. The production report and CLI work-order path
accepted the selector, emitted an empty array, and reported no abstention. Focused recipe, production-builder, and CLI
tests passed. The full all-feature workspace test command also completed without a reported failure; terminal gates
remain to be rerun compactly with explicit final status.

**Invalidated assumption:** a SystemDependence call-site entity cannot serve as the candidate target when the impact
cone is a local PDG cone. Candidate validation correctly rejected it as outside the cone. The target is now the
caller PDG call node; the interprocedural call site remains condition evidence and an expected removed entity.

**Recommendation/checkpoint:** run the terminal workspace gates, mark M5.14 complete only if all pass, replace the
installed CLI, smoke the installed selector's fail-closed production behavior, and persist the target/impact-layer
constraint before proceeding to M5.15.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/SESSION_REPORT.md`, `crates/deslop-recipes/src/inline_helper.rs`,
recipe exports/project dispatch, CLI selector allowlist/tests, and test-only authoritative adapter dependencies.

Signature: Codex `/root` — M5.14 integration and verification owner

## 2026-07-15T15:41:29+02:00 — M5.14 terminal exact single-use inline

**Objective/target:** close M5.14 with terminal workspace evidence, replace the installed executable, and retain the
honest production authority boundary.

**Terminal result:** `cargo test --workspace --all-features`, `cargo build --workspace --all-features`, rustdoc with
`-D warnings`, all-feature/all-target clippy with `-D warnings`, formatting, and diff checks passed. The workspace
tests retained the same three explicit ignores: CLI slow self-scan, parse latency/memory, and the 2,000-case release
recipe corpus. The initial clippy run found two `len() != 0` style violations in the new call-binding gate; both were
changed to `is_empty()`, focused clippy passed, and the complete six-gate sequence then exited 0.

`cargo install --path crates/deslop-cli --all-features --force` replaced
`/home/christos/.cargo/bin/deslop`. The installed `rust-inline-exact-single-use-helper` smoke analyzed the new recipe
source and returned `[]` with exit 0, confirming that production dispatch is available but does not fabricate call
authority. Existing `deslop mcp` processes must restart to load this executable.

**Current authority:** M5.14 is terminal for the exact private zero-parameter implicit-unit frontier. Complete Unique
resolution, complete reference enumeration, and complete SystemDependence prove callee identity and single use in
the authoritative consumer. The exact nested block edit preserves evaluation order and temporary/drop scope. Call-
frame removal, panic/caller location, and backtrace observability remain review obligations. Production's partial
resolution/CallGraph/SDG facts correctly yield no candidates; this is a live authority limitation, not a detector
failure and not permission to use spelling.

**Negative-memory status:** recorded the invalid SystemDependence-target/local-PDG-impact assumption in Hindsight
bank `repo:/home/christos/code/deslop`; the corrected invariant targets the caller PDG node and retains CallSite as
condition/delta evidence.

**Recommendation/checkpoint:** proceed to M5.15 def/use/effect-grounded temporary, expression, and independent-
statement recipes. Do not broaden M5.14 to parameters, outputs, or value substitution without exact evaluation-
multiplicity and ownership evidence.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`, the inline-helper recipe,
recipe exports/project dispatch, CLI selector allowlist/tests, Cargo test dependencies, and durable Hindsight memory.

Signature: Codex `/root` — M5.14 terminal owner

## 2026-07-15T16:04:15+02:00 — M5.15 focused local-cleanup checkpoint

**Objective/target:** implement the temporary, expression, and independent-statement closure of the local
function/expression recipe milestone using exact DefUse/Effects/LocalPdg evidence.

**Changes:** added three versioned Rust recipes and one shared detector. `rust-inline-exact-single-use-temporary`
requires one immutable untyped direct-body definition, one exact reaching Read, an immediately adjacent use, a
closed literal/operator initializer, and an empty initializer access/boundary/effect frontier; it atomically deletes
the declaration and substitutes the parenthesized initializer, remaining review-only for diagnostic/panic location.
`rust-remove-unused-pure-literal-expression` deletes a reachable direct-body literal statement only when its complete
point frontier has no definition, access, boundary, or effect. `rust-remove-independent-unused-literal-local`
deletes one immutable untyped literal local only when Complete data flow contains its exact definition and no symbol
access. The two deletion recipes are automatic. Calls, operator-expression deletion, typed/mutable/pattern locals,
comments, macros, recovered syntax, partial coverage, gaps, and uncertainty are excluded.

**Production boundary:** all selectors are wired through the project detector and CLI allowlist. Current production
graphs lack authoritative DefUse/Effects/LocalPdg and therefore return zero candidates rather than inferring a
definition, use count, purity, or independence from syntax.

**Verification/results:** one complete authoritative callable emitted exactly three non-overlapping candidates. The
fixture also retained a two-read local, typed local, and operator statement without candidates. Applying all three
transactions compiled and preserved measured output `19`. Candidate construction validates strict contracts and
dispositions. Focused recipe, production report, three-selector CLI, and all-target clippy checks passed.

**Invalidated assumptions:** data-flow event ordinals are shared across definitions and accesses at one control point,
not separate per fact category; fixture access ordinals now begin after same-point definitions. Recipe-condition
evidence must name an entity on the condition's declared graph layer; using a PDG target as ControlFlow/DataFlow
evidence was rejected, and the detector now retains exact control/data point entities separately from its PDG target.

**Recommendation/checkpoint:** persist the two contract lessons, run all terminal workspace gates, mark M5.15
complete only if they pass, reinstall the CLI, smoke all three installed selectors, and then proceed to M5.16
dependency projections.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/SESSION_REPORT.md`, local-cleanup recipe/detector/tests, shared test
adapter visibility, recipe exports/project dispatch, and CLI selector allowlist/tests.

Signature: Codex `/root` — M5.15 integration and verification owner

## 2026-07-15T16:08:47+02:00 — M5.15 terminal local-cleanup recipes

**Objective/target:** close the Functions and Expressions milestone with terminal evidence for temporary,
expression, and independent-statement cleanup.

**Terminal result:** `cargo test --workspace --all-features`, `cargo build --workspace --all-features`, rustdoc with
`-D warnings`, all-feature/all-target clippy with `-D warnings`, formatting, and diff checks exited 0. The complete
authoritative fixture emitted exactly three candidates; applying all transactions compiled and preserved output
`19`. The same fixture withheld candidates for a two-read local, typed local, and operator expression. Production
report and CLI tests accepted all selector names but emitted no candidate under partial semantic authority.

`cargo install --path crates/deslop-cli --all-features --force` replaced
`/home/christos/.cargo/bin/deslop`. Installed smokes for `rust-inline-exact-single-use-temporary`,
`rust-remove-unused-pure-literal-expression`, and `rust-remove-independent-unused-literal-local` each returned `[]`
with exit 0. Existing `deslop mcp` processes must restart to load the executable.

**Current authority:** M5.15 is terminal for the bounded direct-body Rust frontier. Complete exact reaching-
definition and effect facts authorize the edits; literal deletion never includes operators or calls, and temporary
inlining never crosses an intervening statement or explicit type/pattern boundary. Missing production facts yield
no candidate. Broader compiler-typed substitution, panic/source-location equivalence, destructor/drop movement,
effectful dead stores, and general statement commutativity remain unsupported rather than silently deferred.

**Negative-memory status:** Hindsight bank `repo:/home/christos/code/deslop` records the shared same-point event-
ordinal invariant and the requirement that condition evidence entities match their declared graph layer.

**Recommendation/checkpoint:** proceed to M5.16 file/module/package/build/API dependency projections before adding
architecture candidates. The dependency schema must preserve source/build/API provenance and partial authority.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`, local-cleanup recipes/tests,
recipe exports/project dispatch, CLI selectors/tests, installed executable, and durable Hindsight memory.

Signature: Codex `/root` — M5.15 terminal owner

## 2026-07-15T16:43:50+02:00 — M5.16 terminal project dependency projection

**Objective/target:** provide the strict retained dependency substrate required by the architecture milestones,
covering source files, exact build modules, packages, build targets, local/external APIs, containment, level-specific
dependencies, API use, evidence, and honest coverage gaps.

**Changes:** added `deslop.dependency/1` and `deslop.dependency-policy/1` in `deslop-parse`, with content-bound
policy/node/edge/gap identities and strict document deserialization. `derive_dependencies` consumes an exact
`Arc<ResolutionProjection>`; only `BuildModule` facts create Module/Package/BuildTarget identities and containment.
Complete Unique one-endpoint results create file/module/package/target dependency edges and file-to-API use evidence.
Definition endpoints canonicalize to their declaration, only complete authoritative Export facts label a local API
as exported, explicit external endpoints remain API-only, and same-level self-edges are excluded. Missing/duplicate
file ownership, incomplete exports/resolution, unsupported endpoints, and missing endpoint files are typed gaps;
aggregate coverage preserves Partial/Unsupported/Failed severity.

**Numerical verification/results:** the exact compiler-backed two-file, two-module, two-package, two-target fixture
emitted 9 nodes and 11 edges: two edges for each containment kind and one each for File, Module, Package, BuildTarget,
and API-use dependency kinds. Six focused tests cover exact hierarchy/use, deterministic byte-identical rebuild,
strict JSON round-trip and tamper rejection, incomplete export downgrade, duplicate ownership, local self-use, and
external API identity. `cargo test -p deslop-parse --all-features` passed 207 tests with one explicit ignore before
the final adversarial additions; the terminal all-feature workspace run passed with `deslop-parse` at 210 passed and
one explicit ignore. `cargo test --workspace --all-features`, `cargo build --workspace --all-features`, rustdoc with
`-D warnings`, all-feature/all-target clippy with `-D warnings`, `cargo fmt --all -- --check`, and `git diff --check`
all exited 0.

**Invalidated assumption:** a selective import segment and exact BuildModule facts for two different packages do not
authorize cross-package reachability. The retained resolver correctly returned Unknown/Partial with
`ImportUnresolved`, because BuildModule owns module identity but does not declare a package-dependency mapping. The
complete fixture now uses an exact compiler semantic endpoint; the projection then maps its files through exact
BuildModule ownership. Bare spelling remains insufficient.

**Current authority/recommendation:** M5.16 is terminal. Under Complete coverage, consumers may use the emitted
hierarchy and dependencies as retained architectural facts. Under any gap, absent edges are not negative dependency
claims. M5.17 should compute architecture metrics and constraints from this projection, preserve its evidence and
coverage, and avoid the legacy bare-name graph. A future explicit package-dependency mapping fact may replace the
compiler fixture prerequisite, but no path/import heuristic may do so.

**Blockers/restart/dependencies:** no blockers. No rebuild, service restart, MCP restart, migration, or CLI install is
needed beyond normal downstream recompilation; this milestone changes the parse-layer Rust API only. The M5.16
contract and the invalid cross-package inference are stored in Hindsight bank
`repo:/home/christos/code/deslop` under session scope `m5-dependencies-2026-07-15`.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/dependency.rs`, parse public exports, test-only complete-resolution pack visibility, and
durable Hindsight memory.

Signature: Codex `/root` — M5.16 integration and terminal verification owner

## 2026-07-15T17:11:20+02:00 — M5.17 terminal architecture topology and constraints

**Objective:** close M5.17 with an authoritative, deterministic topology and architecture-policy projection over the
M5.16 dependency contract.

**Target:** add strict `deslop.architecture/1` and `deslop.architecture-policy/1` parse-layer Rust APIs that compute
structural topology and exact metrics without upgrading partial dependency authority or treating API use as a
structural edge.

**Changes:** added iterative SCC discovery for File, Module, Package, and BuildTarget dependencies; a canonical
condensation DAG; dependency-first inferred layers; distinct same-level fan-in/fan-out and API users/uses; exact
rational instability; content-bound policy, rule, component, edge, violation, and gap identities; exact layer
assignments; direct/transitive forbidden-dependency, forbidden-cycle, required-layer-descent, and
stable-dependency rules; evidence-bearing violations; typed authority gaps; inherited source coverage; strict custom
wire validation; and public parse exports. The M5.16 TODO evidence was also reconciled before closing M5.17.

**Commands run/results:** focused architecture tests passed 8/8. Focused parse clippy passed after replacing an
over-wide rule-evaluation call with a context object. `cargo test -p deslop-parse --all-features` passed 218 tests
with 1 explicit ignore and 4 doctests. `cargo test --workspace --all-features`, `cargo build --workspace
--all-features`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`, `cargo clippy
--workspace --all-features --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` all
passed.

**Numerical evidence:** the retained complete fixture emits 9 node metrics, 8 SCC components, 4 condensation edges,
and two components per structural level at dependency-first layers 0 and 1. The application package has fan-in 0,
fan-out 1, and instability 1/1; the dependency package has fan-in 1, fan-out 0, and instability 0/1. The consumer
file has one API use and the local declaration has one API user without acquiring structural instability.

**Invalidated assumptions/authority lessons:** a cycle is a topology/planning fact, not a defect unless policy
forbids it. Stability comparison cannot be authoritative on partial topology because fan denominators rely on absent
edges, so the projection emits an explicit gap instead. API uses must not distort SCC or structural fan metrics, and
the legacy `deslop.graph/2` spelling graph is not architecture authority.

**Current recommendation/checkpoint:** M5.17 is terminal. Proceed to M5.18 and generate reviewed cycle-breaking seams
from this topology plus exact API/data-flow evidence; never choose a seam from topology alone.

**Blockers/restart/dependencies:** no blockers. No CLI replacement, service restart, migration, or cache clear is
required; normal downstream recompilation is sufficient. The result is stored in Hindsight bank
`repo:/home/christos/code/deslop` under session scope `m5-architecture-2026-07-15`.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/architecture.rs`, `crates/deslop-parse/src/dependency.rs`,
`crates/deslop-parse/src/lib.rs`, and durable Hindsight memory.

Signature: Codex `/root` — M5.17 integration and terminal verification owner

## 2026-07-15T20:44:12+02:00 — M5.18 terminal reviewed cycle-breaking seams

**Objective:** close M5.18 with deterministic cycle-breaking seam candidates grounded in exact topology, API, and
data-flow evidence, while withholding any topology-only or automatic rewrite authority.

**Target:** add a strict parse-layer `deslop.cycle-seams/1` projection bound to the exact M5.17 architecture,
M5.16 dependency/resolution, optional M4.5 data-flow, and cycle-seam policy identities.

**Changes:** added content-bound cycle-seam policy, candidate, and gap identities; exact cyclic-component/internal-edge
enumeration; resolution-to-local-API-use joins; exact-resolution data-flow source matching; retained access and
reaching-definition evidence; canonical authority/API/coupling costs and per-component ranks; fixed review
obligations; inherited source gaps; typed missing, foreign, incomplete, uncertain, and topology-only gaps; strict
canonical deserialization; public parse exports; and real bidirectional declaration/API and topology-only module
fixtures. Every emitted candidate is `ReviewRequired`; no mutation path was added.

**Commands run/results:** focused cycle-seam tests passed 8/8; focused all-target clippy passed. `cargo test -p
deslop-parse --all-features` passed 226 active tests with 1 explicit ignore plus 4 doctests. `cargo test --workspace
--all-features`, `cargo build --workspace --all-features`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace
--all-features --no-deps`, `cargo clippy --workspace --all-features --all-targets -- -D warnings`, `cargo fmt --all
-- --check`, and `git diff --check` all passed.

**Numerical evidence:** the exact bidirectional corpus has 4 cyclic SCCs and emits 8 candidates: 2 directed cuts at
each File, Module, Package, and BuildTarget level. Every candidate retains 1 API-use edge, 1 local API node, 1 exact
resolution, 1 data-flow access, 0 reaching definitions, complete evidence, and rank 1 or 2 within its SCC. The
topology-only corpus has 3 cyclic SCCs, emits 6 `TopologyWithoutApiEvidence` gaps, and produces 0 candidates. Removing
one exact data-flow access downgrades exactly 4 of 8 cross-level candidates; a foreign data-flow projection joins 0
accesses.

**Invalidated assumptions/authority lessons:** SCC membership and edge direction do not identify a legal cut. Matching
analysis or repository shape is insufficient for a data-flow join; exact resolution projection identity is required.
Absent API evidence requires abstention, while absent or partial data-flow evidence may retain an API-grounded review
candidate only as partial. Candidate rank is review ordering, never transformation legality.

**Current recommendation/checkpoint:** M5.18 is terminal. Proceed to M5.19 move/split/merge candidates using cycle
seams as one evidence source alongside cohesion, coupling, impact, and optional history; keep semantic legality and
review disposition independent from rank.

**Blockers/restart/dependencies:** no blockers. No CLI replacement, service restart, migration, or cache clear is
required; normal downstream recompilation is sufficient. Durable memory is in Hindsight bank
`repo:/home/christos/code/deslop` under session `m5-cycle-seams-2026-07-15`.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/cycle_seam.rs`, test-fixture extensions in `data_flow.rs` and `dependency.rs`, parse public
exports, and Hindsight memory.

**Negative-memory status:** no new failed conceptual path requires a separate negative entry; the topology-only and
foreign-source invalidations are enforced in code, tests, this report, and durable positive memory.

Signature: Codex `/root` — M5.18 integration and terminal verification owner

## 2026-07-15T21:21:19+02:00 — M5.19 terminal reviewed module restructuring candidates

**Objective:** close M5.19 with deterministic move, split, and merge-module candidates grounded in exact cohesion,
coupling, public API impact, cycle-seam evidence, and optional retained change history, without granting automatic
rewrite authority.

**Target:** add strict parse-layer `deslop.module-restructure/1`, `deslop.module-restructure-policy/1`, and optional
`deslop.module-change-history/1` contracts bound to the exact M5.16 dependency and M5.17/M5.18 topology/seam
identities.

**Changes:** added content-bound policy, profile, candidate, gap, history, and observation identities; exact
Module-to-File ownership; directed File and Module dependency evidence; public local API ownership and external API
users; exact rational cohesion; move enumeration that cannot empty its source module; weakly disconnected split
communities; canonical bidirectional merge enumeration; retained dependency/API/seam/history evidence; deterministic
review ordering; fixed review obligations; inherited and typed authority gaps; strict canonical deserialization and
tamper rejection; and public parse exports. Every candidate is `ReviewRequired`; no mutation or CLI path was added.

**Commands run/results:** focused module-restructure tests passed 9/9. Focused parse clippy passed after extracting a
`CandidateOrder` type alias. `cargo test -p deslop-parse --all-features --quiet` passed 235 active tests with 1
explicit ignore plus 4 doctests. `cargo test --workspace --all-features --quiet`, `cargo build --workspace
--all-features`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`, `cargo clippy
--workspace --all-features --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` all
passed.

**Numerical evidence:** the exact retained cycle has 2 module profiles and emits 1 merge candidate retaining 2
dependency edges, 2 public API nodes, 2 external API users, and 2 M5.18 seam candidates, with coupling reduction 2,
rank 1, authority penalty 0, and complete evidence when history is not provided. The independent pinned module model
emits all 3 operation kinds: move, split into 2 groups, and merge. Complete optional history adds 1 support
observation with exact Jaccard 3/6 and changes content identity; partial history adds exactly 1 authority penalty and
an explicit gap. One-way module coupling emits 0 merge candidates.

**Invalidated assumptions/authority lessons:** existing heuristic burden is not module cohesion authority. Structural
clustering, candidate rank, and co-change support cannot prove rewrite legality. History absence is genuinely
optional and non-blocking, but missing M5.18 seams cannot be treated the same way when a cyclic Module SCC exists;
the initial uniform optional-input predicate was caught by the adversarial test and replaced with explicit cyclic
seam coverage. Exact bidirectional coupling is required for a merge, and supplied partial evidence must downgrade
rather than disappear.

**Current recommendation/checkpoint:** M5.19 is terminal. Proceed to M5.20 semantically safe import/declaration
ordering recipes. Keep restructuring candidates as planning evidence until language-specific semantic checks and
review obligations are satisfied.

**Blockers/restart/dependencies:** no blockers. No CLI replacement, service restart, migration, or cache clear is
required; normal downstream recompilation is sufficient. Durable positive and negative memory is in Hindsight bank
`repo:/home/christos/code/deslop` under session `m5-module-restructure-2026-07-15`.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/module_restructure.rs`, `crates/deslop-parse/src/cycle_seam.rs`,
`crates/deslop-parse/src/lib.rs`, and Hindsight memory.

**Negative-memory status:** recorded the optional-evidence conflation: absent change history is non-blocking, while a
cyclic Module SCC without required seam evidence must be partial and emit `MissingCycleSeams`. The correction is
enforced in code and an adversarial regression test.

Signature: Codex `/root` — M5.19 integration and terminal verification owner

## 2026-07-15T21:52:50+02:00 — M5.20 terminal semantically safe ordering recipes

**Objective:** close M5.20 with concrete import/declaration ordering transactions whose semantic authority is
explicit, language-specific, deterministic, and fail-closed.

**Target:** add bounded Rust recipe selectors for one simple top-level import block and one private hoisted-function
block, integrate them through library/project/CLI detection, and withhold production candidates until exact
ScopeGraph and Resolution authority exists.

**Changes:** extended graph recipe eligibility with explicit `ScopeGraph` and `Resolution` layers, their prerequisite,
scope-fact capability/coverage checks, non-unique resolution blocks, canonical inherited coverage reasons, and strict
wire validation. Added `rust-sort-simple-import-block` and `rust-sort-hoisted-private-function-block`; exact guarded
block rendering preserves item bytes and original inter-item whitespace. Added scope/resolution impact evidence,
semantic-preservation deltas, parse/format/build/resolution-delta/test validation, reverse-edit rollback, public
exports, project detection, CLI selector validation, production fail-closed checks, and an authoritative synthetic
adapter fixture. Both recipes are `SafeWithPrecondition` and `ReviewRequired`; neither has an automatic path.

**Commands run/results:** focused ordering tests passed 8/8. `cargo test -p deslop-recipes --all-features --quiet`
passed 55 active tests with 1 explicit ignore. `cargo test -p deslop-parse --all-features --quiet` passed 235 active
tests with 1 explicit ignore plus 4 doctests. The focused installed-selector CLI test passed. `cargo test --workspace
--all-features --quiet`, `cargo build --workspace --all-features`, `RUSTDOCFLAGS='-D warnings' cargo doc --workspace
--all-features --no-deps`, `cargo clippy --workspace --all-features --all-targets -- -D warnings`, `cargo fmt --all
-- --check`, and `git diff --check` all passed. `cargo install --path crates/deslop-cli --all-features --force`
replaced the installed executable; both installed selector smokes exited 0 and returned `[]`.

**Numerical evidence:** the complete retained fixture emits exactly 2 candidates, each with 1 exact edit: one import
block and one private hoisted-function block. Their combined rewrite compiles and preserves measured stdout `2`.
Already ordered input emits 0. Partial scope authority emits 0. Side-effect, conditional, and glob import facts emit
0 import candidates; unhoisted, public, and macro-containing functions emit 0 function candidates. A retained
comment splits both runs and emits 0. Strict round-trip rebuilds both candidates; edit tampering rejects both.

**Invalidated assumptions/authority lessons:** lexicographic order is a requested output, never semantic proof.
Rust import/item order behavior is not language-generic. Parse success cannot replace exact import, declaration,
binding-timing, and resolution authority. Comments and attributes cannot be detached from owners, and macros/source
locations keep these candidates review-gated. A valid upstream reason vector is not necessarily canonical for a
downstream identity contract: Resolution coverage reasons are distinct but not ordered, so eligibility must sort and
deduplicate them rather than erroring or suppressing an incomplete-result block.

**Current recommendation/checkpoint:** M5.20 is terminal. Proceed to M5.21 exact subtree fingerprints and
renamed-token normalization. Keep ordering production-fail-closed until the Rust adapter provides the required exact
scope/import/resolution facts; add other languages only through their own order-semantics proof.

**Blockers/restart/dependencies:** no blockers. The installed CLI replacement is active; no further restart,
migration, or cache clear is required. Durable positive and negative memory is in Hindsight bank
`repo:/home/christos/code/deslop` under session `m5-ordering-recipes-2026-07-15`.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/graph_eligibility.rs`, `crates/deslop-recipes/src/ordering.rs`, recipe library/project/test
wiring, CLI selector/test wiring, test-only complete adapter capability, and Hindsight memory.

**Negative-memory status:** recorded the downstream coverage-reason canonicalization failure and correction. The
conditional/glob near-miss test now proves incomplete/non-unique resolution becomes an ineligible decision instead
of an invalid wire or a silently omitted block.

Signature: Codex `/root` — M5.20 integration and terminal verification owner

## 2026-07-16T08:37:38+02:00 — M5.21 terminal exact subtree fingerprints

**Objective:** close M5.21 with deterministic, content-addressed structural fingerprints and safe renamed-token
normalization over retained owned syntax, without starting clone indexing or detection.

**Target:** provide exact and alpha-normalized subtree digest types, preserve all non-identifier and public API tokens
by default, fail closed on incomplete syntax, and expose the typed evidence through transformation candidates for
M5.22.

**Changes:** added strict `deslop.subtree-fingerprint/1` and `deslop.subtree-fingerprint-policy/1` types and
`derive_subtree_fingerprint`. Exact and normalized postorder hashes bind stored grammar identity, raw grammar kind,
fields, flags, child order, and leaf bytes. Explicit symbol evidence may normalize only classified identifier leaves
to deterministic first-occurrence ordinals. Public API identifiers remain exact unless a named recipe policy opts in;
the content-bound policy ID enters the normalized digest. Complete parse provenance and an error/missing/recovery-free
subtree are mandatory. Added strict Serde validation, public parse exports, and an optional target-bound fingerprint
on `CandidateTarget`; all existing recipe constructors explicitly carry `None`, so M5.20 behavior is unchanged.

**Commands run/results:** focused fingerprint tests passed 6/6 and focused recipe contract tests passed 3/3.
`cargo test -p deslop-parse --lib` passed 241 active tests with 1 explicit ignored probe. `cargo fmt --all -- --check`,
`cargo build --workspace --all-features`, `cargo test --workspace --all-features`, and `cargo clippy --workspace
--all-features --all-targets -- -D warnings` all passed. The workspace test gate also passed 4 parse doctests and 55
active recipe tests with 1 explicit ignored evidence gate. `git diff --check` passed before terminal artifacts.

**Numerical evidence:** equal-structure blocks with renamed parameter/local uses have different exact digests and one
equal normalized digest, with exactly 3 owned identifier occurrences normalized. Structural, numeric-literal, and
operator mutations each produce unequal normalized digests. Public-surface renames remain unequal under the default
policy and become equal only under the same named recipe opt-in. Malformed recovered syntax returns
`IncompleteSyntax` and no digest. Strict round-trip succeeds; uppercase digest and stale policy-ID mutations reject.

**Invalidated assumptions/authority lessons:** a parameter declaration outside a selected block is not owned by that
subtree even when its references are; the focused fixture therefore measures 3 normalized occurrences, not 4.
Fingerprint equality is correlation/index evidence only and cannot authorize lookup, editing, or application.
Whitespace absent from the CST is outside structural equality; all owned syntax tokens remain exact except explicitly
evidenced identifiers.

**Current recommendation/checkpoint:** M5.21 is terminal. Proceed to M5.22 scalable candidate indexing and
graph-context clone verification using the typed exact/normalized digests. Do not implement maximal clone classes
until M5.23, and do not reuse or generalize M5.20 Rust ordering semantics.

**Blockers/restart/dependencies:** no blockers. No installed CLI replacement, restart, migration, cache clear, or push
is required; downstream consumers need only rebuild. M5.22 must supply graph-grounded identifier/surface evidence and
must retain exact revision guards for any later proposal.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/subtree_fingerprint.rs`, `crates/deslop-parse/src/lib.rs`,
`crates/deslop-recipes/src/contract.rs`, and existing recipe candidate constructors.

**Negative-memory status:** the existing durable constraint remains enforced: normalized fingerprints are matching
evidence only and never write authority. No new recurring failed implementation path required negative memory.

Signature: Codex `/root` — M5.21 integration and terminal verification owner

## 2026-07-16 — M5.23 terminal maximal clone classes

**Objective:** close M5.23 by collapsing M5.22 graph-verified pair matches into maximal clone classes with one
coordinated candidate per class, without starting M5.24 or granting rewrite authority.

**Target:** enumerate candidate pairs only inside normalized-fingerprint buckets, retain only graph-context-verified
edges, calculate deterministic maximal connected components, and omit singletons/rejected peers.

**Changes:** added `CloneClassId`, strict `deslop.clone-class/1` coordinated candidate evidence, and
`CloneCandidateIndex::maximal_clone_classes`. Bucket-local pair enumeration calls the existing M5.22 verifier, then
uses deterministic union-find to collapse accepted edges. Canonical sorted member IDs determine the class identity
and representative. Each class retains the shared fingerprint policy, normalized digest, exact/renamed match kind,
and measured bucket-local pair-check count. Public parse exports expose the new types. No mutation path, edit,
revision guard, whole-index pair scan, or M5.24 classification was introduced.

**Commands run/results:** `cargo test -p deslop-parse maximal_clone` passed 1/1. `cargo fmt --all`, `cargo build
--workspace --all-features`, `cargo test --workspace --all-features`, and `cargo clippy --workspace --all-features
--all-targets -- -D warnings` all passed. Workspace tests include 248 active parse tests with 1 explicit ignored probe,
4 parse doctests, and 55 active recipe tests with 1 explicit ignored evidence gate.

**Numerical evidence:** one normalized bucket containing four structurally matching entries performs only its six
bucket-local pair checks. Three entries with equal graph context form exactly one class of size 3; the fourth entry
with different topology is excluded. One unrelated singleton bucket emits no class. The class is categorized
`RenamedStructure` and has one deterministic representative and content-addressed `ccl1_` identity.

**Invalidated assumptions/authority lessons:** normalized-bucket membership is only an enumeration boundary; it
cannot make a rejected graph-context peer a class member. Connected accepted-pair evidence can coordinate a maximal
class, but class membership remains non-authoritative for rewriting.

**Current recommendation/checkpoint:** M5.23 is terminal. The next permitted milestone is M5.24 generated/schema/
test/public-API/intentional-repetition classification. Do not propose abstraction or edits from a class before that
classification evidence exists.

**Blockers/restart/dependencies:** no blockers and no runtime restart, migration, installed CLI replacement, or cache
clear is required. Advancing and pushing `main` is permitted only after the described M5.23 change has a clean empty
successor and the bookmark chain is verified.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/clone_candidate_index.rs`, and `crates/deslop-parse/src/lib.rs`.

**Negative-memory status:** existing M5.21/M5.22 matching-only authority constraints remain enforced; no new failed
approach required durable negative memory.

Signature: Codex `/root` — M5.23 integration and terminal verification owner

## 2026-07-16T10:13:48+02:00 — M5.24 terminal repetition classification

**Objective:** classify generated, schema-derived, test, public-API, and intentional repetition before any abstraction
proposal, while preserving the M5.21–M5.23 non-authoritative evidence boundary.

**Target:** require complete canonical per-member evidence for every maximal clone class, exclude protected
repetition from abstraction consideration, and expose ordinary production repetition only as review evidence.

**Changes:** added strict role evidence for ordinary production, generated output, schemas, tests, public API
surfaces, and intentional duplication; aggregate `CloneRepetitionKind`; explicit `AbstractionReadiness`; a
content-addressed `crc1_` classification; and `classify_clone_repetition`. Classification recomputes maximal classes
from the M5.22 index, demands exact evidence coverage of class members, retains exact category details, and emits one
deterministic record per class. Protected or mixed categories are classified non-candidates. Only all-ordinary,
fully evidenced classes become abstraction-review candidates. Missing, extra, duplicate, empty-detail,
ordinary-plus-protected, and noncanonical deserialized role evidence fails closed. Public exports were wired in
`deslop-parse`; no detector, edit, patch, application path, or M5.25 work was added.

**Commands run/results:** `cargo test -p deslop-parse clone_candidate_index::tests` passed 10/10. `cargo fmt --all`,
`cargo fmt --all -- --check`, `cargo build --workspace --all-features`, `cargo test --workspace --all-features`, and
`cargo clippy --workspace --all-features --all-targets -- -D warnings` all passed. Workspace tests include 251 active
parse tests with 1 explicit ignored probe, 4 parse doctests, and 55 active recipe tests with 1 explicit ignored
evidence gate.

**Numerical evidence:** all 5 protected roles independently classify one 2-member graph-verified renamed class as a
non-candidate. One fully ordinary 2-member class produces exactly 1 deterministic review-evidence record with no
protected kinds and a `crc1_` identity. One missing-member fixture, one contradictory ordinary-plus-test fixture, and
one blank intentional-reason fixture each reject.

**Invalidated assumptions/authority lessons:** constructor validation alone is insufficient when evidence can be
deserialized; classification now reconstructs and compares canonical evidence before use. Clone-class membership and
ordinary classification are proposal filters, not proof that an abstraction preserves behavior.

**Current recommendation/checkpoint:** M5.24 is terminal. M5.25 may later add graph-grounded detector families, but
must consume this classification and must not treat review eligibility as write authority. M5.25 was not started.

**Blockers/restart/dependencies:** no blockers. No runtime restart, migration, installed CLI replacement, or cache
clear is required. Advance and push `main` only after the described change has a clean empty successor.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-parse/src/clone_candidate_index.rs`, and `crates/deslop-parse/src/lib.rs`.

**Negative-memory status:** existing normalized-fingerprint/classification-is-not-write-authority constraints remain
enforced. The deserialization canonicalization lesson is fixed in code and covered by the fail-closed path.

Signature: Codex `/root` — M5.24 integration and terminal verification owner

## 2026-07-16 — M5.22 clone candidate index (integration)

- **Objective:** scalable fingerprint indexing + graph-context clone pair verification without whole-project pair scan.
- **Changes:** `crates/deslop-parse/src/clone_candidate_index.rs` (+exports); schema `deslop.clone-candidate-index/1`.
- **Evidence:** 6/6 focused tests; workspace `cargo test --workspace --all-features` green; clippy -D warnings on deslop-parse green.
- **Constraint:** fingerprints and verified pairs are matching evidence only; no rewrite authority; no M5.23 class collapse.
- **Signature:** opencode (xai/grok-4.5), M5.22 implemented after Codex research stall, 2026-07-16.

## 2026-07-16T10:35:58+02:00 — M5.25 terminal graph-grounded clarity candidates

**Objective/target:** complete the forwarding, conversion/allocation, wrapper, repeated-error, and dead-code
candidate families on retained graph evidence, without granting clone or clarity evidence independent rewrite
authority.

**Changes:** added strict `deslop.graph-grounded-clarity-candidate/1` evidence over actual guarded transformation
candidates, retaining their content identity and required ProgramDependence/DataFlow layers. Recipe-specific mapping
covers all five M5.25 families; repeated-error evidence additionally requires an exact error-shaped replacement.
Added `rust-inline-single-use-conversion-allocation`, which accepts only allowlisted Rust constructor calls with one
definition, one adjacent reaching read, complete allocation/call effects, comment/recovery-free syntax, and a use
position that cannot reorder argument evaluation. It emits an exact guarded review-only patch with expected delta,
validation, and rollback. The existing primitive single-use automatic cleanup is unchanged.

**Commands run/results:** `cargo check -p deslop-recipes`; focused clarity tests 2/2; focused conversion fixture 1/1
with successful before/after compilation and identical output; `cargo fmt --all -- --check`; `cargo build --workspace
--all-features`; `cargo test --workspace --all-features`; and `cargo clippy --workspace --all-features --all-targets
-- -D warnings` all passed. The workspace run contains 753 active tests, including 251 active parse tests with 1
explicit ignore, 4 parse doctests, and 57 active recipe tests with 1 explicit ignore.

**Failure modes/invalidated assumptions:** the first focused conversion run exposed that a `RiskySuggest` recipe may
not carry a stronger `SafeWithPrecondition` candidate label. The candidate now uses the recipe's review-only safety;
the rerun and terminal gates pass. Exact effect evidence remains deliberately narrow and fails closed.

**Current recommendation/checkpoint:** M5.25 is terminal. Begin only M5.26 role/scope-aware identifier and comment
evidence next. Do not broaden the Rust constructor allowlist, start M5.DoD early, or infer automatic write authority
from clarity classification.

**Blockers/restart/dependencies:** none. No runtime restart, migration, CLI replacement, or cache clear is required.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/clarity.rs`, `crates/deslop-recipes/src/lib.rs`, and
`crates/deslop-recipes/src/local_cleanup.rs`.

**Negative-memory status:** the transient safety-rank mismatch is fixed and covered by focused and full tests; no
repeating invalid path requires a durable negative-memory entry. Existing matching/classification-is-not-write-
authority constraints remain enforced.

Signature: Codex `/root` — M5.25 integration and terminal verification owner

## 2026-07-16 — M5.26 terminal role/scope and comment evidence

**Objective/target:** add role- and scope-aware identifier evidence plus owned comment intent without permitting
automatic rationale deletion.

**Changes:** added strict, content-addressed `deslop.role-scope-comment-evidence/1`. Identifier evidence retains exact
path/span/spelling, semantic and adapter canonical roles, scope fact, binding fact, and unique terminal resolution for
references. Comment evidence retains exact text, intent, span, and owner scope. Complete scope and comment coverage
is mandatory. Public API identifiers and documentation/rationale/suppression comments require review; an automatic
edit overlapping and dropping rationale text yields `automatic-rationale-deletion-blocked`. The projection never
changes candidate disposition or supplies write authority.

**Commands run/results:** `cargo fmt --all`; focused `cargo test -p deslop-recipes semantic_evidence` passed 4/4;
`cargo fmt --all -- --check`; workspace all-feature build and test; and workspace all-feature/all-target clippy with
`-D warnings` all passed. The terminal run has 757 active tests, including 251 active parse tests with 1 explicit
ignore, 4 parse doctests, and 61 active recipe tests with 1 explicit ignore.

**Numerical/failure evidence:** two equal `value` spellings in distinct exact scope facts remain two records; an
ambiguous reference rejects; duplicate spans reject; public API evidence downgrades automatic compatibility to
review; an exact overlapping automatic replacement that omits a rationale comment is blocked, while retaining the
exact comment is review-only. Partial coverage is not accepted.

**Current recommendation/checkpoint:** M5.26 is terminal. Run the M5.DoD enabled-detector audit next; do not start M6.

**Blockers/restart/dependencies:** none. No restart, migration, CLI replacement, or cache clear is required.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/semantic_evidence.rs`, and `crates/deslop-recipes/src/lib.rs`.

**Negative-memory status:** automatic rationale deletion is now an explicit terminal block. No unresolved failed
path requires a new durable negative-memory entry.

Signature: Codex `/root` — M5.26 integration and terminal verification owner

## 2026-07-16 — M5 definition-of-done terminal checkpoint

**Objective/target:** close M5 only after every enabled detector is joined to the full graph fact -> unique candidate
-> guarded patch -> expected delta -> required verification -> exact rollback chain, with an audited SafeAuto frontier.

**Changes:** added the canonical 16-entry `enabled_rust_recipe_catalog` and production `audit_m5_candidate`. Production
detection rejects duplicate candidate IDs, candidates outside the exact catalog, mismatched recipe identities,
missing guarded edits or graph deltas, missing required verification, incomplete reverse-exact rollback coverage, and
automatic candidates outside the three audited literal-only recipes. The catalog tests require unique recipe names
and IDs, dependency-bound graph layers and conditions, all four fixture roles, required validation, and guarded
rollback. No M6 implementation was started.

**Commands run/results:** focused DoD tests pass 2/2; production projection tests pass 6/6; CLI recipe integration
passes 12/12. The first full run passed all tests but clippy rejected one collapsible conditional. That conditional
was simplified and the complete fmt/build/test/clippy sequence was rerun from the start. The terminal rerun passes:
759 active workspace tests, 251 active parse tests with 1 explicit ignore, 4 parse doctests, 63 active recipe tests
with 1 explicit ignore, workspace all-feature build, fmt check, and all-feature/all-target clippy with `-D warnings`.

**Numerical evidence:** exactly 16 enabled recipes have 16 unique names and identities. Exactly 3 recipes permit
SafeAuto, and all 3 carry both minimal-counterexample and adversarial-near-miss `NoCandidate` fixtures. Every emitted
production candidate is sorted, duplicate-checked, and audited before return.

**Invalidated assumptions/failure modes:** test success alone did not close the terminal gate because clippy found a
style warning under `-D warnings`; the entire gate was rerun after the fix. SafeAuto is not inferred from graph
eligibility alone and cannot expand without an explicit catalog/test change.

**Current recommendation/checkpoint:** M5 is terminal. Advance and push `main` only after describing this slice and
verifying a clean empty successor. The next milestone is M6, but it was not started here.

**Blockers/restart/dependencies:** none. No restart, migration, CLI replacement, or cache clear is required.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`,
`crates/deslop-recipes/src/definition_of_done.rs`, `crates/deslop-recipes/src/project.rs`, and
`crates/deslop-recipes/src/lib.rs`, plus the M5.26 semantic evidence files already recorded above.

**Negative-memory status:** the clippy-only transient failure is fixed and the full gate reran cleanly. No recurring
failed approach requires durable negative memory; the exact SafeAuto frontier and non-authoritative evidence boundary
are enforced in code.

Signature: Codex `/root` — M5 terminal integration and verification owner

## 2026-07-16T11:44:45+02:00 — M6.1–M6.6 shared transaction/planner terminal checkpoint

**Objective/target:** establish one cross-surface transaction schema and the dependency, atomicity, scheduling, and
post-commit lifecycle needed before exposing LLM-facing operations.

**Changes:** added strict `deslop.work-order/1`, binding either one exact finding proposal or M5 transformation
candidate to canonical target, recipe, evidence/counter-evidence, impact, safety, patch budget, verification,
Reads/Writes/Requires/Invalidates, provenance, and unknowns. All summaries and `wo1_` identity are rederived on read.
CLI/MCP now emit it; slim and recipe apply consume it with explicit old-schema migration; LSP carries it in review-only
action data. Added `deslop.work-order-plan/1` with indexed resource edges, path-local overlap sweeps, explicit
prerequisites/alternatives, union-find atomic groups, Tarjan SCC blocking, dependency propagation, and deterministic
conflict-free topological waves. Added revision-bound handles and post-commit expiration/replanning that rejects reused
expired identities instead of shifting spans.

**Commands run/results:** focused protocol schema/planner/lifecycle tests, LSP shared-action test, MCP propose tests,
CLI revision/recipe/work-order integration, slim shared loader, verify recipe loader, and focused clippy all pass. Two
full gate attempts identified stale CLI tests that decoded the former top-level envelope; these were migrated to
unwrap the strict subject. The complete gate was restarted and passes: `cargo fmt --all -- --check`, workspace
all-feature build/test, and all-feature/all-target clippy with `-D warnings`. Counts: 767 active workspace tests, 251
active parse tests plus 1 ignored probe, 4 parse doctests, 63 active recipe tests plus 1 ignored evidence gate, and 29
active protocol tests.

**Failure modes/invalidated assumptions:** shared schema rollout requires fixtures to distinguish envelope identity
from the legacy subject identity; directly parsing CLI output as `/3` is no longer valid. Snapshot invalidation makes
otherwise disjoint graph commits conflict unless explicitly grouped atomically. Atomic groups may share one pinned
snapshot invalidation, but cannot contain overlapping writes or mutually exclusive recipes.

**Current recommendation/checkpoint:** M6.1–M6.6 are terminal. Implement M6.7–M6.9 bounded operations and adversarial
protocol tests next; do not run M6.10 until the operation surface is frozen.

**Blockers/restart/dependencies:** none. No runtime restart, migration command, installed CLI replacement, or cache
clear is required.

**Files/artifacts:** `.agents/PLAN.md`, `.agents/TODO.md`, `.agents/SESSION_REPORT.md`, protocol work-order/planner/
lifecycle modules, protocol exports, and CLI/MCP/LSP/slim/verify cross-surface integrations and fixtures.

**Negative-memory status:** former `/3` and recipe work orders are migration inputs only; stale spans and sort-order
cycle breaking are explicitly rejected. No unresolved failed algorithm requires another negative-memory record.

Signature: Codex `/root` — M6.1–M6.6 integration and terminal verification owner

## 2026-07-16T11:59:07+02:00 — M6.7–M6.9 bounded operation protocol terminal checkpoint

**Objective/target:** expose the complete bounded LLM operation sequence through identical cross-surface objects, with
deterministic budgets, stale/overlap protection, concurrency, and retry behavior.

**Changes:** added `deslop.work-order-service/1`, shared operation request/response enums, index metadata, ranked
paginated triage, bounded explain, exact plan response, scoped/budgeted patch proposal, required-check verification
receipts, policy authorization, and an idempotent gated executor. Added revision-bound cursors, schema negotiation,
hard item/evidence/byte ceilings, operation provenance, explicit unknown/truncation records, and content-addressed
service/cursor/patch/verification/authorization/apply-receipt identities. CLI `work-orders`, MCP `work_orders`, LSP,
and slim execute the same library request. Recipe target spans now retain the owned candidate target so coordinated
multi-edit recipes remain representable.

**Commands run/results:** focused service tests pass 3/3; shared CLI, MCP, LSP, and slim operation tests pass; focused
all-feature/all-target clippy passes. Full `cargo fmt --all -- --check`, workspace all-feature build/test, and
all-feature/all-target clippy with `-D warnings` pass. Counts: 772 active workspace tests, 251 active parse tests plus
1 ignored probe, 4 parse doctests, 63 active recipe tests plus 1 ignored evidence gate, and 32 protocol tests.

**Numerical/adversarial evidence:** a two-item triage uses two deterministic one-item pages; explain honors a one-
evidence ceiling and marks incomplete provenance. Eight simultaneous clients receive identical triage/apply receipts,
while the executor callback runs exactly once. Stale revision handles, overlapping edits, foreign/out-of-scope edits,
oversized budgets, missing schema families, and mismatched recipe-grounded patches reject.

**Current recommendation/checkpoint:** M6.7–M6.9 are terminal. Run the frozen paired M6.10 LLM benchmark next, then
audit M6.DoD. Do not start M7.

**Blockers/restart/dependencies:** none for the protocol. M6.10 still requires measured paired LLM evidence.

**Files/artifacts:** protocol service and exports, CLI/MCP/LSP/slim integrations and tests, plus M6 planning/TODO/
session artifacts.

**Negative-memory status:** client observations cannot grant M7 semantic authority; remote apply produces an
authorization, and only a server-owned gated executor can perform effects. No unresolved failed algorithm remains.

Signature: Codex `/root` — M6.7–M6.9 integration and terminal verification owner

## 2026-07-16T12:31:03+02:00 — M6.10/M6 definition-of-done terminal checkpoint

**Objective/target:** close M6 only with a frozen identical-budget paired LLM benchmark that improves accepted-patch
rate with confidence, respects scope/abstention/regression bounds, and joins that evidence to the existing strict
transaction, planner, and stale-order contracts.

**Changes:** added the deterministic `m6-llm-benchmark` generator/scorer/verifier and a 240-task holdout balanced over
six languages, five opportunity families, four variants, and safe/unsafe labels. The two arms use the same model,
reasoning, no-tool, answer-schema, context-ceiling, and output-token budgets. Added an executable DoD test that
recomputes the stored report from its frozen observations and rejects manifest, observation, counter, slice,
confidence, or gate tampering. Retained the exact manifest, 480-request Batch input, raw 480-response output, report,
Batch records, SHA-256 hashes, split/licence, prompt/model/tool/seed/cache setup, and reference-machine metadata.

**Commands run/results:** focused benchmark tests pass 3/3, including the checked evidence DoD test; strict focused
clippy passes. OpenAI Batch `batch_6a58ad6954b48190a949902ce2c9d1ca` completed 480/480 requests with zero failures.
The terminal rerun passes `cargo fmt --all -- --check`, `cargo build --workspace --all-features`,
`cargo test --workspace --all-features`, and `cargo clippy --workspace --all-features --all-targets -- -D warnings`.
There are 675 active unit/integration tests, 4 active doctests, and 3 explicit ignored probes.

**Numerical evidence:** baseline accepted 67/120 safe patches (55.83%), correctly abstained on 118/120 unsafe tasks,
made 3/240 out-of-scope edits, and made 2 semantic regressions. Graph grounding accepted 120/120 (100%), correctly
abstained on 120/120, made 0 out-of-scope edits, and made 0 semantic regressions. Accepted-patch delta is +44.17
percentage points with paired 95% CI [+35.24, +53.09]. All languages improve by 40–50 points and no family regresses.

**Invalidated assumptions/failure modes:** jj initially refused the 1,062,931-byte raw output under its 1 MiB
new-file ceiling; repository-local `snapshot.max-new-file-size` was raised to that exact measured size so raw evidence
is versioned. Exact `f64` equality was not stable after JSON round-trip for two derived baseline/slice values; the
verifier now uses 1e-12 tolerance only for derived floating values while every identity, observation, integer count,
gate, schema field, and pass decision remains exact. Neither issue changed the frozen prompt, sample, thresholds, or
Batch result.

**Current recommendation/checkpoint:** M6.1–M6.10 and M6.DoD are terminal, and B10 is measured closed. Describe this
slice, create a clean empty successor, advance `main` through the three M6 changes, and push only if jj status is clean.
Do not begin M7 in this slice.

**Blockers/restart/dependencies:** none. The official OpenAI developer-docs MCP was installed during model/API
verification but becomes available only in a new Codex session; it is not required by the repository runtime.

**Files/artifacts:** `crates/deslop-eval` benchmark module, binary, DoD test, Cargo wiring, `.agents/benchmarks/` raw
and derived evidence, `.agents/TODO.md`, `.agents/PLAN.md`, and this report.

**Negative-memory status:** exact post-JSON float equality is invalidated for derived benchmark rates; use tight
tolerance for derived floats and retain exact checks for all discrete evidence. No unresolved failed approach remains.

Signature: Codex `/root` — M6 terminal integration, benchmark, and verification owner

## 2026-07-16 — M7 hardened verification authority terminal checkpoint

**Objective/target:** complete M7.1–M7.8 and M7.DoD as one revision-pinned verifier authority that widens under
incomplete evidence, preserves provider conflict, requires pre-change characterization, enforces explicit execution
policy, compares graph deltas around formatting, atomically commits/undoes, injects failures, and demotes recipes on
counterexamples.

**Changes:** added strict verifier-plan, authority observation/decision, execution policy/failure, verification
evidence, pre-change characterization, transaction report, undo-manifest, and recipe-demotion contracts. Added impact-
indexed dependency closure with project fallback; adapter/compiler/LSP capability conversion and conflict join;
typed parse/format/build/lint/type/test/coverage/characterization/differential/mutation/delta evidence; policy-bound
namespace command runtime; exact revision/budget/format-scope/delta checks; fsynced source journal, recovery and
explicit undo; and immediate durable demotion/supersession. Ordinary patch and recipe-canary writes now use the same
atomic source primitive. Added ADR 0009, capability matrix, migration notes, and an executable M7 DoD.

**Commands run/results:** focused authority/evidence/atomic/demotion/runtime/transaction/legacy-recipe suites and
strict verifier clippy pass. Failure injection covers command failure, timeout, command crash, every multi-file
partial-rename boundary, hard process interruption/recovery, formatting failure, staged/formatted/live graph-delta
mismatch, and failed differential evidence. The terminal rerun passes `cargo fmt --all -- --check`,
`cargo build --workspace --all-features`, `cargo test --workspace --all-features`, and
`cargo clippy --workspace --all-features --all-targets -- -D warnings`: 703 active unit/integration tests, 4 active
doctests, and 3 explicit ignored probes.

**Numerical/behavioral evidence:** verifier tests pass 81/81. The M7 DoD compiles and executes a Rust SafeAuto behavior
assertion on the staged transformed source and the committed live source, then uses the durable manifest to restore
byte-identical original source. All five weaker safety classes are asserted non-Automatic with nonempty uncertainty.
Two-file atomic tests restore both exact originals after injected rename 1/2 and 2/2; hard-crash recovery restores the
partially installed transaction from its fsynced Committing journal.

**Invalidated assumptions/failure modes:** executable discovery is not sandbox authority. `/usr/bin/bwrap` and
`/usr/bin/unshare` exist, but this container rejects network namespace creation with `Operation not permitted`.
Production policy commands therefore fail closed with a structured policy failure here; no inherited-shell fallback
was added. A graph-delta value copied from a candidate would be circular evidence, so the transaction requires a
server-owned `GraphDeltaOracle` and compares its patched/formatted/live observations exactly.

**Current recommendation/checkpoint:** M7.1–M7.8 and M7.DoD are terminal. Describe this slice, create a clean empty
successor, advance/push `main` only while green and clean, then begin M8 in a later slice.

**Blockers/restart/dependencies:** repository implementation has no blocker. Deployments that execute the default
policy command runtime must enable Linux namespaces or provide an equivalently enforcing server-owned runtime. No
schema migration command or cache clear is required; legacy receipts cannot be promoted and must be regenerated.

**Files/artifacts:** `crates/deslop-verify` authority/evidence/runtime/transaction/atomic/demotion modules and DoD,
legacy atomic-write integrations, Cargo wiring, ADR 0009, M7 capability/migration documents, TODO/PLAN, and this report.

**Negative-memory status:** recorded locally and queued for Hindsight: namespace executable presence does not prove
namespace permission; never silently downgrade the policy runtime. No unresolved algorithmic failure remains.

Signature: Codex `/root` — M7 integration, transaction, failure-injection, and terminal verification owner

## 2026-07-16 — M8 readability and ranking calibration terminal checkpoint

**Date/time:** 2026-07-16T16:30:31+02:00

**Objective/target:** complete M8.1–M8.10 and M8.DoD with a versioned transparent feature
contract, licence-authorized data, one-capture convergent evaluation, frozen baseline/calibration
gates, and an enforced portable/language-role/evidence-only product decision.

**Changes:** added deslop.readability-features/1 and deslop.metrics/6 with exclusive
content-addressed per-region axes, aggregation policy, exact CFG McCabe complexity where complete,
per-entropy estimator/sample metadata, explicit unknowns, and embedded calibration disposition.
Added the strict dataset/corpus/capture/policy/report/model-card contracts and the m8-calibration
assembler/evaluator. Added deterministic side blinding, size control, four rankers, eight
leave-one-axis-out ablations at both scales, project/language strata, Wilson intervals, Brier/ECE,
and fail-closed label selection. Added ADR 0010, capability/data/migration/model-card/pilot docs,
frozen registry/corpus/evaluation artifacts, and an executable M8 DoD.

**Published data and limits:** Themis revision
7c366b23590cc9ff8d372bb47280fcd474536344 (Apache-2.0) contributes 300 short
readability-and-maintainability preference pairs: JavaScript/Python/Ruby 65 each, Java 51, C# 24,
Go 20, C/C++ 5 each. It uses pre-2019 merged commits plus classifier/model-consensus selection and
does not expose project identity or direct human perceived-readability labels. AoC-FRP Zenodo
14229849 (CC-BY-4.0) contributes 1,727 Java callable comprehension trials by 24 participants:
ambiguous n=863, 12,204.055 ms, correctness 0.702202; unambiguous n=864, 11,238.246 ms,
correctness 0.884259. The Dorn mirror is recorded rejected because no explicit redistribution
licence was observed.

**Numerical evidence:** the immutable capture
rcp1_fba95f5be345e111f632d94a30ccde649a1e65a2d7f9da200f9207212d66675e contains 760
unique candidates. Corpus: 300 blinded pairs, 8 languages, 4 roles, 226 size-controlled pairs, 240
cleanup tasks (160 published human-commit, 40 controlled LLM-assisted), and 40 unsafe near-misses;
zero authorship keys. Overall challenger accuracy is 0.5700 (171/300; Wilson 95%
0.5134–0.6248), Brier 0.249217, ECE 0.076400. Baseline accuracies: size 0.3933,
NLOC/complexity 0.3933, lexical 0.5333. Size-controlled challenger accuracy is 0.5664
(128/226; Wilson 95% 0.5012–0.6293), ECE 0.077890. All eight overall/size-controlled ablations
and all eight language strata are retained in the JSON report.

**Decision:** evidence-only. The challenger wins the aggregate accuracy comparison but fails the
0.60 lower-bound floor, 0.05 ECE floor, three-project floor, direct-human-target requirement,
complete-axis requirement, and multiple language/baseline checks. No language/role models were
fitted or evaluated. deslop.metrics/6 therefore publishes
readability_label_permitted=false, no model id, and all eight transparent axes. M8 evidence has no
rewrite/safety authority.

**Commands run/results:** generated the corpus with m8-calibration --assemble and regenerated the
frozen report; focused deslop-metrics tests pass 30/30; deslop-eval unit/integration/DoD tests pass;
MCP metrics and CLI algorithm-contract smokes pass. Terminal command passed:
cargo fmt --all -- --check; cargo build --workspace --all-features;
cargo test --workspace --all-features; and
cargo clippy --workspace --all-features --all-targets -- -D warnings.
The inventory is 711 active unit/integration tests, 4 active doctests, and 3 explicit ignored
probes.

**Invalidated assumptions/failure modes:** a public dataset without an explicit compatible licence
cannot be copied; downloadability is not authority. Themis does not expose project identity, so a
true multi-project holdout cannot be fabricated and the project gate must fail. The first import
orientation put the preferred variant on a fixed side, which made tie handling position-dependent;
the final corpus uses BLAKE3 row-id side swapping, and exact ranker ties never count as correct.
Aggregate improvement over simple baselines is insufficient when calibration, confidence,
target-authority, axes, or held-out strata fail.

**Current recommendation/checkpoint:** M8.1–M8.10 and M8.DoD are terminal with evidence-only UX.
Describe this slice, create a clean empty successor, advance and push main only while green and
clean, then start M9 in a later slice.

**Blockers/restart/dependencies:** no repository blocker and no model artifact to deploy. Consumers
must migrate from deslop.metrics/5 to /6 to read the new axes/disposition. No cache clear or
runtime restart is required.

**Files/artifacts:** deslop-metrics feature/CFG/calibration contract; deslop-eval M8 module, binary,
registry, corpus, evaluation, and DoD; CLI/MCP schema wiring; SPEC; ADR 0010; M8 capability,
dataset, migration, model-card, and pilot documents; TODO/PLAN; this report.

**Negative-memory status:** queued for Hindsight: fixed-side pair orientation and correctness for
ties are invalid evaluation designs; missing project identity cannot be replaced by a synthetic
split; public availability is not redistribution authority; aggregate wins cannot override frozen
calibration/holdout gates.

Signature: Codex /root — M8 feature, dataset, evaluation, product-decision, and terminal verification owner
