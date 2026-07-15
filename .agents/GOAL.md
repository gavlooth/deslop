# Goal — Production canary for transformation recipes

## Outcome

Make the existing `rust-remove-unreachable-literal-statement` recipe usable on a real Rust codebase through
one guarded CLI workflow:

```text
detect -> preview -> shared work order -> apply -> verify -> rollback on failure
```

This goal is complete only when the workflow terminates in one of two evidence-backed states:

1. **Enabled:** the production canary passes every safety, verification, rollback, and real-repository gate.
2. **Disabled:** failures are retained with exact evidence and the recipe cannot write automatically.

Adding more recipe families is not a substitute for completing this delivery path.

## Execution order

### 1. Read-only CLI detection and preview

- Add `deslop recipes detect` with recipe and path filters.
- Run the existing detector from retained project analysis rather than analyzer text heuristics.
- Emit strict `TransformationCandidate` JSON and a unified diff without modifying source.
- Preserve candidate, recipe, source revision, graph eligibility, impact, expected delta, and validation metadata.
- Reject incomplete, stale, duplicate, foreign-source, or noncanonical candidates before preview.

### 2. Minimal M6.1-M6.2 delivery contract

- Define one shared versioned `WorkOrder` representation for the recipe path used by library and CLI.
- Bind target identity, recipe/candidate IDs, evidence and counter-evidence, impact, safety, patch budget,
  verification contract, and machine-readable read/write/require/invalidate sets.
- Convert one candidate into one unique work order without rebuilding authority from booleans or display text.
- Keep the legacy analyzer-region work order distinct until its explicit migration.

### 3. Guarded apply, verification, and rollback

- Recheck every exact revision guard immediately before writing.
- Apply only the candidate's declared edit and protect every other byte, API, and file.
- Run required parse and graph-delta validation, then the repository's declared build and test commands.
- Roll back exact bytes automatically if any required check fails.
- Validate the rollback revision and rerun the declared rollback checks.
- Report disk state separately from live/rebuilt state; never call an unverified edit active.

### 4. Real Rust repository canary

- Pin representative repositories and exact revisions; record licence, build/test commands, toolchain, seed,
  reference machine, cache state, and resource budget.
- Run detection read-only first and manually audit every emitted candidate and protected span/API.
- Exercise accepted patches through the complete apply/verify/rollback path.
- Record opportunities, false positives, abstentions, verification results, runtime, and every failure mode.
- Report real-repository evidence separately from the synthetic recipe-specific B2/B7 slice; do not pool them.

### 5. Production enablement gate

Automatic application remains disabled unless all are true:

- zero protected-byte, protected-span, protected-file, or protected-API violations;
- zero confirmed semantic regressions and zero verification bypasses;
- 100% required parse/build/test/graph-delta success for accepted patches;
- 100% exact rollback success for rejected or failed patches;
- actionable precision, recall, hard-negative FPR, calibration, coverage, and abstention meet the declared
  real-repository thresholds with confidence bounds;
- runtime and memory remain within the frozen resource budget;
- candidate, work-order, patch, validation, and rollback wires reject identity and payload mutation.

If any gate fails, retain the evidence, disable automatic application, and identify the exact authority or
implementation change required for a recheck.

## Validation path

Run the smallest focused contract tests first, then CLI integration tests, patch/rollback fault injection,
real-repository canaries, and finally:

```sh
cargo test --workspace --all-features
cargo build --workspace --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Negative-memory constraints

- Do not start M5.5 or add more detectors as a substitute for a usable delivery and verification path.
- Do not broaden `SafeAuto` from inert literals to declarations, calls, macros, operators, or composites without
  production-authoritative def/use, name/type, and effect evidence.
- Do not let an incomplete graph elsewhere contaminate or silently authorize a target-scoped decision.
- Do not evaluate unrelated corpus clusters in one projection when projection-wide eligibility changes their
  semantics.
- Do not infer real-repository readiness from the existing synthetic 1,000-positive/1,000-negative report.
- Do not write when any target identity, revision guard, validation requirement, or rollback prerequisite is
  missing or stale.

## Deferred until this goal terminates

- broader M5.25 dead-code forms;
- M5.5-M5.26 recipe expansion;
- additional languages;
- unattended codebase-wide application.

