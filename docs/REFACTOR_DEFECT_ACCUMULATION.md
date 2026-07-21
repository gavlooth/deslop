# Detecting refactor-defect accumulation

Status: design guide. The detectors proposed here are not shipped capabilities.

## The situation

Some of the most damaging refactor defects are not ordinary code smells. A
change can make one component locally cleaner while leaving the wider system
internally inconsistent:

1. the implementation that owns a value or decision moves;
2. one or more consumers still derive it from the former owner;
3. tests, telemetry, release checks, or operational records continue to certify
   the old path; and
4. later refactors build on the inconsistent state.

The resulting code often parses, type-checks, and passes nearby tests. Each
individual edit can look reasonable in isolation. The defect becomes visible
only when the tool compares revisions and follows the complete contract from
producer to consumer, verifier, test, and observation surface.

This guide calls that pattern **refactor-defect accumulation**. The useful
question is not “does this file look generated?” but:

> Did a refactor move semantic ownership without moving every dependent
> contract, and has that incomplete adoption survived or grown across later
> revisions?

This is the characteristic way LLM-assisted refactoring produces slop. Each
suggested edit is locally plausible, applies cleanly, and often improves the
file it touches. What the model cannot do is track every downstream contract
the edit invalidates: the verifier that still gates on the retired mechanism,
the test whose oracle predates the partitioning change, the status surface
that still publishes the old identity. Reviewers miss it for the same reason —
the diff looks fine. The defect lives between revisions, not inside any one of
them.

Refactor-defect accumulation is therefore deslop's core mission extended over
time. Deslop already detects single-snapshot slop: code that parses, passes,
and quietly fails to do what it claims. This analysis applies the same
evidence discipline to the revision history where incomplete adoption
accumulates, and emits review findings a human or an agent can act on.

This extends deslop's existing config-boundary idea—declared, parsed, consumed—
to arbitrary behavioral contracts over time. It is not an authorship detector,
and it cannot prove runtime behavior from syntax alone.

## Case study: RelationExtractor parallel diffusion

The motivating evidence came from the historical repair of
[RelationExtractor](https://github.com/biotz/relation-extractor-audit), locally
checked out at `/srv/biotz/RelationExtractor`. The detailed audit and repair
records are:

- `.agents/DEFECT_HUNT_FORCED_RE_20260719.md`
- `.agents/PARALLEL_DIFFUSION_POST_CHECKPOINT_REVIEW_20260720.md`
- `.agents/PARALLEL_DIFFUSION_MODEL_IMPACT_REPAIR_IMPLEMENTATION_20260721.md`

The concrete model path was
`src/heads/pointer_canvas/parallel_field_diffusion.jl`. These are historical
case-study observations; the model-impacting items described below were repaired
and verified on 2026-07-21. They are not claims that the current checkout still
contains those defects.

| Refactor symptom | Historical witness | General defect class |
|---|---|---|
| A new commit-time posterior became the real decision owner, but public scores were reconstructed from hard-committed logits | Score provenance no longer represented the information used at commit time | Owner moved; consumer remained attached to the former representation |
| Ranking logic was simplified by flattening document and slot axes together | A document's result could change with unrelated batch companions | Scope collapsed across a boundary that the contract required to remain independent |
| New trainable controllers became live while release checks and telemetry still inspected a retired scalar | A healthy old gate could certify a mechanism it no longer governed | Mechanism live; verifier and observation surfaces stale |
| Activity means were exposed without enough distributional evidence | Non-zero controller activity did not establish the intended response | Telemetry existed but was not causally bound to the claimed behavior |
| A resumed process received a new identity while status still advertised the old process | Operational status and live ownership disagreed | Producer/status identity drift |
| Tests exercised source text, filenames, or a singleton fixture | Green tests did not establish batch independence or production-path adoption | Test oracle lag |
| Equivalent masking or scoring work was recomputed on a hot path | Refactoring preserved output but increased expensive work | Duplicate computation introduced across an ownership boundary |

The common shape was not specific to Julia, diffusion, or machine learning. A
new owner was introduced, but its adoption chain was incomplete:

```text
definition -> configuration -> producer -> consumer -> verifier
           -> test oracle -> telemetry -> release/status surface
```

That shape also appears in web services, compilers, build systems, data
pipelines, command-line tools, and infrastructure repositories.

## Evidence and authority

The detector must preserve deslop's evidence discipline.

- Tree-sitter provides revision-pinned structural facts: declarations,
  references, calls, assignments, branches, configuration reads, assertions,
  serialization, and test structure.
- LSP results may add symbol identity, definitions, references, call hierarchy,
  and diagnostics when a server is available.
- Syntax-highlighting or semantic-token classifications may help distinguish
  declarations, parameters, fields, and functions, but are weak classification
  evidence rather than proof of data flow.
- VCS history supplies ordered snapshots and co-change evidence. The input may
  come from Git, Jujutsu, or a caller-provided snapshot bundle; the analysis must
  not depend on one VCS.
- Runtime, compiler, or domain-specific evidence may enrich a finding, but the
  base detector must not require it.

Every provider result must be bound to the exact source revision it analyzed.
Provider disagreements remain visible. Missing facts remain `Unknown`; they are
not converted to negative facts. A green diagnostic stream does not prove
semantic equivalence, and syntax highlighting does not establish symbol
resolution.

Initial findings from this analysis should be `NeverAuto` or the repository's
equivalent review-only class. The proposed analysis diagnoses an incomplete
contract migration; it does not know the intended repair and must not generate
an automatic rewrite.

## Contract graph

For each revision, build a **contract graph** beside the existing syntax,
dependency, and semantic projections. Nodes use language-neutral roles:

- behavioral owner or mechanism;
- configuration or public parameter;
- producer and transformed value;
- consumer and externally visible result;
- persistence or serialization surface;
- verifier, policy, or release gate;
- test entry point and assertion;
- metric, log, trace, status, or health surface;
- runtime identity, receipt, manifest, or checkpoint.

Edges describe relationships rather than language syntax:

```text
declares  configures  reads  governs  produces  transforms  consumes
persists  reloads     verifies  exercises  asserts  observes  publishes
```

Language adapters map grammar-specific node kinds into these canonical roles.
For example, a field access, map lookup, environment read, and command-line
argument access can all become `reads(config-key)` with distinct provenance.
Raw node names stay in adapter query packs; detector logic operates on canonical
facts.

The graph must retain:

- exact revision and source digest;
- path, byte span, and structural fingerprint;
- provider and capability level;
- coverage and reasons for incomplete coverage;
- symbol or entity match evidence between revisions; and
- positive evidence, counter-evidence, and unresolved gaps.

## Comparing revisions

A single diff is not enough. The analysis needs an ordered revision window.
For each adjacent pair:

1. Build immutable `ProjectAnalysis` snapshots from exact source bytes.
2. Extract contract nodes and edges using Tree-sitter query packs.
3. Add optional LSP and semantic-token facts without merging away their
   independent authority.
4. Match entities by stable symbol identity when available, then by path/span,
   structural fingerprint, rename evidence, and neighborhood similarity.
5. Identify owner additions, removals, moves, representation changes, and scope
   changes.
6. Enumerate the former and current owner's downstream contract edges.
7. Check whether dependent consumers, verifiers, tests, and observations changed
   coherently in the same revision or a declared transition window.
8. Search later revisions for persistence, repair, further dependence, or
   contradictory evidence.
9. Emit a typed review finding only when its minimum evidence contract is met;
   otherwise record a capability gap.

Rename-only and move-only changes are required negative fixtures. A detector
must not report stale consumers merely because source locations changed.

## Detector families

The families below are the deliverable of this design — the findings
themselves. Each names a specific way an LLM (or human) refactor leaves the
system internally inconsistent, the minimum evidence deslop needs before it
may report, and the counter-evidence that must suppress or downgrade the
finding. Every family emits `NeverAuto` review findings: deslop shows the
causal path and a suggested verification; the repair decision stays with the
author.

### `owner-moved-consumer-stale`

A new value, field, function, or object becomes the producer used by a decision,
while a downstream output still reads or reconstructs the value from the former
owner.

Minimum evidence:

- a before/after owner change with a resolvable consumer edge;
- the production decision reaches the new owner;
- the exposed consumer remains reachable from the former owner; and
- no explicit compatibility adapter explains the dual representation.

Useful counter-evidence includes a tested conversion invariant, a deliberate
compatibility layer, or proof that the old and new representations are identical
over the relevant domain.

### `scope-collapse-after-refactor`

An operation that was partitioned by request, document, tenant, batch member,
branch, or other owner becomes global after a reshape, flatten, shared
accumulator, cache, or loop rewrite.

Tree-sitter can identify removed outer loops, changed index expressions, shared
accumulators, and flatten/concatenate calls. It generally cannot prove the
semantic axis represented by an index. The finding should therefore request a
metamorphic independence test: hold one partition fixed, vary its companions,
and compare the fixed partition's result.

### `mechanism-live-gate-retired`

A new mechanism controls behavior, but a verifier, release gate, health check,
or metric still reads a removed or no-longer-governing value.

Strong structural evidence is a new production-path read accompanied by an
unchanged gate whose dependency path terminates at the old owner. Names alone
are not enough; the graph must show the split.

### `producer-verifier-schema-drift`

A producer changes fields, units, meaning, identity, or serialization order
without a coherent verifier or reader change. This covers manifests, receipts,
checkpoints, API payloads, caches, and generated metadata.

The detector should compare construction and validation paths, not only schema
declarations. A field accepted but ignored by the verifier is a relevant edge,
as is a verifier field with no current producer.

### `accepted-config-inert`

A parameter is parsed, echoed, or serialized but does not reach a behavioral
consumer in the selected regime. This generalizes the existing
`config-key-unconsumed` rule across revisions and selectors.

The historical comparison adds two useful cases:

- a formerly live key loses its final behavioral edge; and
- a replacement key becomes live while the retired key remains accepted.

Unknown dynamic uses should suppress promotion rather than be treated as inert.

### `confidence-provenance-lost`

A public score, confidence, explanation, or trace is reconstructed from a later
lossy representation instead of retaining the evidence that governed the
decision. Structural indicators include argmax/threshold/rounding followed by a
reverse lookup or reconstructed score.

This is a review candidate until a behavioral oracle confirms that information
loss matters. The detector should describe the causal path, not assert a
domain-specific meaning for “confidence.”

### `telemetry-not-bound-to-claim`

A metric exists and changes, but its dependency path does not establish the
behavior named by its gate or report. Common forms include inspecting a retired
owner, averaging away the relevant distribution, counting attempted rather than
effective events, or reporting process health from a stale identity.

This detector requires both the telemetry producer and the claimed mechanism to
be graph nodes. Lexical similarity between their names is supporting evidence,
not a sufficient condition.

### `test-oracle-lag`

Production structure changes while tests continue to assert only source text,
registration, filenames, construction, singleton cases, or non-production
helpers. The detector classifies assertion targets and fixture dimensions, then
compares them with the changed production contract.

It should report what remains unproved, such as “partition independence has no
multi-partition oracle,” rather than claim the implementation is wrong. A test
execution manifest, when supplied, is stronger evidence than textual discovery
of a test file.

### `hot-path-work-duplicated`

A refactor introduces two structurally equivalent expensive computations on a
shared reachable path when one result could be reused. Candidate signals include
matching call subtrees, repeated traversals, duplicate conversions, and repeated
host/device or serialization boundaries.

Tree-sitter can nominate the duplication. Cost and safe reuse remain unproved
unless profiling or effect facts are available, so this family stays
review-only.

### `operational-identity-stale`

A process, artifact, checkpoint, receipt, or deployment identity is replaced,
but status, watchdog, resume, or publication code continues to surface the old
identity. Revision-pinned producer/publisher edges are more useful than PID-like
or hash-like text matching by itself.

### `adoption-chain-incomplete`

This is a summary finding emitted only when several specific families share the
same owner migration. It presents the missing chain stages; it must not duplicate
the underlying findings in baselines or severity counts.

## Accumulation and prioritization

The historical dimension distinguishes a transient migration from accumulating
drift. Track:

- how many revisions the stale edge survives;
- how often the new owner and stale consumer are edited separately;
- whether new callers attach to the inconsistent path;
- whether tests and gates remain unchanged through those edits;
- the number and kind of contract boundaries crossed; and
- whether an earlier diagnostic was acknowledged, suppressed, or contradicted.

A transparent priority heuristic can combine those facts:

```text
priority = owner-change evidence
         + stale downstream edges
         + missing production oracle
         + persistence across revisions
         + independent churn
         + boundary distance
```

Priority is triage, not confidence and not fix safety. A persistent syntactic
candidate with incomplete resolution can have high priority and still remain
`Unknown` for semantic correctness.

## Proposed deslop integration

The workspace already contains every surface this feature needs. The work is
new types, one new graph projection, new query families, and new analyzer
rules — not a second parser, language model, or reporting pipeline.

### `deslop-core`: registry and types

Register each detector family in the canonical rule registry
(`deslop_core::rules`), the same registry that backs the `deslop rules`
command, suppression validation, and the MCP `rules` tool. Entries use the
existing `RuleInfo` shape with `safety: "never-auto"`, exactly how
`config-key-unconsumed` is registered today.

Add versioned, serializable types for contract roles, edges, revision pairs,
entity-match evidence, causal paths, counter-evidence, and capability gaps.
Define two new schemas in the house style: `deslop.refactor-history/1`
(ordered exact-byte snapshots with digests and parent links) and
`deslop.refactor-defect/1` (the finding payload sketched below).

One structural note: the scan-path `Finding` type has no evidence payload —
it carries `path`, `span`, `rule`, `severity`, `safety`, `detected_by`,
`message`, `suggestion`, and `fingerprint`. Refactor-defect findings therefore
travel in two forms: a registry-named `Finding` with
`SafetyClass::NeverAuto` for the normal scan/report/LSP path (causal-path
summary in `message`, suggested verification in `suggestion`), and the full
typed payload in a `deslop.refactor-defect/1` artifact for review tooling.

### `deslop-parse`: snapshots and history

`ProjectAnalysis` already builds immutable analyses from exact source bytes,
with `AnalysisProvenance`/`AnalysisStatus` gating and structural fingerprints
(`ExactSubtreeFingerprint`/`NormalizedSubtreeFingerprint`). Build each
revision snapshot through the same entry points so revision pinning comes from
the existing identity machinery rather than new bookkeeping.

`ModuleChangeHistory` already stores co-change observations with explicit
`FactCoverage`. Do not overload it with contract semantics. Add a sibling
`ContractChangeHistory` that may reference module-history evidence while
retaining owner, edge, revision, and entity-match facts specific to this
analysis.

### `deslop-lang`: contract query families

Add the `@contract.*` captures to each adapter's `LanguageQueryPack` as new
`QueryFamilyDeclaration`s, mapped onto the pack's `CanonicalRole`s. Families
an adapter cannot support are declared `unknown`, exactly as existing query
families are, and surface through the adapter's
`LanguageAdapterCapabilityManifest`. This is the mechanism the evidence
discipline above requires: unsupported contract facts become per-language
capability gaps, never silent absences.

### `deslop-graph`: a contract projection

The graph crate today emits one syntactic dependency projection
(`deslop.graph/2`: `contains`/`imports`/`calls`/`inherits` edges with
`resolved`/`syntactic`/`ambiguous`/`external` confidence). The contract graph
is a new projection beside it, with its own projection identity derived via
the existing `derive_projection_id` mechanism (`deslop.graph.projection/1`).
It provides traversals from an owner change to consumers, tests, verifiers,
telemetry, and publication surfaces. Provider-specific facts stay distinct so
disagreements and coverage gaps survive graph construction.

### `deslop-analyzer`: one rule per family

Implement each detector family as a separate analyzer pass following the
`boundary.rs` pattern, with explicit minimum evidence and counter-evidence.
The first slice contains only `owner-moved-consumer-stale` and
`producer-verifier-schema-drift`; both have clear causal paths and cover the
highest-value case-study failures.

### `deslop-cli`: `refactor-risk`

Add a clap subcommand beside `scan`, `baseline`, and `graph`:

```text
deslop refactor-risk --from <revision> --to <revision> [paths...]
```

The CLI resolves revisions through a pluggable history provider and hands the
analyzer an ordered `deslop.refactor-history/1` bundle (exact source bytes,
digests, parent relationships, timestamps only when known, optional provider
artifacts). Git, Jujutsu, editor-local history, or an external review system
can all produce the bundle; no repository model is embedded in the analyzer.

For a single working-tree change, `--from` plus the current exact-byte
snapshot is sufficient. Accumulation scoring requires more than two revisions
and must say explicitly when history coverage is partial.

### `deslop-report`, `deslop-protocol`, `deslop-verify`: mostly free

The existing report envelope (`deslop.findings/2`) renders the new findings
with no format changes: text output already prints the safety class, and
SARIF output already emits `reportOnly` for `NeverAuto` findings. The richer
`deslop.refactor-defect/1` artifact renders alongside, carrying:

- before and after revisions;
- the changed owner with entity-match evidence;
- each stale edge with file and span;
- the causal path;
- provider provenance and coverage;
- counter-evidence and unresolved gaps;
- persistence and priority inputs; and
- a suggested verification, not a generated fix.

No protocol or verifier changes are needed to keep these findings honest.
`deslop-protocol` builds work orders only from findings whose safety permits
proposals and structurally excludes `NeverAuto` regions; `deslop-verify` only
ever sees proposal-eligible work orders. "No automatic rewrite" is therefore
a structural property of the existing pipeline, not a policy this feature
must defend.

### `deslop-lsp` and `deslop-mcp`

The LSP already publishes per-finding diagnostics with the rule name as the
diagnostic code, and offers code actions only for safe-auto findings — so
these findings appear as review diagnostics with no quick-fix, exactly as
required. The new work is the base-revision comparison: the server compares
the current buffer with a configured base revision and must invalidate
external semantic facts when the buffer revision no longer matches them.

The MCP server's read-only posture already matches this feature. Expose the
history comparison as a new tool (or an extension of `scan`) returning
`deslop.refactor-defect/1` payloads; the existing `rules` tool surfaces the
new families through the registry automatically.

### `deslop-eval`: history corpus

Add a frozen, multi-language history corpus under `tests/refactor-history/`
(a sibling of `tests/corpus/`: the graph ratchets pin exact file counts over
`tests/corpus/`, so history snapshots must not live inside it). Each fixture
is a small sequence of complete source snapshots, not a collection of
isolated snippets, with expectations recorded in a
`deslop.refactor-history-manifest/1` manifest. Transcribe the
RelationExtractor patterns into minimal neutral fixtures so evaluation does
not depend on that repository or expose its domain semantics.

## Tree-sitter and optional LSP design

Tree-sitter query packs capture structural candidates through captures such as:

```text
@contract.owner
@contract.config-read
@contract.consumer
@contract.verifier
@contract.assertion
@contract.metric
@contract.identity-publisher
```

Those are canonical capture roles, not literal grammar node names. Each
`deslop-lang` adapter maps its language's declarations, assignments, calls,
indexing, literals, and test constructs to the captures it can support inside
its `LanguageQueryPack`, binds them to `CanonicalRole`s, and declares the rest
`unknown` so the capability manifest reports the gap per language.

Optional LSP integration uses standard server concepts where available:

- document symbols;
- go-to-definition and references;
- call hierarchy;
- diagnostics; and
- semantic tokens.

No particular server, compiler, type checker, or language-specific extension is
required. Servers vary in completeness and may report stale results. Their
facts are stored with revision, provider identity, and advertised coverage —
the same discipline `AnalysisProvenance` already applies to analysis status —
and an LSP fact never silently overrides conflicting syntax or history
evidence.

## Finding schema sketch

```json
{
  "schema": "deslop.refactor-defect/1",
  "rule": "owner-moved-consumer-stale",
  "revisions": { "before": "...", "after": "..." },
  "owner": { "before": "...", "after": "...", "match_evidence": [] },
  "stale_edges": [],
  "causal_path": [],
  "evidence": [],
  "counter_evidence": [],
  "coverage_gaps": [],
  "persistence": { "revisions": 0, "independent_edits": 0 },
  "priority_inputs": {},
  "safety": "never-auto",
  "suggested_verification": "..."
}
```

The schema uses deslop's typed IDs and canonical serialization. Two
representations exist by design: on the scan path the finding is a
registry-named `Finding` (`SafetyClass::NeverAuto`) whose `fingerprint` feeds
baselines and whose `message`/`suggestion` carry the causal-path summary and
suggested verification; the full review payload above is the
`deslop.refactor-defect/1` artifact. The important properties are exact
revision identity, auditable causal paths, and separation of evidence,
counter-evidence, and unknowns.

## Evaluation and false-positive controls

The corpus should contain the same contract histories expressed in several
languages supported by deslop, without requiring their compilers. Required
golden cases include:

- owner moved and every consumer, verifier, test, and metric moved: no finding;
- pure rename or file move: no finding;
- owner moved while one output remains attached to the old representation:
  finding;
- producer schema changed while verifier did not: finding;
- compatibility adapter with an explicit invariant test: suppressed or
  downgraded;
- dynamic or reflective consumer with incomplete resolution: capability gap,
  not a clean result;
- provider disagreement: visible conflict and no promotion;
- singleton oracle after a partitioning change: test-oracle-lag candidate;
- complete multi-partition metamorphic oracle: no oracle-lag finding; and
- generated code excluded by explicit provenance: no source-owner finding.

Fixture expectations live in `deslop.refactor-history-manifest/1` (one case
per snapshot sequence, expectations per rule), and per-family precision/recall
rows join the `deslop.eval-baseline/1` ratchet. Measure precision per detector
family, abstention rate, entity-match accuracy, and causal-path completeness.
Recall should be reported separately for syntax-only and
optional-semantic-provider modes. Baselines and ratchets must compare stable
finding identities so a history-window change does not silently rewrite the
accepted set.

## Phased delivery

### Phase 0: contracts and fixtures (`deslop-core`, `deslop-eval`)

- Define `deslop.refactor-history/1` and `deslop.refactor-defect/1`.
- Register the detector-family names in the `deslop_core::rules` registry.
- Add two-revision and multi-revision fixtures in multiple languages.
- Specify capability gaps and provider-conflict behavior before detector code.

### Phase 1: owner migration (`deslop-parse`, `deslop-lang`, `deslop-graph`, `deslop-analyzer`, `deslop-cli`)

- Build `ContractChangeHistory` from exact `ProjectAnalysis` snapshots.
- Add contract query families to the first adapters' `LanguageQueryPack`s.
- Implement entity matching with rename/move negative cases.
- Ship review-only `owner-moved-consumer-stale` and
  `producer-verifier-schema-drift` behind `deslop refactor-risk`.

### Phase 2: adoption surfaces (`deslop-analyzer`, `deslop-graph`)

Shipped (2026-07, `deslop refactor-risk`): config-key extraction
(`os.environ`/`os.getenv`/`ENV` reads plus module-level acceptance surfaces)
with `accepted-config-inert`, cross-file `test-oracle-lag`,
`adoption-chain-incomplete` summaries in a separate `summaries` report field
(no double counting), and multi-revision windows (`--then`) computing
persistence and co-change triage inputs from contract fingerprints.

- Add config, test-oracle, telemetry, and operational-identity roles.
  (Config and test-oracle shipped. Telemetry deferred: it requires the
  claimed mechanism and telemetry producer as graph nodes. Operational
  identity deferred: distinguishing identity literals from schema literals is
  text matching, which the evidence rules reject as sole evidence.)
- Add incomplete-adoption summaries without double-counting findings. (Shipped.)
- Expose persistence and co-change evidence as triage inputs. (Shipped for
  contract fingerprints; the `deslop-graph` contract projection remains
  future work and ships with the graph-dependent families.)

### Phase 3: editor and review integration (`deslop-lsp`, `deslop-mcp`, `deslop-report`)

- Add base-revision comparison to LSP and MCP.
- Accept revision-bound semantic-provider artifacts.
- Show invalidation, disagreement, and coverage gaps in every output format.

### Phase 4: promotion gates (`deslop-eval`, `deslop-core`)

- Freeze the multi-language corpus and precision thresholds.
- Add history-aware finding identity to baselines (acceptance gate 10).
- Dogfood against real refactor histories, including the transcribed case study.
- Promote detector families independently; unsupported facts continue to block
  stronger claims.

## Acceptance gates

The feature is not complete until all of these hold:

1. Identical history bundles produce byte-stable reports.
2. Every fact and provider artifact is revision-pinned.
3. Rename, move, full-adoption, and compatibility-adapter negative fixtures pass.
4. At least three language adapters demonstrate the same detector contract using
   Tree-sitter alone.
5. Optional LSP evidence improves coverage without changing the authority of
   syntax facts or hiding disagreement.
6. Incomplete coverage yields an explicit gap rather than a clean result.
7. Findings contain a reviewable causal path and suggested verification.
8. No detector in this family creates or applies an automatic edit.
9. The evaluation report separates confidence, priority, and fix safety.
10. Baselines gain history-aware finding identity. Today's baselines are
    reporting-suppression fingerprints over path/rule/span/text; refactor-defect
    findings need an identity stable across history-window changes (rule, owner
    identity, causal-path digest) so ratchets neither churn falsely nor silently
    accept a changed defect. This is new work in `deslop-core`, not a reuse of
    the current fingerprint.

## Known hard cases

- macro expansion, generated source, and code generation boundaries;
- dynamic dispatch, reflection, and string-addressed consumers;
- split or squashed history that hides the actual transition;
- many-to-one and one-to-many symbol migrations;
- intentional dual representations during compatibility windows;
- tests discovered textually but not executed by the governing suite;
- stale editor/LSP results after unsaved changes; and
- semantic scope such as tensor axes, tenancy, or units that syntax cannot name.

These cases are reasons to preserve `Unknown`, not reasons to add lexical guesses.
The first implementation should remain precision-first and avoid a new
dependency: exact snapshots, existing Tree-sitter adapters, structural
fingerprints, graph projections, and optional standard LSP artifacts are enough
to prove the initial architecture and evaluation contract.
