# Recipe production canary report

Date: 2026-07-15

Recipe: `rust-remove-unreachable-literal-statement`

Decision: **DISABLED for automatic application**

The guarded workflow is implemented and the controlled transaction passes, but the production
enablement gate does not. `deslop recipes apply` rejects writes by default; `--canary` is the only
explicit write authority and does not bypass parse, graph-delta, build, test, or rollback checks.

## Frozen thresholds and resource budgets

- Natural evidence: at least 30 natural positives and 300 audited hard negatives; actionable
  precision and recall lower 95% bounds >= 0.95; hard-negative FPR upper 95% <= 0.01; ECE <= 0.05;
  abstention <= 1%.
- Safety: zero protected-resource violations, 100% accepted parse/build/test/graph-delta checks, and
  100% exact rollback plus rollback-check success.
- Detection on a repository below 100k Rust LOC: <= 75 seconds and <= 1 GiB RSS.
- Controlled full transaction: <= 120 seconds and <= 768 MiB RSS.

Reference machine: `spark-05bb`, Linux 6.17.0-1008-nvidia aarch64, 20 ARM
Cortex-X925/Cortex-A725 cores, 121 GiB RAM, rustc 1.94.0
(`4a4ef493e`, `aarch64-unknown-linux-gnu`). Seed: 0.

## Pinned natural repositories

### supervisor-mcp

- Revision: `f241a64f5c2bb9fadac2476e2395c7fdcd488e56`.
- Licence: MIT from package metadata.
- Seven Rust files, extracted without `target`; release detector warm.
- Result: 7/7 files analyzed, zero candidates, zero abstentions, 4.14 seconds, 61,816 KiB RSS.
- Detection report SHA-256:
  `028c2dfe98f03794225a3ac4512692e7b903edce5a7ecd4cc9e982fecb49d591`.

### deslop

- Revision: `cd153f7466e84341936e627bf4172157900051f2`.
- Licence: MIT from workspace metadata.
- 83 Rust files and 92,316 Rust lines, extracted without `target`; release detector warm.
- Result: the frozen 75-second timeout fired at 75.26 seconds. Peak RSS was 994,728 KiB and no
  terminal detection report was emitted. This is a resource-budget failure and a whole-repository
  abstention, not a zero-candidate result.
- Timing evidence SHA-256:
  `e93a0f3e9801cc9f9469abd7f58dc3023fe5ca610b784db10f98a9da8502068a`.

## Controlled opportunity on the pinned supervisor repository

A separate isolated copy of the pinned supervisor revision added one compiled module containing the
exact inert-literal opportunity. This controlled positive is not pooled with natural or synthetic
evidence. The unified diff was manually audited: it deleted only `1;`; all other bytes, source files,
and public APIs were protected.

- Detect: exactly one candidate and one unique strict work order
  `rwo1_5b3d792cb3b167aa928474ac2b2d52db3260823efa309a962605926f0a6ec1e7`.
- Apply commands: `cargo check --locked`, then `cargo test --locked`.
- Apply result: staged and live parse, graph delta, build, and tests all passed; protected resources
  unchanged; disk state `candidate-applied`; live state `rebuilt-and-tested`.
- Cold staged/live transaction: 93.04 seconds, 606,608 KiB RSS.
- Apply report SHA-256:
  `166b87f95ab9c4648094c015a0572ad912e591d6b716917499d764b5d21a4596`.
- Fault injection: the test command failed only in the live phase. Exact original bytes were restored;
  candidate identity returned; rollback parse, graph delta, build, and tests all passed.
- Rollback transaction: 50.98 seconds, 601,660 KiB RSS.
- Rollback report SHA-256:
  `08d6cca59f4e95d6f1199a02bb7da0adbeef8e22d12c498c56005f2c04ff624a`.

The controlled recall point estimate is 1/1, but its Wilson lower 95% bound is only
0.206549314377. Natural positives are zero. Precision, natural recall, hard-negative FPR, and ECE
therefore remain unestablished; the synthetic 1,000-positive/1,000-negative B2/B7 slice is reported
separately and was not pooled.

## Safety and mutation evidence

- Candidate and recipe identities reject payload, order, source, validation, rollback, and unknown-field
  mutation.
- Recipe work-order identity rejects candidate, patch, resource-set, budget, validation, rollback,
  duplicate, foreign-field, and stale-target mutation.
- Immediate pre-write guards reject a concurrent source change without overwriting it.
- The apply engine computes only declared edits, checks protected files after live commands, reports
  disk and live state separately, and reruns exact parse/graph/build/test checks after rollback.
- A control-region residual closure bug found by the first real scan was fixed at its graph invariant:
  rejected SESE candidates now retain their proposed entry and exit in the residual closure.

## Disable reasons and recheck

Automatic application is disabled because:

1. The pinned deslop scan violates the frozen detection runtime budget and does not terminate with a
   report.
2. One controlled positive and zero natural positives cannot establish the required confidence bounds.

Recheck only after the retained graph chain finishes a <=100k Rust LOC repository within 75 seconds
and 1 GiB RSS, and after a frozen real-repository set contains at least 30 natural positives and 300
hard negatives meeting every threshold above. Then rerun the same natural scan, controlled apply, and
live-failure rollback commands.

The exact machine-readable record is [RECIPE_CANARY_EVIDENCE.json](RECIPE_CANARY_EVIDENCE.json).
