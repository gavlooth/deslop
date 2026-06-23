# TASK 2/queue — Characterization-test generation loop (close the graded-removability loop)

Today slim SKIPS `NeedsCharacterizationTest` work orders (lib.rs ~156). The prover emits them for
`CoverageUnknown`/`UntestedRisky` rewrites, and `verify_characterization_tests` accepts a test only
if it passes on CURRENT code — but nothing GENERATES the test. Close the loop: generate a
characterization test that pins current behavior, verify it, then re-verify the rewrite WITH that
test so a risky removal can become safe. Start with `jj new` (separate change on top of rqmuzkxm).

## Real API to use (don't reinvent)
- `deslop_verify::characterization_work_orders_for_patches(&[Patch], &VerifyOptions) -> Vec<WorkOrder>`
  (returns characterize work orders for patches whose verdict `needs_characterization_test()`).
- `deslop_verify::verify_characterization_tests(&[CharacterizationTest], ...) -> report` (accepts
  only tests that PASS on current unmodified code).
- `VerifyOptions.characterization_tests: Vec<CharacterizationTest>` → feeds `run_characterization_gate`
  during `verify_patches`/`apply_patches` (re-verify with accepted tests can upgrade the verdict).
- `deslop_protocol::CharacterizationTest { schema, workorder_id, region_fingerprint, test_path,
  test_text, by }`, `characterization_work_order_for(&WorkOrder)`.

## Build (in deslop-slim)
1. **Prompt kind**: add a `kind` to `SlimPrompt` (`Rewrite` | `Characterization`) so clients/tests
   can distinguish. Add `build_characterization_prompt(&WorkOrder)` — instruct the model to write a
   test that PINS the CURRENT behavior of the region (asserts current outputs), returning only the
   test code. Keep `build_prompt` (rewrite) unchanged in behavior.
2. **Generation step** in `run_slim`, gated by a new `characterize: bool` option (CLI
   `deslop fix --characterize`, default off — it writes test files + runs check_cmd):
   - After the initial `verify_patches`, compute `characterization_work_orders_for_patches(&patches,
     &verify_options)`.
   - For each, `build_characterization_prompt` → `client.rewrite` → construct `CharacterizationTest`
     { schema:"deslop.characterization-test/1", workorder_id, region_fingerprint (=
     workorder_region_fingerprint), test_path (derive a deterministic path under root, e.g.
     `<root>/deslop_characterization/<workorder_id>.rs` or per work order), test_text (LLM output,
     fence-stripped), by:"deslop-slim/<model>" }.
   - `verify_characterization_tests(&tests, ...)` with the same check_cmd; keep only ACCEPTED.
   - Set `verify_options.characterization_tests = accepted`, then re-run `verify_patches` (and
     `apply_patches` if `--apply`) so the gate re-evaluates the rewrites WITH the tests.
3. **Report** (extend SlimReport): characterization attempts, accepted vs rejected (with reason),
   and which rewrites were upgraded (verdict before→after) / applied because of an accepted test.
   Patches still unproven after characterization stay held (existing behavior).

## Tests (deterministic, NO network) — model them on the existing deslop-verify characterization tests
- Add a test-only client that serves DIFFERENT responses by `SlimPrompt.kind` (rewrite vs
  characterization) — e.g. extend `RecordedClient` or add `ScriptedClient`.
- Accept path: a needs-characterization rewrite + `--characterize`, characterization test that
  PASSES on current code (use the same recorded-fixture / check_cmd approach the verify crate's
  characterization tests use) → test accepted → rewrite verdict upgraded → applied (with `--apply`).
- Reject path: characterization test that FAILS on current code → rejected → rewrite NOT upgraded,
  remains held; file unchanged.
- Keep all existing slim tests green (recorded e2e, gating, rejection, openai parser).

## Constraints / gate
Do NOT weaken verify or change the rewrite gating from Task earlier (default --apply still
Removable-only; characterization is how CoverageUnknown becomes safe). Do NOT touch the MCP path,
do NOT pull ureq into MCP, do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
the loop wiring; `--characterize` flag; accept+reject test outcomes; verdict before→after on the
accept path; SPEC.md update (characterization loop). `jj describe -m "<summary>"`. Touch
`.agents/HEARTBEAT.md`. Do NOT start queued items 3-6.
