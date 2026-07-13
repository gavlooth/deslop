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
- [ ] M0.11 Run focused tests, then full fmt/build/test/clippy gates and record measured before/after values. **NEXT**
- [ ] M0.12 Separate the exact-byte `RevisionGuard` from the trimmed cross-revision baseline fingerprint;
  migrate region/work-order IDs explicitly and reject boundary-whitespace staleness.
- [ ] M0.13 Persist proposal analyzer config, capability, and source-revision context so verify/apply
  reconstruct the same work-order set instead of silently rescanning with defaults.
- [ ] M0.14 Reconcile the `NeverAuto` contract: SPEC says report-only while `/1` currently proposes it;
  choose one policy, update every consumer, and add an end-to-end regression.
- [ ] M0.DoD Demonstrate zero duplicate work-order IDs, zero falsely resolved ambiguous fixture edges,
  correct grammar selection, and honest partial/capability labels on the M0 corpus.

## M1 — One parse, one owned syntax snapshot

- [ ] M1.1 Write an ADR for `ProjectAnalysis`, source revisions, ownership, invalidation, and consumers.
- [ ] M1.2 Implement a revision/content-addressed source store and one parse owner per file revision.
- [ ] M1.3 Implement the owned node arena with raw kind, field, span, parent/children, named/error flags,
  token/trivia ownership, source slice, and grammar provenance.
- [ ] M1.4 Define scan-local `NodeId`, serialized revision-bound `NodeKey`, cross-revision baseline
  fingerprint, and exact `RevisionGuard`; test collisions/expiry and prohibit fuzzy write authorization.
- [ ] M1.5 Build containment and smallest-exclusive-region indices.
- [ ] M1.6 Implement exclusive local and declared inclusive aggregation APIs.
- [ ] M1.7 Expose query/cursor-derived captures without reparsing source fragments.
- [ ] M1.8 Add edit/changed-range invalidation and explicit re-anchor-or-expire behavior.
- [ ] M1.9 Migrate analyzer and metrics consumers to the shared snapshot.
- [ ] M1.10 Migrate graph, evaluator, LSP, MCP/protocol, and slim consumers.
- [ ] M1.11 Instrument parse counts, ownership invariants, deterministic node order, latency, and memory.
- [ ] M1.DoD Prove one parse per file revision in all scan/propose paths and no borrowed-node lifetime or
  overlapping exclusive-metric errors on the gold fixture matrix.

## M2 — Language-adapter contract

- [ ] M2.1 Version the adapter/capability schema for `S0` through `S4`.
- [ ] M2.2 Define canonical roles and retain raw grammar kinds/fields alongside them.
- [ ] M2.3 Define query packs for declarations, references, scopes, control, comments, and opaque/generated code.
- [ ] M2.4 Define operator/token classification and language-specific lexical policies.
- [ ] M2.5 Define parse-error, unsupported-construct, macro, generated-code, and dialect policies.
- [ ] M2.6 Implement/repair the Rust adapter and golden fixtures.
- [ ] M2.7 Implement/repair JavaScript, TypeScript, and TSX adapters and golden fixtures.
- [ ] M2.8 Implement/repair Python adapter and golden fixtures.
- [ ] M2.9 Implement/repair Clojure adapter and golden fixtures.
- [ ] M2.10 Implement/repair Julia adapter and golden fixtures.
- [ ] M2.11 Add cross-adapter construct matrices and unsupported-capability leakage tests.
- [ ] M2.DoD Every emitted fact declares adapter/version/capability/provenance, and no confirmed output
  requires a higher tier than the adapter supplies.

## M3 — Scope and project-name graph

- [ ] M3.1 Write an ADR for scope, resolution paths, ambiguity, and authority precedence.
- [ ] M3.2 Model scopes, definitions, references, bindings, imports/exports, visibility, and shadowing.
- [ ] M3.3 Implement per-language declarative resolution rules or an explicitly equivalent adapter.
- [ ] M3.4 Store all candidate paths and unique/ambiguous/unresolved status; prohibit bare-name resolution.
- [ ] M3.5 Stitch file/module/package/build-target names incrementally.
- [ ] M3.6 Add optional compiler/LSP semantic facts with higher authority and conflict reporting.
- [ ] M3.7 Add duplicate-name, nested scope, wildcard/alias import, re-export, dynamic, and unresolved fixtures.
- [ ] M3.8 Measure resolution precision/recall and incremental file-isolation behavior.
- [ ] M3.DoD Meet the frozen gold-corpus resolution gate and block semantic recipes wherever authority is
  incomplete or ambiguous.

## M4 — CFG, PST, PDG, and SDG

- [ ] M4.1 Define the control-edge schema for entry/exit, normal, branch, loop, exceptional, abrupt, and suspension flow.
- [ ] M4.2 Implement CFG lowering for each adapter at its declared capability tier.
- [ ] M4.3 Compute dominance/post-dominance and hierarchical SESE/PST regions.
- [ ] M4.4 Preserve irreducible control regions as explicit non-structured/unknown facts.
- [ ] M4.5 Implement def/use, reaching definitions, liveness, parameter/output, and conservative effect facts.
- [ ] M4.6 Build local PDGs from control and data dependence.
- [ ] M4.7 Build call/parameter/return/global summaries and SDG edges where resolution authority permits.
- [ ] M4.8 Add exception, async/yield, closure, mutation, alias uncertainty, and early-exit fixtures.
- [ ] M4.9 Compare graph edges/regions with hand-labelled gold fixtures and compiler facts where available.
- [ ] M4.DoD Pass frozen CFG/PST/PDG gold gates and propagate every missing/uncertain semantic fact into
  recipe eligibility.

## M5 — Candidate detectors and transformation recipes

### Recipe framework

- [ ] M5.1 Version `TransformationRecipe` and `TransformationCandidate` schemas.
- [ ] M5.2 Implement required facts, `Proven`/`Disproven`/`Unknown` preconditions, forbidden conditions,
  authority evidence, and capability checks; permit only `Proven` automatic obligations.
- [ ] M5.3 Implement expected graph deltas, impact-cone queries, safety class, validation plan, and rollback metadata.
- [ ] M5.4 Add recipe fixture conventions: positive, no-op, minimal counterexample, and adversarial near-miss.

### Branch/control flow

- [ ] M5.5 Detect equivalent arms and common prefix/suffix factoring with effect/order constraints.
- [ ] M5.6 Detect safe adjacent-condition merges with short-circuit and exception constraints.
- [ ] M5.7 Detect independent branch splits from dependence slices.
- [ ] M5.8 Detect guard-clause/condition inversion candidates from PST and exit facts.
- [ ] M5.9 Detect dead arms and exhaustive chain-to-match/table candidates.
- [ ] M5.10 Emit before/after graph evidence and counter-evidence for every branch candidate.

### Functions and expressions

- [ ] M5.11 Generate extract-method candidates from SESE regions and complete computation/object-state slices.
- [ ] M5.12 Infer exact extraction inputs, outputs, mutations, exits, exceptions, captures, and async/ownership constraints.
- [ ] M5.13 Detect multi-responsibility callable splits from dependence cohesion/action clusters.
- [ ] M5.14 Detect safe merge/inline of over-fragmented single-use helpers.
- [ ] M5.15 Add def/use/effect-grounded temporary, expression, and independent-statement recipes.

### Dependencies and modules

- [ ] M5.16 Build file/module/package/build/API dependency projections.
- [ ] M5.17 Compute SCCs, condensation DAG, layers, fan-in/out, instability, and architecture-rule violations.
- [ ] M5.18 Generate reviewed cycle-breaking seams with API/data-flow evidence.
- [ ] M5.19 Generate move/split/merge-module candidates from cohesion, coupling, impact, and optional change history.
- [ ] M5.20 Add semantically safe import/declaration ordering recipes.

### Clones, ceremony, dead code, clarity

- [ ] M5.21 Implement exact subtree fingerprints and renamed-token normalization.
- [ ] M5.22 Implement scalable candidate indexing and graph-context clone verification.
- [ ] M5.23 Collapse pair matches into maximal clone classes and one coordinated candidate.
- [ ] M5.24 Classify generated/schema/test/public-API/intentional repetition before abstraction proposals.
- [ ] M5.25 Add graph-grounded forwarding, conversion/allocation, wrapper, repeated-error, and dead-code candidates.
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
