# M10 stable-evidence release report

Release disposition: `stable-evidence-limited` (`0.1.0-evidence.1`). The product ships only the capability tiers
in [M10_CAPABILITY_MATRIX.md](M10_CAPABILITY_MATRIX.md); every other numerical bar is explicitly downgraded.

## Frozen evidence

| Area | Result |
|---|---|
| Canonical compatibility | 600 deterministic programs, 100 each Clojure/JavaScript/Julia/Python/Rust/TypeScript; 120 malformed/opaque |
| Dogfood | 189 files / 139,694 lines; 182 Complete + 7 Partial reports; 1,536 findings; zero production `SafeAuto` |
| Dogfood dispositions | 0 accepted, 5 intentional-fixture rejected, 1,538 unsafe/unverified, 0 naturally stale |
| Dogfood recipes | 138 isolated files; 7 candidates; 9 timeout abstentions; 245.550 s; 1,386,401,792-byte peak RSS |
| Dogfood work orders | 7 transformation orders / 7 unique IDs / 0 duplicates; 4 edges; 2 waves; no blocked group |
| External projects | 18 exact revisions, three per language/size grid; 1,198 reports / 198,255 lines / 8,322 findings |
| External commands | Refreshed warm-cache run: 4 passed, 11 failed, 3 tool-unavailable; prior run had 2 Julia timeouts |
| LLM paired benchmark | 240 tasks; graph arm accepted-patch +44.17pp; paired 95% CI +35.24 to +53.09pp; 0 graph regressions/out-of-scope edits; 100% unsafe abstention |
| Readability | 300 pairs / 1,727 comprehension samples; challenger 0.570 accuracy, lower 95% 0.5134, ECE 0.0764; `evidence_only` |
| Recipe B2/B7 slice | Rust unreachable-literal: 1,000 opportunities + 1,000 hard negatives; precision/recall lower 95% 0.98115; FPR upper 95% 0.01885; ECE 0 |
| Incremental scale | Three 480-file projects; warm/full p95 2.26–3.05%; 24.909–35.406 ms; deterministic and bounded fan-out |

## Worst slices and coverage

- Readability worst/holdout gates fail and labels are unshipped; results are not pooled into a success claim.
- Transformation precision is numerically demonstrated for one Rust recipe slice only; other languages/families
  remain review-only.
- Nine dogfood recipe partitions timed out and whole-project finding proposals missed 15 minutes. Both are
  unshipped, with bounded protocol operations retained.
- External build/test outcomes are environment-qualified; read-only scans succeeded for all pins.
- One-million-LOC cold/RSS performance, independent 600-case macro F1, human patch preference, and artifact signing
  are not claimed.

## Reproduction

Use the release-profile M10 canonical/external/dogfood binaries, the existing M6/M8/M9 definition-of-done tests,
and the clean-checkout commands recorded in `.agents/benchmarks/m10_gate_report_v1.json`. The strict join is
`.agents/benchmarks/m10_release_evidence_v1.json`; `m10-release verify` recomputes artifact digests, schemas,
numerical summary, documentation ledger, version map, capability matrix, exceptions, and all 32 M10/G/B decisions.

Prior-release delta: M9 established bounded incremental analysis. M10 adds independent pins, dogfood dispositions,
release-documentation/exception/version ledgers, and explicit unshipping of unsupported performance, model, and
proposal claims. No failed slice is hidden by macro pooling.
