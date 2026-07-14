# ADR 0005: Preserve non-structured control regions explicitly

- Status: Accepted
- Date: 2026-07-14
- Decision owners: M4.4 control-region integration

## Context

ADR 0004 and `deslop.control-regions/1` freeze the authoritative structured PST boundary. They retain rejected
SESE candidates as residuals, but a residual reason is not a complete irreducibility model. In particular, an
ordinary reducible loop is cyclic without being irreducible, a multi-entry strongly connected component can
be irreducible even when it is contained by a valid structured root envelope, and a reachable cycle can be
nonterminating independently of whether it is irreducible.

Adding classifications to the frozen `/1` residual payload would be a wire break. Ordering crossing candidates
or multi-entry cycles into the PST would turn deterministic serialization into false structural authority.

## Decision

### Separate strict overlay

M4.4 introduces `deslop.non-structured-control-regions/1`. Every document binds the exact analysis, M4.1
control-flow projection and policy, M4.3 control-region projection and policy, M4.4 classification policy,
source graph keys, and payload-derived projection, graph, and fact identities. M4.3's schema and structured
hierarchy remain unchanged.

The overlay contains one graph record for every source control-region graph, including records with no
non-structured facts. Derived coverage is copied exactly from M4.3 and can never exceed it.

### SCC domain and boundaries

SCCs are computed over entry-reachable CFG points only. A component is cyclic when it has more than one point
or its sole point has a self-edge. Its external entry boundary is the canonical set of component targets with
an incoming edge from outside the component. Its external exit boundary is the canonical set of component
origins with an outgoing edge outside the component.

A cyclic SCC is classified `irreducible-multi-entry-cycle` exactly when it has at least two distinct external
entry targets. A one-entry cyclic SCC is not irreducible. A cyclic SCC is classified
`non-terminating-cycle` exactly when none of its points can reach the virtual exit according to M4.3's frozen
reverse-reachability facts. These classifications are independent, so one SCC may emit both facts.

The virtual exit's existence, a disconnected exit edge, or deterministic SCC convergence does not establish a
terminating path.

### Typed residual preservation

Every M4.3 residual becomes one M4.4 fact with the exact source residual key, point closure, entry, and exit.
Its frozen reason maps to one of:

- `invalid-candidate-boundary`;
- `incoming-boundary-bypass`;
- `outgoing-boundary-bypass`;
- `crossing-candidates`;
- `missing-structured-root`.

Unknown residual reason text fails closed during derivation. It is not guessed into a nearby class. Residual
facts and SCC facts are allowed to overlap because they answer different questions and retain distinct typed
provenance.

When the source CFG coverage is not Complete, the overlay also emits one
`unknown-incomplete-control-flow` fact over the source graph with control-flow coverage provenance. Graph-level
coverage retains the exact inherited reasons. Therefore an empty fact list on a Complete reducible graph is
distinguishable from absence of evidence on an incomplete graph.

### Validation and consumer rule

Facts retain canonical, source-closed point and boundary sets. Irreducible facts require at least two entry
points. Nonterminating facts require every component point to be exit-unreachable; they may still have an edge
to another nonterminating component. Residual-derived facts require exactly one entry, one exit, and a source
residual key; SCC-derived and coverage-derived facts use distinct typed origins. All keys bind the complete
payload.

No M4.4 fact is a `StructuredControlRegion`. Consumers that require a PST unit must use M4.3 regions and must
block or explicitly handle intersecting M4.4 facts. Presence of a stable key is not transformation authority.

## Consequences

- Multi-entry irreducibility, nontermination, and rejected SESE candidates remain queryable without corrupting
  the structured hierarchy.
- Later PDG, SDG, and recipe layers can fail closed on explicit typed facts rather than parsing reason strings.
- SCC computation is linear in points and edges and requires no dependency beyond the existing stack.
- M4.4 does not claim that absence of an emitted fact proves complete reducibility when source coverage is not
  Complete; inherited coverage remains visible.

## Rejected alternatives

- **Change `deslop.control-regions/1`.** This silently breaks the frozen M4.3 contract and its payload keys.
- **Call every cycle irreducible.** Reducible natural loops are cyclic and normally have one external entry.
- **Infer irreducibility from syntax.** Syntax nesting does not establish CFG entry boundaries.
- **Treat any outgoing edge as termination.** Only reachability to the virtual exit establishes an exit path.
- **Insert irreducible SCCs into the PST by stable order.** Serialization order supplies no SESE authority.
- **Drop residuals after SCC classification.** Crossing and invalid SESE candidates are not necessarily SCCs
  and carry independent evidence.
