use std::path::PathBuf;

use deslop_eval::m6_benchmark::{M6_LLM_REPORT_SCHEMA, TASK_COUNT, verify_report_assets};

#[test]
fn m6_graph_grounded_work_orders_meet_the_frozen_llm_gates() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.agents/benchmarks");
    let report = verify_report_assets(
        &root.join("m6_llm_tasks_v1.json"),
        &root.join("m6_llm_report_v1.json"),
    )
    .unwrap();

    assert_eq!(report.schema, M6_LLM_REPORT_SCHEMA);
    assert_eq!(report.paired_tasks, TASK_COUNT);
    assert_eq!(report.observations.len(), TASK_COUNT * 2);
    assert!(report.accepted_patch_delta >= 0.10);
    assert!(report.paired_ci95_lower > 0.0);
    assert!(report.graph.out_of_scope_rate <= 0.02);
    assert!(report.graph.unsafe_abstention_rate >= 0.90);
    assert!(report.graph.semantic_regressions <= report.baseline.semantic_regressions);
    assert!(report.gates.values().all(|passed| *passed));
}
