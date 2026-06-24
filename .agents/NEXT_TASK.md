# TASK 12/queue — streaming progress for slim/fix (per-workorder, to stderr)

`run_slim` is silent until it returns the final `SlimReport`. On long runs (many regions, an LLM
call each) the user sees nothing. Add incremental progress to STDERR while keeping STDOUT (the final
report) byte-for-byte unchanged so pipes still work. Start with `jj new` (separate change on top of
qpywotro).

## Design
- Add a `SlimProgress` event enum, e.g.: `Started { work_orders: usize }`, `Rewriting { index,
  total, path, start_line, end_line }`, `Characterizing { workorder_id }` (only when `--characterize`),
  `Verified { workorder_id, verdict }`, `Outcome { workorder_id, applied|held|rejected }`,
  `Finished { applied, held, rejected }`.
- Thread a progress sink into the slim run. Prefer a single clean API: change `run_slim` to take a
  progress callback `&mut dyn FnMut(SlimProgress)` (update the few callers: CLI fix, MCP auto, tests)
  OR add `run_slim_with_progress` and make `run_slim` delegate with a no-op sink — pick one, justify.
  Emit events at the natural points in the existing loop (don't restructure the logic).
- **CLI**: print progress lines to **stderr** as events arrive (human-readable, e.g.
  `[2/7] rewriting src/foo.rs:12-28 … applied`). STDOUT still emits only the final report (JSON/text).
  Default on when stderr is a TTY; add `--quiet` to suppress. When stderr is NOT a tty (piped/CI),
  either stay silent or emit plain one-line-per-event — pick the less noisy default and document it.
- **MCP**: MCP returns a single JSON result (no streaming channel) — do NOT emit progress there; the
  sink is a no-op for the MCP path. (Note MCP streaming as deferred.)

## Tests (deterministic, NO network)
- A recording sink (collect events into a Vec) over a mock `run_slim` run asserts the event SEQUENCE:
  `Started` → one `Rewriting` per work order → `Verified`/`Outcome` per patch → `Finished` with the
  right tallies. Use RecordedClient/ScriptedClient.
- STDOUT unchanged: the final report content is identical with progress on vs `--quiet` (assert the
  report value/serialization is unaffected by the sink).
- Existing slim/CLI/MCP tests stay green (the no-op/default sink must not change outcomes).

## Constraints / gate
Progress NEVER goes to stdout. No logic/verdict change. API key never logged. MCP default build stays
network-free. Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
(plus `cargo test -p deslop-mcp --features slim-llm`.)

## Report
the `SlimProgress` events; the sink API choice (and why); stderr-only CLI rendering + `--quiet` +
non-tty default; the event-sequence test; stdout-unchanged proof; MCP no-op note. `jj describe -m
"<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start queued item 13.
