# ADR 0011: Version-complete incremental project scale

Status: accepted for M9

## Decision

Deslop persists immutable analysis artifacts under `deslop.artifact-cache-key/1`. Every address contains exact
repository/file revisions (therefore source content), stored grammar selection, and explicit adapter-set,
graph-schema, recipe, and model versions. A cache hit is valid only after strict record decoding, key identity
recomputation, and payload checksum validation. An existing key with different output is a deterministic conflict,
not a replacement.

Cache publication writes a complete private temporary file and creates the live name with a no-clobber hard link.
Readers therefore observe either no record or one complete record. Interactive analysis does not wait for a
power-loss durability sync; deployments requiring synchronous durability may sync the cache filesystem outside the
latency-critical request. Cache loss is recoverable because artifacts are derived and carry no rewrite authority.

`ProjectAnalysisUpdate` remains the syntax-revision authority. `deslop.project-invalidation-plan/1` translates exact
changes into syntax, scope, CFG, PDG, clone-bucket, metric, and candidate invalidations. Complete file-dependency
evidence permits transitive bounded fan-out. Missing dependency evidence expands downstream invalidation to every
possibly dependent file; it never authorizes stale reuse.

File-local detector work is cached per exact file/adapter/policy. Independent misses execute concurrently, while
`deslop.deterministic-graph-commit/1` sorts and validates their artifacts before one observable commit. M5's
normalized fingerprint buckets remain the only clone lookup representation; persisted indexes rebuild and compare
their canonical bucket maps and retain zero construction pair comparisons.

Bounded work uses `deslop.analysis-budget/1`. Files, nodes, input bytes, results, evidence bytes, and elapsed work
produce explicit complete, partial, or pending states plus a deterministic continuation. A first item larger than
the budget is pending, never an empty complete response. Changed-region analyzer results set
`project_semantics_complete=false` until dependency and clone joins have refreshed.

`deslop.project-session/1` stores a version-bound snapshot manifest and deduplicated source blobs in the same cache
namespace selected by `DESLOP_CACHE_DIR`. `DESLOP_SESSION_ID` pins an exact session and rejects a stale snapshot or
semantic-version mismatch. Tree-sitter trees remain process-local: restoring a session reuses portable snapshot
bytes/identity and truthfully reparses syntax.

## Consequences

- CLI scan, MCP, evaluator, agent proposal paths (through `deslop-analyzer`) and LSP workspace refresh use the same
  session/cache contracts when a cache directory is configured.
- Reused `ParsedFile` values own immutable per-file `NodeKey` arrays. Successors reuse those arrays and construct the
  global lookup index lazily, eliminating eager all-project key rehash/sort work without changing identities.
- Partial results carry no project-wide or rewrite authority. Cache artifacts and clone matches remain evidence only.
- The release benchmark must be optimized, use cold-empty and warm content-addressed cache states, and meet the
  existing p95 limit of 500 ms and 5% of cold/full time with exact clean/incremental parity.

## Rejected alternatives

- A project-wide candidate-cache key: one edit invalidated every file and defeated bounded reuse.
- Persisting Tree-sitter `Tree`: it is process-local and not a portable cache format.
- All-pairs clone comparison after persistence: M5 bucket lookup and graph verification already define authority.
- Returning truncated arrays as complete: callers could mistake absent evidence for a negative conclusion.
- Timing debug builds or discarding failed benchmark reports: neither is admissible performance evidence.
