# TASK 9/queue — non-Rust mutation probes (honest scope: add what has real tooling, document the rest)

`MutationRegistry` only has `RustCargoMutantsProbe`. Add non-Rust `MutationProbe`s WHERE a viable
upstream mutation tool exists; where none does, DOCUMENT the blocker — do NOT force a bad adapter.
Start with `jj new` (separate change on top of mtxlzmys).

## Pattern to follow (deslop-verify/src/lib.rs)
- `trait MutationProbe { name; supports(&SourceFile) -> bool; run(...) }` (~line 201).
- `enum MutationConfig { Disabled, Auto, OutcomesFile(PathBuf) }` (~174) — `OutcomesFile` is the
  recorded-fixture mode for deterministic tests.
- `enum MutationStatus { Survived, NoSurvivor, Unknown }`.
- `RustCargoMutantsProbe` (~869): Auto runs `cargo mutants`; OutcomesFile parses a recorded report;
  maps MISSED/surviving mutants by path+line to work-order regions; survivor downgrades the verdict.
- `MutationRegistry` (~828) holds the probe list.

## Do
1. **Python — add a probe** (this language HAS real tooling). Choose the tool with the cleanest
   machine-readable output — prefer **cosmic-ray** (structured/JSON report) over mutmut if its output
   is easier to parse deterministically; justify the choice. Implement `PythonMutationProbe`:
   - `supports` → Python sources.
   - Auto mode: run the tool, parse surviving mutants, map by path+line to regions; degrade
     gracefully (MutationStatus::Unknown + a notice) when the tool is absent — mirror the Rust probe.
   - OutcomesFile mode: parse a RECORDED report fixture (for tests, no tool needed).
   - Register it in `MutationRegistry` alongside the Rust probe.
2. **Clojure & Julia — investigate, then document honestly.** Check (briefly, e.g. via Context7 /
   crates-or-package search) whether a maintained, source-mappable mutation tool exists:
   - Clojure: JVM-bytecode mutators (PITest) don't map cleanly back to Clojure source regions; if you
     find no source-level tool, record it as a documented blocker (which tools exist, why they don't
     fit region mapping).
   - Julia: if no maintained mutation-testing tool exists, record it as a documented blocker.
   Put these in SPEC.md (mutation-tier coverage) and the report — do NOT add a non-functional probe.

## Tests (deterministic, NO tool/network)
- Python OutcomesFile fixture: a recorded report with a surviving (MISSED) mutant in a covered
  region → `MutationStatus::Survived` and the verdict downgrades (mirror the existing cargo-mutants
  recorded-fixture test). A "no survivor" fixture → not downgraded.
- Absent-tool degrade for the Python Auto path (tool missing → Unknown + notice, verdict unchanged) —
  mirror the Rust absent-tool test.
- Keep existing mutation tests green.

## Constraints / gate
Registry-driven, no central `match Lang`. MCP stays network-free (default build). Do NOT touch
`deslop/*.py` source files of THIS repo (you may add test FIXTURE files under tests/). Gate after
each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
the Python probe + tool choice (with why); the recorded-fixture + absent-tool tests; the
Clojure/Julia blocker writeups (tools considered, why deferred); SPEC.md mutation-tier update.
`jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start queued items 10-13.
