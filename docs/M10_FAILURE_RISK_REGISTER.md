# M10 failure taxonomy and known-risk register

## Failure taxonomy

| Class | Examples | Required behavior |
|---|---|---|
| Input authority | malformed/partial/unsupported syntax, macro/dynamic/opaque region | retain provenance; block dependent recipe |
| Semantic authority | ambiguous/unresolved/conflicting name or graph fact | never report unique/proven; widen or block |
| Identity/staleness | changed bytes, stale handle/cursor/plan/cache/artifact | reject; regenerate from current revision |
| Resource | timeout, output/file/memory/query budget, unavailable tool | structured failure/abstention; never truncate silently |
| Environment/policy | missing sandbox/runtime/dependency, network/filesystem mismatch | fail closed or environment-qualify evidence |
| Verification | parse/build/lint/type/test/behavior/graph-delta mismatch | reject; rollback; record counterexample/demotion |
| Transaction | crash, partial write, corrupt journal, undo drift | deterministic recovery or operator-blocking error |
| Evaluation | missing slice, weak CI, failed calibration, no trust root | downgrade/unship claim; do not pool it away |

## Terminal exceptions

1. Python two-blank-line `SafeAuto` false positives are closed by the language-aware policy and tests.
2. Whole-project finding proposals exceeded 15 minutes and are unshipped; bounded protocol operations remain.
3. Nine large dogfood Rust files exceeded 30 seconds; those partitions grant no candidate authority.
4. Refreshed external commands yielded 4 passes, 11 failures, and 3 unavailable Leiningen results with ambient
   dependency caches; the prior run retained 2 Julia timeouts. This qualifies the environment; it neither
   invalidates scans nor proves behavior.
5. Readability labels are unshipped because lower-bound, calibration, holdout, population, and axis gates failed.
6. B2/B7 numerical precision applies only to the frozen Rust unreachable-literal recipe slice.
7. The 600-case canonical corpus is a content-addressed compatibility corpus, not an independently annotated
   macro-F1 claim. Adapter/M3/M4 gold fixtures remain the shipped tier authority.
8. Human preference is unshipped. External findings are `review-pending`, not synthetic approvals.
9. B11 one-million-LOC performance is unmeasured; only the M9 480-file projects are claimed.
10. Artifacts have BLAKE3 content identity but no configured cryptographic signer/trust root.

## Risk handling

Each exception is closed, downgraded, or environment-qualified in `deslop.release-evidence/1`, with a concrete
recheck condition. Missing evidence is never converted into a measured failure or a pass. Counterexamples update
negative memory and demote the affected authority immediately.
