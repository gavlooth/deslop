# NEXT — P0 research-registry checkpoint (replaces stale June-24 Task-15 pointer)

Task 15 (mutation parallelism) is COMPLETE; evidence lives in `.agents/SESSION_REPORT.md`
and VCS history (e158292). Historical reference only — this pointer contains the current task.

Current checkpoint: `docs/RESEARCH_PLAN.md` P0 engineering is DONE; the P0 source
gate is BLOCKED (2 full texts unavailable: sjoberg-2013-maintenance,
buse-2010-readability — attempt detail in `docs/RESEARCH.md` §5 and the registry
`threats` fields). The full P1–P8 plan is NOT complete.

Proof (2026-09-08):
`cargo run -p deslop-eval --bin research-registry -- check` →
`research registry OK: 65 rules, 16 recipes, 281 metric fields, 27 claims
(live metrics shape checked)`.
Canonical registry: `crates/deslop-eval/evaluation/research/registry.json`
(`deslop.research-registry/1` v1.0.0). Claims table derives from it:
`cargo run -p deslop-eval --bin research-registry -- inventory`
(detail: `... -- claims`).

Next: P1 pilot (rubric, provenance schema, importer) and P2 execution-boundary
hardening per `docs/RESEARCH_PLAN.md`. No human-benefit or validation claims until
their gates pass. P0 source review stays blocked pending library copies of the
two unavailable texts — the registry records zero `reproduced` methods.

---

Historical reference only: Task 15 evidence lives in `.agents/SESSION_REPORT.md` and VCS history (e158292). This pointer file contains the current task only.
