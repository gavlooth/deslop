# M10 stable-evidence migration

M10 freezes the evidence-limited `0.1.0-evidence.1` contract. It does not reinterpret older payloads in place.

## Frozen versions

| Surface | Version |
|---|---|
| Graph wire | `deslop.graph/2` |
| Adapter capabilities | `deslop.language-adapter-capabilities/2` |
| Canonical roles | `deslop.canonical-role-projection/1` |
| Shared work order | `deslop.work-order/1` |
| Legacy finding work order | `deslop.workorder/3` |
| Work-order plan/service | `deslop.work-order-plan/1`, `deslop.work-order-service/1` |
| Recipe/candidate | `deslop.transformation-recipe/1`, `deslop.transformation-candidate/1` |
| Readability features | `deslop.readability-features/1` |
| Readability model | unshipped (`evidence_only`) |

Strict deserializers reject unknown fields, stale identities, and unsupported schema versions. Regenerate artifacts
with the matching CLI/library rather than changing a version string. Caches are keyed by source, grammar, adapter,
graph, recipe, and model versions; unmatched objects are misses and may be garbage-collected.

## Behavioral migrations

- Python `consecutive-blank-lines` now preserves two blank lines; only three or more are `SafeAuto` collapses.
- Whole-project finding proposal batching is not part of the stable surface. Clients migrate to bounded work-order
  operations and pagination.
- Readability labels/probabilities remain prohibited. Retain transparent evidence axes and unknowns.
- Only the frozen Rust unreachable-literal recipe slice carries B2/B7 `SafeAuto` numerical authority. Other recipe
  families remain review-only unless their own future frozen corpus passes.
- Old M7 verifier/report payloads cannot be promoted into a current transaction. Rebuild a current SharedWorkOrder,
  verifier plan, and exact authorization.

Compatibility is exercised by strict serde round-trips/tamper rejection, M0/M1/M6/M7/M8/M9 definition-of-done
tests, and the M10 clean-checkout gate. There is no silent alias/default-filled migration.
