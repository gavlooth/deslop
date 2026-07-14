# ADR 0007: Derive local PDGs only from retained control and data evidence

- Status: Accepted
- Date: 2026-07-14
- Decision owners: M4.6 local program-dependence integration

## Context

M4.1-M4.4 retain exact local control-flow, dominance/post-dominance, structured regions, and explicit
non-structured facts. M4.5 retains exact resolved symbols, ordered definitions/accesses, reaching definitions,
liveness, boundaries, effects, and coverage gaps. A local program dependence graph must join those artifacts
without treating syntax adjacency, same-spelled names, liveness, or a stable traversal order as dependence.

Classical control dependence requires a usable post-dominator chain. M4.3 intentionally does not force
exit-unreachable/nonterminating points to a virtual exit, so M4.6 must preserve that missing control-dependence
evidence rather than manufacture a total graph.

## Decision

### Strict projection and nodes

M4.6 introduces `deslop.program-dependence/1` and `deslop.program-dependence-policy/1`. A projection binds the
exact analysis, CFG, control-region, non-structured-control, resolution/dataflow projections and policies.
There is exactly one PDG graph for every source dataflow graph. Every local PDG node represents one exact CFG
point and cites its matching M4.3 relation fact and M4.5 point fact.

The graph retains the exact M4.4 non-structured fact keys that intersect it. Irreducibility does not by itself
invalidate a mathematically complete dependence edge, but consumers cannot lose or ignore the fact.

### Flow data dependence

Each resolved M4.5 access produces one flow-dependence edge for every exact reaching definition retained on
that access. The edge runs from the definition's point to the access's point and cites the symbol, definition,
and access keys. Multiple reaching definitions remain multiple edges. Unknown/unresolved accesses produce a
typed gap and never a guessed edge. Empty reaching sets are preserved without being relabelled uninitialized.

Anti- and output-dependence are not inferred from liveness or source order; they require a future explicit
memory/write-order contract and are outside `program-dependence/1` v1.

### Control dependence

For every entry-reachable CFG edge `x -> y`, if `y` post-dominates `x`, the edge induces no control
dependence. Otherwise, when `ipdom(x)` exists, walk the retained immediate-post-dominator chain from `y` up to
but excluding `ipdom(x)`. Every visited point is directly control-dependent on `x`; the PDG edge cites the
exact inducing CFG edge. Equal controller/dependent pairs aggregate canonical inducing-edge keys.

If the required source point, target point, stop point, or chain link lacks exit-reachable post-dominance
evidence, emit a typed control-post-dominance gap for the inducing CFG edge and do not emit partial guessed
edges for that witness. Unreachable CFG edges emit no execution dependence.

### Coverage and validation

Complete PDG coverage requires Complete control-region, non-structured-control, and dataflow coverage and no
typed gaps. Incomplete upstream evidence and every typed local gap propagate exact canonical reasons. Strict
deserialization validates source closure, canonicality, payload keys, edge endpoints, evidence keys, and graph
identity. Policy or any exact source projection change changes PDG identity.

## Consequences

- M4.7 can attach call/parameter/return/global summary and SDG edges to exact local PDG points and M4.5
  boundaries.
- Nonterminating control remains honest: data dependence may still be available while control dependence is
  explicitly incomplete.
- Same-point flow-dependence self-edges are retained when an access reads a reaching definition at that point.
- Consumers can distinguish no dependence from unavailable dependence without parsing reason strings.

## Rejected alternatives

- **Treat every CFG successor as control-dependent.** Sequencing is not control dependence.
- **Force nonterminating flow through virtual exit.** This contradicts M4.3 and fabricates post-dominance.
- **Match access text to definitions.** Only M4.5 resolved reaching-definition keys authorize data edges.
- **Infer anti/output dependence from liveness.** Liveness lacks the required ordered memory/write contract.
- **Drop M4.4 facts after deriving edges.** A dependence consumer must retain non-structured uncertainty.
