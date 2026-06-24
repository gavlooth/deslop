# TASK 10/queue — non-Rust coverage live wiring (Auto mode actually runs the tool)

The non-Rust coverage providers (`ClojureCloverageProvider`, `JuliaCoverageProvider`,
`PythonCoveragePyProvider`, registered ~deslop-verify lib.rs:1413) parse RECORDED report files
(`*File` modes, tested) and degrade gracefully when the tool is absent. The gap: their `Auto` /
`AutoWithCommand` mode should actually INVOKE the language coverage tool, locate the generated
report, and parse it — like `RustCargoLlvmCovProvider`'s Auto path (~1461). Start with `jj new`
(separate change on top of mvnszkqq).

## First: inspect what each non-Rust provider's Auto arm currently does
For each of the three providers, check the `CoverageConfig::Auto`/`AutoWithCommand` → `CoverageProviderMode`
mapping and the run path. If Auto is currently a no-op / immediate-degrade (vs. the Rust provider
which runs `cargo llvm-cov`), implement the live invocation. Report which were no-ops before.

## Do (live Auto for each; mirror the Rust llvm-cov Auto pattern)
- **Clojure (`ClojureCloverageProvider`)**: Auto runs `lein cloverage` (AutoWithCommand overrides the
  command), produces the cloverage report (JSON/EDN), locate + parse it into line coverage.
- **Julia (`JuliaCoverageProvider`)**: Auto runs Julia with `--code-coverage` (or the Coverage.jl
  flow), locate the generated `.cov`/LCOV, parse it.
- **Python (`PythonCoveragePyProvider`)**: Auto runs `coverage run` then `coverage json` (or `coverage
  lcov`), locate + parse the report.
- Each: `AutoWithCommand(cmd)` overrides the default command. Locate the report deterministically
  (temp/working dir). On ANY failure or missing tool → `CoverageStatus::Unknown` + a clear notice,
  NEVER a panic or a hard reject (keep the existing graceful-degrade contract).
- Registry-driven; no central `match Lang`.

## Tests (deterministic, NO tools/network)
- Command construction: assert each provider builds the expected command + args for Auto and
  AutoWithCommand (unit-test the builder, don't run it).
- Absent-tool degrade: with the tool missing, Auto → Unknown + notice, verdict unchanged (extend the
  existing degrade tests to the Auto path for all three).
- Recorded `*File` parsing stays green (don't regress).
- (Live runs are NOT unit-tested — they need the tools installed; note that explicitly.)

## Constraints / gate
Don't change the `*File` parsers or the degrade contract. MCP stays network-free (default build).
Do NOT touch this repo's `deslop/*.py` sources. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
which providers were no-op-Auto before; the live command per language (default + AutoWithCommand
override); report-location strategy; the command-construction + absent-tool tests; what's
un-unit-testable (live) and why; SPEC.md coverage-tier update. `jj describe -m "<summary>"`. Touch
`.agents/HEARTBEAT.md`. Do NOT start queued items 11-13.
