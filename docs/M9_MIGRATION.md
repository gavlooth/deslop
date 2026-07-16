# M9 migration notes

M9 adds new `/1` schemas and does not reinterpret older artifacts:

- `deslop.artifact-cache-key/1` and `deslop.artifact-cache-record/1`
- `deslop.project-invalidation-plan/1`
- `deslop.deterministic-graph-commit/1`
- `deslop.analysis-budget/1`
- `deslop.project-session/1`
- `deslop.m9-scale-benchmark/1`

There was no supported persistent cache before M9, so no record migration is required. Point `DESLOP_CACHE_DIR` at
an empty directory for the first run. Cache keys include all semantic versions; after a grammar, adapter, graph,
recipe, or model upgrade, old records simply stop matching and may be removed later as storage maintenance.

`ProjectAnalysis` now stores immutable `NodeKey` arrays with each `ParsedFile` and builds its global lookup index on
first key lookup. Public node ordering, identities, lookup errors, and successor re-anchoring semantics are unchanged.
Memory instrumentation reports zero lookup-index bytes until that lazy index is materialized; this reflects retained
state rather than a hypothetical eager allocation.

Analyzer projections expose the deterministic local commit id, cache hit/miss counts, and
`project_semantics_complete`. Consumers that request `scan_analysis_regions_with_cache` must merge returned reports
into the previous revision and refresh invalidated project joins before publishing a complete result.

Persistent session restore does not restore Tree-sitter trees. Expect one parse per restored source revision in a new
process. In-process LSP successors continue to use Tree-sitter incremental parsing and unchanged `ParsedFile` reuse.
