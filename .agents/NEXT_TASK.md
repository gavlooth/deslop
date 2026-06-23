# TASK 3/queue — MCP coverage-mode parity

MCP `verify`/`apply` only accept `coverage: boolean` (true→Auto, false→Disabled). The CLI supports
full modes (disabled/auto/auto:<cmd>/lcov:<path>/cloverage:<path>/julia-cov:<path>/coverage-py:<path>).
Without mode parity, an agent CANNOT reach a `Removable` verdict via MCP (can't point at an LCOV
file etc.). Fix it. Start with `jj new` (separate change on top of txmxlptr).

## The parser to share (currently CLI-only)
`crates/deslop-cli/src/main.rs:760` `parse_coverage_config(&str) -> Result<CoverageConfig>` (+
`parse_coverage_config_with_value` at 769) handles every mode. deslop-verify has NO public mode
parser. MCP `coverage_config(args)` (in deslop-mcp) only branches on a bool.

## Do
1. **Lift the parser into `deslop-verify`** as a public fn (e.g.
   `pub fn parse_coverage_mode(s: &str) -> Result<CoverageConfig>`, or `impl FromStr for
   CoverageConfig`). Move the mode logic from the CLI into it verbatim (disabled/off/none, auto,
   auto:<cmd>, lcov:<path>, cloverage:<path>, julia-cov:<path>|julia, coverage-py:<path>|python,
   with the same error message). Update the CLI to delegate to the shared fn — NO behavior change;
   keep the CLI's `parses_slim_coverage_modes` test green.
2. **MCP `coverage_config(args)`**: accept `coverage` as EITHER
   - a boolean (back-compat: true→Auto, false→Disabled), OR
   - a string mode parsed via the shared `parse_coverage_mode` (return a clear error on bad mode —
     `coverage_config` may need to return `Result<CoverageConfig>`; thread that through verify/apply
     tool handlers).
   Apply to BOTH the `verify` and `apply` tools (the two that take coverage). `fix` returns prompts
   only — no coverage needed there.
3. **Update tool schemas** in `tools_list_result` so `verify`/`apply` document `coverage` accepting
   either a boolean or a mode string (list the modes in the description). Keep the default false.

## Tests (deterministic, NO network) — reuse the LCOV fixture pattern
- MCP `apply` (or `verify`) with `coverage: "lcov:<path>"` on a covered region → verdict `Removable`
  (mirror the LCOV fixture used in deslop-verify / deslop-slim tests). With the patch covered, it
  applies WITHOUT `allow_non_removable`.
- Back-compat: `coverage: true` still behaves as `Auto`; `coverage: false`/absent → `Disabled`.
- Bad mode string → clear error (tool returns an error, doesn't panic).
- Keep existing MCP tests green (scan/propose/verify/apply/fix).

## Constraints / gate
No CLI behavior change. No new deps. MCP stays network-free (`cargo tree -p deslop-mcp -i ureq`
empty). Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
shared parser location; MCP coverage accepting bool|mode; the LCOV→Removable MCP test; back-compat
+ bad-mode tests; SPEC.md + tool-schema updates. `jj describe -m "<summary>"`. Touch
`.agents/HEARTBEAT.md`. Do NOT start queued items 4-6.
