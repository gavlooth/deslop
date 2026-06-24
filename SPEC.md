# `deslop` — Specification (Rust implementation)

**Status:** Draft v0.4 · 2026-06-23
**Author:** spec drafted with Claude (Opus 4.8); adversarially reviewed with Codex (gpt-5.5)
**One line:** A deterministic code-bloat **analyzer** whose product is rich, **agent-ready
output** — it *proposes* behavior-preserving cleanups and *verifies/applies* the patches
that come back. Any LLM is a **swappable, external consumer**, never a dependency.

> **v0.4 change — the analyzer + its output is the product; the LLM is ad-hoc.** v0.3 still
> treated a bundled LLM (`slim`) as a pipeline pillar. It isn't. The moat is the
> deterministic analyzer and an **LLM-friendly output format** that lets *any* agent
> (Claude Code, Cursor, Codex, a CI bot — or the optional built-in consumer) do the rewrite.
> What deslop must own on **both ends** is the contract: **`propose`** (work orders) and
> **`verify`/`apply`** (the deterministic safety gate). The LLM is the swappable middle. The
> tool is fully useful with the LLM feature compiled out. This mirrors `fallow`'s
> "No AI inside the analyzer, but agent-ready" stance.
>
> Retained from v0.3: the **fix-safety lattice** (§3) and the counterexamples that justify
> it. Deviation from the original "auto-fix in place": only `safe-auto` writes in place;
> everything else is propose → verify → apply.

---

## 1. Thesis & positioning

| Tool | Lang | Analyzer | Agent-ready output | Bundled LLM | Verify/apply loop | CLJ/Julia |
|---|---|---|---|---|---|---|
| `fallow` | Rust | strong | yes (JSON/MCP/LSP) | No (by design) | partial (`fix --dry-run`) | No |
| `antislop` | Rust | medium | JSON | No | No | No |
| Claude "clean-ai-slop" skills | — | none | — | Yes (in-harness) | No | partial |
| **`deslop`** | Rust | strong | **first-class (work orders + MCP)** | **optional/swappable** | **yes (`verify`/`apply`)** | **yes** |

**Thesis.** The winning artifact is *not* "a tool with an LLM in it." It is **a precise
analyzer + an output an LLM can act on + a deterministic gate that checks the LLM's work.**
deslop owns the deterministic ends; the model in the middle is interchangeable.

The AI-slop catalog is empirical, not provenance-based. Recent smell-taxonomy work on
LLM-generated code reports that implementation smells dominate the measured gap, with an
average implementation-smell increase of roughly 73% per task
(https://arxiv.org/abs/2510.03029). deslop uses that evidence to prioritize intrinsic,
baseline-free cleanup rules such as stubs, magic numbers, long methods, duplication, and
over-narration. It is **not** an AI-authorship detector: clean code should pass whether a
human or model wrote it, and sloppy code should be fixable regardless of provenance.

### Goals
- A genuine analyzer (tree-sitter CST + scope/ref graph + token duplication + complexity),
  Clojure/Julia/Rust first-class by consuming clj-kondo / StaticLint or JET / clippy.
- **Report broadly; auto-fix narrowly** (only provably behavior-preserving edits, in place).
- A **machine/agent output** rich enough to rewrite from without re-deriving anything.
- A **`propose → verify → apply`** loop so the safety contract survives even when the LLM
  is external and unknown to deslop.
- Adoptable on bloated repos via baseline/ratchet. No telemetry. LLM feature optional.

### Non-goals
Not a formatter, not a type checker, not an LLM-first analyzer. The bundled consumer is a
thin reference implementation of the existing work-order/patch contract.

---

## 2. Tiered analysis (deterministic) → external rewrite (swappable)

```
 T0 PARSE      tree-sitter CST → nodes, spans, comments, byte ranges
 T1 SEMANTIC   locals.scm scope/ref graph (unused/single-use); token suffix-automaton
               duplication; CST branch-count complexity; AST idiom matching
 T2 EXTERNAL   Clojure: clj-kondo JSON (authoritative). Julia (opt): StaticLint/JET.
               Rust (opt): clippy JSON (machine-applicable lint confirmation).
 ───────────────────────────────────────────────────────────────────────────────────
 OUTPUTS:  scan → findings (text / json / sarif / AGENT work-orders)
           fix  → safe-auto + analyzer-confirmed edits, in place
           propose → work orders for non-deterministic findings  ──┐
                                                                    ▼
                              [ ANY LLM / AGENT — external, swappable, ad-hoc ]
                                                                    │ returns a patch
           verify ◄───────────────────────────────────────────────┘
           apply  = verify (parse + --check-cmd + defensive-guard) then atomic write
```

The deterministic tiers (T0–T2) and the `verify`/`apply` gate are deslop. The rewrite step
is *outside* the trust boundary — which is exactly why `verify` must be deterministic, local,
and mandatory before any non-`safe-auto` write.

---

## 3. The fix-safety lattice (retained spine)

Every rule has a **safety class**. `scan` reports all; `fix`/`apply` key off it.

| class | meaning | in-place by `fix`? | to apply otherwise |
|---|---|---|---|
| `safe-auto` | behavior-preserving under **all** syntactic conditions | **yes** | — |
| `analyzer-confirmed` | safe **iff** clj-kondo/StaticLint/JET/clippy confirms the fact | yes, iff T2 confirms | — |
| `safe-with-precondition` | safe only under a stated, not-always-checkable precondition | no (suggest) | `apply` with passing `--check-cmd` |
| `risky-suggest` | plausible but real semantic surface | no (suggest) | `apply` with `--check-cmd` |
| `llm-only` | needs judgment; no deterministic edit | no | `propose` → LLM → `verify`/`apply` |
| `never-auto` | report only | never | — |

Canonical counterexamples that forbid "obvious" auto-fixes:
- **CLJ `(= (count x) 0)`→`(empty? x)`** changes termination on infinite/lazy `x` ⇒
  `safe-with-precondition` (finite/countable).
- **Julia `1:length(x)`→`eachindex(x)`** is wrong for `OffsetArrays`/ordinal use ⇒
  `safe-with-precondition` (1-based, positional).

Each non-`safe-auto` rule ships a machine-readable **precondition** + **counterexample** +
**default**, surfaced in `deslop rules` and in every work order.

---

## 4. The `propose → verify → apply` loop  *(new core)*

This is how "the repo proposes to the LLM" works without losing safety.

1. **`deslop propose`** emits **work orders** (§5) for findings that have no `safe-auto`
   edit — one self-contained unit per region: the region source, the findings inside it
   (rule, message, safety class, precondition), and an explicit **instruction** +
   **acceptance contract** (`check_cmd`, "must still parse", "must not delete error
   handling", "must not add public defs"). No model is called.
2. **Any agent** (external Claude Code/Cursor/Codex session, a CI script, or the optional
   bundled consumer) reads the work orders and returns **patches** in deslop's patch schema
   (§5): region fingerprint + replacement text.
3. **`deslop verify`** ingests patches and runs the **deterministic gate** — re-parse
   (tree-sitter/balance), the `--check-cmd` (tests/typecheck/clj-kondo re-lint), the
   defensive-code guard, and the size/scope guards — reporting pass/fail per patch. Writes
   nothing.
   With `--coverage`, the gate uses registry-driven `CoverageProvider`s: Rust via
   `cargo-llvm-cov` LCOV, Clojure via cloverage JSON/EDN, Julia via Coverage.jl `.cov`/LCOV,
   and Python via coverage.py JSON/XML. Missing tools degrade to `coverage-unknown`; they never
   fail verification by themselves.
4. If the verifier verdict is `coverage-unknown`, `untested-risky`, or `dead-candidate`, the
   oracle is too weak to trust deletion. **`deslop characterize --patches ...`** emits
   `needs-characterization-test` work orders. An external agent writes the test, then
   **`deslop verify-characterization --tests ... --check-cmd ...`** accepts it only if it
   compiles and passes on the current unmodified code. Accepted characterization tests can be
   passed to `verify`/`apply` with `--characterization-tests`; deslop writes them into the temp
   project and requires the same `--check-cmd` to pass after the patch.
5. **`deslop apply`** = `verify` + atomic write (`*.deslop.bak`, `undo`-able), skipping any
   patch that fails the gate.

The model is interchangeable because the contract is carried *in the data* (work order out,
patch in) and the gate is owned by deslop. Swap Claude for Codex for a local model — the
safety guarantees are unchanged.

Coverage mode parsing is shared by the CLI and MCP through `deslop-verify`. The accepted mode
strings are `disabled`/`off`/`none`, `auto`, `auto:<cmd>`, `lcov:<path>`,
`cloverage:<path>`, `julia-cov:<path>`/`julia:<path>`, and
`coverage-py:<path>`/`coverage.py:<path>`/`python:<path>`.

Mutation probes are also registry-driven and opt-in. Rust uses `cargo-mutants` outcomes. Python
uses Cosmic Ray because it has a project configuration, a durable SQLite session, and reports that
can be reduced to source path + line + killed/survived status; deslop's live mode runs
`cosmic-ray init`/`exec` when a Cosmic Ray config is present and degrades to `mutation-unknown`
when the command/config/session inspection is unavailable. Recorded outcome files are accepted for
deterministic tests. Clojure and Julia are intentionally not wired until their source-mappable
contracts are stable enough for region gating: JVM bytecode tools such as PITest do not map cleanly
to Clojure source regions; Heretic is promising and Clojure-specific but currently labels itself
experimental/not released, so its JSON/EDN contract is not yet a stable verifier input. Julia's
older Vimes.jl path reports patches/diffs but is legacy, while Gremlins.jl is a new 0.x
source-splicing project; both are deferred until a maintained, source-line machine-readable report
contract is proven.

---

## 5. Agent-ready output / protocol (`deslop-protocol`)  *(new, first-class)*

The **work-order** (proposal) and **patch** (ingest) schemas are the product surface.
Stable, versioned, emitted by `--format agent` (JSONL) and over MCP.

```jsonc
// WORK ORDER  (deslop propose / scan --format agent)
{ "schema": "deslop.workorder/1",
  "kind": "rewrite-region | needs-characterization-test",
  "id": "wo_<fingerprint>",
  "path": "src/core.clj",
  "region": { "start_line": 40, "end_line": 71, "text": "<exact region source>" },
  "findings": [
    { "rule": "duplicate-block", "severity": "major", "safety": "llm-only",
      "message": "lines 44-58 duplicate src/io.clj:12-26",
      "precondition": null }
  ],
  "instruction": "Rewrite the region to remove the flagged bloat without changing behavior or the public API. Preserve language and indentation.",
  "contract": { "must_parse": true, "no_new_public_defs": true,
                "keep_error_handling": true, "max_growth_ratio": 1.0,
                "check_cmd": "clojure -M:test" } }

// PATCH  (input to deslop verify / apply)
{ "schema": "deslop.patch/1",
  "workorder_id": "wo_<fingerprint>",
  "region_fingerprint": "<fingerprint>",   // must match current bytes or verify rejects
  "replacement": "<rewritten region source>",
  "by": "claude-code | cursor | codex | deslop-slim | human" }

// CHARACTERIZATION TEST  (input to deslop verify-characterization and optional verify/apply gate)
{ "schema": "deslop.characterization-test/1",
  "workorder_id": "wo_<fingerprint>",
  "region_fingerprint": "<fingerprint>",
  "test_path": "tests/characterization_test.clj",
  "test_text": "<test source written by an external agent>",
  "by": "claude-code | cursor | codex | human" }
```

- `scan --format agent` / `propose` produce work orders; `verify`/`apply` consume patches.
- `characterize` produces `needs-characterization-test` work orders for weak-oracle verifier
  verdicts; `verify-characterization` consumes submitted tests and rejects tests that fail on
  current unmodified code.
- Same surface is exposed as **MCP** tools (`propose`, `fix`, `verify`, `apply`) so an
  in-loop agent calls deslop directly (§9). The MCP `fix` tool is agent-as-consumer: it
  returns prompts and fingerprints, never a server-side LLM result.
- `region_fingerprint` mismatch ⇒ the file changed under the patch ⇒ reject (no stale write).

---

## 6. Core types (`deslop-core`)

```rust
pub enum Severity { Info, Minor, Major }
pub enum SafetyClass { SafeAuto, AnalyzerConfirmed, SafeWithPrecondition,
                       RiskySuggest, LlmOnly, NeverAuto }
pub enum DetectedBy { TreeSitter, ScopeGraph, Duplication, Complexity,
                      Idiom, CljKondo, JuliaAnalyzer, RustAnalyzer }

pub struct Finding {
    pub path: PathBuf, pub span: Span, pub rule: &'static str,
    pub severity: Severity, pub safety: SafetyClass, pub detected_by: DetectedBy,
    pub message: String, pub suggestion: String,
    pub precondition: Option<&'static str>,
    pub edit: Option<Edit>,        // Some only for safe-auto / analyzer-confirmed
    pub fingerprint: u64,          // stable: rule + norm-path + span-shape + node-text
}
pub struct Edit { pub splices: Vec<Splice>, pub kind: EditKind }   // ropey, fmt-preserving
pub enum Lang { Clojure, Julia, Rust, Python, Generic }
```

## 6a. Plugin architecture (`LangPack`, `Rule`, `ExternalAnalyzer`)

Language support is a registry-driven plugin boundary, not a pile of core `match`
arms. `deslop-lang` is the low crate shared by parsing and analysis; it owns path
detection, tree-sitter grammar selection, CST region extraction, and comment syntax.
`deslop-parse` and `deslop-analyzer` query this registry rather than switching on
`Lang`. Analyzer rule packs and external analyzers attach to the same stable `Lang`
id. Adding low-level language behavior should require only a new pack module plus one
registry registration line.

```rust
pub trait LangPack {
    fn name(&self) -> &'static str;
    fn lang(&self) -> Lang;
    fn extensions(&self) -> &'static [&'static str];      // detection
    fn grammar(&self) -> Option<TreeSitterGrammar>;       // parse + ERROR-node check
    fn line_comments(&self) -> &'static [&'static str];   // tokenizer/comment rules
    fn enclosing_region(&self, node: Node, text: &str) -> Option<RegionSpan>;
}

pub trait Rule {
    fn name(&self) -> &'static str;
    fn check(&self, source: &SourceFile, cfg: AnalyzerConfig) -> Vec<Finding>;
}

pub trait ExternalAnalyzer {
    fn name(&self) -> &'static str;
    fn covered_rules(&self) -> &'static [&'static str];
    fn analyze(&self, path: &Path, source: &SourceFile) -> Result<ExternalFindings>;
}
```

External analyzers are all the same shape: subprocess + machine-readable output →
`Finding`s, with graceful degradation when the executable or analyzer package is absent.
`clj-kondo`, `clippy`, and Julia StaticLint/JET adapters share this contract. When an
external analyzer is available, covered built-in rules are suppressed to avoid
double-reporting.

---

## 7. Rule catalog (class-tagged; `scan` reports all)

**Scope/dead-code** (T1 `locals.scm`, upgraded by T2): `unused-import/require`,
`unused-private-def`, `unused-binding` → `analyzer-confirmed` (else `risky-suggest`);
`single-use-binding`, `unreachable-form` → `risky-suggest`; `unused-public-def` →
`never-auto` (candidate). **Duplication** (token suffix-automaton): `duplicate-block`,
`near-duplicate` → `llm-only`. **Complexity/shape**: `high-cyclomatic`, `deep-nesting`,
`long-method` → `llm-only`.

**Clojure idioms** — `safe-auto`: `reimpl-not=` `(not (= …))→(not= …)`, `reimpl-some?`
`(not (nil? x))→(some? x)`, `reimpl-boolean` `(if x true false)→(boolean x)`, `redundant-do`
`(when c (do …))→(when c …)`. `safe-with-precondition`: `reimpl-empty?`, `reimpl-seq`,
`reimpl-vec` (finite coll). `risky-suggest`: `threading-opportunity`.

**Julia idioms** — `safe-with-precondition`: `explicit-return` (AST tail position),
`reimpl-isempty` (well-behaved collection), `reimpl-eachindex` (1-based/positional).
`risky-suggest`: `reimpl-isnothing` (`==` overloadable), `trivial-fn-block`. Optional
StaticLint JSON upgrades `unused-arg`/`unused-binding` to `analyzer-confirmed`; StaticLint
missing references and JET diagnostics are report-only/`never-auto`.

**Rust idioms** — `safe-with-precondition`: `needless-return` (tail-position
`return x;`→`x`), `useless-format` (`format!("{}", x)`→`x.to_string()`). `risky-suggest`:
`redundant-closure` (`|x| f(x)`→`f`), `let-and-return`. `llm-only`: `needless-clone`
(`.clone()` where a borrow may suffice). Optional clippy JSON upgrades covered lints to
`analyzer-confirmed`.

**Intrinsic slop** — `llm-only`: `incompleteness` (stubs/placeholders/TODO implementation
holes), `long-method`; `risky-suggest`: `magic-number` (inline numeric literals without a
named constant, excluding conventional tiny values). `slop-score` is a report metric, not an
edit rule: it summarizes weighted slop-rule density per file/repo.

**Comment/structure** — `llm-only`: `narrating-comment`, `comment-block` (≥4 lines, not
header). `safe-auto`: `consecutive-blank-lines` (collapse).

> The only unconditional in-place auto-fixes: blank-line collapse + four exact CLJ idioms.

---

## 8. Commands (`deslop-cli`)

```
deslop scan     [PATHS…] [--format text|json|sarif|agent] [--baseline FILE] [--since REF] [--fail-on major] [--julia-external[=staticlint|jet|off]] [--julia-project DIR]
deslop metrics  [PATHS…] [--format text|json] [--hotspots-only] [--sigma N]
deslop health   [PATHS…] [--format text|json] [--hotspots-only] [--sigma N] # alias
deslop slop     [PATHS…] [--format text|json]                 # weighted slop score
deslop fix      [--paths PATH… | --workorders FILE] [--apply] [--characterize] [--allow-unverified] [--coverage MODE] [--provider anthropic|openai] [--base-url URL] [--model M] [--mock recorded.txt] [--check-cmd "CMD"] [--no-backup] # bundled slim consumer; dry-run by default
deslop propose  [PATHS…] [-o workorders.jsonl] [--julia-external[=staticlint|jet|off]] [--julia-project DIR] # emit work orders
deslop characterize --patches FILE [-o workorders.jsonl] [--check-cmd "CMD"] [--coverage] [--mutation]
deslop verify-characterization --tests FILE --check-cmd "CMD"
deslop verify   --patches FILE  [--check-cmd "CMD"] [--coverage] [--mutation] [--characterization-tests FILE] # run the gate; write nothing
deslop apply    --patches FILE  [--check-cmd "CMD"] [--coverage] [--mutation] [--characterization-tests FILE] [--no-backup] [--yes]   # verify then write
deslop-lsp                                                       # synchronous LSP server binary
deslop baseline write [PATHS…] [-o deslop-baseline.json]
deslop undo     [PATHS…]
deslop mcp                                                      # feature=mcp stdio MCP server
deslop rules                                                   # class, precondition, counterexample
```

- **`scan`**: `ignore`-walk; `--since` = changed files only; `--baseline` suppresses known
  findings and **fails CI only on new slop** (ratchet). Heavy external analyzers are
  opt-in; `--julia-external` selects StaticLint by default and `--julia-project` passes
  `julia --project=...`.
- **`metrics`/`health`**: computes per-region complexity and expressivity metrics, ranks
  repo-relative bloat hotspots, and emits text or JSON.
- **`fix`**: the bundled `deslop-slim` consumer. It proposes work orders from `--paths` or
  reads JSONL from `--workorders`, builds prompts, asks a swappable `LlmClient`, converts
  rewrites into `deslop.patch/1`, verifies them, and prints a dry-run JSON report unless
  `--apply` is passed. Default `--apply` writes only `removable` verifier verdicts;
  `coverage-unknown`, `untested-risky`, and `dead-candidate` are reported as held-unproven
  unless `--allow-unverified` is explicit. With `--characterize`, slim asks the same
  `LlmClient` for tests only for rewrites whose initial verdict needs characterization,
  accepts generated tests only through `verify_characterization_tests` on the current
  unmodified code, then re-verifies and applies rewrites with the accepted tests in
  `VerifyOptions.characterization_tests`. The report includes characterization attempts,
  accepted/rejected tests, and verdict upgrades such as `coverage-unknown` -> `removable`.
  `--coverage` accepts `disabled`, `auto`, `auto:<cmd>`, `lcov:<path>`,
  `cloverage:<path>`, `julia-cov:<path>`, and `coverage-py:<path>`, mapping directly to
  `CoverageConfig`. `--provider` selects `anthropic` or OpenAI-compatible `openai`;
  `--base-url` overrides the OpenAI-compatible base URL for providers such as Together,
  Groq, Ollama, OpenRouter, or vLLM. `--mock` uses `RecordedClient` for deterministic tests
  and offline replay.
- **`propose`/`verify`/`apply`**: the swappable-LLM loop (§4). `verify`/`apply` are the
  trust boundary; they run with **no network** and need no model.
- **`mcp`**: feature-gated stdio MCP server exposing `scan`, `propose`, `verify`,
  `apply`, `metrics`, and `rules` as tools for in-loop agents.

---

## 9. Metrics / health

`deslop metrics` measures bloat-prone regions, not just rule findings. It is built on
`LangPack`: each pack declares metric region node kinds, branch node kinds,
nesting/control-flow node kinds, line-comment tokens, and Halstead operator tokens.
`deslop-parse` supplies the CST; languages without a grammar use text-only metrics.

Per region:
- **Complexity:** cyclomatic (`branch_count + 1`), cognitive control/nesting/flow-break
  score, max nesting, NLOC, Halstead Volume/Difficulty/Effort, Maintainability Index
  normalized 0-100.
- **Expressivity / density:** decision density (`cyclomatic / tokens`), unique-token
  ratio, comment-to-code ratio, and a compression/redundancy proxy.
- **Compression proxy:** currently byte entropy normalized to `0.0..1.0`, chosen to avoid
  a compression dependency while still flagging repetitive low-information regions.

After scanning, metrics are compared against the run's own distribution. A hotspot is a
region at least `--sigma` standard deviations from the repo median on high complexity or
low expressivity. Low-expressivity checks require a minimum token count to avoid tiny helper
false positives. Ranked hotspots are `llm-only` cleanup candidates for future slim/MCP
workflows; metrics itself does not rewrite code.

---

## 10. MCP and optional LLM reference consumer (`deslop-mcp`, `deslop-slim`)

`deslop-mcp` is the preferred integration: an in-loop coding agent lists tools, requests
findings/work orders, rewrites regions, and submits patches that deslop verifies. It is a
feature-gated stdio MCP server (`--features mcp`) and is network-free: it only runs the
deterministic analyzer, protocol serializer, metrics engine, and `deslop-verify` gate.
It implements the core JSON-RPC MCP methods needed by coding agents:
`initialize`, `tools/list`, and `tools/call`. Tool payloads reuse the existing
`deslop.findings/1`, `deslop.workorder/1`, `deslop.fix/1`, `deslop.patch/1`,
`deslop.characterization-test/1`, `deslop.verify/1`, `deslop.apply/1`, and
`deslop.metrics/1` schemas. The `fix` tool scans/proposes work orders, reuses
`deslop_slim::build_prompt`, and returns prompt entries containing `workorder_id`, `path`,
line range, `region_fingerprint`, contract, findings, and prompt text. The caller rewrites
the region and submits `deslop.patch/1` patches through `apply`, so the existing
verify-gated removable-only default remains the write boundary. MCP `verify` and `apply`
accept `coverage` as either the original boolean (`true` = `auto`, `false`/absent =
`disabled`) or a shared parser mode string such as `lcov:<path>`, so agents can reach
`removable` verdicts from recorded coverage without CLI-only affordances.

The MCP `fix` tool has two modes. `mode="prompts"` is the default and is always available:
it returns the agent-as-consumer `deslop.fix/1` prompt payload described above. `mode="auto"`
is opt-in server-run LLM execution: it constructs a `deslop-slim` client from
`provider`/`model`/`base_url` or a recorded `mock` response, runs `run_slim`, and returns the
`deslop.slim/1` report. Auto mode is compiled only with the `deslop-mcp` `slim-llm` cargo
feature, which enables `deslop-slim/anthropic` and `deslop-slim/openai`; default MCP builds
keep `slim-llm` off and return a clear feature-required error for `mode="auto"`.

`deslop-slim` exists to prove the loop and to serve users with no agent harness. The runtime
loop is: propose/load work orders → build a constrained prompt from instruction, exact region
text, findings, and contract → `LlmClient::rewrite` → strip markdown fences → emit
`deslop.patch/1` with `by = deslop-slim/<model>` → `verify_patches` → default dry-run report
or `apply_patches` when `--apply` is explicit. Its report separates patches into `applied`,
`held_unproven`, and `rejected`; held patches include a suggestion to pass coverage, add
characterization tests, or explicitly use `--allow-unverified`. `SlimPrompt.kind` labels
rewrite and characterization prompts so deterministic tests and providers can distinguish the
two phases. When `--characterize` is enabled, slim generates characterization tests only for
weak-oracle rewrites, verifies those tests on current code, carries accepted tests into the
second verifier pass, and reports attempts plus before/after verdict upgrades. Failed
generated tests are rejected and do not weaken the removable-only apply gate.
`AnthropicClient` uses Anthropic Messages via `ureq` and `ANTHROPIC_API_KEY`.
`OpenAiClient` uses the OpenAI-compatible Chat Completions shape at
`{base_url}/chat/completions`, defaults
`base_url` to `https://api.openai.com/v1`, and reads `OPENAI_API_KEY` with
`DESLOP_SLIM_API_KEY` fallback. Neither client logs keys. `RecordedClient` reads a response
from disk and is the test/replay client. It enforces nothing the core doesn't; all guarantees
live in `verify`. The HTTP clients are behind `deslop-slim`'s optional `anthropic` and
`openai` features; default slim builds enable both, while MCP depends on `deslop-slim` with
default features disabled unless `deslop-mcp/slim-llm` is explicitly enabled. Deferred
integration work: streaming progress and non-OpenAI-compatible provider families.

`deslop-lsp` is a synchronous editor-integration server built on `lsp-server` and
`lsp-types`. It advertises full text synchronization and code-action support. On
`didOpen`, full-document `didChange`, and `didSave`, it analyzes the in-memory document text
through `deslop-analyzer::scan_source` and publishes diagnostics with `source = "deslop"`,
rule code, finding message, and severity mapped as major/error, minor/warning, info/hint.
MVP ranges use 0-based lines and whole-line columns; precise UTF-16 columns are deferred.
Code actions are deliberately narrower than diagnostics: only `safe-auto` and
`analyzer-confirmed` findings with edits produce a `quickfix`, using
`deslop_fix::apply_findings_to_text` and returning a whole-document `WorkspaceEdit`.
`safe-with-precondition`, `risky-suggest`, `llm-only`, and `never-auto` findings get no
editing action.

---

## 11. Architecture (Rust workspace)

```
crates/
  deslop-core/       # Finding, SafetyClass, Edit, Span, Lang, fingerprint
  deslop-lang/       # LangPack registry: detection, grammar, region, comment syntax
  deslop-parse/      # SourceFile + tree-sitter parse calls via deslop-lang
  deslop-analyzer/   # T1: scope graph, duplication, complexity, idiom rules
  deslop-metrics/    # per-region complexity/expressivity + hotspot ranking
  deslop-external/   # T2: clj-kondo / clippy / StaticLint/JET adapters (subprocess; graceful degrade)
  deslop-fix/        # safe-auto / analyzer-confirmed CST edits (ropey)
  deslop-protocol/   # work-order + patch schemas (serde); --format agent
  deslop-verify/     # the deterministic gate: parse + check-cmd + defensive/scope guards
  deslop-slim/       # bundled optional LLM reference consumer
  deslop-lsp/        # sync LSP diagnostics + safety-gated code actions
  deslop-report/     # text / json / sarif renderers
  deslop-cli/        # orchestration, exit codes
  deslop-mcp/        # feature=mcp stdio MCP tools over protocol/verify
```

`core/parse/analyzer/fix/protocol/verify` have **no network deps**. `mcp` is optional,
network-free, and depends only on deterministic deslop crates plus `deslop-slim` with
default features disabled for prompt construction. `slim` is isolated and only its
`anthropic` and `openai` features are allowed to depend on HTTP/LLM client code.

---

## 12. Baseline, output, config, safety, deps
- **Baseline/ratchet** (M1): `baseline write`; `scan --baseline` fails CI only on new
  `fingerprint`s. Gates reporting/CI only — **never weakens `fix`/`apply`**.
- **Output:** text (shows `class`+`detected_by`), JSON, SARIF 2.1.0, **agent JSONL** (§5).
- **CI/pre-commit packaging:** root `action.yml` installs the local CLI, optionally writes
  `deslop.sarif` with `scan --format sarif`, and gates with `scan --fail-on`; the example
  `.github/workflows/deslop.yml` uploads SARIF through
  `github/codeql-action/upload-sarif@v3`. `.pre-commit-hooks.yaml` exposes a system
  `deslop scan --fail-on major` hook. `docs/CI.md` documents SARIF upload, fail-on exit
  codes, and baseline ratchets.
- **Config** `deslop.toml`: `--config <path>` loads project defaults with CLI > env >
  config > built-in precedence. Implemented sections are `[scan] fail_on/baseline`,
  `[fix] check_cmd/coverage/allow_unverified`, `[slim] provider/model/base_url`,
  `[external] clippy/julia_analyzer/julia_project`, and analyzer thresholds
  `[analyzer] min_duplication_tokens/long_method_nloc/min_meaningful_tokens`.
  API keys are env-only. Inline `deslop-ignore` remains deferred.
- **Safety:** `verify` owns the gate; `*.deslop.bak` + `undo`; `git`/`jj` dirty check;
  atomic temp+rename; `region_fingerprint` guards against stale patches.
- **Deps:** `clap`, `ignore`, `tree-sitter`+grammars, `regex`, `serde`/`toml`/
  `serde_json`, `anyhow`/`thiserror`; `lsp-server`/`lsp-types` are isolated to
  `deslop-lsp` for synchronous JSON-RPC/LSP types; `ureq` is used only by `deslop-slim`
  HTTP provider features as a minimal synchronous HTTP client.

---

## 13. Testing
Golden fixtures + `.expected.json`; **fix round-trip** (gone, still parses, idempotent);
**FP corpus** (clean CLJ/Julia/Rust → zero findings); **parser-survival fuzz**; **safety-class
property tests** (`safe-auto` ⇒ AST-equivalent on a corpus; `safe-with-precondition` ⇒ fix
**withheld** without `--check-cmd`); **protocol round-trip** (work order → patch → verify);
**verify-gate tests** (a patch deleting a `try/catch` is **rejected** by the defensive
guard; a stale `region_fingerprint` is rejected); clj-kondo/clippy/Julia external
present/absent fixture/degrade tests; plugin registry dispatch; Rust CST region
extraction; `slim` deterministic prompt/client/e2e tests with no network/API key, including
default hold of `coverage-unknown`, `--allow-unverified` opt-in apply, rejected rewrites
blocked in both modes, `--characterize` accept/reject paths that prove accepted tests upgrade
weak verdicts while failing tests stay held, and LCOV-backed `removable` apply by default.
Optional live smoke sits outside the default suite.
Metrics tests cover cyclomatic counts, known Halstead numbers, hotspot detection, and a
throwaway pack driving metric declarations without central language edits.
MCP tests cover `tools/list` schemas, `tools/call scan`, `fix` prompt generation,
default-build `fix mode=auto` feature-required errors, propose→verify round-trip, stale
`region_fingerprint` rejection, MCP coverage bool back-compat/defaults, bad coverage-mode
errors, LCOV mode-string `apply` upgrading a covered patch to `removable`, and an
initialize/list/scan stdio transcript. With `deslop-mcp/slim-llm`, a deterministic mock
auto-mode test proves a covered `deslop.slim/1` rewrite writes and a rejected rewrite does
not.
LSP unit tests cover pure finding→diagnostic mapping and safety-lattice code-action gating:
safe fixable findings produce a quickfix edit, while `llm-only` findings produce no edit.
CLI integration tests cover `scan --fail-on major` exiting non-zero on a sloppy fixture and
zero on a clean fixture. SARIF shape remains covered by `sarif_render_has_required_shape_and_locations`.

---

## 14. Milestones  *(LLM bundling is last and optional)*
1. **M1 — Analyzer + safe core + agent output:** `core`+`parse`+`analyzer` (report) +
   idiom detection (CLJ/Julia/Rust) + `scan` (text/json/SARIF/**agent**) +
   `fix` (**safe-auto only**) + `baseline` + `undo` + `--since`. Useful, safe, every
   language, no model.
2. **M2 — The loop + clj-kondo:** `deslop-protocol` + `propose`/`verify`/`apply` +
   `deslop-verify` gate (parse + `--check-cmd` + defensive guard) + clj-kondo/clippy
   `analyzer-confirmed` fixes + duplication detection + plugin registry.
3. **M3 — Metrics + MCP + breadth:** `deslop metrics/health`, `deslop-mcp` (the real
   "propose to the LLM" channel), SARIF 2.1.0, and config-gated Julia StaticLint/JET adapter.
4. **M4 — Optional consumers:** `deslop-slim` reference consumer + `deslop-lsp` +
   pre-commit hook +
   GitHub Action (ratchet gate).
5. **M5 — Later/experimental:** statistical per-repo verbosity anomalies; learned detector.

---

## 15. Open questions
- clj-kondo overlap: defer entirely to it for covered rules when present (one source of
  truth); our CLJ rules are the no-kondo fallback. (Lean yes.)
- Julia T2 is runtime-heavy and analyzer-package dependent, so it defaults `off`. StaticLint
  is the preferred bloat-oriented adapter (`unused-arg`/`unused-binding` confirmed);
  JET remains available for report-only correctness diagnostics. Missing Julia/package,
  helper failure, or timeout emits one notice and falls back to T1 Julia rules.
- Cross-file `unused-public-def`: labeled `never-auto` candidates first.

---

## Sources (survey, 2026-06-23)
- fallow — https://github.com/fallow-rs/fallow · antislop — https://github.com/skew202/antislop
- sloppylint — https://github.com/rsionnach/sloppylint · DCE-LLM — https://aclanthology.org/2025.naacl-long.501.pdf
- clj-kondo analysis — https://github.com/clj-kondo/clj-kondo/blob/master/analysis/README.md
- tree-sitter locals.scm — https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html
- JET.jl — https://github.com/aviatesk/JET.jl · StaticLint.jl — https://github.com/julia-vscode/StaticLint.jl
