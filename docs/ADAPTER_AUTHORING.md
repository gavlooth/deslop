# Language adapter authoring

An adapter translates a pinned grammar into Deslop's owned syntax and capability contracts. It must describe what
it knows and what it does not know; grammar coverage must never be inferred from another language.

## Required contract

1. Pin the grammar crate/version and a distinct adapter schema.
2. Select exact dialects/extensions. JavaScript, TypeScript, TSX, and JSX are not interchangeable fallbacks.
3. Implement query packs for declarations, references, scopes, control, comments, and opaque/generated regions.
4. Map canonical roles while retaining raw grammar kind, field, span, parent/children, named/error/missing flags,
   token ownership, and provenance.
5. Provide lexical/operator policy. Identifiers, strings/numbers, operators, keywords, and public API tokens must
   remain distinguishable.
6. Publish `deslop.language-adapter-capabilities/2` support for S0-S4. Missing support is `Unknown` or explicitly
   `Unsupported`, never a zero/default fact.
7. Define malformed, macro/dynamic, generated, schema, test, public-API, and intentional-repetition behavior.

## Capability tiers

- S0: bytes, language/dialect identity, parse provenance.
- S1: owned CST, tokens, canonical syntax roles, containment/ownership.
- S2: scopes, definitions/references/imports/exports and resolution paths.
- S3: control-flow/PST facts.
- S4: def/use, effects, PDG/SDG and transformation-grade semantic facts.

Advertising a higher tier requires exact fixtures and coverage reasons for every gap. Optional compiler/LSP facts
must be revision- and artifact-bound and cannot silently replace adapter facts.

## Test matrix

Add valid and malformed golden fixtures, raw kind/field/span/token/role assertions, parse-once and deterministic
node-order tests, duplicate/shadow/alias/import cases, generated/opaque markers, dialect negatives, and
fail-closed leakage tests. Control/data tiers additionally require hand-labelled edge/region fixtures and
exception/async/closure/mutation uncertainty cases.

Run the smallest adapter tests first, then the cross-adapter matrix and full workspace fmt/build/test/clippy gate.
Update capability/migration docs and negative memory for any invalidated grammar assumption. New dependencies need
an explicit maintenance and compatibility justification.

M10 ships Clojure, JavaScript, Julia, Python, Rust, and TypeScript syntax at S1. Deeper facts ship only where the
versioned capability and frozen evidence explicitly provide them.
