# TASK — Build deslop-slim: the bundled LLM consumer that closes propose → verify → apply

Self-debloat converged; tree-sitter 0.26 held (clojure blocker, documented). Now build the
headline feature: a slim, bundled LLM consumer realizing the SPEC thesis — **deslop proposes &
verifies; the LLM is a swappable consumer that only rewrites.** deslop stays the verifier/applier.

## Start
Run `jj new` first so deslop-slim is a SEPARATE change on top of the now-described analyzer change
(do NOT amend the analyzer change). Describe the slim change at the end.

## Existing seam (use these real types — do not reinvent)
- `deslop-protocol`: `WorkOrder { schema, kind: WorkOrderKind (RewriteRegion|NeedsCharacterizationTest),
  id, path, region: Region { start_line, end_line, text }, findings: Vec<WorkOrderFinding {rule,
  severity, safety, message, precondition}>, instruction, contract: Contract { must_parse,
  no_new_public_defs, keep_error_handling, max_growth_ratio, check_cmd } }`. `Patch { schema, workorder_id,
  region_fingerprint, replacement, by }`. Helper: `workorder_region_fingerprint(&WorkOrder) -> String`.
- `deslop-cli`: `Propose` emits `deslop.workorder/1`; `Verify`/`Apply` consume `deslop.patch/1`.
- `deslop-verify`: `verify_patches(&[Patch], &VerifyOptions) -> Result<VerifyReport>` (per-patch
  `verdict: VerificationVerdict`), `apply_patches(...)`, `load_patches(&Path)`.
- Workspace deps available: anyhow, clap, serde, serde_json. No HTTP client yet.

## Build (crates/deslop-slim, new crate in the workspace)
1. **`LlmClient` trait** (swappable): `fn rewrite(&self, prompt: &SlimPrompt) -> Result<String>`.
   - `AnthropicClient` (default): blocking HTTP via **`ureq`** (add to workspace deps — justified:
     existing stack can't make HTTP calls; ureq is minimal/sync, no tokio). Anthropic Messages API;
     model from `--model` / `DESLOP_SLIM_MODEL` (default a current Claude model id), key from
     `ANTHROPIC_API_KEY`. NEVER log the key. On missing key / network error: clear error, no panic.
   - `RecordedClient` (for tests): returns a recorded response read from a path — NO network.
2. **Prompt builder**: from `instruction` + `region.text` + each finding (rule/message/precondition)
   + contract constraints. Instruct the model to return ONLY the rewritten region (behavior-
   preserving, no prose, no fences). Strip code fences if the model adds them.
3. **Consumer loop** `run_slim`: for each `RewriteRegion` work order → prompt → client.rewrite →
   build `Patch { schema:"deslop.patch/1", workorder_id: wo.id, region_fingerprint:
   workorder_region_fingerprint(&wo), replacement, by: format!("deslop-slim/{model}") }`. Collect
   patches, run `verify_patches`, and apply only verified patches (`apply_patches`) — DEFAULT
   dry-run (print verdicts); `--apply` writes. Skip `NeedsCharacterizationTest` work orders (out of
   scope; note them).
4. **CLI**: add `deslop fix` subcommand: `--paths <...>` (propose internally) OR `--workorders
   <file.jsonl>`; `--apply`; `--model`; `--mock <recorded.json>` (uses RecordedClient); `--check-cmd`.
   Wire the contract's check_cmd / VerifyOptions through. `deslop fix --help` must show these.

## Tests (REQUIRED — deterministic, NO network, NO api key)
- Mock end-to-end: RecordedClient returns a valid behavior-preserving rewrite for a known work
  order → assert Patch shape (schema/workorder_id/fingerprint/by) → verify runs → with `--apply`
  in a tempdir the verified replacement is written.
- Rejection path: a recorded BAD rewrite (fails to parse, or violates max_growth_ratio / adds a
  public def) → `verify_patches` REJECTS it → it is NOT applied. Assert the verdict + that the file
  is unchanged.
- Prompt builder unit test: prompt contains the region text + the finding message + contract.

## Constraints
- The LLM only rewrites; deslop-slim NEVER applies an unverified patch. Respect the safety class /
  contract. No new deps beyond `ureq`. Do NOT touch `deslop/*.py`. Do NOT weaken verify.
- Update SPEC.md: deslop-slim is no longer deferred — document the consumer + the swappable client.

## Gate after EACH change (revert anything that breaks)
`cargo fmt --all && cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
crate layout; LlmClient + Anthropic/Recorded impls; CLI flags; the verify-gated apply; test
results (mock e2e + rejection path, no network); SPEC.md update; remaining deferred (MCP `fix`
tool, streaming, multi-provider). `jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md` each
round. If you need multiple rounds, keep going — this is a big feature; gate each round.
