# ADR 0008: Build SDG edges from explicit interprocedural bindings

- Status: Accepted
- Date: 2026-07-14
- Decision owners: M4.7 summaries and system-dependence integration

## Context

M4.5 retains ordered local definitions/accesses, explicit parameter/output boundaries, effects, and exact M3
resolution keys. M4.6 retains local PDG points and control/flow edges. These facts can identify a Complete
Unique call target when the resolved declaration/definition node is exactly a retained CFG owner, but they do
not encode call-argument/formal-parameter order, a caller definition receiving a return, variadic/default
mapping, or which non-local symbol an opaque global-state effect touches.

Inferring those relations from syntax child order, parameter source order, same-spelled names, liveness, or
effect flags would fabricate S3 authority. `AdapterCapability::CallGraph` and `AdapterCapability::Sdg` are
independent S3 declarations and production adapters currently keep them Unknown.

## Decision

### Strict summaries and source bindings

M4.7 introduces `deslop.system-dependence/1` and `deslop.system-dependence-policy/1`, bound to the exact
analysis, M3 resolution, M4.1-M4.6 source projections and policies, and the CallGraph/Sdg capability
declarations of every participating adapter. The wire document retains support and authority for both
capabilities per local PDG; aggregate coverage never substitutes for that graph-specific evidence.

There is one callable summary per local PDG graph. A source-validated summary draft explicitly orders formal
parameter-input boundary keys, classifies return/mutation/exception/suspension outputs, and identifies global
symbol summaries. A global summary cites one normalized declaration plus exact local read accesses, write
definitions, and mutation-output boundaries; every cited fact must resolve to that declaration.

### Calls and parameter/return bindings

Every call-site draft cites one M4.5 `Call` access. The builder derives the callee only from the call access's
exact Complete Unique resolution conclusion. Its single preferred Declaration or Definition fact must be
retained at exactly one local CFG owner node. No name, containment, type-name, or source-order fallback exists.

The draft explicitly pairs actual-input access keys with ordered callee parameter-input boundary keys and
pairs callee return/mutation outputs with exact caller definition keys. Cardinality, order, default/variadic,
receiver, destructuring, and alias semantics remain gaps unless the draft states and the source facts validate
them. A missing binding produces a typed gap for that exact boundary. Other independently source-validated
bindings may still emit edges, but coverage remains incomplete; no guessed edge fills the missing binding.

### SDG nodes, edges, and gaps

The SDG reuses exact M4.6 local graph/node keys. Interprocedural edge kinds are:

- call: caller call point to callee entry;
- parameter-in: caller actual access point to callee formal-input point;
- return: callee return-output point to the exact caller receiving definition point;
- parameter-out: callee mutation-output point to the exact caller receiving definition point.

Each edge cites the call-site key and its exact access/boundary/definition evidence. Strict deserialization
checks graph direction and binding membership against that call site, in addition to content-addressed keys.
Global summaries are retained but do not create global flow edges in v1: M4.5 has no interprocedural
memory-order or alias contract.

Typed gaps distinguish unresolved/non-local callee, missing formal/actual binding, unsupported output kind,
capability insufficiency, and inherited local-PDG uncertainty. Complete coverage requires Provided CallGraph
and Sdg capabilities for participants, Complete source summaries/PDGs, and no gaps.

## Consequences

- M4.8 can add exception, async/yield, closure, mutation, and alias fixtures without changing call/parameter
  evidence into syntax guesses.
- Production adapters remain honest Unknown until an S3 extractor/rule pack supplies explicit bindings.
- Local PDGs remain useful when SDG construction is Partial or Unsupported.
- Default/variadic/receiver behavior is visible as a gap rather than silently zipped by position.

## Rejected alternatives

- **Zip arguments and parameters by syntax order.** The retained facts do not authorize language-specific
  default, variadic, receiver, keyword, destructuring, or evaluation semantics.
- **Resolve the callee by name or owner containment.** Only one exact Complete Unique endpoint at a CFG owner
  authorizes a local call edge.
- **Infer return destinations from liveness.** Return and receiving-definition bindings must be explicit.
- **Connect all same-global symbols.** No interprocedural memory order or alias fact currently authorizes it.
- **Treat CallGraph as sufficient for SDG.** Call target and parameter/return dependence are distinct evidence.
