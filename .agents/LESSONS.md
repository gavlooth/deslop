# Durable lessons

## M4.5 resolution ambiguity fixtures require visible bindings

- Scope: M3 resolution fixtures consumed by M4.5 dataflow integration.
- Tried: two same-scope Declaration facts with the same lookup key and one matching Reference.
- Failed result: resolution was `Unknown`, not `Ambiguous`, because declarations alone were not entered into
  the visible binding candidate set.
- Invalidated assumption: duplicate Declaration facts alone exercise duplicate-definition resolution.
- Authority downgrade: an `Unknown` result cannot be relabelled or interpreted as ambiguity by a consumer.
- Preferred alternative: add explicit Binding facts for both declarations at an applicable timing, then assert
  the resolver itself returns `Ambiguous`; dataflow must retain that access with no symbol and Partial coverage.
- Search handles: `M4.5 declarations need bindings`, `ambiguous resolution fixture Unknown`, `collision binding`.
- Status: resolved and locked by `m4_5_ambiguous_resolution_remains_unknown_and_partial`.
- Recheck condition: any change to scope candidate extraction, binding timing, or duplicate-definition rules.

## M4.6 entry and exit reachability are independent domains

- Scope: M4.3 relation facts consumed by M4.6 PDG node validation and control-dependence derivation.
- Tried: rejected any PDG node with `reachable == false` and `exit_reachable == true`.
- Failed result: a valid nonterminating CFG with an unreachable exit-dispatch component was rejected even
  though that component correctly reaches the required virtual exit.
- Invalidated assumption: exit reachability implies entry reachability.
- Authority downgrade: neither domain may be reconstructed or overwritten from the other.
- Preferred alternative: retain both exact M4.3 booleans independently; require entry reachability only for
  emitted execution-dependence edges, and require usable exit-reachability/ipdom evidence for control walks.
- Search handles: `M4.6 reachability domains independent`, `unreachable exit dispatch exit reachable`.
- Status: resolved and locked by the ambiguous/nonterminating M4.6 integration fixture.
- Recheck condition: any change to M4.3 reachability domains, virtual-exit topology, or PDG node validation.
