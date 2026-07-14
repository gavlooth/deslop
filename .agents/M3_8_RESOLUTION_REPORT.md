# M3.8 Resolution Measurement Report

Date: 2026-07-14

Status: terminal; all focused and workspace all-feature gates pass.

## Corpus

Authority input: frozen `deslop.resolution-adversarial-gold/1` from
`tests/fixtures/resolution_m3_7_adversarial_gold.json`.

- Cases: 16/16 exact result-object matches.
- Retained paths: 36 expected, 36 actual, 36 exact multiset matches.
- Expected statuses: 7 Unique, 1 Ambiguous, 2 Unresolved, 6 Unknown, 0 Conflict.
- Supported subset: 10 Complete-coverage cases.
- Expected-Unknown subset: 6 cases. These remain in the denominator.

## Status confusion matrix

Rows are expected; columns are actual.

| Expected \\ Actual | Unique | Ambiguous | Unresolved | Unknown | Conflict | Row total |
|---|---:|---:|---:|---:|---:|---:|
| Unique | 7 | 0 | 0 | 0 | 0 | 7 |
| Ambiguous | 0 | 1 | 0 | 0 | 0 | 1 |
| Unresolved | 0 | 0 | 2 | 0 | 0 | 2 |
| Unknown | 0 | 0 | 0 | 6 | 0 | 6 |
| Conflict | 0 | 0 | 0 | 0 | 0 | 0 |
| Column total | 7 | 1 | 2 | 6 | 0 | 16 |

## Agreement and precision/recall

| Segment | Cases/status | Path TP / predicted / expected | Path precision | Path recall | Endpoint TP / predicted / expected | Endpoint precision | Endpoint recall |
|---|---:|---:|---:|---:|---:|---:|---:|
| Complete supported | 10/10 | 27 / 27 / 27 | 27/27 = 1.0 | 27/27 = 1.0 | 18 / 18 / 18 | 18/18 = 1.0 | 18/18 = 1.0 |
| Expected Unknown | 6/6 | 9 / 9 / 9 | 9/9 = 1.0 | 9/9 = 1.0 | 5 / 5 / 5 | 5/5 = 1.0 | 5/5 = 1.0 |
| All cases | 16/16 | 36 / 36 / 36 | 36/36 = 1.0 | 36/36 = 1.0 | — | — | — |

Path scoring is a multiset comparison of the complete semantic gold path object. Alternate explicit/glob
paths to one endpoint are not deduplicated. Endpoint scoring compares every retained non-null semantic
endpoint label. Opaque revision keys are not scoring labels.

## Incremental file isolation

Every incremental document is byte-for-byte equal to a clean build for the same current graph and policy.

| Scenario | Previous | Current | Reused | Rebuilt | Added refs | Removed refs | Invalidation reason counts | Status transition | Clean parity |
|---|---:|---:|---:|---:|---:|---:|---|---|---|
| Unrelated same-spelled peer addition | 5 | 6 | 5 | 0 | 1 | 0 | none | Unique → Unique | yes |
| Reachable equal-precedence addition | 1 | 1 | 0 | 1 | 0 | 0 | ReachableScopeChanged=1 | Unique → Ambiguous | yes |
| Reachable export addition | 6 | 6 | 1 | 5 | 0 | 0 | ReachableScopeChanged=5 | Unknown → Unique | yes |
| Matching module appearance | 1 | 1 | 0 | 1 | 0 | 0 | ReachableScopeChanged=1; MatchingModuleAdded=1 | Unknown → Unknown | yes |

The matching-module rebuild has two retained causes. Reason counts are evidence dimensions, not mutually
exclusive rebuilt-result counts.

## Commands and failures

Run and passed:

```text
cargo test -p deslop-parse resolution::tests::m3_8_frozen_corpus_reports_exact_confusion_precision_and_recall -- --exact --nocapture
cargo test -p deslop-parse 'resolution::tests::m3_8_' -- --nocapture
```

One first isolation assertion failed because it expected only `MatchingModuleAdded`; the measured result also
retained `ReachableScopeChanged`. The expected report was corrected to preserve both causes. No resolver code
changed.

Run and passed on the final M3.8 source state:

```text
cargo test -p deslop-parse --all-features
cargo test --workspace --all-features
cargo build --workspace --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Scope and residual risk

The 1.0 precision/recall claims apply only to the ten frozen Complete-coverage synthetic adapter cases. They
do not promote production adapters beyond their declared authority and do not claim compiler/LSP/runtime
coverage outside the separately tested M3.6 provider contract. Expected Unknown cases are measured for exact
agreement but do not authorize unique bindings or semantic transformations.

Workspace results include 131 passing `deslop-parse` tests, one designated ignored instrumentation probe,
four passing compile-fail doctests, unchanged M0/M1/M2 definition-of-done locks, and all three graph
false-resolution probes.

Signature: Codex (GPT-5), M3.8 measurement owner, terminal checkpoint, 2026-07-14.
