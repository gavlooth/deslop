# TASK — MCP `fix` tool parity (option B: agent-as-consumer, NO server-side LLM)

deslop-slim works via CLI. Expose it through MCP so an agent can run the consumer loop — but the
MCP server must NOT call its own LLM (the caller IS an LLM; circular). Option B: the `fix` tool
returns deslop-slim's rewrite PROMPTS + fingerprints; the calling agent rewrites; patches go back
through the EXISTING verify-gated `apply` tool (already defaults to removable-only,
allow_non_removable=false). Start with `jj new` (separate change on top of kxunkwxn).

## Step 1 — feature-gate the HTTP client so MCP stays network-free
In `crates/deslop-slim/Cargo.toml`: make `ureq` optional; add
```
[features]
default = ["anthropic"]
anthropic = ["dep:ureq"]
```
Gate `AnthropicClient` (struct + impl + anthropic_text_response + any ureq use) behind
`#[cfg(feature = "anthropic")]`. Everything else — `SlimPrompt`, `build_prompt`, `strip_code_fences`,
`RecordedClient`, `run_slim`, gating types — stays feature-independent and must compile with
`--no-default-features`. The CLI keeps default features (AnthropicClient still works there).
Verify: `cargo build -p deslop-slim --no-default-features` succeeds with no ureq in the tree.

## Step 2 — deslop-mcp depends on deslop-slim (no default features)
In `crates/deslop-mcp/Cargo.toml`: add `deslop-slim = { workspace = true, default-features = false }`
so the MCP server reuses `build_prompt` / `SlimPrompt` WITHOUT pulling ureq/network.

## Step 3 — add the MCP `fix` tool (match existing tool pattern)
- Register in `tools_list_result()` (crates/deslop-mcp/src/lib.rs:85) alongside the others:
  `tool("fix", "Return deslop-slim rewrite prompts (deslop.fix/1) for agent-as-consumer; submit resulting deslop.patch/1 via the apply tool.", object_schema(json!({ "paths": paths_schema() })))`.
- Dispatch in `tools_call_result` (the `match name` ~line 211): `"fix" => fix_tool(args)?`.
- Implement `fix_tool(args)` mirroring `propose_tool`: propose work orders
  (`work_orders_for_source`), and for each `RewriteRegion` work order (SKIP NeedsCharacterizationTest)
  emit a prompt entry. Reuse `deslop_slim::build_prompt(&work_order)` and
  `deslop_protocol::workorder_region_fingerprint(&work_order)`. Return:
  ```
  { "schema": "deslop.fix/1",
    "prompts": [ { "workorder_id", "path", "region": {"start_line","end_line"},
                   "region_fingerprint", "contract", "findings", "prompt" } ],
    "next": "Rewrite each region. Build deslop.patch/1 patches { schema:\"deslop.patch/1\", workorder_id, region_fingerprint, replacement, by } and call the `apply` tool (default applies only Removable; pass coverage / allow_non_removable to widen)." }
  ```
- NO server-side LLM call. Do NOT add AnthropicClient to the MCP path. Note option A
  (server-run client) as deferred.

## Tests (deterministic, no network)
- `fix_tool` on a fixture with a known rewrite finding returns schema `deslop.fix/1`, ≥1 prompt,
  each prompt's `region_fingerprint == workorder_region_fingerprint(work_order)`, and `prompt`
  contains the region text + a finding message. (Reuse the MCP test pattern already in the crate.)
- Confirm `deslop-slim` builds with `--no-default-features` (the feature gate works) — note it in
  the report (a CI/check line is enough; a build invocation in the gate covers it).

## Gate after each change (revert anything that breaks)
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Constraints / report
Do NOT change the CLI slim behavior, the prompt builder, or verify. Do NOT touch `deslop/*.py`. No
new deps (ureq becomes optional, not new). Update SPEC.md (MCP `fix` tool documented; option A
deferred) and the MCP tools list. Report: feature-gate result, the `fix` tool schema, the
agent-as-consumer flow (fix → agent rewrites → apply), test outcomes, `--no-default-features` build
proof. `jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`.
