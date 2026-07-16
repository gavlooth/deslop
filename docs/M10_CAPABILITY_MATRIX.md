# M10 stable-evidence capability matrix

The release disposition is `stable-evidence-limited`: ship only the rows whose authority is stated here.

| Surface | Shipped tier | Authority | Explicit boundary |
|---|---|---|---|
| Clojure | S1 syntax | Owned CST, roles, tokens, malformed provenance, adapter goldens | S2-S4 unknown unless a fact says otherwise |
| JavaScript | S1 syntax | Distinct grammar/dialect, owned CST and goldens | TypeScript/TSX facts are not borrowed |
| Julia | S1 syntax | Owned CST, roles/tokens, generated/opaque policy | External analyzer optional and revision-bound |
| Python | S1 syntax | Owned CST, decorators/async regions, goldens | Two module blank lines preserved; semantic tiers explicit |
| Rust | S1 plus evidenced graph facts/recipes | M3/M4 graph gold; M5 contracts; B2/B7 unreachable-literal slice | Other recipe families review-only; nine dogfood files abstained on budget |
| TypeScript | S1 syntax | Dedicated TypeScript grammar/roles/tokens | TSX is a distinct dialect; deeper tiers explicit |
| Graph-grounded LLM work orders | Bounded protocol | M6 240 paired tasks, +44.17pp accepted-patch rate | Model output never grants write authority |
| Readability | Transparent evidence axes | M8 300 pairs / 1,727 comprehension observations | No readability label/model/probability ships |
| Incremental analysis | 480-file Rust/Python/TypeScript benchmark | Deterministic M9 cache/invalidation evidence | No one-million-LOC claim |
| External repositories | Read-only scans on 18 exact revisions | 1,198 reports / 198,255 lines | Refreshed warm-cache commands: 4 pass, 11 fail, 3 unavailable; environment qualified |
| Whole-project finding proposals | Unshipped | Measured M10 timeout | Use bounded index/triage/explain/plan |
| Apply/undo | M7 transaction authority | Exact bytes, selected checks, graph delta, durable journal | Missing/conflicting/stale evidence blocks |

Unknown/partial/unsupported/failed facts remain visible and block any consumer requiring them. A review label does
not upgrade authority. The machine-readable matrix and gate decisions are in
`.agents/benchmarks/m10_release_evidence_v1.json`.
