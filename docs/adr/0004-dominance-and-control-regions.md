# ADR 0004: Reachability-bounded dominance and control regions

- Status: Accepted
- Date: 2026-07-14
- Owners: deslop maintainers
- Roadmap: M4.3; governs M4.4-M4.9 and constrains M5-M9
- Builds on: ADR 0001, ADR 0002, and ADR 0003

## Context

ADR 0003 defines one virtual-entry/virtual-exit control-flow graph per executable owner. M4.2 now lowers Rust
from an exact stored adapter rule pack and retains explicit Partial coverage and Unknown adapter gaps. A graph
may also retain syntax points after an abrupt-only prefix, and a reachable loop may have no path to the virtual
exit. Serialization membership therefore does not imply entry reachability, termination, dominance, or
structured control.

Dominance and post-dominance are required for control dependence, program-structure regions, and later
transformation preconditions. Computing them over the wrong point domain is actively misleading: a dead point
cannot be dominated in an execution from entry, and a nonterminating point cannot be post-dominated by a
virtual exit it never reaches. Stable point order cannot repair either error.

## Decision

Deslop will use the strict wire schema `deslop.control-regions/1` as a derived projection over one exact
`deslop.control-flow/1` projection. It records reachability, dominance, post-dominance, and canonical
single-entry/single-exit point regions without changing or backfilling the source CFG.

### Ownership and identity

Every document binds the exact control-flow projection ID, analysis ID, control-flow policy, region-algorithm
policy, and a canonical graph analysis for every source CFG. Each graph analysis binds the source graph key,
owner, entry, exit, inherited adapter/capability evidence, derived coverage, every retained point fact, every
structured region, and every non-structured candidate. Projection, graph-analysis, point-fact, and region keys
are derived from the complete payload they identify.

Keys expire when the source CFG, adapter identity, source revision, lowering policy, region policy, or derived
payload changes. They are correlation identities, not edit authority. No syntax reparse, canonical role,
control-query capture, or `deslop.graph/2` fact participates in the algorithms.

### Reachability domains

For a source graph `G = (V, E)` with virtual entry `s` and virtual exit `t`:

- `R_s` is the least set containing `s` and closed under outgoing CFG edges;
- `R_t` is the least set containing `t` and closed under incoming CFG edges;
- the terminating core is `R_s ∩ R_t`;
- points outside `R_s` are explicitly unreachable from entry;
- points in `R_s - R_t` are explicitly reachable but exit-unreachable.

All retained edge families participate. A Conservative edge remains in the graph-theoretic relation but its
source CFG already prevents Complete derived semantic coverage. Point serialization order and syntax position
never add reachability.

The virtual exit is not evidence that every point terminates. An exit edge in a disconnected component does
not place an entry-reachable cycle in `R_t`.

### Dominance and post-dominance

Dominance is defined only on `R_s`. The entry dominates itself. For every other entry-reachable point, the
dominator set is the point itself plus the intersection of its entry-reachable predecessors' dominator sets.
Points outside `R_s` have an empty dominator set and no immediate dominator.

Post-dominance is defined only on `R_t` using the reverse relation. The exit post-dominates itself. For every
other exit-reachable point, the post-dominator set is the point itself plus the intersection of its
exit-reachable successors' post-dominator sets. Points outside `R_t` have an empty post-dominator set and no
immediate post-dominator.

For a non-root point, the immediate dominator is the unique strict dominator dominated by every other strict
dominator. Immediate post-dominance is dual. A missing or non-unique immediate parent when strict parents exist
is invalid; deterministic order cannot select one. Full relation sets, immediate parents, and tree depths are
stored together and cross-validated.

### Structured point regions

M4.3 uses point-hammock SESE regions. The root is the terminating core bounded by virtual entry and exit when
that core satisfies the boundary rules. A nontrivial candidate begins at a reachable branch dispatch or loop
header and ends at its immediate post-dominator. Its point set contains exactly the terminating-core points
dominated by the entry and post-dominated by the exit.

A structured region is valid only when:

1. it contains distinct entry and exit points and at least one interior point;
2. every edge entering a member other than the region entry originates inside the region;
3. every edge leaving a member other than the region exit targets a member inside the region;
4. its entry dominates and its exit post-dominates every member;
5. it is either disjoint from or strictly contains/is contained by every other structured region; and
6. its parent is the unique smallest strict containing region.

Children are canonical and reciprocal with parent links. Equal point sets or overlapping non-laminar
candidates are not ordered by kind, source position, size ties, or keys. They are retained as explicit
non-structured candidates for M4.4. M4.3 does not claim that every reducible-language syntax construct becomes
a region, nor that graph-theoretic SESE alone proves a safe transformation.

### Coverage and authority

Derived coverage cannot exceed source CFG coverage. Source Partial/Unsupported/Failed status and canonical
reasons are retained. A graph with any entry-reachable exit-unreachable point is additionally Partial with an
exact reason. Conservative/recovered source evidence therefore cannot become Complete by running a
deterministic fixed point.

Complete region coverage requires a Complete source CFG, no reachable exit-unreachable points, internally
consistent complete dominance/post-dominance relations, and no invalid/overlapping candidate represented as
structured. Unreachable points may be recorded under Complete coverage because the complete result explicitly
states that they are unreachable rather than assigning them execution facts.

Region facts inherit the CFG's stored static ControlFlow authority. The algorithm adds graph-theoretic
derivation, not language authority. M5 consumers must still require Complete coverage and sufficient source
authority for every semantic obligation.

## Verification requirements

The schema and algorithms are accepted only when executable tests establish:

1. linear, diamond, nested-branch, loop, abrupt-only, unreachable-suffix, nonterminating, and Partial graphs
   have exact numerical reachability, dominator, post-dominator, immediate-parent, depth, and region results;
2. unreachable points receive no dominance claims and exit-unreachable points receive no post-dominance claims;
3. immediate parents are unique consequences of full relation sets, not point order;
4. every structured region satisfies both boundary directions and canonical laminar parent/child closure;
5. overlapping candidates, source uncertainty, and nontermination cannot serialize as Complete structure;
6. unknown fields, stale IDs/keys, noncanonical relations, dangling/cross-graph links, relation asymmetry,
   false reachability flags, parent/depth mismatch, and region-boundary corruption fail closed; and
7. full workspace tests, build, rustdoc, clippy, formatting, diff checks, and prior milestone gates pass.

## Consequences

Downstream control-dependence and transformation code receives explicit domains instead of silently assuming
all retained points execute and terminate. The schema is larger because it stores full relations as well as
immediate trees, but this makes authority, corruption, and consumer behavior auditable without rerunning an
implicit algorithm.

M4.4 has a clear boundary: classify retained non-laminar or otherwise irreducible candidates without weakening
M4.3's structured-region claims. Later PDG/PST/recipe layers must propagate the same coverage truth.

## Rejected alternatives

- **Initialize dominance over every serialized point.** This assigns execution relations to unreachable code.
- **Post-dominate every point from the forced virtual exit.** A virtual boundary does not create a path from a
  nonterminating or disconnected point.
- **Use syntax nesting as the PST.** Syntax containment does not establish graph entry/exit boundaries and
  fails for abrupt, exceptional, suspension, and irreducible control.
- **Enumerate every dominance/post-dominance pair as a region.** The resulting intervals are redundant and may
  overlap without a unique hierarchy.
- **Break region overlap ties deterministically.** Stable order is serialization, not semantic evidence.
- **Upgrade Partial CFGs after deterministic analysis.** Fixed points are exact over retained edges but cannot
  prove that missing language edges do not exist.
