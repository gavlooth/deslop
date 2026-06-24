# TASK 15 — Mutation parallelism (concurrent-state-evolution: edge workers + one serialized drain loop)

The native mutation engine (task 14, `TreeSitterMutationProbe` in deslop-verify + `deslop-mutate`)
scores mutants SERIALLY. Parallelize it using the concurrent-state-evolution pattern. This is a pure
optimization — it MUST NOT change which mutants survive. Start with `jj new` (on top of lmmlzykp).

## Pattern (apply exactly this shape)
Mutation is parallel only in the expensive side-effecting part (apply mutant → run check-cmd). Keep
ALL state evolution serialized:
- A bounded **worker pool** (`std::thread::scope`, no rayon/tokio) runs mutants. Each worker is PURE
  "do": it owns an isolated workspace, applies one mutant, runs check-cmd with the existing timeout,
  and pushes a message onto a channel (`std::sync::mpsc`) — `MutantOutcome { id, status }` or
  `MutantError { id, reason }`. A worker NEVER touches shared aggregate state.
- A SINGLE **drain loop** consumes channel messages and owns ALL aggregate state evolution: tally
  killed/survived/unviable/timeout, compute the region `MutationStatus`, emit progress, finish when
  every mutant is accounted for. (This is the trampoline: results are named actions; the loop bounces.)

## Hard rules (from the pattern's caveat — see memory: concurrent-state-evolution-pattern)
- **Catch at the worker boundary.** Wrap each worker's work in `std::panic::catch_unwind` (and map
  any error/spawn failure) → send a `MutantError` action. A panic/throw on a worker thread must NEVER
  escape; it becomes an action the drain loop handles. (The pattern explicitly warns spawned-thread
  exceptions sail past a normal try/catch.)
- **Determinism by construction.** Aggregation happens only in the serialized drain loop, so the
  result is order-independent. Survivors/killed/verdict MUST be identical to the serial engine.

## Isolation & concurrency
- Each worker gets its OWN temp workspace copy (don't share the temp dir). For compiled langs
  (Rust/Julia) give each worker a separate build dir (e.g. per-worker `CARGO_TARGET_DIR`) to avoid
  build-lock contention. Pin each mutant's suite single-threaded (e.g. `--test-threads=1` where
  applicable) so total concurrency = the worker count, not workers × suite-threads.
- Concurrency bound configurable: default `std::thread::available_parallelism()` (std, no dep), plus a
  config/flag override. No new dependencies (reuse task-14's timeout mechanism).

## Tests (deterministic, NO real toolchains)
- **parallel == serial**: same fixture + content-keyed check-cmd, run the probe serial vs parallel →
  assert identical MutationAssessment (same survivors/killed/verdict). This is the core guarantee.
- **worker-boundary error capture**: a worker whose work panics / check-cmd fails to spawn →
  surfaces as a `MutantError` action, the run completes, no process crash.
- **concurrency is bounded**: a sink/counter asserts in-flight workers never exceed the configured max.
- Keep all task-14 mutation tests green.

## Constraints / gate
No behavior change to verdicts. Registry-driven; MCP default build stays network-free. No new deps.
Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
the worker-pool + drain-loop structure; the channel message type; worker-boundary catch_unwind;
the parallel==serial test; isolation (per-worker workspace/build dir); concurrency bound + override;
deferred (equivalent-mutant pruning, cross-file scheduling). `jj describe -m "<summary>"`. Touch
`.agents/HEARTBEAT.md`. This is the LAST mutation task — note it in the report.
