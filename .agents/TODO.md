# Ultimate Deslop TODO

This is the markable execution ledger for the authoritative plan in `.agents/PLAN.md`, section
“Ultimate Generic Deslop Plan (2026-07-12)”. Check an item only when its stated evidence exists.

Legend: `[ ]` pending, `[x]` completed. IDs are stable and should be cited in commits, session
reports, benchmark records, and work orders.

## Current checkpoint

- [x] A0.1 Audit the parser, metrics, graph, work-order, and language-pack algorithms.
- [x] A0.2 Record numerical correctness/performance probes and current semantic regressions.
- [x] A0.3 Align the proposed architecture with primary research.
- [x] A0.4 Write the authoritative product roadmap and dependency-ordered implementation plan.
- [x] M0.1 Repair the current `deslop.workorder/1` implicit `rewrite-region/v1` contract: emit one
  deterministic work order per authoritative source path and exact enclosing region, aggregate every
  supporting finding, deduplicate overlapping scan roots, and reject duplicate legacy work-order/patch
  IDs before rewriting or writes. True `(ProjectSnapshotId, NodeKey, RecipeId)` identity remains in
  M1.4/M5.1/M6.1. Evidence: `13 -> 3` orders with all 13 findings retained; full sloppy corpus
  `62 -> 31` with 31 unique IDs; one LLM call/patch/verification/write per region.
- [x] M0.2 Remove first-wins bare-name authority from `deslop.graph/1`: preserve every candidate,
  emit path-qualified symbols, route unique best scoped candidates as syntactic evidence, report
  competing candidates as ambiguous, keep unresolved placeholders syntactic, and originate
  inheritance at the subclass. Evidence: 15 graph regressions; live `compact_label` probe has 2
  definitions and 10/10 syntactic calls targeting the definition in the caller's file.

## M0 — Repair present contracts

- [x] M0.3 Harden duplicate-definition, shadowing, alias/import, and cross-file resolution behavior:
  remove project-wide bare-name fallback, block outer/module candidates behind local, parameter,
  receiver, and import bindings, resolve import edges from source modules rather than aliases, and keep
  unsupported aliases on unresolved syntactic placeholders. Evidence: 19 graph tests across Rust,
  Python, JavaScript, TypeScript-compatible syntax, Julia, and Clojure plus CLI JSON/DOT and MCP
  structured-output regressions; live corpus has zero resolved reference edges and zero false Clojure
  `:require` calls.
- [x] M0.4 Select distinct JavaScript, TypeScript, TSX, and supported dialect grammars: add the
  maintained official `tree-sitter-typescript` grammar, keep public language identity compatible,
  select TypeScript versus TSX from the source path, and migrate analyzer, metrics, graph, mutation,
  verifier, LSP, CLI, and MCP parsing/config consumers to the path-aware contract. Evidence: extension
  and positive/negative grammar truth table, typed graph/metrics regions, analyzer/LSP suppression,
  verifier TSX parse guard, honest mutation capability, and MCP TypeScript configuration regressions.
- [x] M0.5 Add shared typed TypeScript, JSX, TSX, and malformed fixtures with explicit parser
  recovery and exact behavioral-region assertions. Cover generic constraints/arrows, overloads,
  decorators, private fields, type-only import/export, `satisfies`, fragments, spread/member JSX,
  and generic JSX type arguments; prove valid analyzer/metrics/graph/protocol consumers and verifier
  rejection of malformed `.ts`/`.tsx`. Preserve public `TypeScript` identity and defer malformed-file
  cross-consumer policy to M0.8.
- [x] M0.6 Emit decorator-aware Python behavioral regions and add a shared async/decorator/
  nested-function/class-method fixture. Class declarations remain canonical declaration containers
  without hiding methods from behavioral duplication; long-method traversal evaluates nested
  callables; metrics use the decorated ownership span without duplicating the wrapped definition;
  protocol selects the nearest callable; graph containment remains file → function/class → nested
  callable with no synthetic decorator symbol. Evidence: exact CST kind/count/byte assertions plus
  parser, analyzer long-method/duplication, metrics, protocol, and graph consumer regressions.
- [x] M0.7 Correct Clojure branch/decision/nesting counting with contextual adapter callbacks and add a
  shared reader/macro-edge fixture. Control-form list heads contribute decisions; ordinary calls do not;
  `recur` is a flow break rather than a branch; quoted, syntax-template, var-quoted, reader-eval, and
  discarded forms do not contribute, while unquoted forms re-enter evaluated context. Treat `defmacro`
  and `defmethod` as behavioral regions. Evidence: exact reader CST counts/regions and measured
  cyclomatic/cognitive/nesting regressions for nested `if`/`when`, ordinary calls, macro templates,
  quote/discard edges, live forms, and `loop`/`recur`.
- [x] M0.8 Enforce fail-closed `unknown|complete|partial|unsupported|failed` provenance and exact
  parse diagnostics across scan, metrics, graph, LSP, MCP, slim, fix, baseline, and verification.
  Non-complete inputs are report-only: project-derived analysis is withheld; metrics expose no
  authoritative candidates/scores; graph retains file identity only; CLI read-only output exits 2;
  agent/propose output is atomic; MCP returns structured blocked files; safe fixes, work orders,
  prompts, imported work orders, provider consent/credentials, code actions, verify overrides, and
  writes cannot bypass the gate. Evidence: exact malformed TS/TSX diagnostic spans; legacy missing
  provenance defaults unknown; mixed-scan aggregate suppression; zero findings/orders/prompts/model
  calls/writes; metrics/4 null scores; graph/2 file-only; LSP diagnostic/no action; findings/2 and
  slim/2 schema regressions; scoped verifier rediscovery; full workspace and MCP feature gates.
- [x] M0.9 Replace the uncalibrated health/readability/refactor-confidence contract with
  `deslop.metrics/5` experimental heuristic burden. Remove the health alias, aggregate scalars,
  confidence bands, absolute threshold, and refactor candidates; expose typed
  `triage_only`/`gating_permitted=false` metadata, measurement support, transparent components, and
  complete-snapshot-only scan-relative outliers. Evidence: exact formula/distribution values,
  n<8/flat/no-absolute selection, partial null relative context, clean/sloppy `/5` smokes with zero
  legacy keys, neutral text, rejected health alias, MCP parity, and zero rewrite authority.
- [x] M0.10 Add the exact clean/sloppy, performance, duplicate-order, and false-resolution probes from
  `.agents/ALGORITHM_AUDIT.md` to automated regression suites. Evidence: honest clean/sloppy `/5`
  schema plus exact independent slop scores; 5-region/8-parse amplification and helper invariance;
  current 28 unique work orders conserving all 62 findings under overlapping/reordered inputs;
  exact 2-definition/10-call `compact_label` and 21-file/74-symbol/197-edge corpus graph probes;
  all 11 largest-region findings preserved through slim/verify; ignored measured self-scan probe.
- [x] M0.11 Run focused tests, then full fmt/build/test/clippy gates and record measured before/after values.
  Evidence: focused M0.10 contracts pass; workspace has 259 passing tests plus one intentional ignored
  performance probe; MCP `slim-llm` has 22; fmt, whitespace, workspace/minimal-slim builds, doc-tests,
  and warnings-denied clippy pass; `.agents/ALGORITHM_AUDIT.md` records the numerical before/after table.
- [x] M0.12 Separate the exact-byte `RevisionGuard` from the trimmed cross-revision baseline fingerprint;
  migrate region/work-order IDs explicitly and reject boundary-whitespace staleness. Evidence:
  byte-for-byte-compatible baseline identity plus typed domain-separated BLAKE3 `rg1_` guard;
  `wo2_`/workorder/2/patch/2/characterization-test/2, MCP workorders/2/fix/2, and slim/3 migration
  with no legacy write alias; exact line/byte regions; six boundary whitespace/newline mutations
  preserve matching identity but expire the guard; verifier, characterization, slim pre-egress,
  MCP, and CLI reject; apply writes zero and rechecks exact bytes immediately before replacement.
- [x] M0.13 Persist proposal analyzer config, capability, and source-revision context so verify/apply
  reconstruct the same work-order set instead of silently rescanning with defaults. Evidence:
  canonical `deslop.proposal-context/1` with effective thresholds/suppression/boundary/external
  settings, root-relative deduplicated scope, baseline exclusions, clean/finding/config source
  revisions and provenance, external availability, and expected-set digest; context-bound `wo3_`
  IDs plus workorder/3/patch/3/characterization-test/3, MCP workorders/3/fix/3, and slim/4; CLI,
  MCP, slim, characterization, verify, and apply reconstruct without defaults and reject legacy,
  tampered, mixed, peer-stale, capability-stale, scope-mismatched, or root-escaping contexts.
- [x] M0.14 Reconcile the `NeverAuto` contract as strictly report-only. Evidence: explicit typed
  proposal allowlist; overlapping, nested, and zero-width `NeverAuto` evidence quarantines the
  complete rewrite region while disjoint regions remain eligible; WorkOrder/prompt/agent validation,
  verifier reconstruction, slim, MCP, deterministic fix, and LSP deny rewrite authority; JSON/SARIF
  retain per-finding safety; supported-Julia boundary E2E proves scan visibility with zero CLI agent,
  workorder, prompt, model-call, check-command, verification, or write output even under overrides.
- [x] M0.DoD Demonstrate zero duplicate work-order IDs, zero falsely resolved ambiguous fixture edges,
  correct grammar selection, and honest partial/capability labels on the M0 corpus. Evidence: one
  executable CLI snapshot locks 30 workorders/30 IDs/30 targets/65 grouped findings; 21 files/74
  symbols/197 graph edges with 123 syntactic and zero falsely resolved reference edges; one actual
  ambiguous edge and the 10-call `compact_label` regression with zero false resolution; three complete
  typed dialect scans paired with the AST grammar truth table; two partial scans with zero metric
  regions/graph symbols; and one persisted unavailable JET capability observation.

## M1 — One parse, one owned syntax snapshot

- [x] M1.1 Write an ADR for `ProjectAnalysis`, source revisions, ownership, invalidation, and consumers.
  Evidence: `docs/adr/0001-project-analysis.md` fixes the immutable source/syntax ownership model,
  identity and wire domains, centralized root/grammar selection, invalidation matrix, consumer
  migration, partial-analysis authority, concurrency, consent, and executable M1 acceptance gates.
- [x] M1.2 Implement a revision/content-addressed source store and one parse owner per file revision.
  Evidence: `deslop-parse` now owns exact `sr1_` source revisions, reusable content-addressed
  `SourceStore` blobs, typed/default/exact scope snapshots, canonical alias/root-bound read plans,
  overlay-before-disk semantics, atomic stored grammar resolution, immutable per-file Trees and line
  indices, `ps1_`/`pa1_` identities, and isolated parse ledgers. Focused tests cover 26 parse cases,
  one request/owner/invocation for valid/partial revisions, zero invocation for invalid UTF-8,
  deterministic paths/variants, shared blobs, escapes, and exact-empty scope; full gates pass.
- [x] M1.3 Implement the owned node arena with raw kind, field, span, parent/children, named/error flags,
  token/trivia ownership, source slice, and grammar provenance.
  Evidence: `deslop-parse` now copies each private Tree into deterministic preorder
  `deslop-raw-arena/1` storage with visible and grammar kind IDs/names, incoming fields, exact
  byte/point spans, reciprocal ordered structure, named/extra/error/missing flags, grammar
  provenance, and a lossless token/trivia byte partition. File-boundary trivia has an explicit file
  owner; syntax-owned segments are contained by their owner. Arena slots remain private until M1.4.
  Thirty-four parse tests lock private-Tree parity, aliases/repeated fields, Unicode byte columns,
  comments/gaps, empty/whitespace inputs, zero-width missing nodes, partial TS/TSX recovery, exact
  source reconstruction, deterministic IDs, and unchanged one-parse ledgers; full gates pass.
- [x] M1.4 Define scan-local `NodeId`, serialized revision-bound `NodeKey`, cross-revision baseline
  fingerprint, and exact `RevisionGuard`; test collisions/expiry and prohibit fuzzy write authorization.
  Evidence: `deslop-parse` exposes owner-checked, non-Serde project-global `NodeId`; strict
  `deslop.node-key/1` identities tied to the exact repository/path/source/grammar and raw arena;
  validated structural anchors and collision ordinals; and collision-prone `nb1_` comparison evidence
  with no lookup or write-authority API. Forty-two parse tests lock reversed-input node order and
  topology, wrong-owner precedence, key round-trips/expiry/collisions, ambiguous baselines, structural
  digest vectors, and portable path rejection. Existing `rg1_` reconstruction remains exact and all
  compatibility, workspace, feature, build, formatting, whitespace, and strict clippy gates pass.
- [x] M1.5 Build containment and smallest-exclusive-region indices.
  Evidence: each owned arena now builds validated preorder subtree/depth and co-minimal zero-width
  indices before publication. Public owner-checked APIs provide O(1) structural containment and
  subtree iteration, zero-allocation whole-file/per-node exclusive token/trivia iteration, O(log S)
  byte ownership, O(log S + height) strict positive-range ownership via endpoint LCA, explicit named
  promotion, and unbiased point context with exact zero-width nodes plus separate before/after owners.
  Exhaustive tests lock 1,369 ordered node pairs, 254 reflexive/217 strict containment pairs, 1,953
  positive ranges, every byte in a 27-region partition, equal spans, root-external File ownership,
  missing/partial/empty/whitespace inputs, cross-file/foreign/out-of-range IDs, and all full gates.
- [x] M1.6 Implement exclusive local and declared inclusive aggregation APIs.
  Evidence: `deslop-parse` now initializes the File owner and every raw node exactly once, folds the
  positive-width token/trivia partition exactly once in source order, and derives separately named
  full-inclusive and normalized reset-aware declared projections bottom-up without revisiting bytes.
  Explicit reset `NodeId`s are owner/file/range validated before callbacks, deduplicated in preorder,
  and never inferred from raw kinds; fallible initialization/fold/merge callbacks retain exact
  owner/range/edge/projection context. Tests lock 37 owner/region visits over 62 bytes, the exact
  `2N-R=71` reset merge count, independent parent-chain oracles, conserved nested/equal-span/every-node
  partitions, 49-byte mixed File/root ownership, cross-build `NodeKey` determinism, partial/missing/
  empty/whitespace/unavailable syntax, invalid reset side-effect isolation, and unchanged parse
  ledgers. Parse has 47 passing tests; workspace, feature, build, rustdoc, formatting, whitespace,
  and strict all-target/all-feature clippy gates pass.
- [x] M1.7 Expose query/cursor-derived captures without reparsing source fragments.
  Evidence: exact-grammar `SyntaxQuery` values retain source, identity, capture names, quantifiers,
  pattern ranges/flags, properties, and general predicates. Grouped owned matches preserve engine
  match/capture order; a separate documented flat stream preserves source order. Every capture maps
  the retained private Tree's unique node identity to an existing owner-checked `NodeId` after full
  preorder Tree/arena parity validation, with no borrowed handle, fragment parse, path reread, or
  parse-ledger change. Built-in text predicates run against pinned bytes; unevaluated property/general
  predicates and match-limit partials fail closed. Tests lock all 37 named/anonymous/equal-span nodes,
  exact grouped/flat orders and duplicates, fields, NodeKey stability after a ten-node global shift,
  missing/ERROR nodes, JS/JSX dialect mismatch, all reachable compile-error kinds/coordinates,
  query-source ownership/u32 bounds, and owned Send/Sync/'static results. Parse has 56 passing tests;
  workspace, feature, build, rustdoc, formatting, whitespace, and strict all-target/all-feature clippy
  gates pass.
- [x] M1.8 Add edit/changed-range invalidation and explicit re-anchor-or-expire behavior.
  Evidence: immutable successor analyses reuse exact unchanged `ParsedFile` Arcs, parse compatible
  edited files against cloned old Trees, and match clean rebuild IDs, arenas, keys, queries, and
  provenance. Canonical old-to-final byte invalidation is separate from final-coordinate structural
  changed ranges, which are allowed to be empty. Plain derived diffs expire every changed-file node;
  verified sequential edit histories alone may return transition-local Tree-sitter subtree
  re-anchors after mapped-range, exact-byte, visible/grammar-kind, flag, field-path, and structural
  validation. Retained, re-anchored, removed, grammar-changed, syntax-unavailable, and changed nodes
  have explicit outcomes. Tests pin exact ledger counts, duplicate ambiguity, multi-edit coordinates,
  partial/empty/invalid UTF-8 files, rename lifecycle, u32/UTF-8 bounds, and predecessor immutability.
  Parse has 66 passing tests; workspace, slim MCP, build, rustdoc, formatting, whitespace, and strict
  all-target/all-feature clippy gates pass.
- [x] M1.9 Migrate analyzer and metrics consumers to the shared snapshot.
  Evidence: analyzer and metrics primary, path, and source-compatibility APIs now construct or accept
  one immutable `ProjectAnalysis`; owned `NodeId`/adapter facts drive all syntax rules, duplication,
  boundary analysis, nested/reset-aware metrics, and exact dialect dispatch. A shared planner pins
  source/config bytes, repository identity, discovery, and presentation before projection identity.
  Static production guards reject legacy parse/read/pack-reselection calls, source adapters record
  zero legacy parser invocations, and complete files retain exact cold ledger `1/1/1/0`. Partial
  sources and invalid UTF-8 withhold project-negative claims. Unpinned live external analyzers are
  capability-reported unavailable. Workspace tests, strict all-target/all-feature clippy, build,
  rustdoc, formatting, and whitespace gates pass.
- [x] M1.10 Migrate graph, evaluator, LSP, MCP/protocol, and slim consumers.
  Evidence: graph, analyzer/protocol, evaluator, and LSP now consume retained
  `Arc<ProjectAnalysis>` values plus pinned presentation/source maps. Graph traversal is entirely
  `NodeId`/`NodeView` based; proposal grouping uses owned containment and adapter facts; evaluator
  batches its corpus into one snapshot; and LSP open/change/close builds one workspace-wide overlay
  generation shared by every dirty document, while save without text reuses the current revision.
  MCP and slim delegate through
  those migrated path/proposal APIs, with remaining reads limited to explicit config, JSONL,
  provider, apply, or stale-state I/O. Static guards reject production parse/read/reselection,
  repeated consumers preserve cold `1/1/1/0` ledgers, and LSP revisions record zero legacy parser
  calls. All-feature workspace tests, strict all-target clippy, build, warnings-denied rustdoc,
  formatting, and whitespace gates pass.
- [x] M1.11 Instrument parse counts, ownership invariants, deterministic node order, latency, and memory.
  Measure and compact M1.4's repeated per-node `FileRevisionKey`/field-path storage, allocating
  `NodeView::children`, linear range/key lookups, M1.5 index storage, point-context allocation, and
  M1.6's retained local/full/declared aggregate values and caller-defined clone/merge costs, plus
  M1.7's retained query source/metadata/result strings and per-execution Tree preorder/`Node::id`
  map, and M1.8's O(K*B) sequential edit validation, rebuilt edited-file arena/keys, O(N) transition
  map, and O(total project nodes) successor assembly, before declaring the traversal API
  migration-ready.
  Evidence: identity-neutral analysis/query/aggregation/point/update reports lock parse ownership,
  node order, visible retained bytes, callback/value counts, exact/derived edit work, rebuilt nodes,
  and transitions. On the fixed Rust/Python/TSX matrix, exact structure remains 3 files, 188 bytes,
  94 nodes, 91 edges, and digest
  `pao1_437c1bdc53a43224fde0a0c23fcebbca531996848a87585944f60fe5759c55ed`.
  Shared revision/field-path payloads reduce node-key storage from 75,873 to 36,195 bytes; the final
  visible lower bound is 61,900 versus the 98,234-byte baseline (36,334 bytes / 37.0% lower) after
  adding compact key/query indices. Children and point contexts are allocation-free iterators,
  file/range/key lookup is logarithmic, capture names share query-owned `Arc<str>` payloads, query
  execution reuses a 1,504-byte retained node index, and all-descendant aggregation avoids 38
  redundant declared values. Five ignored probe runs report timing without gating correctness;
  all-feature workspace tests, build, warnings-denied rustdoc, strict clippy, format, and whitespace
  pass.
- [x] M1.DoD Prove one parse per file revision in all scan/propose paths and no borrowed-node lifetime or
  overlapping exclusive-metric errors on the gold fixture matrix.
  Evidence: the joined Rust/Python/TSX/Clojure/Julia contract pins 5 files, 1,651 bytes, 746 nodes,
  700 gap-free exclusive regions, 21 analyzer findings, 17 metric regions, a 45-node/49-edge graph,
  and 9 work orders grouping 17 findings. Analyzer, metrics, and graph repeat deterministically over
  the same `ProjectAnalysis`; cold ownership is exact `5/5/5/0`, the unchanged warm successor is
  `5/5/0/5`, all 746 transitions are retained, each disk input is read once, and proposal batches
  retain their exact analysis/ledger. The private metric oracle assigns all 1,651 bytes and 67
  nonblank lines exactly once across 17 semantic owners despite nested regions. Public-surface and
  compile-fail guards reject borrowed Tree-sitter node/cursor handles and `NodeId` serialization.
  The unchanged M0 numerical gate, all-feature workspace tests, build, warnings-denied rustdoc,
  strict clippy, format, and whitespace pass.

## M2 — Language-adapter contract

- [x] M2.1 Version the adapter/capability schema for `S0` through `S4`.
  Evidence: `deslop.language-adapter-capabilities/1` pins one ordered 23-entry catalog across
  `S0..S4` with exact tier counts `6/4/6/5/2`; every declaration is total and explicitly
  `provided`, `unsupported`, or `unknown`, with authority required only for provided facts.
  All seven registry packs publish valid manifests and honestly derive no complete tier until
  canonical roles exist. Malformed totals/order/authority and adapter-schema mismatches fail closed;
  a capability-only change preserves raw analysis identity but changes stored adapter/projection
  identity. Exact JSON, tier truth-table, registry, snapshot, and identity tests pass together with
  all-feature workspace tests, build, warning-denied rustdoc, strict clippy, format, and whitespace.
- [x] M2.2 Define canonical roles and retain raw grammar kinds/fields alongside them.
  Evidence: `deslop.canonical-roles/1` pins 23 composable roles and a canonical sorted,
  duplicate-free wire set. `deslop.canonical-role-projection/1` is capability-gated, retains its
  owning analysis, and pairs every `NodeId`/role set with raw visible kind/id, grammar kind/id, and
  parent field. The fixed custom-adapter oracle locks 32 nodes, 11 raw fields, and 22 assignments,
  including an aliased `type_identifier`/`identifier`; unknown production capability fails typed.
  Raw analysis identity and `NodeKey/1` remain unchanged. Exact wire/malformed-input tests and all
  workspace tests, build, warning-denied rustdoc, strict clippy, format, and whitespace pass.
- [x] M2.3 Define query packs for declarations, references, scopes, control, comments, and opaque/generated code.
  Evidence: `deslop.language-query-pack/1` makes all six ordered families total with explicit
  provided/unsupported/unknown support, authority, exact query source, capture names, and role sets.
  Exact packs are stored in framed adapter identity. `deslop.language-query-projection/1` compiles
  provided entries against the stored grammar, retains unknowns and its analysis, and rejects capture
  drift. A fixed custom pack executes capture counts `[1,1,2,1,1,2]` (8 total) with no reparse;
  query-only changes preserve raw analysis identity and change projection identity. Seven production
  registry packs remain honest all-unknown. Full workspace gates pass.
- [x] M2.4 Define operator/token classification and language-specific lexical policies.
  Evidence: `deslop.language-lexical-policy/1` freezes nine token classes, eight operator classes,
  case/Unicode identifier behavior, comment delimiters, exact ordered raw-kind/text rules, and a
  required terminal fallback. The stored policy is adapter-schema checked and identity-framed.
  `deslop.lexical-token-projection/1` retains the owning analysis, selects explicitly classified
  composite token owners otherwise leaves, emits non-overlapping exact spans, and never reparses.
  The fixed Unicode/comment/operator fixture locks 26 facts and class counts; policy-only changes
  preserve raw analysis identity while changing derived identity. Production packs remain honest
  unknown pending M2.6-M2.10. Exact wire/rejection tests and all workspace gates pass.
- [x] M2.5 Define parse-error, unsupported-construct, macro, generated-code, and dialect policies.
  Evidence: `deslop.language-construct-policy/1` makes parse recovery, unsupported constructs,
  macros, generated code, and dialect variants independently explicit with support, authority,
  handling, strict ordered rules, and exact dialect/grammar/version identities. The policy is stored
  and framed in adapter identity. `deslop.construct-policy-projection/1` retains its analysis and
  emits only grammar error/missing flags or exact adapter-rule facts; it rejects claimed dialect
  drift and never reparses. A fixed malformed Rust-grammar fixture locks four facts: generated
  attribute, opaque unsafe block, opaque macro invocation, and file-incomplete `ERROR`; unknown
  policy emits no construct claims. Policy-only identity, adapter-schema, legacy-wire, and all
  workspace gates pass. Production packs remain honest unknown pending M2.6-M2.10.
- [x] M2.6 Implement/repair the Rust adapter and golden fixtures.
  Evidence: production `RustPack` now provides composable canonical roles, all six syntactic query
  families, a total Unicode/comment/operator lexical policy, file-incomplete recovery, exact
  macro/generated/unsafe policies, and `rust/tree-sitter-rust/0.24.2` dialect provenance. Its
  manifest honestly derives S1 while S2-S4 remain unknown. Valid/malformed goldens lock 161 CST
  nodes, 110 token owners, 90 role assignments across 17 roles, query captures `[5,2,5,1,2,3]`,
  six construct regions, and one exact `ERROR` fact, with one parse per file. The literal `*` raw
  token is now distinct from the terminal wildcard. All workspace gates pass.
- [x] M2.7 Implement/repair JavaScript, TypeScript, and TSX adapters and golden fixtures.
  Evidence: JavaScript and TypeScript now share a production ECMAScript role/lexical/recovery policy
  while retaining distinct JavaScript, TypeScript, and TSX query compilation and exact stored grammar
  provenance. All three dialects derive S1 and leave S2-S4 unknown; macros are explicitly unsupported,
  `with_statement` is opaque, and generated facts require exact markers. Fixed goldens lock role/token
  totals JS 61/42, TS 143/90, TSX 107/68; query vectors `[1,1,3,0,2,1]`, `[4,2,3,0,1,0]`, and
  `[3,0,2,0,1,0]`; exact generated/unsupported facts; malformed TS/TSX error evidence; and one parse
  per file. All workspace gates pass.
- [x] M2.8 Implement/repair Python adapter and golden fixtures.
  Evidence: production `PythonPack` now provides composable canonical roles, all six syntax query
  families, a total Unicode/comment/operator lexical policy, file-incomplete recovery, opaque legacy
  `exec`/`print`, exact generated markers, and `python/tree-sitter-python/0.25.0` provenance. It derives
  S1 while S2-S4 stay unknown. Valid/malformed goldens lock 127 CST facts, 75 token owners, 108 role
  assignments across 21 roles, query captures `[4,1,8,3,2,2]`, four exact construct facts, one exact
  malformed `ERROR`, query-to-role consistency, and one parse per file. Exact-text keyword rules keep
  `await`/`lambda`/`type`/`yield` composites from suppressing their operands. All workspace gates pass.
- [x] M2.9 Implement/repair Clojure adapter and golden fixtures.
  Evidence: production `ClojurePack` now derives S1 with evaluated list-head canonical roles, total
  symbol/operator lexical ownership, file-incomplete recovery, explicit opaque reader-macro and `#=`
  policies, exact generated markers, and `clojure/tree-sitter-clojure/0.1.0` provenance. Goldens lock
  160 CST facts, 90 token owners, 183 role assignments across 14 roles, safe query vector
  `[0,0,1,0,2,7]`, nine exact construct facts, one exact malformed `ERROR`, quoted-control non-leakage,
  and one parse per file. Declaration/reference/control queries remain honestly unknown because stored
  Tree-sitter queries cannot exclude arbitrary quoted ancestors; scope/comment/reader queries are
  provided. Grammar-field head extraction also repairs metadata-prefixed metric/region forms. All
  workspace gates pass.
- [x] M2.10 Implement/repair Julia adapter and golden fixtures.
  Evidence: production `JuliaPack` now derives S1 with canonical roles, all six direct grammar queries,
  total Unicode/comment/operator lexical ownership, file-incomplete recovery, opaque macros/quoted ASTs,
  exact generated markers, and `julia/tree-sitter-julia/0.23.1` provenance. Valid/malformed goldens lock
  95 CST facts, 61 token owners, 94 role assignments across 18 roles, query captures `[2,4,2,2,3,3]`,
  five exact construct facts, one exact whole-file `ERROR`, interpolation ownership, query-role
  consistency, and one parse per file. Call argument lists are not mislabeled parameters; exact named
  assignment operators do not fall through. S2-S4 and StaticLint/JET authority remain unknown. All
  workspace gates pass.
- [x] M2.11 Add cross-adapter construct matrices and unsupported-capability leakage tests.
  One retained-analysis oracle now spans Rust, JavaScript, TypeScript, TSX, Python, Clojure, and
  Julia with 21 valid/malformed/near-marker sources. It pins exact dialect triples, query and
  construct support/payload matrices, construct counts and generated texts, syntax-only malformed
  recovery, contextual Clojure non-leakage, S1 ceilings with every S2-S4 capability unknown, and one
  parse per source. All workspace gates pass.
- [x] M2.DoD Every emitted fact declares adapter/version/capability/provenance, and no confirmed output
  requires a higher tier than the adapter supplies.
  Evidence: the joined seven-dialect DoD oracle walks 854 role facts, 536 token facts, 28 construct
  facts, and 88 query captures through their exact node, raw grammar evidence, retained projection,
  adapter schema `/2`, stored grammar dialect/version, and capability/policy authority. It locks 640
  canonical-role assignments, four syntax-only analyzer findings with zero AnalyzerConfirmed output,
  15 metric regions, and a 15-symbol/42-edge graph whose 27 non-containment edges have zero resolved
  claims while S2-S4 remain unknown. The audit repaired Rust scoped/field and TypeScript member callee
  role omissions, keeps all seven parses one-per-file, preserves M0/M1 DoD, and passes every workspace
  gate.

## M3 — Scope and project-name graph

- [x] M3.1 Write an ADR for scope, resolution paths, ambiguity, and authority precedence.
  ADR 0002 accepts versioned `deslop.scope-graph/1` and `deslop.resolution/1` contracts with exact
  build-context identity, language-declared scopes/namespaces/precedence, all viable/rejected candidate
  paths, Complete/Partial/Unsupported/Failed coverage, Unique/Ambiguous/Unresolved/Unknown/Conflict
  outcomes, separate lookup and evidence authority, import/export/re-export rules, incremental reverse
  dependencies, consumer gates, adversarial verification, and graph/2 syntactic-only compatibility.
  Structural ADR checks and every workspace gate pass.
- [x] M3.2 Model scopes, definitions, references, bindings, imports/exports, visibility, and shadowing.
  `deslop.scope-graph/1` now provides analysis-owned dense fact handles, complete-payload revision keys,
  strict build-context/policy-bound wire documents, exact node/raw/canonical/grammar/adapter/capability
  evidence, explicit coverage reasons, all ten ADR fact classes, and validated namespace, visibility,
  parent, link, module, and shadowing invariants. A 14-fact executable fixture plus adversarial schema,
  identity, ownership, and authority tests pass. Production adapters remain S1 with S2/S3 Unknown; M3.3
  still exclusively owns executable resolution rules and capability promotion. Every workspace gate and
  the unchanged M0/M1/M2 definition-of-done tests pass.
- [x] M3.3 Implement per-language declarative resolution rules or an explicitly equivalent adapter.
- [x] M3.4 Store all candidate paths and unique/ambiguous/unresolved status; prohibit bare-name resolution.
  Strict `deslop.resolution/1` documents now retain every viable, rejected, and unknown path with exact
  edges, checks, precedence, endpoint, source-fact closure, per-path/result coverage and authority, then
  derive coverage-bounded Unique/Ambiguous/Unresolved/Unknown/Conflict status by distinct maximum
  endpoints. Deferred imports, unresolved qualifications, dynamic boundaries, missing precedence, and
  adapter-rejected duplicates remain Unknown; lower-precedence paths remain stored. Payload-bound keys,
  owner-checked dense handles, strict wire validation, 12 focused cases, 115 parse tests plus four
  compile-fail doctests, and every all-feature workspace gate pass. Production capability declarations
  remain unchanged and repository-global bare-name lookup/order-based selection is absent.
- [x] M3.5 Stitch file/module/package/build-target names incrementally.
  Exact build-context package/target/module declarations now constrain alias, selective, glob, export,
  and re-export paths without file-stem or global-name inference. Explicit export-set coverage gates
  terminal outcomes; wrong-target paths remain rejected and pure re-export cycles remain Unknown.
  Stable revision-bound fact keys plus reverse dependency invalidation give byte-identical successor/
  clean documents: unrelated edits reuse 5/rebuild 0 results, export additions reuse 1/rebuild 5, and a
  newly matching module rebuilds the formerly unresolved reference. All workspace gates pass; production
  adapter semantic capabilities remain unchanged.
- [x] M3.6 Add optional compiler/LSP semantic facts with higher authority and conflict reporting.
  `deslop.language-adapter-capabilities/2` distinguishes LanguageServer from Compiler authority, while
  strict `deslop.semantic-resolution-facts/1` documents pin provider executable/configuration/project-
  model/result artifacts, graph/build identity, coverage, references, endpoints, and diagnostics.
  Resolution retains adapter and every provider conclusion/path: compiler outranks complete LSP, complete
  LSP outranks adapter, lower disagreement is Conflict with a preferred diagnostic, equal-authority
  disagreement has no winner, and incomplete facts cannot authorize or conflict. Semantic artifact changes
  rebuild exactly their references (4 reused/1 rebuilt for one result artifact; 3/2 for shared provider
  configuration) with byte-identical clean successors. All workspace gates pass; runtime evidence remains
  orthogonal and production adapter authority is unchanged.
- [x] M3.7 Add duplicate-name, nested scope, wildcard/alias import, re-export, dynamic, and unresolved fixtures.
  A versioned 16-case adversarial gold corpus freezes 36 retained paths (13 viable, 18 rejected, 5
  unknown) with exact semantic endpoint labels, traversal edges, rejection/check evidence, provenance
  kinds, authority, coverage, and dynamic boundaries. It includes unrelated same-spelled declarations,
  equal-precedence duplicates, nested explicit shadowing, namespace/visibility/timing rejection, selective/
  alias/glob imports, re-exports and cycles, dynamic/deferred/qualified unknowns, and complete versus partial
  zero-candidate absence. All workspace all-feature gates pass; production resolution authority is unchanged.
- [x] M3.8 Measure resolution precision/recall and incremental file-isolation behavior.
  The published M3.8 report scores the frozen 16-case corpus with a full five-status confusion matrix:
  diagonal counts `[7,1,2,6,0]`. Ten Complete cases have 27/27 exact path and 18/18 endpoint precision/
  recall; all six expected Unknown cases remain counted with 9/9 paths and 5/5 endpoints. Four clean-parity
  incremental scenarios prove 5 reused/0 rebuilt plus one new unrelated reference, 0/1 for a reachable
  Unique→Ambiguous duplicate, 1/5 for an export cone, and 0/1 for exact module appearance. All workspace
  gates pass; production authority is unchanged.
- [x] M3.DoD Meet the frozen gold-corpus resolution gate and block semantic recipes wherever authority is
  incomplete or ambiguous. The public `deslop.resolution-consumer-gate/1` accepts only exact projection/
  result identity, capability-specific static authority, and projection-bound downgrade-only dependency
  evidence; it has no graph/2 fallback. Exactly the seven Complete Unique frozen cases pass, while all nine
  non-eligible cases plus provider conflict, insufficient authority/capability, incomplete dependency, and
  foreign evidence cases block without exposing an endpoint. All workspace all-feature gates pass.

## M4 — CFG, PST, PDG, and SDG

- [x] M4.1 Define the control-edge schema for entry/exit, normal, branch, loop, exceptional, abrupt, and suspension flow.
  Accepted ADR 0003 and public `deslop.control-flow/1` bind exact analysis/projection/policy, owner/grammar/
  adapter capability evidence, virtual entry/exit, owner-contained points, and payload-bound graph/point/edge
  keys. Eight typed edge families and 35 portable sub-kind instances round-trip strictly; malformed topology,
  stale keys, incomplete authority/coverage, conservative/recovered evidence, and graph/2 fallback fail closed.
  All workspace all-feature gates pass; production ControlFlow authority remains Unknown pending M4.2.
- [x] M4.2 Implement CFG lowering for each adapter at its declared capability tier.
  Strict stored `deslop.language-control-flow-rules/1` packs and adapter schema `/3` bind lowering behavior to
  snapshot identity, capability authority, and exact grammar dialect. Rust is Provided/Adapter with a
  fixture-backed 17-rule catalog and explicit Partial boundaries; Clojure, Julia, Python, JavaScript, and
  TypeScript remain honest Unknown gaps. Shared owned-arena lowering preserves labeled abrupt targets,
  reachability, coverage reasons, and deterministic graph identity. All workspace gates pass.
- [x] M4.3 Compute dominance/post-dominance and hierarchical SESE/PST regions.
  Accepted ADR 0004 and strict `deslop.control-regions/1` bind exact CFG/projection/policy identity, independent
  entry/exit reachability domains, full and immediate dominance relations, coverage, structured root/branch/
  loop hammocks, laminar hierarchy, and residual candidates. Eight numerical/adversarial suites and all
  workspace gates pass.
- [x] M4.4 Preserve irreducible control regions as explicit non-structured/unknown facts.
  Accepted ADR 0005 and strict `deslop.non-structured-control-regions/1` bind exact M4.1/M4.3 source
  projections and policies, iterative entry-reachable SCC classification, canonical external boundaries,
  typed residual provenance, inherited coverage, and payload identities. Multi-entry irreducibility,
  exit-unreachable nontermination, and incomplete-flow unknown facts remain outside the structured PST. Eight
  numerical/adversarial suites and all workspace gates pass.
- [x] M4.5 Implement def/use, reaching definitions, liveness, parameter/output, and conservative effect facts.
  Accepted ADR 0006 and strict `deslop.data-flow/1` join exact Complete Unique M3 resolution evidence to
  exact M4 CFG points. Ordered def/use events drive entry-reachable reaching definitions and liveness;
  parameter/return/mutation boundaries and conservative effects remain explicit, capability-bound facts.
  Eight numerical/integration/adversarial suites and all workspace all-feature gates pass.
- [x] M4.6 Build local PDGs from control and data dependence.
  Accepted ADR 0007 and strict `deslop.program-dependence/1` bind exact CFG/region/non-structured/dataflow
  sources. Direct control edges require complete post-dominator chains; flow edges require exact reaching
  definitions; unresolved/nonterminating evidence becomes typed gaps. Eight focused suites and all workspace
  all-feature gates pass.
- [x] M4.7 Build call/parameter/return/global summaries and SDG edges where resolution authority permits.
  Accepted ADR 0008 and strict `deslop.system-dependence/1` bind exact M3/M4.5/M4.6 sources, one callable
  summary per local PDG, graph-specific CallGraph/Sdg support plus authority, explicit actual/formal and
  output/receiver bindings, exact local-callee resolution, global summaries, four interprocedural edge kinds,
  and typed gaps. Eight focused suites and all workspace all-feature gates pass.
- [x] M4.8 Add exception, async/yield, closure, mutation, alias uncertainty, and early-exit fixtures.
  Eight focused suites freeze typed exceptional/suspension/abrupt CFG topology, total dual reachability,
  honest production await/yield/closure Partial coverage, no early-return fallthrough, conservative Capture/
  Borrow/ReadWrite semantics, advanced outputs/effects, ambiguous-capture PDG gaps, and SDG retention without
  fabricated interprocedural edges. All workspace all-feature gates pass.
- [x] M4.9 Compare graph edges/regions with hand-labelled gold fixtures and compiler facts where available.
  Strict external `deslop.m4-graph-gold/1` freezes 50 normalized semantic vectors across CFG, PST, and PDG;
  exact comparison, schema/dangling validation, and three semantic mutation classes pass. All six production
  adapters explicitly lack a retained compiler-authoritative graph oracle, so compiler comparison remains an
  exact unavailable result rather than borrowed resolution authority. All workspace gates pass.
- [x] M4.DoD Pass frozen CFG/PST/PDG gold gates and propagate every missing/uncertain semantic fact into
  recipe eligibility. Public strict `deslop.graph-recipe-eligibility/1` requirements walk the exact retained
  CFG/PST/non-structured/dataflow/PDG/SDG source chain and return content-bound canonical blocks for every
  incomplete coverage reason, capability/authority gap, conservative edge, residual, non-structured fact,
  access/effect/call uncertainty, PDG/SDG gap, missing layer, or foreign source. Seven DoD suites lock Complete
  positive cases, exact 9- and 25-block incomplete matrices, missing/partial/foreign SDG behavior, strict wire
  rejection, and the 50-vector M4 gold join. All workspace all-feature gates pass.

## M5 — Candidate detectors and transformation recipes

### Recipe framework

- [x] M5.1 Version `TransformationRecipe` and `TransformationCandidate` schemas. Strict
  `deslop.transformation-recipe/1` and `deslop.transformation-candidate/1` wires retain content-bound `rcp1_`
  and `tcn1_` identities, exact M4 eligibility, source projection IDs, targets, edits, and revision guards.
- [x] M5.2 Implement required facts, `Proven`/`Disproven`/`Unknown` preconditions, forbidden conditions,
  authority evidence, and capability checks; permit only `Proven` automatic obligations. Automatic candidates
  require `SafeAuto`, eligible graph evidence, every required obligation Proven, and every forbidden condition
  Disproven; strict mutation tests reject weaker or stale payloads.
- [x] M5.3 Implement expected graph deltas, impact-cone queries, safety class, validation plan, and rollback
  metadata. Canonical typed changes, bounded directional PDG traversal, exact validation/rollback plan binding,
  guarded patch validation, expected-removal checks, and byte-exact rollback are executable tests.
- [x] M5.4 Add recipe fixture conventions: positive, no-op, minimal counterexample, and adversarial near-miss.
  Every recipe must declare exactly the four canonical roles; the first recipe executes all four through the
  retained CFG/PST/PDG chain.

### Branch/control flow

- [x] M5.5 Detect equivalent arms and common prefix/suffix factoring with effect/order constraints.
  Rust `if` candidates now cover exact equivalent arms and exact common boundary fragments, retain one
  condition evaluation before the factored fragment, reject recovered/conservative branch edges and comment/
  attribute-bearing rewrites, and carry explicit `Unknown` DefUse/Effects evidence. Production candidates are
  therefore deterministic `SafeWithPrecondition` review work orders and can never enter automatic apply.
- [x] M5.6 Detect safe adjacent-condition merges with short-circuit and exception constraints.
  Rust detection covers nested no-fallback `&&`, nested shared-fallback `&&`, and shared-success `||` forms.
  It proves exact left-to-right evaluation count and retained outcome bodies from two exact dispatches; abstains
  on recovered/conservative edges, let conditions/chains, comments, and mismatched outcomes; and retains
  production Effects uncertainty. Candidates are deterministic `SafeWithPrecondition` review work orders and
  cannot enter automatic apply.
- [x] M5.7 Detect independent branch splits from dependence slices. Rust no-`else` branches containing two to
  eight direct call statements now receive one flow closure per action; any overlapping slice or crossing Flow
  edge suppresses the proposal. The rewrite stores the predicate once and retains action order. Production
  DefUse/Effects/LocalPdg gaps keep independence and scope/drop obligations explicit `Unknown`, so candidates
  are `SafeWithPrecondition` review work orders and cannot enter automatic apply.
- [x] M5.8 Detect guard-clause/condition inversion candidates from PST and exit facts. Rust
  statement-position `if`/`else` branches now support direct-return guards in either polarity when exact branch,
  abrupt-exit, virtual-exit, merge-reachability, and PST point facts are retained. Statement-only continuations
  are flattened; comments, let conditions, tail-valued branches, non-direct exits, and conservative paths
  abstain. Production DefUse/Effects gaps keep scope, lifetime, drop, and effect obligations `Unknown`, so
  candidates are `SafeWithPrecondition` review work orders and cannot enter automatic apply.
- [x] M5.9 Detect dead arms and exhaustive chain-to-match/table candidates. Exact Rust `true`/`false` branches
  with explicit block arms now produce selected-block dead-arm reviews when the discarded tree has no comment,
  attribute, or macro boundary. Two-to-six `==` comparisons over one identifier, distinct literal/qualified-path
  cases, and an explicit fallback now produce one final-wildcard match-table review. Rust match lowering now
  retains exact case/default CFG edges for unguarded final-wildcard forms; guarded or non-wildcard exhaustiveness
  stays conservative. Production type/DefUse/Effects gaps keep both recipes review-only.
- [x] M5.10 Emit before/after graph evidence and counter-evidence for every branch candidate. M5.5-M5.9 now all
  expose retained dispatch, slice, PST, exit, selected/dead-arm, and match-table entities; expected modify/remove/
  preserve deltas; and capability-tagged control, scope, equality, effect, and non-structured counter-evidence.

### Functions and expressions

- [x] M5.11 Generate extract-method candidates from SESE regions and complete computation/object-state slices.
  `rust-extract-sese-branch-method` now emits one exact compiling helper transaction for bounded direct-body Rust
  branch regions: free non-generic synchronous functions, primitive/reference parameter frontier, no prior locals,
  and no abrupt/suspending/macro/unsafe/capture boundary. Candidates retain the exact SESE entity, bidirectional
  flow-closed computation slice, region object-state boundaries/effects, touching flow edges, and expected graph
  changes. Slice completeness is Proven only with Complete authoritative DefUse/Effects/LocalPdg and no typed gap;
  current production gaps keep candidates review-only. Generated helpers do not recursively re-extract.
- [x] M5.12 Infer exact extraction inputs, outputs, mutations, exits, exceptions, captures, and async/ownership
  constraints. Extract-method v2 selects only used typed parameter/prior-local inputs, classifies copy/shared-borrow/
  mutable-reborrow ownership and direct writes, and supports unit statements or directly typed primitive initializer
  outputs. Candidate evidence separately retains all seven signature dimensions; current partial DefUse/Effects
  authority leaves mutation and exception absence Unknown and review-only. Typed initializer CFG/SESE lowering is
  exact while let-else remains conservative. A four-case executable before/after matrix matched exactly, sixteen
  unsafe shapes abstained, all workspace gates passed, and the installed CLI was replaced.
- [x] M5.13 Detect multi-responsibility callable splits from dependence cohesion/action clusters.
  `rust-split-dependence-cohesive-callable` finds two-to-four direct-body branch action cores with retained internal
  PDG connectivity, exact M5.12 signatures, disjoint computation frontiers, and no retained crossing Flow edge. It
  emits one atomic multi-helper callable replacement. Production DefUse/LocalPdg gaps keep frontier independence
  Unknown and review-only. Compiled pre/post behavior, five near misses, strict wire/rebuild/CLI checks, all workspace
  gates, and the installed selector smoke passed.
- [x] M5.14 Detect safe merge/inline of over-fragmented single-use helpers.
  `rust-inline-exact-single-use-helper` consumes Complete SystemDependence, DataFlow, and resolution authority for
  one private zero-parameter implicit-unit Rust helper with exactly one direct call/reference. It emits one atomic
  call-block replacement plus helper deletion, preserving nested temporary/drop scope and retaining call-frame/
  panic-location review. Compiled before/after behavior matched; second-call, function-value, and public-boundary
  cases abstained. Production lacks exact call facts and therefore fails closed with zero candidates. All workspace
  gates passed, the CLI was replaced, and the installed selector smoke returned `[]` without guessing a binding.
- [x] M5.15 Add def/use/effect-grounded temporary, expression, and independent-statement recipes.
  Three strict Rust selectors now inline an adjacent exact single-use temporary, remove a reachable semantically
  empty literal expression, and remove an independent unused literal local. Complete DefUse/Effects/LocalPdg is
  mandatory; exact reaching definitions, use counts, point effects, and CST shapes authorize the edits. The two
  literal deletions are automatic; temporary inlining remains review-only for source-location observations. One
  combined compiled fixture preserved output `19` while two-read, typed-local, and operator near misses abstained.
  Production honestly returns zero under partial authority. All gates and three installed-selector smokes passed.

### Dependencies and modules

- [x] M5.16 Build file/module/package/build/API dependency projections. Strict `deslop.dependency/1`
  derives File/Module/Package/BuildTarget/local-and-external-API nodes only from retained resolution and exact
  BuildModule facts. It preserves containment, level dependencies, API use, authority, coverage, and typed gaps;
  six adversarial tests and every workspace gate pass. Partial authority never makes an absent edge negative proof.
- [x] M5.17 Compute SCCs, condensation DAG, layers, fan-in/out, instability, and architecture-rule violations.
  `deslop.architecture/1` now derives deterministic SCCs at every structural level, a condensation DAG,
  dependency-first layers, distinct fan/API metrics, exact rational instability, and evidence-bearing policy
  violations/gaps from `deslop.dependency/1`; eight numerical/adversarial tests and every workspace gate pass.
- [x] M5.18 Generate reviewed cycle-breaking seams with API/data-flow evidence.
  Strict `deslop.cycle-seams/1` now emits only API-backed internal cuts for cyclic SCCs, joins exact matching
  data-flow access/reaching-definition evidence, ranks canonical review-required candidates, and preserves every
  authority gap. The retained corpus produces 8 exact candidates; topology-only cycles produce 0. All gates pass.
- [x] M5.19 Generate move/split/merge-module candidates from cohesion, coupling, impact, and optional change history.
  Strict `deslop.module-restructure/1` now emits deterministic review-required moves, splits, and merges from exact
  ownership, dependency, API-impact, cycle-seam, and optional content-bound history evidence. Nine focused tests,
  235 active parse tests (1 explicit ignore), 4 doctests, and every workspace gate pass.
- [x] M5.20 Add semantically safe import/declaration ordering recipes.
  Added explicit ScopeGraph/Resolution eligibility and two guarded Rust selectors for simple import blocks and
  private hoisted-function blocks. The exact fixture emits 2 review candidates whose combined rewrite preserves
  output `2`; 8 focused tests, production/CLI fail-closed checks, and every workspace gate pass.

### Clones, ceremony, dead code, clarity

- [ ] M5.21 Implement exact subtree fingerprints and renamed-token normalization.
- [ ] M5.22 Implement scalable candidate indexing and graph-context clone verification.
- [ ] M5.23 Collapse pair matches into maximal clone classes and one coordinated candidate.
- [ ] M5.24 Classify generated/schema/test/public-API/intentional repetition before abstraction proposals.
- [ ] M5.25 Add graph-grounded forwarding, conversion/allocation, wrapper, repeated-error, and dead-code candidates.
  Partial vertical slice complete: `rust-remove-unreachable-literal-statement` removes only exact
  entry-unreachable inert Rust literal statements, fails closed on recovered/conservative/non-structured or
  non-literal forms, and completes candidate -> guarded patch -> expected delta -> validation -> rollback.
- [ ] M5.26 Add role/scope-aware identifier and comment evidence without automatic rationale deletion.
- [ ] M5.DoD Every enabled detector completes graph fact -> unique candidate -> patch -> expected delta ->
  verification -> rollback on its fixtures, with no known unsafe `safe-auto` counterexample.

## M6 — Work-order DAG and LLM protocol

- [ ] M6.1 Version one shared `WorkOrder` schema for library, CLI, MCP, LSP, and slim.
- [ ] M6.2 Include target identity, recipe, evidence/counter-evidence, impact, safety, patch budget,
  verification contract, and machine-readable `Reads`/`Writes`/`Requires`/`Invalidates` sets.
- [ ] M6.3 Add prerequisite, invalidation, conflict, and mutually-exclusive-recipe edges.
- [ ] M6.4 Collapse atomic work groups, detect SCCs, and block unresolved planning cycles.
- [ ] M6.5 Topologically schedule independent work and serialize conflicting graph commits.
- [ ] M6.6 Expire/replan orders after impacted edits; never silently rebase by span.
- [ ] M6.7 Implement `index`, `triage`, bounded `explain`, `plan`, `propose_patch`, `verify`, and policy-gated `apply`.
- [ ] M6.8 Add deterministic ordering, pagination, query budgets, provenance, unknowns, and schema negotiation.
- [ ] M6.9 Add stale-handle, overlap, concurrent-client, retry, and context-budget tests.
- [ ] M6.10 Benchmark LLM workflows with and without graph-grounded work orders under identical budgets.
- [ ] M6.DoD Demonstrate one reviewable transaction per candidate, valid dependency ordering, safe stale-order
  rejection, and a measured LLM task-success improvement without more semantic regressions.

## M7 — Verification authority

- [ ] M7.1 Implement impact-cone test/build/lint/type selection with conservative fallback.
- [ ] M7.2 Integrate adapter/compiler/LSP precondition checks and authority conflicts.
- [ ] M7.3 Add targeted tests, coverage evidence, characterization, differential checks, and mutation evidence.
- [ ] M7.3a Require risky-change characterization to be captured/approved on the pinned pre-change snapshot,
  never inferred solely from tests generated after the rewrite.
- [ ] M7.4 Define verifier resource/time/filesystem/environment/network policies and structured failures.
- [ ] M7.5 Pin revisions, recheck preconditions, compare expected/actual graph delta, and reanalyze after format.
- [ ] M7.6 Make patch write/commit atomic with durable undo/rollback metadata.
- [ ] M7.7 Inject command, timeout, crash, partial-write, formatting, and graph-delta failures.
- [ ] M7.8 Implement immediate recipe demotion and negative-memory capture for counterexamples.
- [ ] M7.DoD Show zero known behavior changes in `safe-auto`, deterministic rollback under every injected
  failure, and explicit residual uncertainty for every weaker safety class.

## M8 — Readability and ranking calibration

- [ ] M8.1 Version the exclusive per-node feature schema and aggregation policies.
- [ ] M8.2 Separate structural, lexical/visual, surprisal, entropy, redundancy, cohesion, impact, and safety axes.
- [ ] M8.3 Implement CFG-based complexity and declare estimator/sample size for every entropy feature.
- [ ] M8.4 Licence-check/import published readability datasets and preserve their task/population limits.
- [ ] M8.5 Collect multilingual, role-stratified pairwise readability and timed/correct comprehension data.
- [ ] M8.6 Include human/LLM cleanup pairs and unsafe near-misses without authorship labels.
- [ ] M8.7 Capture every candidate feature once; run size-controlled ablations post hoc.
- [ ] M8.8 Run leave-project-out and leave-language-out evaluation with calibration and confidence intervals.
- [ ] M8.9 Compare against size, NLOC/complexity, and simple lexical baselines.
- [ ] M8.10 Publish a model card and choose portable model, language/role models, or evidence-only UX.
- [ ] M8.DoD Ship no readability label unless it beats frozen baselines on held-out data with acceptable
  calibration; preserve transparent axes in every outcome.

## M9 — Incremental project scale and integrations

- [ ] M9.1 Persist caches keyed by content, grammar, adapter, graph schema, recipe, and model versions.
- [ ] M9.2 Implement changed-range and dependency-driven invalidation for scopes, CFG/PDG, clones, metrics, and candidates.
- [ ] M9.3 Add indexed clone buckets and eliminate all-pairs project comparison.
- [ ] M9.4 Parallelize independent file/region analysis with serialized deterministic graph commits.
- [ ] M9.5 Add query/response budgets and explicit partial/pending project analysis.
- [ ] M9.6 Implement git-changed scans, baselines/ratchets, false-positive feedback, SARIF/CI, and editor refresh.
- [ ] M9.7 Reuse persistent snapshots across CLI, MCP, LSP, evaluator, and agent sessions.
- [ ] M9.8 Benchmark cold/full and warm/incremental latency, throughput, parse count, cache hit, memory, and fan-out.
- [ ] M9.DoD Demonstrate deterministic results and measured incremental advantage with bounded changed-region
  invalidation on representative repositories before advertising project-scale incrementality.

## M10 — Dogfood, external evaluation, and stable release

- [ ] M10.1 Run the complete pipeline on deslop and record all accepted, rejected, unsafe, and stale candidates.
- [ ] M10.2 Run human workflows on independent projects for every demonstrated language tier.
- [ ] M10.3 Run LLM workflows on the same tasks/models/budgets with and without graph grounding.
- [ ] M10.4 Publish graph, detector, transformation, readability, LLM, and performance benchmark results.
- [ ] M10.5 Publish the failure taxonomy, unsupported constructs, capability matrix, and known-risk register.
- [ ] M10.6 Close or explicitly downgrade every release-gate exception.
- [ ] M10.7 Freeze graph/protocol/recipe/model versions and provide migration compatibility tests.
- [ ] M10.8 Complete security, verifier-policy, undo/recovery, adapter-authoring, and agent-integration docs.
- [ ] M10.9 Run focused gates, full workspace gates, integration/e2e suites, and external smoke tests from a clean checkout.
- [ ] M10.DoD Release only the language and safety tiers demonstrated by frozen evidence; retain explicit
  unknown/blocked output everywhere else.

## Cross-cutting release gates

- [ ] G1 No duplicate work-order IDs in the benchmark corpus.
- [ ] G2 No ambiguous reference reported as uniquely resolved.
- [ ] G3 No fact/finding/recipe exceeds the adapter's declared capability.
- [ ] G4 No known behavior-changing `safe-auto` patch; counterexamples demote immediately.
- [ ] G5 Atomic rollback passes injected failures and preserves disk/graph consistency.
- [ ] G6 Output is deterministic for a pinned source/tool/config/model revision.
- [ ] G7 Readability labels pass frozen held-out baseline/calibration gates or remain unshipped.
- [ ] G8 Graph-grounded LLM work orders improve verified completion without more semantic regressions.
- [ ] G9 Incremental scans show bounded invalidation and measured benefit over full scans.
- [ ] G10 Docs, schemas, fixtures, benchmark evidence, ADRs, session report, and negative memory are current.

## Frozen benchmark assets and numerical gates

- [ ] B1 Freeze and hash a canonical microcorpus of at least 600 programs, at least 100 per non-generic
  language adapter, with gold roles/spans/containment/ownership/edges and malformed/opaque cases.
- [ ] B2 Freeze and hash at least 1,000 labelled transformation opportunities and 1,000 hard negatives
  with protected spans/APIs, expected safety class, behavior oracle, and resource budget.
  Recipe-specific slice complete, global gate open: corpus `b2r1_71f0651edc3d3bf26564715ba11214f8ff6dc2962bdb0405871e2c98a1235207`
  freezes 1,000 Rust opportunities and 1,000 hard negatives in 400 five-variant design clusters, with protected
  APIs/spans, `SafeAuto`, behavior oracle, exact expanded digest, and a 60-second optimized-run budget.
- [ ] B3 Pin 18 real repositories, three per language across size strata, with tests, APIs, generated
  boundaries, and reproducible build commands.
- [ ] B4 Freeze at least 300 blinded readability pairs and 240 fixed LLM refactoring tasks balanced by
  language and opportunity family.
- [ ] B5 Record corpus licence, split, prompts, model/tool versions, seeds, reference machine, cache state,
  and signed result-schema version.
- [ ] B6 Meet canonical-role macro F1 >= 0.99 (no language < 0.97), exact gold containment/ownership,
  control-edge F1 >= 0.98, and local-resolution precision >= 0.98 at coverage >= 0.80.
- [ ] B7 Meet actionable precision lower 95% bound >= 0.90 overall/0.85 per language, recall lower bound
  >= 0.70 overall/0.60 per language-family, hard-negative FPR upper bound <= 0.02 overall/0.05 per language,
  and ECE <= 0.05.
  Recipe-specific Rust slice passes, global gate open: 200/200 positive clusters and 200/200 hard-negative
  clusters yield precision/recall lower 95% `0.981154673623`, hard-negative FPR upper 95%
  `0.018845326377`, ECE `0`, opportunity coverage `1`, and hard-negative abstention `1`.
- [ ] B8 Show 100% declared parse/build/type/behavior-oracle success for accepted benchmark patches and
  zero confirmed semantic failures or verification bypasses in `safe-auto`.
- [ ] B9 Meet blinded human-preference lower 95% bound >= 0.60 overall/0.55 per language and improve the
  declared primary quality axis in at least 90% of accepted patches without displaced project regressions.
- [ ] B10 Show graph-rich LLM work orders improve accepted-patch rate by >= 10 percentage points with paired
  95% confidence excluding zero, <= 2% out-of-scope edits, and >= 90% correct unsafe/impossible abstention.
- [ ] B11 On the recorded reference machine, scan 1 MLOC cold in <= 60 seconds and <= 3 GiB RSS; process a
  single-file incremental edit at p95 <= 500 ms and <= 5% of a full scan; preserve exact clean/incremental parity.
- [ ] B12 Publish macro/worst-language/worst-family results, confidence intervals, abstention/coverage, failure
  taxonomy, and prior-release deltas; do not pool or cherry-pick away a failed slice.

## Deferred-work template

When an item cannot be completed, append under it: blocker, why it blocks the user-visible contract,
the exact next action, prerequisites/authority required, useful validation already obtained, and the
negative-memory constraint that prevents repeating a failed path. Do not check the item.

Signature: Codex (GPT-5), markable ultimate-deslop execution ledger, 2026-07-12.
