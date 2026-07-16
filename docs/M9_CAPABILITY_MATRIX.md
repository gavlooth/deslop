# M9 incremental-scale capability matrix

| Capability | Complete evidence | Partial/pending behavior | Authority boundary |
|---|---|---|---|
| Persistent artifacts | Exact file revisions/grammars plus adapter, graph, recipe, model versions; checksum and identity valid | Miss, corruption, or conflict is explicit | Derived evidence only; no rewrite authority |
| Dependency invalidation | Exact changed ranges and complete reverse-dependency coverage | Missing coverage expands downstream invalidation project-wide | No stale scope/CFG/PDG/clone/metric/candidate reuse |
| Clone lookup | Persisted canonical M5 normalized buckets; graph verification remains bucket-local | Corrupt/noncanonical index rejects | Match/classification never grants rewriting |
| Parallel regions | All worker artifacts succeed and canonical commit identity validates | Deterministic first sorted error rejects the batch | Worker completion order is unobservable |
| Budgets | Every requested item fits and completes | `partial` has a continuation; oversized first item is `pending` | Partial project semantics cannot support high-authority conclusions |
| Git/CI | `--changed`, baseline ratchet, false-positive feedback, SARIF, fail-on gate | Unsupported history/incomplete analysis fails explicitly | Baselines suppress reporting only, never verification |
| Editor refresh | New overlay snapshot and generation; affected regions refresh | Stale session/document versions reject | Code actions retain revision/verifier guards |
| Shared sessions | Snapshot/session and semantic versions agree | Missing cache is an explicit miss; stale pin rejects | Restored snapshots reparse process-local syntax |
| Performance | Release report passes clean parity, one-file parse/miss, bounded fan-out, p95 floors | Failed runs are preserved but cannot close M9 | No project-scale incrementality claim without the report |

First-party integration path: CLI scan, MCP scan/proposal, evaluator, and agent proposals delegate to
`deslop-analyzer`; LSP calls the same owned analyzer over successor snapshots. Set `DESLOP_CACHE_DIR` consistently to
share artifacts and manifests. Use `DESLOP_SESSION_ID=pss1_...` only for a pinned immutable request; an editor session
that advances revisions should let each refresh publish its successor identity.
