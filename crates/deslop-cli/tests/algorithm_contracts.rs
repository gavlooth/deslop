use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::Value;

const LEGACY_METRIC_KEYS: &[&str] = &[
    "health_score",
    "readability_score",
    "readability_model",
    "readability",
    "measurement_confidence",
    "refactor_confidence",
    "refactor_confidence_score",
    "refactor_candidates",
    "refactor_confidence_distribution",
    "compression_ratio",
];

#[test]
fn clean_and_sloppy_corpus_probes_are_machine_honest() {
    let clean = corpus("clean");
    let sloppy = corpus("sloppy");

    let clean_metrics = command_json("metrics", &clean);
    let sloppy_metrics = command_json("metrics", &sloppy);
    assert_metrics_probe(&clean_metrics, 8);
    assert_metrics_probe(&sloppy_metrics, 13);

    let clean_slop = command_json("slop", &clean);
    let sloppy_slop = command_json("slop", &sloppy);
    assert_eq!(clean_slop["schema"], "deslop.slop/2");
    assert_eq!(sloppy_slop["schema"], "deslop.slop/2");
    assert_eq!(clean_slop["status"], "complete");
    assert_eq!(sloppy_slop["status"], "complete");
    // These are deterministic weighted-density snapshots, not probabilities or readability
    // calibration. The metrics contract above deliberately makes no clean/sloppy burden ordering.
    assert_close(
        clean_slop["score"].as_f64().expect("clean slop score"),
        0.819_672_131_147_541,
    );
    assert_close(
        sloppy_slop["score"].as_f64().expect("sloppy slop score"),
        60.323_886_639_676_11,
    );
    assert!(sloppy_slop["score"].as_f64() > clean_slop["score"].as_f64());
}

#[test]
#[ignore = "slow self-scan probe; run explicitly at algorithm checkpoints"]
fn crates_metrics_and_graph_performance_probe() {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");

    let metrics_started = Instant::now();
    let metrics = command_json("metrics", &crates);
    let metrics_elapsed = metrics_started.elapsed();
    let graph_started = Instant::now();
    let graph = command_json("graph", &crates);
    let graph_elapsed = graph_started.elapsed();

    let metric_files = metrics["analyses"]
        .as_array()
        .expect("metric analyses")
        .len();
    let metric_regions = metrics["functions"]
        .as_array()
        .expect("metric regions")
        .len();
    let graph_nodes = graph["nodes"].as_array().expect("graph nodes").len();
    let graph_edges = graph["edges"].as_array().expect("graph edges").len();
    eprintln!(
        "algorithm performance probe: metric_files={metric_files}, metric_regions={metric_regions}, graph_nodes={graph_nodes}, graph_edges={graph_edges}, metrics={metrics_elapsed:?}, graph={graph_elapsed:?}"
    );

    assert_eq!(metrics["schema"], "deslop.metrics/6");
    assert_eq!(metrics["status"], "complete");
    assert!(metric_files > 0);
    assert!(metric_regions > metric_files);

    assert_eq!(graph["schema"], "deslop.graph/2");
    assert_eq!(graph["status"], "complete");
    assert!(graph_nodes > metric_files);
    assert!(graph_edges > graph_nodes);
    assert!(
        graph["edges"]
            .as_array()
            .expect("graph edges")
            .iter()
            .all(|edge| edge["kind"] == "contains" || edge["confidence"] != "resolved")
    );
}

fn assert_metrics_probe(report: &Value, expected_files: usize) {
    assert_eq!(report["schema"], "deslop.metrics/6");
    assert_eq!(report["status"], "complete");
    assert_eq!(
        report["analyses"]
            .as_array()
            .expect("metric analyses")
            .len(),
        expected_files
    );
    assert!(
        report["analyses"]
            .as_array()
            .expect("metric analyses")
            .iter()
            .all(|analysis| {
                analysis["analysis"]["status"] == "complete"
                    && analysis["analysis"]["diagnostics"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
            })
    );
    assert!(report["functions"].is_array());
    assert!(report["heuristic_outliers"].is_array());
    assert!(report["heuristic_burden_distribution"].is_object());
    assert_eq!(report["heuristic_model"]["experimental"], true);
    assert_eq!(report["heuristic_model"]["human_calibrated"], false);
    assert_eq!(report["heuristic_model"]["authority"], "triage_only");
    assert_eq!(report["heuristic_model"]["gating_permitted"], false);
    assert_no_legacy_metric_keys(report);
}

fn assert_no_legacy_metric_keys(value: &Value) {
    match value {
        Value::Object(object) => {
            for key in LEGACY_METRIC_KEYS {
                assert!(!object.contains_key(*key), "legacy metric key `{key}`");
            }
            for child in object.values() {
                assert_no_legacy_metric_keys(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_no_legacy_metric_keys(child);
            }
        }
        _ => {}
    }
}

fn command_json(command: &str, path: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .arg(command)
        .arg(path)
        .args(["--format", "json"])
        .output()
        .unwrap_or_else(|error| panic!("run deslop {command}: {error}"));
    assert!(
        output.status.success(),
        "deslop {command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("command JSON")
}

fn corpus(label: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus")
        .join(label)
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "expected {expected}, got {actual}"
    );
}
