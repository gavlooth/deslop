# Deslop Algorithm Audit — Graph-first, per-node analysis

Date: 2026-07-12

Status: AUDIT COMPLETE. No production code was changed.

## Verdict

The current deterministic rules and verifier are a useful base, but the analysis engine is not yet
graph-first, per-node, or safe to use as a cross-language readability/refactor gate. The central
problem is architectural: parsing, rules, metrics, dependency extraction, and work-order generation
operate as separate passes over text or separately parsed trees. Several passing tests validate
self-consistency while live probes violate the user-visible contract.

The next change should not be another readability-weight tweak. It should establish one owned
Tree-sitter analysis snapshot per file and make scan, metrics, graph, LSP, and work orders consume
that snapshot.

## Measured evidence

| Probe | Result | Consequence |
|---|---:|---|
| `metrics tests/corpus/clean` | health `40.38`, 3 candidates | Known-clean code is penalized. |
| `metrics tests/corpus/sloppy` | health `46.14`, 4 candidates | Sloppy corpus ranks healthier than clean corpus. |
| Clean candidate intrinsic scores | `0.15–0.17` | They are called refactor candidates only through the repo-relative OR gate. |
| `metrics crates` | 1,556 overlapping regions, `30.50s` | The file is reparsed for each region; containers and members are double-counted. |
| `graph crates` | `0.74s`, 10,872 edges, 4,203 resolved | The graph is much cheaper but is a separate symbol projection, not analyzer IR. |
| `compact_label` graph probe | 2 definitions; all 10 calls resolve to `builder.rs` | `confidence=resolved` is unsound and ambiguity is hidden. |
| Typed TypeScript fixture | metrics falls back to one file region; graph skips extraction | TypeScript uses the JavaScript grammar. |
| Clojure `if`/`when` fixture | cyclomatic `1`, cognitive `0` | Pack head tokens are compared with grammar node-kind strings and never match. |
| Rust `propose` fixture | 13 orders, 3 unique IDs; one region repeated 11 times | Slim rewrites one region repeatedly and apply later rejects overlaps. |
| Entire sloppy corpus (historical, before M0.1 grouping) | 62 orders, 31 unique IDs, 8 duplicated region IDs | Multi-finding region grouping was missing system-wide; this count is not a current snapshot invariant. |
| Current workspace gate | 179 tests pass; fmt and clippy pass | Existing tests do not cover these external-contract failures. |

The labelled rule corpus still shows useful analyzer signal: `deslop slop` separates clean from
sloppy (`0.82` versus `60.32`). The broken surface is specifically the uncalibrated health/
readability aggregation and the disconnected graph/work-order pipeline, not every rule.

## P0 correctness failures

### 1. Multi-finding regions produce duplicate work orders

`work_orders_for_source` maps each non-safe finding independently to a work order, even when all
findings resolve to the same function. The work-order ID is region-derived, so duplicate orders also
have duplicate IDs. Slim rewrites every copy; verification stores current work orders in a map that
overwrites equal IDs; apply rejects the overlapping replacements.

Required fix: resolve findings to a region node, group by `(path, region_node_key)`, merge all
findings/evidence into one order, and emit one rewrite request and one patch per region.

Primary locations:

- `crates/deslop-protocol/src/lib.rs:97`
- `crates/deslop-slim/src/lib.rs:410`
- `crates/deslop-verify/src/lib.rs:3021`
- `crates/deslop-verify/src/lib.rs:3394`

### 2. Graph resolution overstates certainty

Top-level symbol names omit file/module identity. The first simple name is inserted into the
qualified-name index, and exact-name lookup runs before the ambiguity check. A unique simple name
anywhere in the scan can therefore be called locally resolved even when lexical/module scope points
elsewhere. Inheritance edges are also captured before a class becomes the current owner.

Required fix: local scope and import resolution must be evidence-producing operations. Only a
lexically/import-proven target is `resolved`; name-only candidates are `syntactic` or `ambiguous`.
The current dependency graph should become a projection over a shared semantic graph.

Primary locations:

- `crates/deslop-graph/src/builder.rs:152`
- `crates/deslop-graph/src/builder.rs:248`
- `crates/deslop-graph/src/builder.rs:334`
- `crates/deslop-graph/src/extract.rs:35`

### 3. Language-pack contracts do not normalize grammar semantics

The metric engine compares raw `node.kind()` strings with pack arrays. That cannot generalize to
languages such as Clojure, where control constructs are list heads rather than node kinds. Python
declares metric regions but does not classify behavioral regions, so Python long-method and
behavioral duplication analysis are disabled. TypeScript is parsed with the JavaScript grammar.

Required fix: every language adapter must map grammar-specific nodes and context to canonical roles
using Tree-sitter queries or contextual callbacks. Registering an extension must require passing a
contract suite for callable, block, branch, binding, reference, call, comment, literal, and region
captures.

Primary locations:

- `crates/deslop-lang/src/lib.rs:29`
- `crates/deslop-lang/src/lib.rs:234`
- `crates/deslop-lang/src/lib.rs:382`
- `crates/deslop-lang/src/lib.rs:569`
- `crates/deslop-metrics/src/lib.rs:539`

### 4. Health/readability/refactor-confidence labels exceed the evidence

The score weights, half-saturation values, interactions, bands, and `0.50` threshold are hand-set.
`measurement_confidence` is mostly sample size, not measured correctness. Candidate selection ORs
raw burden with repo-relative outlier status, recreating the invalidated assumption that unusualness
is absolute refactor evidence. Health subtracts the ratio of relative hotspots from average
Maintainability Index, which produces the measured clean/sloppy reversal.

Required fix: remove the health scalar from gating; rename the current model to an experimental
`heuristic_burden`; expose repo outliers separately; do not use probability/confidence labels until
human calibration. Refactor safety continues to come only from graph impact, tests, coverage,
mutation, characterization, and verifier results.

Primary locations:

- `crates/deslop-metrics/src/lib.rs:812`
- `crates/deslop-metrics/src/lib.rs:1007`
- `crates/deslop-metrics/src/lib.rs:1034`
- `crates/deslop-metrics/src/lib.rs:1305`

### 5. Parse-error behavior is inconsistent and misleading

Metrics silently replace a syntax tree containing any ERROR node with a whole-file text region;
graph extraction drops the whole file. The metrics fallback can still report moderate-looking
measurement confidence from token count.

Required fix: retain valid subtrees, mark error/missing nodes, publish byte/token coverage, and make
unknown evidence explicit. Never synthesize cyclomatic `1` or health `100` for missing structural
data.

## P1 graph-first analysis substrate

Tree-sitter should be the syntax backbone, but its CST is not by itself a scope graph, CFG, call
graph, or model of semantic equivalence. The shared architecture should be:

```text
source text
   │ parse once
   ▼
ParsedFile ──► dense SyntaxNode arena ──► canonical roles + token ownership
                                           │
                          ┌────────────────┼────────────────┐
                          ▼                ▼                ▼
                    node features     semantic edges    clone classes
                          └────────────────┼────────────────┘
                                           ▼
                                  ProjectAnalysis snapshot
                     ┌──────────────┬──────┴──────┬──────────────┐
                     ▼              ▼             ▼              ▼
                  findings       metrics      graph views     work orders
```

### Owned node representation

Store stable data rather than borrowed `tree_sitter::Node` handles:

```text
StructuralNode {
  node_id, durable_node_key, parent_id, ordered_children,
  raw_kind_id, raw_kind, canonical_roles, field_id,
  byte_span, point_span, line_span, token_range,
  owner_callable_id, parse_status
}
```

- `NodeId` is a dense ID valid for one analysis snapshot.
- `NodeKey` is a durable structural fingerprint for baselines and work-order identity.
- A separate revision/text hash guards writes against stale source.
- Do not include absolute line/byte position as the main durable identity; unrelated preceding edits
  currently churn finding and graph identities.

Represent four intersecting layers:

1. tokens/trivia;
2. physical and logical lines;
3. Tree-sitter syntax nodes;
4. virtual nodes for callable regions, basic blocks/action blocks, and clone classes.

Tokens have one exclusive syntax owner and links to physical/logical lines. Inclusive subtree
aggregates are derived bottom-up; they are never summed from already-inclusive peer records.
Ownership resets at nested callables so inner-function complexity cannot leak into an outer score.

### Semantic edges

Add edges progressively:

- ordered parent/child and field ownership;
- token-to-line and token-to-syntax ownership;
- lexical scope, definition, and reference;
- CFG next/branch/merge/loop-back;
- imports, calls, and inheritance with evidence confidence;
- comment/documentation attachment;
- structural clone/motif membership.

Use per-language query captures similar to `locals.scm` for scopes, definitions, and references.
Tree-sitter 0.25 exposes node kind IDs, fields, cursors, queries/captures, old-tree reuse, and changed
ranges needed for this design. Exact types/dispatch and semantic equivalence still require external
analyzers or the verifier.

### Rule execution

Replace `Rule<SourceFile, ...>` full-file scans with interest-based node dispatch:

```text
Rule::roles() -> RoleMask
Rule::check_node(context, node_id) -> findings
Rule::finish_file(context) -> findings
Rule::finish_project(project) -> findings
```

This gives one traversal plus indexed rule checks rather than `rules × tree traversals`. Findings
must carry node ID/key, exact span, semantic role, structured metric evidence, related nodes/edges,
and the region node that owns a rewrite.

## Per-node metric model

Keep raw evidence before any model score. For each actionable block/node, retain:

- exclusive and inclusive token/NLOC counts;
- line length, indentation change, and density summaries (`p50`, `p90`, max);
- local decision and cognitive increments plus canonical nesting depth;
- Tree-sitter-classified operators, operands, identifiers, and literals;
- identifier subtokens and local naming consistency;
- attached-comment consistency, rationale/staleness signals, and commented-out code;
- zero-order token/byte entropy with sample support;
- conditional token surprisal attributed from a repo/language model;
- subtree/production regularity and clone coverage;
- parse/canonicalization coverage and cohort support.

### Complexity

- Exact McCabe complexity is `E - N + 2P` over a CFG. Until a CFG exists, call the current value
  `decision_count + 1` or `ast_cyclomatic_estimate`.
- The present cognitive value is a nesting-weighted branch count, not a faithful cognitive-complexity
  implementation. Ordinary returns should not automatically cost one; else-if, boolean sequences,
  recursion, match/switch, catches, comprehensions, and labeled jumps need explicit semantics.
- Emit the contributing nodes so the score is explainable and actionable.

### Entropy is three different signals

1. **Within-node Shannon entropy** measures diversity in the observed token distribution.
2. **Cross-entropy/surprisal** measures contextual unpredictability under a model:
   `s_i = -log2 P(token_i | context)`.
3. **Compression/structural regularity** measures repeated sequence or motif structure.

They are not interchangeable and can point in opposite directions. The current
`compression_ratio` is byte entropy divided by eight; rename it `byte_entropy_bits_per_byte`.
Zero-order entropy is permutation-invariant and cannot detect repeated sequences. Do not give
entropy a universal bad direction; residualize length and allow a non-linear relationship if a
labelled model supports it.

For surprisal, train/score with repository and language context, attribute token loss to nodes, and
normalize by `language × grammar version × canonical role × size cohort`. Use hierarchical backoff
and publish support. Surprisal is useful for anomaly/defect triage; it is not automatically human
readability or AI authorship.

### Separate output axes

| Axis | Meaning |
|---|---|
| readability evidence | Human-perceived legibility model, only after calibration. |
| structural load | Decisions, nesting, control flow, information access cost. |
| anomaly/naturalness | Contextual surprisal relative to a supported cohort. |
| redundancy/bloat | Clone coverage, ceremony, semantic yield, change amplification. |
| evidence reliability | Parse, mapping, sample, and cohort coverage. |
| refactor safety | Graph impact plus tests/coverage/mutation/verifier evidence. |

Refactor priority should combine actionable burden, expected removable/clarifiable tokens, graph
impact, and safety. A hard function can encode a hard domain and is not necessarily slop.

## Algorithm replacements

### Duplication

Current same-file matching is fixed-window nested comparison, worst-case roughly `O(n² × k)`.
Identifier normalization collapses all alphabetic tokens, including keywords, so consistent
alpha-renaming is not preserved and different structures collide.

Replace it with:

1. bottom-up normalized subtree hashes for exact structural clones (`O(nodes)` grouping);
2. rolling token hashes plus winnowing for candidate discovery (`O(tokens)` expected);
3. maximal left/right extension to emit one clone class rather than overlapping windows;
4. scope-consistent identifier/literal mappings for Type-2 clones;
5. bounded structural similarity only on shortlisted candidates for Type-3 clones.

Keep structural regularity distinct from harmful duplication: repetition becomes a refactor signal
only with abstraction payoff, change amplification, semantic equivalence evidence, and safe scope.

### Halstead and lexical features

The formulas are conventional, but the current line tokenizer treats many delimiters/quotes as
operands, mistakes comment markers inside strings, lacks block comments/Unicode semantics, and
cannot produce configured three-character operators such as JavaScript `===`.

Derive tokens from CST leaf roles. Treat a literal as one operand, exclude comments structurally,
and keep grammar-specific operators in adapter queries. Keep Halstead Volume as lexical size/
diversity evidence; do not present derived `effort` as observed human effort.

### Hotspots

Do not pool all languages, grammar kinds, containers, and callables. Compare exclusive executable
regions within supported cohorts. Prefer empirical percentiles or robust z-scores:

```text
robust_z = (value - median) / (1.4826 × MAD + epsilon)
```

Require minimum support, report cohort metadata, separate generated/test/vendor code, and account
for multiple metric tests. Repo-relative hotspots remain triage signals, never absolute refactor
candidates.

## Literature alignment

- **Buse and Weimer (2008/2010):** learned local readability from 12,000 ratings of 100 short Java
  snippets. Mean/max per-line lexical and visual features mattered. This supports node/block and
  line feature extraction, not universal weights.
  <https://doi.org/10.1109/TSE.2009.70>
- **Posnett, Hindle, and Devanbu (2011):** a small-snippet logistic baseline used Halstead Volume,
  lines, and byte entropy: `z = 8.87 - 0.033V + 0.40Lines - 1.5Entropy`. The paper explicitly warns
  against blind use on whole functions/classes; byte entropy added little beyond size/volume.
  <https://doi.org/10.1145/1985441.1985454>
- **Hindle et al. (2012):** code is predictable and strongly project-local under cross-entropy
  models. Predictability is not itself readability. <https://doi.org/10.1109/ICSE.2012.6227135>
- **Ray et al. (2016):** buggy lines were somewhat more surprising; line scores were normalized by
  enclosing AST type. This supports node-kind-conditioned defect triage, not a readability claim.
  <https://doi.org/10.1145/2884781.2884848>
- **Scalabrino et al. (2018):** textual/identifier/comment features complement structural features
  in human-labelled readability models. <https://doi.org/10.1002/smr.1958>
- **McCabe (1976):** cyclomatic complexity is a CFG metric. <https://doi.org/10.1109/TSE.1976.233837>
- **Torres et al. (2025):** token and AST-edge entropy can detect unusual software-evolution events,
  but the target is change anomaly/complexity, not human readability.
  <https://doi.org/10.1007/s10664-025-10644-y>

## Convergent validation plan

Instrument once and make one benchmark run produce the full feature matrix. Do not serially tune
one coefficient and rerun.

Decision resolved: can deslop support a portable calibrated readability model, or should it expose
only separate evidence axes?

Terminal outcomes:

1. Stable leave-project-out and leave-language-out performance with calibration: ship a versioned
   cross-language readability probability.
2. Stable only within some languages/roles: ship separate calibrated models and no global score.
3. No stable gain over size/simple baselines: retain transparent evidence and hotspot ranking; do
   not claim readability.

Benchmark design:

- import licensed Buse/Scalabrino ratings as baselines;
- add stratified human pairwise ratings for supported languages and canonical node roles;
- record timed and correct comprehension separately from perceived readability;
- include behavior-preserving human and AI cleanup pairs, but never predict authorship;
- compute all candidate features once and run size-controlled ablations post hoc;
- split by project, then test language transfer;
- report Spearman/Kendall rank agreement, AUC/PR, Brier score, calibration error, top-k precision,
  inter-rater agreement, and per-language confidence intervals;
- validate defect/change prediction separately from readability and refactor safety.

## Implementation sequence

1. **Contract repair:** group work orders; correct/downgrade graph resolution; use real TS/TSX
   grammars; implement Python/Clojure role mapping; localize parse errors; remove misleading gates.
2. **Shared IR:** parse once, build the owned syntax arena and token/line layers, establish NodeId/
   NodeKey, and make all consumers use one `ProjectAnalysis` snapshot.
3. **Per-node algorithms:** canonical complexity contributions, CST lexical features, clone classes,
   scope/ref edges, and exclusive/inclusive aggregation.
4. **Evidence outputs:** separate burden, anomaly, redundancy, reliability, impact, and safety;
   provide worst-node heatmaps and structured finding evidence.
5. **Calibration experiment:** run the single convergent benchmark and choose one of the terminal
   product outcomes above.

## Required regression gates

- One parse per file instrumentation.
- One work order and one patch per region node with merged findings.
- Exact duplicate-name lexical/module resolution; no false `resolved` edge.
- Typed TS/TSX callable extraction.
- Python behavioral regions, long-method, and duplication.
- Clojure `if`/`when` branch increments; ordinary calls do not increase control nesting.
- Cross-language construct matrix: if/else-if, boolean chain, loop, match/switch, catch, guard
  return, labeled jump, nested callable.
- Wrapping a method in a container does not change its callable metrics.
- Adding trivial helpers does not improve another region or dilute repo health.
- Exact entropy/tokenizer fixtures including permutation, short samples, strings, block comments,
  Unicode, and three-character operators.
- Partial parse errors report coverage/unknown rather than synthetic clean metrics.
- Clean/sloppy smoke cannot reproduce the current reversed health ordering.

M0.10 automation preserves the post-grouping contract separately from the historical evidence above:
the current sloppy corpus emits 28 unique region work orders containing all 62 findings, and repeated,
overlapping, or reordered equivalent path inputs serialize identically. The clean/sloppy metric smoke
asserts honest `deslop.metrics/5` authority metadata and absence of removed health/readability fields;
only the independent slop detector's deterministic `0.819672131147541` versus
`60.32388663967611` separation is frozen. A slow ignored self-scan logs performance and structural
counts without treating source-tree-dependent totals or wall time as stable gates.

## Negative-memory constraints

- Passing current unit tests does not establish graph/metric semantic correctness.
- Repo-relative unusualness is not absolute refactor evidence.
- Tree-sitter-derived does not mean graph-first or semantically resolved.
- Shannon entropy, contextual surprisal, and compression are different signals.
- Compression/regularity is not monotonically unreadable or removable.
- Readability must never weaken deterministic fix/apply safety gates.

Signature: Codex (GPT-5), integration owner; architecture, metrics, and primary-literature audit,
2026-07-12.
