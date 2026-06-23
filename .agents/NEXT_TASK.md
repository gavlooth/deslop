# TASK 6/queue (final) — Config file: extend deslop.toml for project defaults

A `DeslopConfig` already loads `deslop.toml` (cwd) with an `[external]` section
(crates/deslop-cli/src/main.rs:320, `read_default()`). EXTEND it to cover the rest, with clear
precedence. Start with `jj new` (separate change on top of lnlzsupu).

## Do
1. **Extend `DeslopConfig`** with optional sections (keep `[external]` working unchanged):
   - `[slim]` → `provider` (anthropic|openai), `model`, `base_url`
   - `[fix]` → `check_cmd`, `coverage` (mode string, parsed via `deslop_verify::parse_coverage_mode`),
     `allow_unverified` (bool)
   - `[scan]` → `fail_on` (severity: info|minor|major), `baseline` (path)
   - (optional) `[analyzer]` → `min_duplication_tokens` (AnalyzerConfig already has this field; only
     expose knobs that AnalyzerConfig actually supports — long-method NLOC etc. are consts, NOT in
     AnalyzerConfig, so DEFER those with a note; do not refactor the analyzer this pass).
2. **`--config <path>`** global flag to load a specific file (default `./deslop.toml`; absent file →
   defaults, same as today). Thread it into `read_default`/a new `read_from(path)`.
3. **Precedence** (apply everywhere the config now feeds): explicit CLI flag > env (where one exists,
   e.g. API key, model) > `deslop.toml` > built-in default. Implement by resolving
   `cli.or(config).or(default)` for the affected options (make fields `Option` where needed to detect
   "not provided"). Keep current behavior identical when no `deslop.toml` exists.
   - SECURITY: do NOT read API keys from `deslop.toml`. Keys stay env-only. Document this.
4. **Sample + docs**: a commented `deslop.toml.example` (all sections) and a `docs/CONFIG.md` (or a
   README section) describing each key + the precedence order + the env-only-secrets rule.

## Tests (deterministic, NO network)
- Parsing: a fixture `deslop.toml` with all sections deserializes into `DeslopConfig` with expected
  values (slim/fix/scan/external).
- Precedence: with a config setting (e.g. slim.model or scan.fail_on), an explicit CLI flag wins;
  with no CLI flag, the config value is used; with neither, the built-in default. Unit-test the
  resolution helper(s) directly (don't require running the whole CLI).
- Coverage mode in `[fix]` parses via `parse_coverage_mode` (reuse).
- Keep all existing tests green (external config, slim, scan exit codes).

## Constraints / gate
No analyzer behavior change beyond reading existing AnalyzerConfig knobs. No new deps (toml already
present). MCP stays network-free. Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
config sections added; `--config`; the precedence rule + helper; env-only-secrets note; sample +
docs; test outcomes; what's deferred (analyzer threshold knobs needing AnalyzerConfig changes).
`jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`. This is the LAST queued item — note the
queue is complete in your report.
