# ADR 0006: Bind dataflow and effects to resolved symbols and exact CFG points

- Status: Accepted
- Date: 2026-07-14
- Decision owners: M4.5 dataflow integration

## Context

M4.1-M4.4 provide exact local CFG points, edges, reachability, structured regions, and explicit non-structured
facts. M3 provides versioned scope/name facts and resolution conclusions. A PDG requires a further authority
boundary: identifier text, syntax containment, source order, or a unique-looking name is not a symbol; a write
is not a definition of every same-spelled reference; and deterministic fixed-point convergence does not make
incomplete def/use or effect extraction Complete.

`AdapterCapability::DefUse` and `AdapterCapability::Effects` are independent S2 capabilities. Production
adapters currently declare both Unknown. M4.5 must make the schema and algorithms real without silently
promoting those adapters or fabricating facts from canonical roles.

## Decision

### Strict projection and evidence builder

M4.5 introduces `deslop.data-flow/1` and `deslop.data-flow-policy/1`. A projection binds the exact analysis,
control-flow and control-region projections and policies, scope graph/build context/fact policy, resolution
projection/policy/provider facts, dataflow policy, adapter capability declarations, and payload-derived
projection, graph, symbol, access, definition, boundary, effect, and per-point keys.

The public builder accepts explicit drafts but validates every draft against retained source evidence:

- every graph identifies one exact source CFG/region graph and owner;
- every symbol is normalized to a retained declaration/definition target and carries its binding/symbol kind;
- every use or write cites one retained Reference scope fact and its exact resolution result;
- a resolved access requires a Complete `Unique` conclusion to the same normalized symbol at sufficient
  authority; ambiguous, unresolved, conflict, dynamic, or incomplete evidence remains an explicit unknown
  access and downgrades graph coverage;
- every access, definition, boundary, and effect identifies an exact point in the same source CFG;
- Complete def/use or effect coverage requires the matching adapter capability to be Provided with explicit
  authority, no recovered/conservative source evidence, and no unresolved draft evidence.

The builder never infers a symbol from spelling, first match, source order, canonical roles, or
`deslop.graph/2`. Those inputs can locate work but cannot authorize data dependence.

### Local facts

Each graph retains canonical symbols, definition occurrences, accesses, parameter/output boundary facts,
per-point conservative effects, coverage, and explicit gaps. Access kinds are read, write, read-write, call,
address/borrow, capture, and adapter-defined. Boundary kinds distinguish parameter input, return output,
mutation output, exceptional output, suspension output, and adapter-defined facts.

Effects are a conservative may-set: reads memory, writes memory, allocates, calls, throws, suspends, returns,
terminates, performs I/O, accesses global state, captures, and adapter-defined effects. An empty effect set
means known pure only under Complete Effects coverage. `Opaque`/unknown effect evidence is retained as a gap;
it is never erased by another precise point.

### Reaching definitions

For entry-reachable points, each definition occurrence generates itself and kills every other definition of
the same normalized symbol. Parameter-input definitions occur at the virtual entry. The forward least fixed
point is:

```text
RD-in[n]  = union(RD-out[p]) for predecessors p in the reachable domain
RD-out[n] = GEN[n] union (RD-in[n] - KILL[n])
```

Unreachable points retain local source facts but have empty reaching sets and an explicit unreachable flag.
Uses retain the exact reaching definition keys for their symbol; an empty set is not silently called
uninitialized when coverage is incomplete.

### Liveness

For entry-reachable points, `USE[n]` is every resolved symbol read before a same-point definition and `DEF[n]`
is every resolved symbol defined at the point. The backward least fixed point is:

```text
LIVE-out[n] = union(LIVE-in[s]) for successors s in the reachable domain
LIVE-in[n]  = USE[n] union (LIVE-out[n] - DEF[n])
```

This equation remains meaningful in nonterminating SCCs and converges without inventing a virtual-exit path.
Unreachable points carry empty liveness sets. Point-local evaluation order must be explicit in the draft; if
the adapter cannot order a same-point read/write, the access remains unknown and coverage is not Complete.

### Parameters, outputs, and consumers

Parameter inputs are source-bound definitions at virtual entry. Return, mutation, exceptional, and suspension
outputs are explicit boundary facts; they are not inferred from a symbol being live at virtual exit. Later
PDG/recipe consumers must require the exact boundary/effect classes they use and must propagate M4.4
non-structured facts and all M4.5 gaps.

## Consequences

- M4.6 can derive data-dependence edges from exact reaching-definition/use links and control dependence from
  M4.3 post-dominance.
- Provided-capability fixtures can numerically validate the full engine before production adapters claim
  DefUse or Effects.
- Production adapters remain honest Unknown until a stored rule pack/extractor is implemented and golden.
- Retained facts are larger, but canonical sets make identity, corruption, and incremental comparison direct.

## Rejected alternatives

- **Match identifier text inside a CFG point.** Spelling does not resolve shadowing, namespaces, or ambiguity.
- **Use canonical Read/Write roles as symbol authority.** Roles classify syntax but do not identify endpoints.
- **Treat all assignments as definitions.** Mutation, compound access, aliasing, and destructuring differ.
- **Infer outputs from exit liveness.** Liveness is not a return/mutation/exception contract.
- **Compute dead-code dataflow as executable truth.** Unreachable local facts do not have reaching execution.
- **Mark deterministic fixed points Complete.** Algorithm determinism cannot upgrade extraction authority.
- **Assume empty effects means pure.** It means pure only with Complete Effects coverage.
