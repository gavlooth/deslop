use std::fs;
use std::path::PathBuf;

use deslop_eval::m9_scale::M9ScaleBenchmarkReport;

#[test]
fn m9_definition_of_done_is_locked_to_release_measurements_and_integrations() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report_path = repository.join(".agents/benchmarks/m9_scale_report_v1.json");
    let report: M9ScaleBenchmarkReport =
        serde_json::from_slice(&fs::read(&report_path).expect("M9 benchmark report"))
            .expect("strict M9 report");
    report.validate_terminal().expect("M9 terminal benchmark");

    assert_eq!(report.projects.len(), 3);
    assert_eq!(report.environment.build_profile, "release");
    assert!(report.environment.peak_rss_bytes.is_some());
    assert!(report.projects.iter().all(|project| {
        project.files == 480
            && project.cold_parse_count == 480
            && project.incremental_parse_count_max == 1
            && project.incremental_reused_files_min == 479
            && project.candidate_cache_hits_min == 479
            && project.candidate_cache_misses_max == 1
            && project.invalidation_fan_out_max == 1
            && project.incremental_projection_files_max == 1
            && project.warm_incremental_p95_micros <= 500_000
            && project.incremental_to_full_ratio <= 0.05
    }));

    let ci = fs::read_to_string(repository.join("docs/CI.md")).expect("CI integration guide");
    for required in [
        "scan --changed",
        "baseline update",
        "--format sarif",
        "--fail-on",
    ] {
        assert!(ci.contains(required), "CI guide missing {required}");
    }
    let workflow =
        fs::read_to_string(repository.join(".github/workflows/deslop.yml")).expect("CI workflow");
    assert!(workflow.contains("upload-sarif"));

    let capability = fs::read_to_string(repository.join("docs/M9_CAPABILITY_MATRIX.md"))
        .expect("M9 capability matrix");
    assert!(capability.contains("false-positive feedback"));
    assert!(capability.contains("Editor refresh"));
    assert!(capability.contains("Shared sessions"));
}
