# TASK 8/queue — Analyzer threshold knobs in config (move consts into AnalyzerConfig)

Make the remaining rule thresholds configurable via `AnalyzerConfig` + `deslop.toml [analyzer]`.
Behavior-preserving by default. Start with `jj new` (separate change on top of svrplorq).

## The consts to migrate (only these two; `min_duplication_tokens` is already configurable)
- `crates/deslop-analyzer/src/agnostic.rs:9` `const LONG_METHOD_NLOC: usize = 40` (long-method).
- `crates/deslop-analyzer/src/tokens.rs:8` `const MIN_MEANINGFUL_TOKENS: usize = 8`
  (near-duplicate / duplicate-block meaningful-token floor).

`AnalyzerConfig` currently: `min_duplication_tokens` (default 24), `rust_external`, `julia_external`,
`julia_project`. Rules already receive `&AnalyzerConfig` via `Rule::check(source, config)`.

## Do
1. Add to `AnalyzerConfig`: `long_method_nloc: usize` (Default 40) and `min_meaningful_tokens: usize`
   (Default 8). Keep the `Default` impl producing the SAME values as today (40 / 8 / 24) — no
   behavior change unless configured.
2. Replace the const usages with `config.long_method_nloc` / `config.min_meaningful_tokens`. Thread
   `&AnalyzerConfig` (or just the needed usize) into the token/duplication functions in `tokens.rs`
   if they don't already receive it — minimal plumbing, no logic change. Remove the now-unused consts.
3. Extend the CLI `[analyzer]` config section (currently only `min_duplication_tokens`) to also
   accept `long_method_nloc` and `min_meaningful_tokens`, wiring them into the `AnalyzerConfig` the
   CLI builds. Precedence stays CLI/flags > config > default (there may be no CLI flag for these — if
   not, config > default is fine; do NOT add new flags unless trivial).
4. Update `deslop.toml.example` + `docs/CONFIG.md` with the new `[analyzer]` keys.

## Tests (deterministic)
- Default preserved: existing analyzer/corpus tests stay green (defaults 40/8/24 unchanged).
- `long_method_nloc`: a method just under the default (e.g. 40 lines) is NOT flagged at default but
  IS flagged when `long_method_nloc` is lowered; assert the finding count changes with config.
- `min_meaningful_tokens`: changing it changes near-duplicate/duplicate-block findings on a fixture
  (lower floor → more/looser matches). Assert via `scan_source_with_config`.
- Config parse: a `[analyzer]` section with all three keys deserializes and reaches `AnalyzerConfig`.

## Constraints / gate
No rule LOGIC change — only the threshold source. MCP stays network-free (default build). Do NOT
touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
fields added + defaults; the const→config replacement (and any plumbing into tokens.rs); the
`[analyzer]` config keys; the threshold tests (default-preserving + config-changes-behavior); docs.
`jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start queued items 9-13.
