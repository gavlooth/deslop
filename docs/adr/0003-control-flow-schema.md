# ADR 0003: Evidence-bounded control-flow schema

- Status: Accepted
- Date: 2026-07-14
- Owners: deslop maintainers
- Roadmap: M4.1; governs M4.2-M4.9 and constrains M5-M9
- Builds on: ADR 0001 and ADR 0002

## Context

Deslop has one immutable, revision-bound `ProjectAnalysis`, exact syntax-node keys, stored language-adapter
identities and capability manifests, and separate scope/name-resolution projections. It does not yet have a
control-flow representation. `deslop.graph/2` is a syntactic project dependency view; its call/import edges
do not establish execution order and it cannot be promoted into a CFG.

Control captures, canonical branch/loop roles, token order, and syntax containment are useful lowering seeds,
but none proves control-flow semantics. A complete CFG depends on adapter rules for evaluation order, branch
and loop behavior, abrupt exits, exception propagation and handlers, async/await, yield/resume, recovery, and
language-specific constructs. Missing rules must remain visible and must block stronger consumers.

## Decision

Deslop will use the strict wire schema `deslop.control-flow/1` as a local-semantic projection over one exact
`ProjectAnalysis`. Adapter-specific lowering begins in M4.2; M4.1 freezes the shared contract.

### Ownership and identity

Each control-flow document retains its schema, exact analysis and projection IDs, lowering-policy ID, and a
canonical list of per-owner graphs. A graph owns exactly one callable, initializer, module initializer, or
adapter-defined executable region and retains:

- the owner's revision-bound `NodeKey`;
- exact grammar and stored adapter identity;
- the stored `ControlFlow` capability support and authority;
- graph-level coverage and explicit uncertainty;
- payload-bound graph, point, and edge keys.

Keys are correlation identities, not edit authority. They bind all semantic payload that they identify and
expire when the source revision, grammar, adapter, policy, or analysis changes. Dense process-local identities
may be added by lowering implementations, but never appear on the wire.

Graphs are local to the owner's file. Cross-file calls and returns are later SDG facts and require M3
resolution authority; a local CFG edge never targets a node from another file.

### Control points

Every graph contains exactly one virtual entry point and one virtual exit point. Virtual boundaries are
explicit and never impersonate syntax nodes. Other points are either exact syntax nodes or synthetic points
with an exact source anchor and one of these portable roles:

- no-op, branch dispatch, merge, loop header, handler dispatch, finally dispatch;
- abrupt dispatch, suspension, resume, or exit dispatch;
- a versioned adapter-defined role.

Point order is canonical serialization order only. It does not imply execution order, dominance, reachability,
or precedence.

### Edge families

Every edge belongs to exactly one typed family:

| Family | Meaning |
| --- | --- |
| entry | virtual entry to the executable graph body |
| exit | an exit-dispatch point to the single virtual exit, with normal/exceptional/abrupt/suspended outcome |
| normal | ordinary evaluation-order flow |
| branch | true, false, case, default, guard, or adapter-defined selection |
| loop | enter, body, back, or condition-false transition |
| exceptional | throw, propagate, handler, finally-enter, finally-resume, or adapter-defined exception flow |
| abrupt | return, break, continue, goto, terminate, or adapter-defined abrupt transfer |
| suspension | await-ready, await-pending, yield, suspend, resume, or adapter-defined suspension flow |

Entry edges must leave the unique entry point and may not target it. Exit edges must enter the unique exit
point and may not leave it. No other edge may touch either virtual boundary. Empty bodies therefore use an
explicit synthetic no-op/exit-dispatch path rather than a polymorphic entry-to-exit edge. Every endpoint must
exist in the same graph. Duplicate semantic edges and duplicate keys are invalid.

Structured case labels and adapter-defined variants are identity inputs. Free-form rendering labels never
select semantics. Deterministic ordering stabilizes bytes and cannot break a semantic tie.

### Coverage, precision, and authority

Coverage is independent from the stored edge set:

- `Complete`: all required lowering rules and exact inputs for the owner are available;
- `Partial`: some required construct, rule, recovery region, or input is unknown;
- `Unsupported`: the adapter explicitly declines CFG lowering for the owner/construct;
- `Failed`: a required lowering/provider step failed.

Complete coverage carries no uncertainty reasons. Every incomplete state carries one or more canonical,
distinct, nonempty reasons. Enumerating plausible edges does not prove completeness.

An edge is either `Exact`, meaning the static language model positively admits that transition, or
`Conservative`, meaning it is retained because the adapter cannot rule it out. A conservative edge carries an
exact nonempty reason. This precision describes the edge model, not whether an execution takes that edge.

A complete graph requires the exact stored adapter manifest to declare `ControlFlow` Provided at Adapter,
LanguageServer, or Compiler authority. Syntax authority cannot establish CFG semantics. RuntimeVerification
may record observed paths in a future separate overlay but cannot prove exhaustive static control flow. A
recovered owner or unresolved graph uncertainty also prevents Complete coverage. Incomplete graphs retain
their stored support/authority truth without inventing authority.

### Consumer boundary

M4.1 stores facts; it does not authorize transformations. M4.3-M4.7 derive dominance, PST, PDG, and SDG facts
only from the exact control-flow projection and must propagate incomplete coverage and conservative edges.
M5 recipe eligibility must fail closed unless every required control fact is complete at sufficient static
authority. Neither `deslop.graph/2`, a query capture, syntax role, endpoint count, deterministic order, nor a
runtime observation is a fallback.

## Verification requirements

The schema is accepted only when executable tests establish:

1. all eight edge families and their sub-kinds round-trip distinctly with stable bytes and keys;
2. unknown fields, unsupported schema/IDs, stale payload keys, duplicate keys, dangling endpoints, duplicate
   semantic edges, noncanonical order, foreign-file points, and virtual-boundary misuse fail closed;
3. coverage/reason, edge-precision/reason, support/authority, recovered-owner, and complete-authority
   contradictions fail closed;
4. current production adapters remain ControlFlow Unknown and cannot construct Complete graphs;
5. the schema has no dependency on or conversion fallback from `deslop.graph/2`; and
6. full workspace tests, build, rustdoc, clippy, formatting, and diff checks pass.

## Consequences

The schema distinguishes syntax seeds from semantic flow, makes virtual boundaries and non-normal transfers
queryable, and gives later dominance/dependence/recipe layers an explicit fail-closed input. The cost is more
verbose lowering and fixtures: adapters must describe uncertainty rather than omitting it or folding it into
normal flow.

## Rejected alternatives

- **Extend `deslop.graph/2`.** It has project-dependency identity and syntactic authority, not per-executable
  evaluation semantics.
- **Infer flow from source order and roles.** This loses language evaluation order, abrupt flow, exceptions,
  suspension, recovery, and adapter-specific constructs.
- **Use one untyped `next` edge with labels.** Render labels are not a validated semantic vocabulary and make
  downstream dominance/dependence consumers guess.
- **Treat an enumerated graph as complete.** Absence is authoritative only when all required lowering inputs
  and rules are covered.
- **Use observed runtime paths as the CFG.** An observation proves one execution, not all statically possible
  paths or absence.
