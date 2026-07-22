# deslop

deslop finds the kind of bloat that LLM-assisted coding tends to leave behind:
duplicated blocks, dead scaffolding, needless wrappers, over-long functions,
narration comments, magic numbers. It scans deterministically across several
languages, proposes cleanups as machine-readable work orders, and verifies and
applies the patches that come back through a safety gate that never takes a
model's word for anything.

Two things are worth knowing up front.

First, "slop" here means removable without changing behavior. It does not mean
"written by an AI", and deslop is not an authorship detector. A finding says
"this looks like it can go"; nothing actually goes until the gate proves it.

Second, no LLM is bundled or required. Whatever rewrites code, whether that is
Claude Code, Cursor, a CI bot, or the optional built-in client, is an external
consumer that can be swapped out. The tool works fine with the LLM feature
compiled out entirely.

## How the pieces fit

The product is the deterministic analyzer and its output contract, not a model.
The loop has three steps:

1. `deslop propose` emits work orders (`deslop.workorder/3`) with the full
   analyzer, scope, and source context, plus an exact-byte revision guard.
2. Something rewrites the flagged regions. That can be you, an editor agent, a
   CI bot, or the bundled `deslop fix` consumer.
3. `deslop verify` and `deslop apply` are the gate (`deslop.patch/3`). They
   reconstruct the persisted proposal, re-parse, run your `--check-cmd`, check
   the revision guard against the exact bytes on disk, and only write patches
   that clear all of it.

## Install

```sh
cargo install --path crates/deslop-cli --features mcp   # CLI + MCP server
cargo install --path crates/deslop-lsp                  # LSP is a separate binary
```

## Commands

| Command | What it does |
|---|---|
| `deslop scan <paths>` | Report findings as text, JSON, agent output, or SARIF; `--fail-on <sev>` gates CI |
| `deslop slop <paths>` | Slop score with per-rule counts |
| `deslop metrics` | Structural and lexical measurements plus triage outliers; never a rewrite gate |
| `deslop graph <paths>` | Dependency graph (`deslop.graph/2`) for refactor planning; `--contract` emits the contract graph instead |
| `deslop refactor-risk --from <rev> --to <rev>` | Compare revisions for refactor-defect accumulation (see below) |
| `deslop propose <paths>` | Work orders for agent-rewrite findings |
| `deslop fix` | The bundled LLM consumer: propose, rewrite, verify, apply |
| `deslop fix --diff` | Preview deterministic safe-auto edits as a diff without writing |
| `deslop verify` / `deslop apply` | Verify and atomically apply `deslop.patch/3` patches |
| `deslop characterize` | Generate behavior-pinning tests for risky regions (`verify-characterization` accepts them) |
| `deslop baseline` | Snapshot current findings so scans gate only on regressions |
| `deslop eval` | Run the labeled corpus and report per-rule precision and recall |
| `deslop feedback <fingerprint> --false-positive` | Turn a reviewed false positive into an eval case |
| `deslop undo`, `deslop rules`, `deslop mcp` | Undo safe-auto edits, print the rule catalog, run the MCP server |

## How findings are graded

Every finding carries a fix-safety class: `safe-auto`, `analyzer-confirmed`,
`safe-with-precondition`, `risky-suggest`, `llm-only`, or `never-auto`. Only
`safe-auto` findings are ever written in place, and `never-auto` findings are
report-only; they never enter work orders or prompts.

Separately, the prover behind `verify` and `apply` assigns a removability
verdict: `Removable`, `DeadCandidate`, `UntestedRisky`, `CoverageUnknown`, or
`Rejected`. By default `apply` writes only `Removable`. You can widen that with
`--coverage <mode>` (prove it with coverage data) or `--allow-unverified`
(explicitly opt into the unproven band).

Detection runs in tiers. The core is deterministic tree-sitter parsing with
scope, duplication, and complexity analysis. External analyzers are opt-in:
clippy for Rust, clj-kondo for Clojure, StaticLint.jl and JET.jl for Julia.
Coverage tools (cargo-llvm-cov, cloverage, Coverage.jl, coverage.py) feed the
removability proof, either from recorded reports or a live mode, and degrade
gracefully when absent. On top of that sits a native tree-sitter mutation
engine for Rust, Clojure, Julia, and Python (with Cosmic Ray as a Python
alternative): surviving mutants downgrade a removal verdict. Mutation refines
removal safety; it is not itself a slop detector.

## The cleanup loop

`deslop fix` runs the whole loop: propose, build a per-region prompt, call an
LLM, build a patch, verify, apply.

It is dry-run unless you pass `--apply`, and applies only `Removable` unless
widened. `--provider anthropic|openai` selects the client, and `--base-url`
lets the openai client talk to any compatible endpoint (Together, Groq,
Ollama, OpenRouter, vLLM). API keys come from the environment only, never from
config, and both clients are optional cargo features; with neither enabled, no
network code is compiled at all.

For weakly-tested regions, `--characterize` generates a test that pins current
behavior, accepts it only if it passes on unmodified code, and then re-verifies.
That turns an unprovable removal into a safe one.

Sending source to a real provider requires explicit consent: `--yes`, the
`DESLOP_SLIM_CONSENT` variable, `[slim] egress_consent` in config, or an
interactive prompt. Without consent in a non-interactive run, it refuses and
says so. Progress goes to stderr; stdout stays machine-readable.

## Refactor history analysis

`deslop refactor-risk` compares an ordered window of revisions and reports the
defects that accumulate when a refactor moves behavioral ownership but leaves
consumers, verifiers, tests, telemetry, or operational identity attached to
the former owner. Eleven detector families cover stale consumers, schema
drift, retired gates, scope collapse, lost score provenance, inert config
keys, lagging test oracles, duplicated hot-path work, stale telemetry and
status surfaces, and incomplete adoption chains.

Revisions can be snapshot directories, Git revisions, or Jujutsu revsets;
`--to` defaults to the working tree, and `--bundle` accepts a caller-produced
`deslop.refactor-history/1` file, whose revision-pinned LSP facts join as
supporting or conflicting evidence without overriding the syntax analysis.
Every finding in this family is review-only (`never-auto`) and carries a
causal path, counter-evidence, explicit coverage gaps, and a suggested
verification instead of a fix. The full design and its evaluation gates are in
[`docs/REFACTOR_DEFECT_ACCUMULATION.md`](docs/REFACTOR_DEFECT_ACCUMULATION.md).

## Editors and MCP

The MCP server (`deslop mcp`, behind `--features mcp`) exposes `scan`,
`propose`, `verify`, `apply`, `characterize`, `verify_characterization`,
`metrics`, `graph`, `refactor_risk`, `rules`, and `fix`. The default build is
network-free. `fix` defaults to agent-as-consumer: it returns rewrite prompts
and exact revision guards, and the calling agent submits patches back through
`apply`. A server-side LLM mode exists only behind the `slim-llm` feature.
Tool calls accept a `config` path and inline analyzer overrides, which win
over the config file for that call.

The LSP gives live diagnostics and code actions wired to the fix-safety
lattice: only safe-auto findings get quickfixes, plus a `source.fixAll`. With
`refactor_base` set in the `[lsp]` section of `deslop.toml`, it also compares
the live buffer against a base revision and publishes refactor-risk findings
as review diagnostics.

## CI

Run `deslop scan --changed[=<ref>] --format sarif` and upload the result to
GitHub code scanning. `--fail-on major` turns severity into an exit code, and
`--baseline` gates only on new findings; `deslop baseline update` ratchets the
accepted set. A reusable `action.yml`, an example workflow, and
`.pre-commit-hooks.yaml` are included; see `docs/CI.md`.

## Configuration

`deslop.toml` (or `--config <path>`) sets project defaults. Precedence is CLI
flag, then environment, then config, then built-in default. The sections are
`[external]`, `[slim]`, `[fix]`, `[scan]`, and `[analyzer]`; see
`deslop.toml.example` and `docs/CONFIG.md` for the full reference.

The long-method threshold defaults to 40 non-comment lines and can be set per
language:

```toml
[analyzer]
long_method_nloc = 40

[analyzer.rust]
long_method_nloc = 45

[analyzer.python]
long_method_nloc = 35
```

Per-language tables exist for `rust`, `clojure`, `julia`, `python`,
`javascript`, `typescript`, and `generic`.

Thresholds are blunt; suppression is the scalpel. You can disable a rule
everywhere, skip paths for all rules, or skip one rule on selected paths.
Unknown rule names and unknown `[analyzer]` keys are errors, not silent no-ops:

```toml
[analyzer]
disabled_rules = ["magic-number"]
ignore_paths = ["**/generated/**"]

[analyzer.rules.duplicate-block]
ignore_paths = ["**/tests/**"]
```

## Config-boundary analysis

`deslop scan` also follows the lifecycle of config keys, from declaration in
TOML, YAML, or JSON, through parsing, to actual behavioral use, and flags keys
that only look wired:

- `config-key-unread`: declared in a config artifact but never read by any
  scanned code.
- `config-key-unconsumed`: parsed, and usually echoed or serialized, but
  nothing behavioral consumes it.
- `config-key-shadowed`: parsed, then unconditionally overwritten by a literal
  before any behavioral use.

The analysis is language-agnostic (key names are normalized, so `canvas-top-k`
in TOML matches `canvas_top_k` in code) and precision-first: any use that
cannot be confidently classified as an echo counts as live and suppresses the
finding. Tune it under `[analyzer.boundary]`; tool configs like `Cargo.toml`
and `package.json` are skipped by default.

## Languages

Stable syntax-level analysis covers Rust, Clojure, Julia, Python, JavaScript,
and TypeScript, plus language-agnostic rules wherever their structural
preconditions hold. JavaScript/JSX, TypeScript, and TSX get distinct grammar
selection; TSX shares TypeScript's thresholds and rules. Deeper facts (scope
resolution, control and data flow, external analyzers, transformations) carry
explicit per-adapter capability and provenance rather than being implied.

## Release status

The current release, `0.1.0-evidence.1`, is deliberately labeled
stable-evidence-limited. The exact shipped/unshipped matrix, benchmarks,
failure taxonomy, and migration rules are in
[`docs/M10_RELEASE_REPORT.md`](docs/M10_RELEASE_REPORT.md) and
[`docs/M10_CAPABILITY_MATRIX.md`](docs/M10_CAPABILITY_MATRIX.md).

The honest limits: readability labels are unshipped, global transformation
precision is demonstrated only for one frozen Rust recipe slice, million-line
performance is not claimed, and whole-project finding-proposal batching is
unshipped after it timed out in a measured dogfood run. Agent workflows should
use the bounded work-order protocol. Unknown, partial, and unsupported facts
stay visible and block dependent rewrites instead of being papered over.

## Workspace

Seventeen crates. `deslop-core` holds the shared types; `deslop-parse` owns
tree-sitter parsing and snapshots; `deslop-lang` defines the language adapter
traits and registries; `deslop-analyzer` implements the rules;
`deslop-external` wraps clippy, clj-kondo, StaticLint, and JET;
`deslop-metrics`, `deslop-graph`, and `deslop-mutate` do what their names say;
`deslop-verify` is the prover with the coverage and mutation tiers;
`deslop-protocol` defines the work-order and patch schemas; `deslop-report`
renders text, JSON, agent output, and SARIF; `deslop-fix` applies the
deterministic safe-auto edits; `deslop-slim` is the optional LLM client;
`deslop-mcp` and `deslop-lsp` are the servers; `deslop-eval` runs the corpus;
and `deslop-cli` ties it together.

## Known gaps

The full design rationale lives in `SPEC.md`. Still deferred: the tree-sitter
0.26 upgrade (blocked upstream by the Clojure grammar's pin), external
mutation tools for Clojure and Julia (the native engine already covers both;
the ecosystem's tools are either bytecode-based and not source-mappable, or
too experimental to be a stable verifier input), workspace-wide LSP scanning,
and equivalent-mutant detection, which is the path to using mutation as an
actual slop detector rather than a safety check. Release exceptions and their
recheck conditions are tracked in
[`docs/M10_FAILURE_RISK_REGISTER.md`](docs/M10_FAILURE_RISK_REGISTER.md)
rather than hidden behind a broader "complete" claim.
