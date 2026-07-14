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
