# TASK 14 — Native tree-sitter mutation engine (language-agnostic; supersedes Cosmic-Ray-only)

Build mutation testing from deslop's own primitives: tree-sitter (generate mutants) + check-cmd
(score them) + coverage (gate them). This works for Rust/Clojure/Julia/Python uniformly and unblocks
the Clojure/Julia mutation that had no external tool. Big feature — MULTIPLE ROUNDS expected; gate
each round. Start with `jj new` (separate change on top of xumlpqvs).

## Why this is possible
Mutation = (1) syntactic mutant generation — a CST job tree-sitter does well; (2) run the tests, a
surviving (still-passing) mutant = a test gap — which deslop already does via `--check-cmd` +
temp-copy patching (`run_check_cmd_on_temp_copy`, `selected_check_cmd`). Combine them.

## Existing surface to build on
- `parse_tree(lang, text) -> Option<Tree>` (deslop-parse) for the CST.
- `MutationProbe { assess(&mut self, MutationRequest{ root, source, work_order }) -> MutationAssessment }`,
  `MutationStatus { Survived, NoSurvivor, Unknown }`, `MutationConfig`, `MutationRegistry` (deslop-verify).
  A surviving mutant → `MutationStatus::Survived` → already downgrades the removal verdict (keep that).
- `CoverageStatus { Covered, Uncovered, Unknown }` + the CoverageProvider tier (tasks 3/10).
- check-cmd resolution: `selected_check_cmd(options, work_order)` (options.check_cmd ?? contract.check_cmd).

## P1 — Mutant generation engine (PURE CST; fully unit-testable)
A new module/crate (prefer `deslop-mutate`, depends on deslop-parse + deslop-core — keep it pure).
- A `MutationOperator` abstraction + a portable operator set over tree-sitter node kinds:
  - relational swaps (`<`↔`<=`, `>`↔`>=`, `==`↔`!=`), arithmetic (`+`↔`-`, `*`↔`/`),
    logical (`&&`↔`||`), boolean-literal flip (`true`↔`false`), condition negation.
  - Infix languages (Rust/Julia/Python): mutate `binary_expression` operator tokens. Clojure: the
    operator is a symbol in call position `(< a b)` — mutate that symbol node. Operator applicability
    is per-language (reuse the LangPack notion; no central `match Lang` in the runtime path).
- `fn generate_mutants(source: &SourceFile, restrict_lines: Option<&BTreeSet<usize>>) ->
  Vec<Mutant { line, byte_span, operator, original, mutated, mutated_source }>`. Deterministic.
- Tests: per language (Rust/Clojure/Julia/Python) a fixture yields the EXACT expected mutants
  (operator + line + mutated text). This is the core deliverable and must be airtight.

## P2 — TreeSitterMutationProbe (execution + scoring)
Implement `MutationProbe` in deslop-verify using the P1 engine:
- For the work-order region: generate mutants (restricted to the region's lines).
- For each mutant: write the mutated source to a temp copy (mirror `run_check_cmd_on_temp_copy`), run
  the resolved check-cmd, classify:
  - check-cmd FAILS (non-zero) → mutant KILLED.
  - check-cmd PASSES (zero) → mutant SURVIVED (test gap).
  - build/parse failure → UNVIABLE (excluded from the score; don't count as survived).
- Region status = `Survived` if ANY mutant survives, else `NoSurvivor`; `Unknown` if no viable mutants
  / no check-cmd. Keep the verdict-downgrade contract.
- Register in `MutationRegistry`. Make the native probe the DEFAULT engine for all supported languages.
  KEEP the recorded `OutcomesFile` mode and the cosmic-ray probe as opt-in alternatives (don't delete
  working code; native becomes the default `Auto`).
- The probe needs the resolved check-cmd: thread it into `MutationRequest` (add a field) or have the
  registry pass it — minimal plumbing, justify.

## P3 — timeout + coverage-gating (perf + correctness)
- **Timeout**: a mutant can hang (flipped loop condition). Add a per-mutant timeout to the check-cmd
  run (use `wait_timeout` — a small justified dep — or a thread+kill). Timeout → treat as KILLED
  (behavior changed enough to hang). Make the timeout configurable (default a sane value).
- **Coverage-gating**: only mutate COVERED lines (uncovered code trivially survives → skip, big perf
  win + avoids false "survived"). Thread the coverage assessment into the probe (extend
  `MutationRequest` or compute via the CoverageProvider when coverage is enabled). With coverage
  disabled, mutate all region lines (document the cost).

## Tests (deterministic, NO real test suite / NO language toolchains)
- P1 operator generation per language (exact mutant sets).
- P2 scoring: a CONTENT-KEYED check-cmd (like the characterization PIN trick: `grep` the temp file)
  so a specific mutant makes check-cmd fail (KILLED) vs pass (SURVIVED) — assert classification with
  no real tests. Plus a `Survived` mutant downgrades the verdict (reuse the existing pattern).
- P3 timeout: a check-cmd that sleeps + a short timeout → classified KILLED-by-timeout. Coverage-gating:
  uncovered lines are not mutated (assert the mutant set is restricted).

## Constraints / gate
Registry-driven, no central `match Lang` in the runtime path. MCP default build stays network-free.
Only new dep allowed: `wait_timeout` (for P3). Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
the operator set + per-language applicability; generation tests; the probe scoring + verdict
downgrade; timeout + coverage-gating; how it relates to the cosmic-ray probe (default vs opt-in);
deferred (equivalent-mutant pruning, parallelism, unviable-vs-killed nuance). `jj describe`. Touch
`.agents/HEARTBEAT.md`. If you need multiple rounds, keep going; this is a large feature.
