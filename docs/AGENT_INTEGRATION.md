# Agent integration

Use Deslop as a bounded evidence and transaction service, not as a prompt that grants an agent permission to edit.

## Recommended loop

1. `deslop scan --format agent <scope>` for findings and explicit partial/unknown provenance.
2. `deslop graph --format json <scope>` for `deslop.graph/2` ownership and dependency evidence.
3. Use bounded work-order `index` and `triage`; page with the returned cursor and keep the query budget.
4. Call `explain` for a selected order, then `plan` to respect prerequisites, conflicts, invalidations, atomic
   groups, and schedule waves.
5. Produce a patch only within the work-order target and patch budget. Preserve protected APIs/spans and unknowns.
6. Submit the patch through `verify`; use policy-gated `apply` only with the returned current authorization.
7. On source change, discard stale handles and re-index. Never relocate an old byte span heuristically.

## Model boundary

The M6 paired benchmark used the same 240 tasks, model, reasoning setting, output-token limit, and task budget for
graph and baseline arms. Graph-rich work orders improved accepted-patch rate by 44.17 percentage points with a
positive paired 95% interval, zero graph-arm semantic regressions/out-of-scope edits, and 100% unsafe abstention.
That result supports graph context; it does not make arbitrary model output safe.

## Failure behavior

- Partial/unsupported/failed analysis blocks semantic rewrite authority.
- `NeverAuto` never becomes a patch task.
- Pagination/truncation is explicit; absence beyond a budget is not proof of no candidates.
- A stale cursor, plan, work order, revision guard, source byte, or verifier authorization is rejected.
- Whole-project finding-proposal batching is unshipped after the M10 timeout. Use bounded operations.
- Readability labels are unshipped; agents may consume transparent axes but not infer readability/authorship/safety.

For MCP/LSP, negotiate schemas and retain the exact provenance returned by the server. Do not rebuild work-order
IDs or graph facts in the client. See [SECURITY.md](SECURITY.md) and [VERIFIER_POLICY.md](VERIFIER_POLICY.md).
