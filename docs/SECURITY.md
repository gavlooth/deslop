# Security model

Deslop analyzes untrusted repositories and can, only through an explicit verifier/apply transaction,
write source files. Parsing and reporting do not grant command, network, filesystem, or rewrite authority.

## Trust boundaries

- Source text, repository configuration, compiler/LSP output, model output, patches, cached artifacts,
  baselines, and work-order handles are untrusted inputs.
- Tree-sitter establishes syntax structure, not behavior or semantic equivalence.
- External analyzers and language servers contribute versioned observations. Missing, stale, partial, or
  conflicting observations remain `Unknown`/`Conflict`; provider rank never silently chooses a winner.
- An LLM is a proposal producer. It never receives apply authority and cannot weaken verification policy.
- `NeverAuto` output is report-only. `SafeAuto` still requires exact current bytes and every selected check.

## Read-only analysis

`scan`, `metrics`, `graph`, bounded work-order queries, and recipe detection are read-only. Discovery respects
repository ignore rules. Malformed, generated, macro/dynamic, unsupported, and partial regions retain explicit
provenance and cannot authorize a rewrite.

Content-addressed artifacts bind source bytes, grammar/adapter/schema versions, configuration, and relevant
tool/model metadata. A digest detects mutation; it is not an identity signature. M10 has no configured signing
trust root, so release artifacts claim digest integrity only.

## Commands and sandboxing

Verifier commands must come from explicit policy, not repository text or model output. The M7 transaction
runtime clears inherited environment state, bounds time/output/files, applies the requested filesystem/network
policy, and returns structured failure when the host cannot enforce it. There is no unsandboxed fallback.

Do not place secrets in patches, prompts, source snapshots, command arguments, or benchmark artifacts. Give CI
only the repository permissions it needs; SARIF upload normally requires read access plus code-scanning upload.

## Write authorization

An automatic write requires all of the following:

1. A current content-addressed `SharedWorkOrder` and revision guard.
2. Complete required preconditions and no active recipe demotion/counterexample.
3. A Ready verifier plan with explicit resource, environment, network, and filesystem policy.
4. Passing selected parse/format/build/lint/type/test/behavior checks.
5. Exact expected-versus-actual graph delta after formatting and reanalysis.
6. A durable undo journal and successful atomic commit.

Any missing, stale, partial, conflicting, timed-out, crashed, or mismatching input blocks the write. Review does
not convert unknown evidence into proof.

## Recovery and disclosure

Run incomplete-transaction recovery before any new write. Do not manually edit `.deslop/undo` journals. Preserve
the failing work order, exact revision, policy, command evidence, and recovery result when reporting a security or
integrity issue. A counterexample immediately demotes the affected recipe through the negative-memory path.

See [VERIFIER_POLICY.md](VERIFIER_POLICY.md), [UNDO_RECOVERY.md](UNDO_RECOVERY.md), and
[M10_FAILURE_RISK_REGISTER.md](M10_FAILURE_RISK_REGISTER.md).
