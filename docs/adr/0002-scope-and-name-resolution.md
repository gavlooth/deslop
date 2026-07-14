# ADR 0002: Scope graphs and evidence-bounded name resolution

- Status: Accepted
- Date: 2026-07-14
- Owners: deslop maintainers
- Roadmap: M3.1; governs M3.2-M3.8 and constrains M4-M7
- Supersedes: semantic interpretation of non-containment edges in `deslop.graph/2`

## Context

ADR 0001 established one immutable, revision-bound `ProjectAnalysis`. M2 added exact grammar
dialects, versioned adapter capabilities, canonical roles, query packs, lexical policy, construct
boundaries, and retained provenance. Production adapters currently supply complete S0/S1 syntax
facts and explicitly report S2-S4 capabilities as unknown.

The existing `deslop.graph/2` projection is useful syntax routing, not name resolution. It extracts
definitions and reference spellings, searches heuristic owner/module/name indexes, and emits:

- `Resolved` only for syntax-containment edges;
- `Syntactic` for a single best call/import/inheritance candidate or an unresolved placeholder;
- `Ambiguous` when multiple candidates survive.

This boundary fixed a prior first-wins failure where two `compact_label` definitions existed and all
ten calls were assigned to one file. It also exposed what graph/2 cannot represent: complete lexical
scopes, namespaces, declaration timing, imports and exports, aliases, re-exports, visibility,
build-target module identity, all resolution paths, or authority conflicts. Graph/2 collapses an
ambiguous result to a placeholder and does not retain the candidate set.

M3 must make these semantics explicit before any call graph, impact analysis, or transformation can
consume a `resolved` edge. Repository-global uniqueness, deterministic sorting, or a single
heuristic candidate is never proof of binding.

This decision follows the separation in scope-graph work between language-specific graph
construction and reusable resolution, particularly Néron et al., “A Theory of Name Resolution”
(ESOP 2015), <https://doi.org/10.1007/978-3-662-46669-8_9>.

## Decision

Deslop will introduce two immutable, versioned projections over one `ProjectAnalysis`:

- `deslop.scope-graph/1` stores language/build-context-bound scope, declaration, definition,
  binding, import/export, visibility, and reference facts.
- `deslop.resolution/1` stores every candidate path and a coverage-bounded outcome for every
  reference/namespace query.

The exact Rust API may differ, but the data and authority rules in this ADR are normative.

### Goals and non-goals

M3 resolves project names only where an adapter or higher-authority provider supplies complete rules
for the exact dialect and build context. It must preserve ambiguity and missing coverage.

M3 does not implement type inference, overload selection, dynamic dispatch, macro expansion,
control/data flow, effects, or runtime equivalence. Those facts remain separate capabilities. A
unique lexical binding does not imply a unique runtime call target.

### Identity and build context

Every scope and resolution projection retains its `Arc<ProjectAnalysis>`, `ProjectAnalysisId`,
`ProjectionId`, exact path revisions, grammar selections, adapter identities/manifests, and effective
resolution rule packs.

Name resolution is additionally bound to a `BuildContextId`. It includes every available input that
can alter name visibility or module identity:

- package/workspace manifests and lock files;
- build target, source roots, generated-source roots, and module maps;
- feature, conditional-compilation, platform, language-mode, and edition settings;
- dependency identities and versions;
- compiler or language-server configuration and artifact identity when used;
- explicit prelude/builtin environment;
- exclusions and unavailable required inputs.

Two targets that compile the same source with different features or dependency graphs have distinct
contexts and distinct resolution projections. Missing name-affecting inputs produce partial coverage;
they are not replaced with process environment defaults.

Process-local IDs are dense and analysis-owned. Wire identities are revision-bound and include the
build context and schema. A path, line, byte range, spelling, or graph/2 node ID is not a binding ID.

### Core fact model

The scope graph contains at least these fact classes:

| Fact | Required content |
| --- | --- |
| Scope | ID, kind, owning node/region, parent scope, namespace policy, adapter/version, coverage |
| Declaration | ID, original name, normalized lookup key, namespace, scope, source order, visibility, modifiers |
| Definition | ID, declaration link, defining node/region, symbol kind, optional body/type scope |
| Binding | ID, declaration/definition target, binding form, lifetime/timing, mutability where known |
| Reference | ID, original spelling/segments, namespace demand, owning scope, source position, syntactic role |
| Import | ID, source scope/module expression, alias, selective/glob form, conditions, source order |
| Export | ID, local target/name, exported alias, re-export path, visibility, conditions |
| Build module | ID, package/target/source-root identity, module path, constituent file scopes |
| Dynamic boundary | ID, construct kind, affected scopes/namespaces, reason resolution is incomplete |

All facts retain their exact `NodeId` or owned region and revision-bound serializable key, raw grammar
evidence, canonical roles, adapter schema, capability declaration, grammar dialect/version, and build
context. A fact from a recovered or opaque region carries its recovery/coverage boundary.

Declarations and definitions are distinct. A declaration can exist without a body, several
declarations can denote one definition where a language permits it, and duplicate definitions are
not silently merged. An adapter supplies the legal relationship.

### Scopes and namespaces

Scopes form an explicit parent graph, normally a tree within one language module plus declared
cross-module edges. Supported scope kinds include project, package/build target, module/file,
namespace, type, callable, block, comprehension/generator, pattern/case, handler/catch, and
adapter-defined scopes.

Each language adapter declares which syntax creates a scope and which declarations belong to it.
Brace/indent/list containment alone does not prove scope. Source order, hoisting, recursive binding,
forward declaration, and temporal-dead-zone behavior are adapter rules.

Names are resolved in an explicit namespace. The portable catalog begins with value, type, module,
macro, label, and member namespaces; adapters may declare that some are unified or add a versioned
namespace. A type candidate never satisfies a value reference merely because the spelling matches.

Lookup keys preserve original spelling and use adapter-declared normalization only. Case folding,
Unicode normalization, sigils, qualified separators, and private-name mangling are not inferred by
the resolution engine.

### Resolution rule packs

Each adapter that provides `LexicalScopes` or `NameResolution` publishes a versioned, total resolution
rule pack for its exact grammar dialect. The pack declares:

- scope creators and parent selection;
- declaration, definition, reference, and binding extraction;
- namespaces and allowed cross-namespace transitions;
- visibility and declaration-timing rules;
- shadowing and duplicate-definition behavior;
- qualified/self/super/root/member path semantics;
- import, alias, glob, prelude, export, and re-export traversal;
- build-module mapping prerequisites;
- dynamic or opaque constructs that make coverage incomplete;
- a structured lookup-precedence relation.

Rule packs are projection identity inputs. Unknown or unsupported sections carry no executable
payload or authority. Query compilation or a canonical `Read` role alone cannot provide a rule pack.

Lookup precedence is language-specific. The shared engine enforces the declared relation but does not
hard-code “locals before imports” or similar rules for every language. Precedence is represented as
structured rule steps, lexical distance, namespace, import specificity, and adapter-declared ordering;
it is not a floating score. A deterministic sort key stabilizes serialization only and never breaks a
semantic tie.

### Resolution paths

Resolution begins at the reference's exact owning scope and namespace. A repository-global spelling
index may accelerate candidate retrieval, but it may return only candidates reachable through a
declared path. It cannot introduce a candidate or turn uniqueness into authority.

Every attempted candidate is stored as a `ResolutionPath`. A path contains:

1. the reference and starting scope;
2. every traversed edge, such as lexical-parent, defines, binds, member, module, explicit-import,
   alias, glob-import, export, re-export, prelude, package, or external-provider;
3. the endpoint declaration/definition when one exists;
4. its structured precedence key;
5. viability and any rejection reason;
6. visibility, namespace, timing, condition, and build-target checks;
7. evidence authority and the exact source facts/providers;
8. coverage and dynamic-boundary observations.

Rejected and shadowed paths remain in the result. Standard rejection reasons include shadowed,
wrong-namespace, not-visible, declared-later, inactive-condition, wrong-build-target, import-unresolved,
export-incomplete, opaque-boundary, provider-conflict, and duplicate-definition.

If multiple maximum-precedence paths converge on the same definition, the endpoint can still be
unique; all paths remain visible. If maximum-precedence viable paths end at distinct definitions, the
outcome is ambiguous. Lower-precedence candidates are retained as shadowed and do not create
ambiguity unless the language rules make their precedence equal.

### Coverage and outcomes

Coverage and resolution status are separate fields.

| Coverage | Meaning | Permitted terminal status |
| --- | --- | --- |
| Complete | Every required scope/rule/input for this reference and namespace is available | Unique, Ambiguous, Unresolved, Conflict |
| Partial | Some graph, rule, build, export, macro, generated, or provider input is missing | Unknown or Conflict |
| Unsupported | The adapter explicitly does not resolve this construct/namespace | Unknown |
| Failed | Required construction/provider execution failed | Unknown or Conflict |

Terminal statuses are normative:

- `Unique`: complete coverage and exactly one distinct maximum-precedence viable endpoint.
- `Ambiguous`: complete coverage and multiple distinct maximum-precedence viable endpoints.
- `Unresolved`: complete coverage and no viable endpoint.
- `Unknown`: coverage is not complete, regardless of how many candidates were discovered.
- `Conflict`: authoritative providers disagree or equal-authority evidence is inconsistent. All
  conflicting results remain present.

Zero candidates is not `Unresolved` unless coverage is complete. One candidate is not `Unique` unless
coverage is complete and its full path is viable. Dynamic imports, reflection, `eval`, unresolved
macro expansion, conditional readers, generated sources without a producer manifest, and similar
constructs normally produce `Unknown` with an explicit boundary.

External symbols are endpoints only when package/module/provider authority positively identifies
them. Failure to find an internal candidate does not prove externality.

### Lookup precedence versus evidence authority

Lookup precedence chooses among candidates under one language model. Evidence authority decides how
results from different providers relate. They are never combined into one rank.

| Evidence | May assert | May not assert |
| --- | --- | --- |
| Syntax | spelling, containment, raw/canonical roles, candidate seeds | binding, absence, externality |
| Adapter rules | S2 lexical/name result when rule pack and coverage are complete | compiler types, runtime dispatch, incomplete absence |
| Language server | result for its exact server/version/workspace/build context | compiler authority unless explicitly equivalent and recorded |
| Compiler | static binding/type result for its exact compiler/artifact/build context | other targets/configurations or unobserved runtime dispatch |
| Runtime observation | an observed target for one instrumented execution/context | universal static binding or absence on unobserved paths |

M3 will version/extend the M2 authority catalog before storing language-server evidence; it will not
mislabel it as `Compiler` or `Adapter`.

For the same static query and build context, compiler evidence takes precedence over language-server
and adapter conclusions. Language-server evidence takes precedence over adapter conclusions only when
its project model and artifact identity are complete. Lower-authority disagreements are retained as
conflicts and block semantic transformations even when a higher-authority endpoint is shown as the
preferred diagnostic result. Equal-authority disagreement is `Conflict`, never first-wins.

Runtime observations are orthogonal. They attach observed-dispatch edges to the static result and do
not overwrite it. Several observed targets do not make a static lexical binding ambiguous; one
observed target does not prove it exhaustive.

### Imports, exports, and module stitching

Import resolution first resolves the source module/package in the exact build context, then applies
adapter rules for selective names, aliases, globs, and conditions. An alias creates a binding in its
declared scope; it is not string substitution.

A glob/wildcard path is viable only when the source module and relevant export set are complete. If
that set is incomplete, references that could depend on the glob are `Unknown`. Explicit and glob
import precedence is adapter-declared.

Exports and re-exports retain their full path. Re-export cycles are evaluated as a deterministic
fixed point over strongly connected components. Non-convergence, unsupported dynamic export, or a
missing constituent module makes affected results `Unknown`.

File paths do not inherently define modules. Package/build-target manifests and adapter module rules
stitch file scopes into module scopes. Heuristic stem/dotted/slashed keys from graph/2 remain candidate
seeds only until a declared module map validates them.

Visibility is checked at every traversed boundary. Inaccessible candidates remain as rejected paths;
they do not disappear or become external placeholders.

### Incremental construction and invalidation

Scope and resolution projections are immutable. An incremental successor must be byte-for-byte
equivalent to a clean rebuild for the same snapshot and build context.

Invalidation follows explicit reverse dependencies:

- a local declaration/reference edit rebuilds its owning scope and dependent descendant lookups;
- a scope-parent, shadowing, visibility, or namespace change invalidates affected descendants until
  a proven unchanged boundary;
- an export/import/alias/glob change invalidates importing scopes and transitive re-export consumers;
- a module/package/build-target change invalidates its module mapping and reverse dependents;
- a rule-pack, grammar, adapter, compiler/LSP artifact, feature, or conditional change invalidates
  every result that records that input;
- unrelated files with no dependency path retain their scope facts and resolution results.

Every reused result retains evidence of its unchanged dependencies. Cache or incremental reuse cannot
change candidate order, paths, status, authority, diagnostics, or identity.

### Consumer authority

`deslop.graph/2` remains unchanged and syntactic. Only containment may use `Resolved`, meaning exact
syntax ownership. Calls, imports, and inheritance remain `Syntactic` or `Ambiguous`; they cannot
authorize semantic recipes.

M3 will introduce a new graph schema after scope/resolution projections exist. A semantic edge must
carry the resolution result ID, endpoint, complete candidate paths, build context, authority, and
coverage. Call-graph edges additionally require the declared call/dispatch capability; lexical name
resolution alone is insufficient.

Analyzer findings, impact queries, refactor candidates, and transformations must declare required
capabilities and accept only outcomes/authority that meet them. `Unknown`, `Ambiguous`, `Unresolved`,
`Conflict`, dynamic boundaries, or incomplete reverse dependencies block any recipe that needs a
unique binding. No fallback to graph/2 best candidates is permitted.

## Verification requirements

M3 implementation is accepted only when executable gold gates establish:

1. duplicate names in unrelated files never resolve from repository-global uniqueness;
2. nearest lexical scope, parameters, locals, members, type/module scopes, and language-specific
   shadowing/timing match hand-labelled paths;
3. explicit, aliased, wildcard, conditional, and re-export imports retain all traversed paths and
   exact rejection reasons;
4. multiple maximum-precedence endpoints are ambiguous with the complete candidate set; stable sorting
   never selects a winner;
5. complete zero-candidate cases are unresolved, while every incomplete/dynamic near-case is unknown;
6. namespaces, visibility, overload/duplicate forms, self/super/root qualification, and build targets
   follow exact adapter rules;
7. compiler/language-server disagreement is reported and blocks recipes; runtime observations remain
   separate from static results;
8. supported frozen-corpus references achieve exact status/path/endpoint agreement—precision and recall
   are both 1.0 for the supported subset, while unsupported cases must match expected `Unknown` rather
   than being omitted from the denominator;
9. adding an unrelated same-spelled definition changes no existing result; adding a reachable equal-
   precedence definition changes Unique to Ambiguous;
10. one-file edits rebuild exactly the reverse dependency cone, reuse unrelated facts, and equal a full
    clean rebuild;
11. every result retains analysis, adapter/rule, grammar, build-context, capability, authority, coverage,
    candidate-path, and source-fact provenance; and
12. M0, M1, and M2 numerical/authority gates remain unchanged, including zero resolved graph/2
    non-containment edges.

The M3.8 report must publish exact corpus counts, confusion matrices by status, unsupported/unknown
coverage, incremental invalidation counts, commands, and failures. A percentage without the underlying
counts is insufficient.

## Consequences

Positive consequences are explicit ambiguity, explainable paths, honest absence, build-context
isolation, deterministic incremental invalidation, and a sound substrate for CFG/PDG/call/impact work.
The model can incorporate compiler or runtime evidence without erasing lower-authority disagreements.

Costs are larger retained projections, language-specific rule packs, build-system inputs, and more
complex fixtures. Some references that graph/2 currently points at syntactically will become Unknown
until their adapter and build context are complete. This is required honesty, not a regression.

## Rejected alternatives

- **Resolve a globally unique bare name.** Reachability and shadowing are unproven; this caused the
  original `compact_label` false resolution.
- **Pick the first or sorted candidate.** Ordering is deterministic but has no semantic authority.
- **Store only the chosen endpoint.** It prevents ambiguity explanation, conflict reporting,
  incremental dependency tracking, and safe re-evaluation.
- **Treat zero candidates as unresolved by default.** Missing imports, macros, build inputs, or dynamic
  behavior make absence unknown.
- **Use one universal hard-coded precedence order.** Namespace, hoisting, import, and shadowing rules
  differ by language and dialect.
- **Treat syntax query captures as bindings.** Queries provide candidate syntax, not scope traversal or
  absence proof.
- **Let compiler/LSP output overwrite adapter facts.** Conflicts and provider/build identity would be
  lost; stale external results could silently authorize transformations.
- **Let runtime observations replace static resolution.** An observed execution is not exhaustive and
  has a different semantic domain.
- **Upgrade graph/2 in place.** Existing consumers understand its confidence values and placeholder
  shape; semantic results require a new strict schema.

## Rollout

M3.2 introduces scope/reference/binding/import/export facts and identities. M3.3 adds versioned adapter
rule packs and the shared path engine. M3.4 stores complete candidates and outcomes. M3.5 adds
build-context module stitching and incremental reverse dependencies. M3.6 integrates version-bound
compiler/language-server facts and conflict reporting. M3.7 freezes adversarial fixtures. M3.8 measures
exact resolution and invalidation behavior. M3.DoD publishes the joined gold gate before any later
recipe may require resolved semantics.
