# M8 capability matrix

| Surface | Capability | Authority and failure behavior |
| --- | --- | --- |
| deslop-metrics | Exclusive node features | deslop.readability-features/1, content-addressed and tamper-evident |
| Structural axis | Cyclomatic complexity | Exact CFG E-N+2P when complete; syntax fallback is explicitly marked |
| Entropy axis | Token, AST-kind, byte entropy | Each value records estimator and sample size; no compression/surprisal alias |
| Other axes | Surprisal, redundancy, cohesion, impact, safety | Present as measurements or explicit unknowns; never zero-filled |
| Dataset registry | Licence/provenance gate | Exact revision/checksum/task/population/limits; rejected licences cannot import |
| Feature capture | Candidate capture | One candidate ID exactly once; capture is content-addressed and tamper-evident |
| Evaluation | Baselines and ablations | Size, NLOC/complexity, lexical, eight leave-one-axis-out runs from one capture |
| Evaluation | Generalization | Frozen-score project/language holdouts, size-controlled subset, Wilson 95% intervals |
| Evaluation | Calibration | Brier score and ten-bin ECE |
| Label decision | Portable model | Allowed only if every frozen gate passes |
| Label decision | Language/role models | Not evaluated in M8; cannot ship |
| Current product | Evidence-only UX | Required; transparent axes retained, readability labels prohibited |
| Safety/write path | Rewrite authority | None; M7 policy remains the only apply authority |

The executable evidence is crates/deslop-eval/tests/m8_definition_of_done.rs and
crates/deslop-eval/evaluation/m8/evaluation_report.json.
