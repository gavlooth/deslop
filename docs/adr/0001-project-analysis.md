# ADR 0001: One revision-bound `ProjectAnalysis`

- Status: Accepted
- Date: 2026-07-13
- Owners: deslop maintainers
- Roadmap: M1.1; governs M1.2-M1.11

## Context

Deslop currently passes `SourceFile` values to independent analyzer, metrics, graph, protocol,
mutation, and LSP paths. Those paths can reread a file and call `parse_source` independently. Some
rules parse independently, graph provenance and graph extraction parse separately, proposal
construction rereads analyzed files, and metrics can parse once to find regions and again for every
region. Besides wasted work, this creates correctness hazards:

- consumers may observe different bytes for what appears to be one scan;
- grammar selection, parse diagnostics, and partial-analysis policy can diverge by consumer;
- borrowed Tree-sitter nodes cannot be retained safely as shared identities;
- findings, metrics, graph facts, and work orders lack one common revision owner;
- an edit can leave a mixture of fresh local facts and stale project-wide facts;
- line/byte locations are being asked to serve as both identity and write authorization.

M0 repaired the immediate public contracts. In particular, partial parses do not authorize
rewrites, `NeverAuto` evidence is report-only and absorbing over overlapping rewrite regions,
proposal reconstruction persists its dependency context, and exact `RevisionGuard` values protect
writes. M1 must replace repeated parsing without weakening any of those contracts.

## Decision

The public analysis substrate is an immutable, revision-bound `ProjectAnalysis` over one immutable
`ProjectSnapshot`. A command, request, evaluator case, or LSP workspace-overlay generation builds or
acquires one analysis and passes a shared reference to every consumer. Consumers may derive
different projections, but every projection carries the same analysis identity and may not reread
or reparse its inputs.

The foundational source and syntax types live in `deslop-parse`, which is already below analyzer,
metrics, graph, protocol, verifier, and LSP in the dependency graph. M1 will not add a new umbrella
crate merely to rename this layer. Higher-level projections remain in their present crates and are
tagged with the originating analysis identity; putting them as concrete fields in the foundational
crate would create dependency cycles.

Conceptually:

```text
explicit roots/scope + optional in-memory overlays
                    |
                    v
        SourceStore / ProjectSnapshot
      (read once, immutable, content addressed)
                    |
                    v
             ProjectAnalysis
   (one parse owner per file revision, private Trees,
     owned nodes/tokens/trivia, parse provenance)
          /          |          |          \
         v           v          v           v
     analyzer      metrics     graph      evaluator
         \           |          /           /
          +----- revision-bound projections ------+
                              |
                              v
                  protocol / CLI / MCP / LSP / slim
                              |
                              v
                  verifier + exact RevisionGuard
```

### Identity domains

The following identities are separate types and are never substituted for one another:

| Identity | Meaning | Lifetime | May authorize a write? |
| --- | --- | --- | --- |
| `SourceRevision` | Domain-separated BLAKE3 of the exact raw bytes | Any path containing those bytes | No |
| `FileRevisionKey` | Repository, normalized path, source revision, and stored grammar selection | One path revision under one grammar | No |
| `ProjectSnapshotId` | Repository/scope and the sorted set of all input path revisions | One immutable input set | No |
| `ProjectAnalysisId` | Snapshot plus parser, grammar-selection, and raw-arena schema identities | One immutable syntax analysis | No |
| `ProjectionId` | Analysis plus projection schema, effective policy/config, capabilities, and budgets | One derived result generation | No |
| `NodeId` | Owner-tagged dense arena index | One `ProjectAnalysis` only | No; never serialized |
| `NodeKey` | Serialized, revision-bound node identity containing its full file revision key, structural anchor/span, and collision ordinal | One file revision | No |
| `RegionKey` | Serialized identity for an owned syntax, line, or virtual rewrite region, optionally anchored to a `NodeKey` | One file revision | No |
| baseline/finding fingerprint | Best-effort comparison across revisions | May survive an edit probabilistically | Never |
| `RevisionGuard` | Exact target path, range, and bytes for compare-before-write | Until any guarded target byte/boundary changes | Yes, after all other policy gates |

This decision supersedes any older wording that calls `NodeKey` a cross-revision durable
fingerprint. A `NodeKey` is durable for serialization but bound to its exact revision. Cross-revision
matching belongs only to the baseline/finding fingerprint and cannot grant edit authority.

### Source ownership and snapshot construction

`SourceStore` content-addresses exact raw bytes by `SourceRevision`. A `ProjectSnapshot` owns shared
references to those bytes and a sorted map from `(RepositoryId, normalized repository-relative
path)` to snapshot entries. An entry is either a parseable source or another analysis input such as
a build/config file consulted as a code fact. All inputs that can change a project-level result or
proposal context must be represented even when they are not parsed as source code.

Root and repository identity resolution is centralized in `ProjectSnapshotBuilder`; CLI, MCP, slim,
LSP, evaluator, and protocol code may not infer their own roots. An explicit root wins and every
scope entry must resolve beneath it. Otherwise the builder selects the one shared `.jj`/`.git`
repository ancestor; inputs spanning repositories require an explicit root. With no repository
marker it uses the lowest common ancestor of the canonical input paths (a single file uses its
parent). Empty/default scope resolves from the current directory before this algorithm runs.

`RepositoryId` is an explicit logical namespace when configured. Otherwise a VCS-backed project
uses a domain-separated digest of its repository identity (normalized primary remote when present
plus root commit set); an unversioned project uses an opaque digest of its canonical root and is
therefore deliberately path-bound. Absolute paths never appear in serialized IDs. Moving a
path-bound project expires its handles; supplying the same explicit `RepositoryId` makes equivalent
checkouts portable and deterministic.

Snapshot construction follows these rules:

1. Resolve one repository authority boundary and normalize requested scope beneath it.
2. Discover the complete input set deterministically, including in-memory LSP overlays. Relative,
   absolute, overlapping-scope, and symlink aliases of one physical file collapse to one logical
   entry using a canonical root-relative display path; identical bytes at distinct logical paths
   remain distinct entries. An alias resolving outside the repository boundary is rejected.
3. Read each disk input once as raw bytes and compute its revision before UTF-8 decoding.
4. Reject invalid UTF-8 for syntax analysis with explicit failed provenance; never parse lossy text.
5. Record the exact requested scope and sorted discovered input set in `ProjectSnapshotId`.
6. Select a grammar variant from path, language, dialect, and declared grammar version before parsing.
7. Create exactly one parse owner for each supported `FileRevisionKey`. A cold build invokes the
   parser once; a validated cache hit supplies that owner's artifact without another invocation.

Grammar selection is atomic and authoritative. Snapshot construction derives exactly one
`GrammarSelection` containing language family, dialect/variant, grammar-selector identity/version,
exact grammar artifact package version/digest, and a versioned parser implementation/build key from
the normalized logical path and registered selector. It is part of the file and analysis keys.
Consumers use that stored selection and may not supply a separate language or reselect a pack from
path. Semantic adapter/query/role versions are projection inputs, not parse keys. A disagreement is a
construction error, never a fallback. Public constructors that permit a caller-supplied language to
disagree with path grammar, and pathless public parsing bypasses, are internalized or removed by
M1.DoD.

Identical bytes may share storage. A future cache may also reuse a parse artifact for identical
bytes and grammar, but each path revision still has one explicit parse owner and distinct path-bound
node identities. Rename or copy therefore expires external node handles even if an internal parse
artifact can be reused.

The canonical repository root is runtime authority, not a portable wire path. Serialized contexts
use normalized root-relative paths plus a repository identity; they do not expose or trust an
absolute path supplied by another machine.

### Parse and arena ownership

Each analyzed file owns:

- its `FileRevisionKey`, grammar provenance, source reference, and line index;
- one private Tree-sitter `Tree` when a grammar produced a tree;
- parse diagnostics and complete/partial/unsupported/failed provenance;
- an owned arena containing raw kind, field, byte/point/line span, parent, ordered children,
  named/error/missing flags, token/trivia ownership, source slice coordinates, and grammar
  provenance;
- deterministic containment and smallest-exclusive-region indices added during M1.

Every non-root syntax node has exactly one parent; parent/ordered-child/field relationships are
reciprocal and preserve grammar order. Every leaf token and trivia byte has exactly one smallest
exclusive owner. Derived/virtual nodes link to syntax ownership rather than stealing it, and nested
callables reset metric ownership so their evidence is never double counted by an outer callable.

Tree-sitter `Node<'tree>`, `TreeCursor`, and query captures are implementation-local borrows. They
may be used inside construction or query callbacks, but no public consumer stores or returns them.
Public query results contain owned values, `NodeId`s guarded by a borrowed `ProjectAnalysis`, or
revision-bound `NodeKey`s. The private `Tree` may be retained to execute grammar queries; its
borrowed handles never cross the API boundary.

`ProjectAnalysis` is immutable after publication and is shared as `Arc<ProjectAnalysis>`. It owns
its `Arc<ProjectSnapshot>` and all per-file parse owners. There is no hidden global current analysis
and no consumer-owned parser cache. An optional cache is an explicit dependency of the builder and
cannot change observable results.

An illustrative API, not a promise of exact Rust spelling, is:

```rust
pub struct ProjectSnapshot {
    id: ProjectSnapshotId,
    root: RepositoryRoot,
    scope: Vec<ScopeEntry>,
    entries: BTreeMap<RepoPath, SnapshotEntry>,
}

pub struct ProjectAnalysis {
    id: ProjectAnalysisId,
    snapshot: Arc<ProjectSnapshot>,
    files: BTreeMap<RepoPath, Arc<ParsedFile>>,
}

pub struct ParsedFile {
    key: FileRevisionKey,
    source: Arc<[u8]>,
    text: Option<Arc<str>>, // None records invalid UTF-8 with failed provenance
    selection: GrammarSelection,
    tree: Option<tree_sitter::Tree>, // private outside deslop-parse
    arena: SyntaxArena,
    provenance: AnalysisProvenance,
}

impl ProjectAnalysis {
    pub fn file(&self, path: &RepoPath) -> Option<FileView<'_>>;
    pub fn node(&self, id: NodeId) -> Result<Option<NodeView<'_>>, NodeLookupError>;
    pub fn captures(
        &self,
        query: QueryId,
        within: NodeId,
    ) -> Result<Vec<OwnedCapture>, NodeLookupError>;
}
```

`NodeId` is an opaque, non-Serde pair of a process-local analysis-instance tag and a project-global
dense index. Indices are assigned after files are ordered and each file owns a deterministic
contiguous range. Every lookup validates the instance tag and returns a structured wrong-snapshot
error instead of silently addressing the same index in another analysis. The dense index order,
capture order, paths, diagnostics, and projection output use stable sorting independent of worker
scheduling; only revision-bound `NodeKey`, never `NodeId`, appears on the wire.

`NodeKey/1` uses the raw grammar kind, field path, structural anchor/span, and collision ordinal.
Canonical roles are not available until M2. When roles become part of identity, that change ships as
an explicit `NodeKey/2` schema and expires `NodeKey/1`; it is not silently folded into the old key.
Rewrite targets that are whole-line, token-run, or other derived regions use a revision-bound
`RegionKey` and exact owned region bytes, optionally anchored to a syntax `NodeKey`; a synthetic
region is never misrepresented as a syntax node.

### Analysis and projection keys

Parsing and derived policy are invalidated independently:

- `ProjectAnalysisId` includes the snapshot, exact stored `GrammarSelection` (including parser build
  and grammar artifact), and raw-arena construction/schema version. Canonical-role and semantic
  adapter policy are later derived projections until M2.
- A derived projection key additionally includes the projection schema and every input that affects
  that projection: effective analyzer/metrics/graph configuration, suppressions, enabled rules,
  external analyzer execution policy/capability/version, and relevant budgets. Its digest is the
  `ProjectionId` carried by findings, actions, graph/metric reports, and cursors/handles.
- A proposal context includes its source/project revisions, projection identity, effective config,
  capability observations, exclusions, and expected work-order set. It remains the reconstruction
  authority introduced in M0.13.

A config or external-tool change can invalidate findings without forcing another syntax parse. A
grammar or source change invalidates both syntax and every dependent projection.

Project/build/config artifacts consulted as code facts are snapshot entries, so their byte edits
change `ProjectSnapshotId`. Deslop settings, suppressions, baselines, and command policy are
projection inputs: persist their effective canonical values (and the settings-file revision when one
was read), and change `ProjectionId` without reparsing source. No setting is ambiguously hashed as
both raw project code and effective policy.

### Partial, unsupported, and failed analysis

The owned arena retains Tree-sitter error and missing nodes and can retain valid recovered subtrees.
That storage improvement does not itself increase authority. Until M2 defines fact-level capability
and coverage, the M0 policy remains:

- partial or failed syntax analysis produces no rewrite-authorizing findings, metrics, or graph facts;
- unsupported text evidence may be reported only as `NeverAuto`, with no proposal or fix surface;
- project-level conclusions are `unknown` or withheld when a required input is partial;
- no consumer may silently reparse, fall back to text, or relabel partial evidence as complete.

Later fact-level recovery must be explicit in provenance, capability, and schema version and must
pass the same safety gates; it may not be introduced as a fallback hidden behind an existing
authoritative label.

### Invalidation and edit lifecycle

Published snapshots never mutate. A disk change, overlay edit, rename, scope change, or relevant
configuration change creates a new snapshot or projection. The old value remains valid as historical
read-only evidence but cannot authorize actions against current state.

| Change | Reusable | Invalidated or expired |
| --- | --- | --- |
| Same path, bytes, grammar, and schema | Source, parse owner, arena | Only projections whose policy inputs changed |
| Source-byte edit | Unrelated file parse owners | Edited file parse/arena; local facts; all dependent project facts, proposals, and handles |
| Rename/copy | Content storage; possibly private parse cache | Path-bound file/node keys, project snapshot, graph paths, proposals |
| Grammar/dialect/parser version change | Raw source | Tree, arena, parse provenance, all projections |
| Raw-arena schema change | Raw source; private tree only when explicitly compatible | Arena ownership, analysis identity, and all dependent projections/serialized handles |
| Semantic adapter/query/role change | Snapshot and raw syntax analysis | Affected derived projections; `NodeKey/1` survives unless its schema is explicitly bumped |
| Analyzer config, suppression, or rule change | Snapshot and syntax analysis | Findings, candidates, work orders, proposal context |
| External analyzer availability/version change | Snapshot and syntax analysis | Covered findings, capability provenance, candidates, proposal context |
| Requested scope or discovered file set change | Unchanged per-file parse owners | Project snapshot identity and every project-wide projection |
| LSP document version/overlay edit | Unchanged files | Edited overlay revision; stale async results and code actions |

M1.8 may use `Tree::edit` and Tree-sitter changed ranges to construct the new revision. This is a new
parse for the new revision, not mutation of a published `ProjectAnalysis`. Incremental output must be
byte-for-byte equivalent to a clean full rebuild. A revision-bound handle either re-anchors with
explicit structural evidence and receives a new `NodeKey`, or expires; arbitrary nearest-span or
fuzzy matching is forbidden. Cross-file reverse dependencies determine which graph, metric, clone,
candidate, and work-order projections must be rebuilt.

### Consumer contract

| Consumer | Contract after migration |
| --- | --- |
| Analyzer and language rules | Accept file/project views and owned query captures; do not call `parse_source` or reread paths. External analyzers consume the snapshot bytes (stdin or an isolated mirror), record exact capability/version observations, and reject results if they cannot bind them to that revision. |
| Metrics | Traverse owned nodes and exclusive token/region ownership once. Inclusive aggregates are explicitly declared and derived, never produced by reparsing each region. |
| Graph | Build syntax/name edges from the shared arena and tag every node/edge with analysis provenance. Project stitching invalidates through explicit dependency edges. |
| Evaluator | Build one independent pinned analysis per manifest case/scenario, preserving case isolation so cross-file/boundary rules do not leak across the corpus. Validate the manifest language against stored grammar selection and record projection/config/capabilities. Mark non-complete or unpinned-capability cases invalid instead of scoring empty evidence as false negatives. False-positive fixtures and baselines use pinned snapshot bytes/metadata, never a later path reread. |
| Protocol | Build work orders and proposal context from snapshot bytes and revision-bound projections; never reread reports to reconstruct regions. |
| Verifier/mutator/fix | Reconstruct and compare the persisted context, materialize every declared analysis input from the pinned snapshot, compose all non-overlapping edits into one batch candidate snapshot, and verify exactly that combined state. Never rebuild analysis from a later live-tree copy. Validate the full declared read set before verification and again at commit, plus `RevisionGuard` before verification and each per-file atomic replacement. Peer drift rejects the batch even when target bytes match. Other files used only by a check command are verification-environment inputs, not silently substituted analysis evidence. All-or-nothing multi-file commit/rollback remains the M6/M7 transaction layer. |
| CLI | Discover/build once per command and pass the same `Arc<ProjectAnalysis>` to scan, metrics, graph, slop, and propose orchestration. Rendering is pure. |
| MCP | Build once per request/context and return only schema-versioned handles bound to analysis and projection IDs. No tool handler may maintain a hidden divergent snapshot. MCP tool schemas have one executable source of truth before new identity fields are added. |
| Slim | Consume a `PreparedSlimRun` containing one pinned analysis/projection/work-order-set digest. Egress summary, consent, and prompts derive from that same object; the full declared read set is revalidated before provider egress, and drift aborts instead of reusing consent. Slim never reads or parses source independently and remains removable from the deterministic contract. |
| LSP | Maintain one workspace snapshot containing the complete open-document overlay map `(path, bytes, client version)` plus unchanged workspace inputs; any edit/config change creates a new result generation. M1 preserves current file-local diagnostic scope and disables external analyzers on keystrokes unless an explicit execution policy says otherwise. Results/actions carry document version, analysis ID, and projection ID; stale work is discarded. Versioned workspace edits do not bypass verifier policy. |

Migration is a flag day per public workflow: once a workflow accepts `ProjectAnalysis`, its legacy
read/parse fallback is removed. Temporary adapters may wrap a single `SourceFile` into a one-file
snapshot for tests, but they must still create exactly one parse owner and must not survive M1.DoD.

### Local and wire boundary

`ProjectAnalysis`, private Trees, owned arenas, `NodeId`, views, and parse ledgers remain
process-local. Only schema-versioned snapshot/analysis/projection IDs, `NodeKey`/`RegionKey`, exact
source and revision guards, effective config, capability provenance, and owned source regions may
serialize.

Required identity fields are not added in place to the strict M0 `/3` schemas. M1.10 introduces
`deslop.proposal-context/2`, `deslop.workorder/4`, `deslop.patch/4`, and corresponding `/4`
characterization schemas. Legacy values fail closed; no missing snapshot, projection, node, or region
identity is inferred from paths/spans or default configuration. The protocol definitions generate
the MCP schemas so executable and documented shapes cannot drift.

### Concurrency, caching, and determinism

Independent files may be read and parsed in parallel. Publication waits for all workers, commits
results in normalized path order, and assigns deterministic node/token/capture order from syntax
child order rather than completion order. The same inputs and tool/schema identities must produce
the same snapshot, analysis, node keys, diagnostics, and projection serialization.

Persistent caching is not required for M1. A cache added later is content addressed by all relevant
identities, validates its schema, and is observationally equivalent to a cold build. Cache hits do
not weaken parse-count accounting: an explicit `Arc<ParseLedger>` is owned by one builder/analysis,
shared atomically across only its workers, and records requested file revisions, actual parser
invocations, and reuse separately by `FileRevisionKey`/`GrammarSelection`. A global observability
sink may aggregate ledgers, but it is not test or acceptance authority; concurrent requests cannot
contaminate each other's M1.DoD counts.

## Verification requirements

M1 is complete only when executable checks establish:

1. every scan/propose workflow has exactly one parse owner per supported file revision across
   analyzer, metrics, graph, evaluator, protocol, MCP, LSP, and slim consumers; a cold build records
   one parser invocation and a validated warm reuse records zero additional invocations;
2. snapshot construction reads each disk input once and every projection reports the same analysis ID;
3. changing bytes, path, grammar, scope, config, adapter schema, or external capability produces the
   invalidation described above;
4. incremental construction is equivalent to a clean rebuild and reports bounded invalidation;
5. node/token ownership is total and exclusive where declared, inclusive aggregates do not double
   count nested regions, and node order is deterministic under parallel execution;
6. no public API exposes a borrowed Tree-sitter node/cursor or serializes a `NodeId`;
7. stale `NodeKey`, proposal context, LSP version, and `RevisionGuard` cases fail closed with zero writes;
8. partial/unsupported/failed inputs preserve the M0 authority boundary; and
9. latency, actual parse count, cache reuse, peak memory, and invalidation fan-out are measured rather
   than inferred;
10. deterministic equality holds across input order/spelling/aliases, worker counts, and repeated
    processes, and across checkout roots when the same explicit `RepositoryId` is supplied;
11. consent summary, prompt contents, and provider egress use one `PreparedSlimRun`, while any
    intervening declared-input revision change aborts before egress; and
12. new wire schemas reject missing or legacy analysis/projection/region identity rather than
    defaulting or inferring it.

The unchanged M0 executable snapshot is a compatibility gate: 30 work orders, 30 unique IDs, 30
unique targets, 65 grouped findings, and its graph, grammar, partial-analysis, and capability probes
must still pass throughout M1.

The smallest meaningful verification runs before the broad workspace gate. M1.11 and M1.DoD publish
the numerical results.

## Consequences

Positive consequences are one coherent revision for all evidence, elimination of repeated parses,
explicit invalidation, owned identities that can cross async/API boundaries, deterministic sharing,
and a substrate capable of later scope/CFG/PDG work.

Costs are higher up-front memory because source, a private Tree, and an owned arena coexist; migration
touches every consumer; projection keys must be maintained whenever an input changes; and incremental
reuse needs equivalence tests. M1.11 will measure the memory cost before any optimization claim.

## Rejected alternatives

- **Keep parsing inside each consumer.** This cannot guarantee a common revision or one-parse
  behavior and preserves contradictory partial-parse policy.
- **Pass borrowed Tree-sitter nodes between crates.** Their lifetimes couple all consumers to one
  `Tree`, do not serialize, and are unsafe for retained async/LSP state.
- **Use one mutable global project cache.** It hides ownership, makes tests and concurrent roots
  interfere, and allows requests to observe mid-update state.
- **Treat `NodeKey` as a fuzzy cross-revision identity.** It would let approximate matching leak into
  safety decisions. Baseline matching and write guards have different authority.
- **Put every projection inside `deslop-parse`.** That creates dependency cycles and makes the
  foundational layer depend on policy. Revision-tagged projections give one logical analysis without
  reversing dependencies.
- **Enable recovered subtrees as authoritative during the ownership migration.** That would combine
  an architecture change with a safety-policy change and violate the established M0 contract.

## Rollout

M1.2 introduces the source store, snapshot/revision types, and single parse owner. M1.3-M1.7 add the
owned arena, identity, ownership, containment, and query surfaces. M1.8 adds immutable incremental
construction and re-anchor-or-expire behavior. M1.9 migrates analyzer and metrics. M1.10 migrates
graph, evaluator, protocol, verifier-facing reconstruction, CLI, MCP, slim, and LSP. M1.11 instruments
the invariants and performance, after which M1.DoD removes remaining legacy parse paths from all
scan/propose workflows.
