# deslop

**A deterministic, multi-language detector for AI/LLM code bloat — and a behavior-preserving way to remove it.**

deslop scans code for "slop" (behavior-preservingly removable bloat: duplication, dead scaffolding,
needless wrappers, over-long functions, narration comments, magic numbers, …), and emits rich,
**agent-ready** output. It *proposes* cleanups as work orders and *verifies/applies* the patches that
come back through a deterministic safety gate. **Any LLM is a swappable, external consumer — never a
dependency.** The tool is fully useful with the LLM feature compiled out.

> Slop is defined by *removability*, not authorship — deslop is not an "AI-authorship detector." A
> finding means "this looks behavior-preservingly removable," and removal is only ever applied when
> the safety gate proves it.

## Why it's structured this way

The product is the **deterministic analyzer + its output contract**, not a bundled model:

- **`propose`** emits work orders (`deslop.workorder/1`).
- An LLM (Claude Code, Cursor, Codex, a CI bot, or the optional built-in consumer) rewrites regions.
- **`verify` / `apply`** are the deterministic safety gate (`deslop.patch/1`): re-parse, run your
  `--check-cmd`, match a region fingerprint, and only apply changes that clear the gate.

## Install

```sh
cargo install --path crates/deslop-cli --features mcp   # CLI + MCP server
# deslop-lsp is a separate binary: cargo install --path crates/deslop-lsp
```

## CLI

| Command | Purpose |
|---|---|
| `deslop scan <paths>` | Findings (`text`/`json`/`agent`/`sarif`); `--fail-on <sev>` for CI gating |
| `deslop slop <paths>` | Slop score + per-rule counts |
| `deslop metrics` (`health`) | Repo health, hotspots, complexity/expressivity metrics |
| `deslop graph <paths>` | Agent-ready `deslop.graph/1` dependency graph for refactor planning (`json`/`dot`) |
| `deslop propose <paths>` | Work orders for non-safe-auto findings |
| `deslop fix` | The bundled LLM consumer: propose → rewrite → verify → apply |
| `deslop fix --diff` | Preview deterministic safe-auto edits as a unified diff without writing |
| `deslop verify` / `apply` | Verify / atomically apply `deslop.patch/1` patches |
| `deslop characterize` / `verify-characterization` | Generate/accept behavior-pinning tests for risky regions |
| `deslop baseline` / `scan --baseline` | Snapshot findings; gate only on regressions |
| `deslop eval` | Run the labeled corpus; per-rule precision/recall |
| `deslop feedback <fingerprint> --false-positive` | Turn reviewed false positives into eval cases |
| `deslop undo` · `deslop rules` · `deslop mcp` | Undo safe-auto edits · rule catalog · run MCP server |

## How findings are graded

**Fix-safety lattice** (per finding): `safe-auto` (only this writes in place) · `analyzer-confirmed`
· `safe-with-precondition` · `risky-suggest` · `llm-only` · `never-auto`.

**Removability verdicts** (the prover, used by `verify`/`apply`): `Removable` · `DeadCandidate` ·
`UntestedRisky` · `CoverageUnknown` · `Rejected`. By default `apply` writes only `Removable`; widen
explicitly with `--coverage <mode>` (prove it) or `--allow-unverified` (opt into the unproven band).

### Detection tiers

- **T0/T1 — deterministic core:** tree-sitter CST + scope/duplication/complexity analysis.
- **T2 — external analyzers (opt-in):** clippy (Rust), clj-kondo (Clojure), StaticLint.jl / JET.jl (Julia).
- **Coverage (prove removability):** Rust `cargo-llvm-cov`, Clojure cloverage, Julia Coverage.jl,
  Python coverage.py — recorded reports or live `Auto` mode, graceful degrade when a tool is absent.
- **Mutation (high-signal confirmation tier):** a native tree-sitter mutation engine (`deslop-mutate`)
  for Rust/Clojure/Julia/Python, plus Cosmic Ray (Python) as an opt-in; coverage-gated, timed out,
  and parallelized (surviving mutants downgrade a removal verdict). Mutation refines removal *safety*;
  it is not itself a slop detector.

## The cleanup loop (`deslop fix`)

`fix` runs propose → build a per-region prompt → call an LLM → build a patch → `verify` → apply.

- **Providers:** `--provider anthropic|openai` (`--base-url` makes "openai" cover any
  OpenAI-compatible endpoint: Together, Groq, Ollama, OpenRouter, vLLM, …). API keys are read from the
  environment only — never from config. Both clients are optional cargo features (`anthropic`,
  `openai`); with neither, no network code is compiled.
- **Safe by default:** dry-run unless `--apply`; applies only `Removable` unless widened.
- **Characterization (`--characterize`):** for weakly-tested regions, generate a test that pins
  current behavior, accept it only if it passes on unmodified code, then re-verify — turning an
  unprovable removal into a safe one.
- **Source-egress consent:** real-provider calls require explicit consent (`--yes`, `DESLOP_SLIM_CONSENT`,
  `[slim] egress_consent`, or an interactive prompt); non-interactive without consent refuses cleanly.
- **Progress** streams to stderr (`--quiet` to silence); stdout stays the machine-readable report.

## Editor & MCP

- **MCP server** (`deslop mcp`, `--features mcp`): tools `scan`, `propose`, `verify`, `apply`,
  `characterize`, `verify_characterization`, `metrics`, `graph`, `rules`, and `fix`. The default build is
  **network-free**. `fix` defaults to *agent-as-consumer* (returns rewrite prompts + fingerprints;
  the calling agent rewrites and submits patches to `apply`); a server-run LLM mode is available only
  behind the `slim-llm` feature. `scan`, `propose`, and prompt-mode `fix` accept a `config` path and
  inline `analyzer` overrides, including per-language `long_method_nloc`; inline MCP values override
  the config file for that tool call.
- **LSP** (`deslop-lsp`): live diagnostics + code actions wired to the fix-safety lattice (only
  safe-auto findings get quickfixes; plus a `source.fixAll`), precise UTF-16 ranges, incremental sync.

## CI

`deslop scan --changed[=<ref>] --format sarif` → upload to GitHub code scanning; `--fail-on major`
as an exit-code gate; `--baseline` to gate only on new findings; `deslop baseline update` to ratchet
accepted current findings. A reusable `action.yml`, an example
`.github/workflows/deslop.yml`, and `.pre-commit-hooks.yaml` are included (see `docs/CI.md`).

## Configuration

`deslop.toml` (or `--config <path>`) sets project defaults; precedence is **CLI flag > env > config >
default**. Sections: `[external]`, `[slim]` (provider/model/base_url/egress_consent), `[fix]`
(check_cmd/coverage/allow_unverified), `[scan]` (fail_on/baseline), `[analyzer]`
(min_duplication_tokens/long_method_nloc/min_meaningful_tokens). See `deslop.toml.example` and
`docs/CONFIG.md`.

`long_method_nloc` defaults to 40 non-comment lines globally and can be overridden per language:

```toml
[analyzer]
long_method_nloc = 40

[analyzer.rust]
long_method_nloc = 45

[analyzer.python]
long_method_nloc = 35

[analyzer.typescript]
long_method_nloc = 45
```

Supported per-language analyzer tables are `rust`, `clojure`, `julia`, `python`, `javascript`,
`typescript`, and `generic`.

Thresholds are global and blunt; **suppression** is the scalpel. Filter findings by rule and by
path — unknown rule names and unknown `[analyzer]` keys are errors, not silent no-ops:

```toml
[analyzer]
disabled_rules = ["magic-number"]          # drop a rule everywhere
ignore_paths = ["**/generated/**"]          # skip paths for all rules

[analyzer.rules.duplicate-block]
ignore_paths = ["**/tests/**"]              # skip one rule on selected paths
```

See `docs/CONFIG.md` for the full suppression reference.

## Languages

Full analysis packs for **Rust, Clojure, Julia**, seeded idiom packs for **Python** and
**JavaScript/TypeScript**, plus the language-agnostic rules that apply to all sources.

## Workspace

17 crates: `deslop-core` (types) · `deslop-parse` (tree-sitter) · `deslop-lang` (LangPack/Rule/provider
traits + registries) · `deslop-analyzer` (rules/packs) · `deslop-external` (clippy/clj-kondo/StaticLint/JET)
· `deslop-metrics` · `deslop-graph` (refactor dependency graph) · `deslop-mutate` (CST mutation operators) · `deslop-verify` (prover, coverage &
mutation tiers, apply) · `deslop-protocol` (workorder/patch schemas) · `deslop-report` (text/json/agent/SARIF)
· `deslop-fix` (deterministic safe-auto fixes) · `deslop-slim` (LLM consumer) · `deslop-mcp` · `deslop-lsp`
· `deslop-eval` (corpus harness) · `deslop-cli`.

## Design notes & known gaps

The full design rationale is in `SPEC.md`. Deferred: tree-sitter 0.26 (blocked upstream by the
Clojure grammar's pin), Clojure/Julia mutation tools (none source-mappable), workspace-wide LSP scan,
and equivalent-mutant detection (the path to using mutation as an actual slop *detector*). The honest
scope: deslop's core — deterministic detection + behavior-preserving removal — is complete; the
surrounding tiers are confirmation/usability layers.
