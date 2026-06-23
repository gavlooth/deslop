# TASK 1/queue — OpenAI-compatible LLM provider (decouple LlmClient providers)

deslop-slim has a swappable `LlmClient` trait; only `AnthropicClient` exists (feature-gated behind
`anthropic`, uses `ureq`). Add an OpenAI-compatible provider so Together/Groq/Ollama/OpenRouter/
vLLM/etc. work via base_url. MCP must stay network-free (it depends on deslop-slim with
default-features=false). Start with `jj new` (separate change on top of otlwomyy).

## Build
1. **`OpenAiClient`** in deslop-slim, behind a new `openai` cargo feature (also `dep:ureq`):
   - `[features]` → `default = ["anthropic", "openai"]`, `openai = ["dep:ureq"]` (keep `anthropic`).
     Both http clients are now optional; with `--no-default-features` NEITHER is compiled (MCP path
     unaffected — re-verify `cargo build -p deslop-slim --no-default-features` and that ureq stays
     out of deslop-mcp's tree).
   - Chat Completions shape: `POST {base_url}/chat/completions` with
     `{ "model": <model>, "messages": [{"role":"user","content": prompt.text}] }`, parse
     `choices[0].message.content`, then `strip_code_fences`. Mirror AnthropicClient's structure
     (blocking ureq, clear errors, NEVER log the key).
   - `base_url` default `https://api.openai.com/v1`; key from `OPENAI_API_KEY` (fall back to a
     generic `DESLOP_SLIM_API_KEY` if you add one); model from `--model`/`DESLOP_SLIM_MODEL`.
   - Factor a small shared `anthropic_text_response`-style pure parser (`openai_text_response(body)
     -> Result<String>`) so it's unit-testable without network.
2. **Provider selection** in the `deslop fix` CLI:
   - `--provider <anthropic|openai>` (default `anthropic`), `--base-url <url>` (override), keep
     `--model`. Build the matching client. `--mock` (RecordedClient) still overrides everything.
   - `deslop fix --help` must show `--provider` and `--base-url`.
   - The CLI depends on deslop-slim with DEFAULT features (both clients available) — confirm.

## Tests (deterministic, NO network, NO key)
- `openai_text_response` parses `{"choices":[{"message":{"content":"```rust\nfn f() {}\n```"}}]}` →
  `fn f() {}` (fence-stripped). Mirror the existing anthropic parser test.
- Provider selection: a small unit asserting `--provider openai` + `--base-url` constructs the
  OpenAI client (or a factory fn returns the right variant). No live call.
- Keep all existing slim tests green (recorded e2e, gating, rejection).

## Constraints / gate
Do NOT change the prompt builder, run_slim gating, verify, or the MCP path. Do NOT touch
`deslop/*.py`. Reuse `strip_code_fences`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
new feature flags; OpenAiClient + parser; CLI `--provider`/`--base-url`; test outcomes;
re-confirm MCP network-free (`cargo tree -p deslop-mcp -i ureq` empty); SPEC.md update (providers).
`jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`.

---
## Queue after this (do NOT start these now; one task per pass, I verify between)
2. Characterization-test generation loop (slim/agent fulfills NeedsCharacterizationTest).
3. MCP coverage-mode parity (verify/apply/fix accept coverage modes, not just bool).
4. LSP server. 5. CI/pre-commit packaging. 6. Config file (deslop.toml).
(Python LangPack intentionally EXCLUDED.)
