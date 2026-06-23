# TASK 7/queue — MCP `fix` option A: opt-in server-run LLM (feature-gated, default stays network-free)

The MCP `fix` tool today only does option B (returns prompts; `fix_tool` at deslop-mcp lib.rs:269).
Add option A — the server runs the LLM end-to-end (slim `run_slim` → verify → apply) — but ONLY
behind a cargo feature that is OFF by default, so the default MCP build remains provably
network-free. Start with `jj new` (separate change on top of znzxmqym).

## Design
1. **Cargo feature** on deslop-mcp, e.g. `slim-llm = ["deslop-slim/anthropic", "deslop-slim/openai"]`
   (deslop-mcp currently depends on deslop-slim with `default-features = false`). Default features:
   feature OFF. With it OFF, `cargo tree -p deslop-mcp -i ureq` MUST stay empty (keep that test).
2. **`fix` tool gains `mode`** (string, default `"prompts"`):
   - `"prompts"` → current option-B behavior, ALWAYS available (unchanged output `deslop.fix/1`).
   - `"auto"` → option A. Requires the `slim-llm` feature. Build a slim `LlmClient` from args and run
     `deslop_slim::run_slim`, returning the `SlimReport` JSON (schema `deslop.slim/1`).
     - If the feature is NOT compiled in, return a CLEAR error: "fix mode=auto requires deslop-mcp
       built with --features slim-llm". Do NOT panic.
3. **auto-mode params** (mirror the CLI `fix` surface, reuse `SlimOptions`): `paths`, `provider`
   (anthropic|openai), `model`, `base_url`, `apply` (bool, default false → dry-run), `allow_unverified`
   (default false), `coverage` (mode string via the shared `parse_coverage_mode`), `check_cmd`,
   `characterize` (bool), and `mock` (path to a recorded response → use `RecordedClient`/`ScriptedClient`
   for deterministic tests). API key stays env-only (never from args). Never log the key.
4. Update the `fix` tool schema + description (document mode + the auto params + the feature
   requirement). SPEC.md: note option A is available behind `slim-llm`, default build is network-free.

## Tests
- DEFAULT build (no feature): `fix` mode=prompts unchanged; mode=auto returns the clear
  feature-required error; `cargo tree -p deslop-mcp -i ureq` empty. (Keep/extend existing MCP tests.)
- `#[cfg(feature = "slim-llm")]` test: `fix` mode=auto with `mock` (recorded rewrite) on a fixture →
  returns a `deslop.slim/1` report; with `apply:true` in a tempdir a verified (Removable / covered or
  allow_unverified) rewrite is written; a rejected rewrite is not. Deterministic, NO network/key.

## Gate (run BOTH feature states)
Default: `cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Feature: `cargo test -p deslop-mcp --features slim-llm && cargo clippy -p deslop-mcp --features slim-llm -- -D warnings`

## Constraints / report
Do NOT change option B output, the slim gating, or verify. Do NOT touch `deslop/*.py`. Report: the
feature; `mode` param; default-build network-free proof; the auto-mode mock test; both gate states.
`jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start queued items 8-13.
