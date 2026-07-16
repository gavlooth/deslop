# M7 verifier migration

M7 adds an authoritative transaction path without treating legacy verification output as equivalent authority.

- `deslop.verify/1`, `deslop.apply/1`, and `deslop.recipe-apply/1` remain readable compatibility/reporting surfaces.
  Their historical verdicts are not `deslop.verifier-plan/1` evidence and cannot be promoted into an M7 transaction.
- New automatic writes must use an exact `SharedWorkOrder`, a Ready `VerifierPlan`, a server-owned
  `VerificationRuntime` with an authoritative `GraphDeltaOracle`, complete selected evidence, and explicit write
  authority. Missing inputs reject; there is no inferred migration.
- Existing ordinary patch and controlled recipe-canary writes now use the durable `deslop.undo-manifest/1` atomic
  source journal. Before any new write, callers should run `recover_incomplete_transactions(root, ".deslop/undo")`.
- Risky characterization files in `deslop.characterization-test/3` do not by themselves satisfy M7.3a. Capture and
  approve a `deslop.pre-change-characterization/1` on the exact work-order snapshot before patch authorship, then
  produce matching passing characterization evidence.
- Compiler/LSP/adapter results must be re-emitted as snapshot- and artifact-bound `AuthorityObservation`s. Old rank-
  collapsed conclusions are not accepted; conflicting providers block.
- Project-local recipe counterexamples are appended under `.deslop/negative-memory/recipes.jsonl`. A recipe remains
  demoted until an explicit supersession entry names the incorporated fix/review authority.
- Hosts that cannot establish the requested namespace/network/filesystem policy receive structured policy failure.
  They must install/configure an enforceable sandbox or use a server-owned runtime with equivalent enforcement; they
  must not fall back to inherited shell execution.
