# M7 verifier migration

M7 adds an authoritative transaction path without treating legacy verification output as equivalent authority.

- `deslop.verify/1`, `deslop.apply/1`, and `deslop.recipe-apply/1` remain readable compatibility/reporting surfaces.
  Their historical verdicts are not `deslop.verifier-plan/2` evidence and cannot be promoted into an M7 transaction.
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
- Verifier plans cut over to `deslop.verifier-plan/2` / `vp2_` identities with mandatory
  `maximum_memory_bytes` and `maximum_processes`. Old /1 plans are rejected, not silently upgraded.
  The hermetic defaults are 2 GiB memory, no swap and 256 processes, enforced by a fresh systemd
  user scope before Bubblewrap starts; `prlimit` enforces per-file size. A working user manager,
  delegated controllers, `/usr/bin/prlimit` and the sandbox are required for external checks.
  File-count watchdogs and output/time budgets remain additional rejection/termination limits,
  not a claim of a hard aggregate disk quota.
- Hosts that cannot establish the requested namespace/network/filesystem policy receive structured policy failure.
  They must install/configure an enforceable sandbox or use a server-owned runtime with equivalent enforcement; they
  must not fall back to inherited shell execution.
