use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use deslop_protocol::WorkOrder;
use serde_json::Value;

#[test]
fn m0_corpus_definition_of_done_is_numerically_locked() {
    let corpus = repo_path("tests/corpus");

    let proposed = deslop()
        .arg("propose")
        .arg(&corpus)
        .output()
        .expect("run corpus proposal");
    assert_success(&proposed, "corpus proposal");
    let work_orders = parse_workorders(&proposed);
    let unique_ids = work_orders
        .iter()
        .map(|work_order| work_order.id.as_str())
        .collect::<BTreeSet<_>>();
    let unique_targets = work_orders
        .iter()
        .map(|work_order| {
            format!(
                "{}:{}:{}",
                work_order.path.display(),
                work_order.region.start_byte,
                work_order.region.end_byte
            )
        })
        .collect::<BTreeSet<_>>();
    let grouped_findings = work_orders
        .iter()
        .map(|work_order| work_order.findings.len())
        .sum::<usize>();
    assert_eq!(work_orders.len(), 30);
    assert_eq!(unique_ids.len(), 30, "duplicate workorder IDs");
    assert_eq!(unique_targets.len(), 30, "duplicate rewrite targets");
    assert_eq!(grouped_findings, 65);

    let corpus_graph = graph(&corpus);
    let corpus_edges = corpus_graph["edges"].as_array().expect("corpus edges");
    let resolved_reference_edges = corpus_edges
        .iter()
        .filter(|edge| edge["kind"] != "contains" && edge["confidence"] == "resolved")
        .count();
    let syntactic_reference_edges = corpus_edges
        .iter()
        .filter(|edge| edge["confidence"] == "syntactic")
        .count();
    assert_eq!(corpus_graph["status"], "complete");
    assert_eq!(corpus_graph["summary"]["files"], 21);
    assert_eq!(corpus_graph["summary"]["symbols"], 74);
    assert_eq!(corpus_graph["summary"]["edges"], 197);
    assert_eq!(resolved_reference_edges, 0);
    assert_eq!(syntactic_reference_edges, 123);

    let ambiguous_temp = tempfile::tempdir().expect("ambiguous graph fixture");
    for name in ["left.rs", "right.rs"] {
        std::fs::write(
            ambiguous_temp.path().join(name),
            "struct Alpha;\nimpl Alpha { fn ping() {} }\n",
        )
        .expect("duplicate qualified definition");
    }
    std::fs::write(
        ambiguous_temp.path().join("caller.rs"),
        "fn run() { Alpha::ping(); }\n",
    )
    .expect("ambiguous caller");
    let ambiguous_graph = graph(ambiguous_temp.path());
    let ambiguous_nodes = ambiguous_graph["nodes"]
        .as_array()
        .expect("ambiguous nodes");
    let ambiguous_edges = ambiguous_graph["edges"]
        .as_array()
        .expect("ambiguous edges");
    let ambiguous_references = ambiguous_edges
        .iter()
        .filter(|edge| edge["kind"] != "contains" && edge["confidence"] == "ambiguous")
        .collect::<Vec<_>>();
    assert_eq!(ambiguous_graph["summary"]["ambiguous_edges"], 1);
    assert_eq!(ambiguous_references.len(), 1);
    assert_eq!(
        ambiguous_edges
            .iter()
            .filter(|edge| edge["kind"] != "contains" && edge["confidence"] == "resolved")
            .count(),
        0
    );
    let ambiguous_target = ambiguous_nodes
        .iter()
        .find(|node| node["id"] == ambiguous_references[0]["to"])
        .expect("ambiguous placeholder target");
    assert_eq!(ambiguous_target["kind"], "external-symbol");

    let graph_source = repo_path("crates/deslop-graph/src");
    let resolution_probe = graph(&graph_source);
    let nodes = resolution_probe["nodes"].as_array().expect("probe nodes");
    let compact_definitions = nodes
        .iter()
        .filter(|node| node["name"] == "compact_label")
        .count();
    let compact_calls = resolution_probe["edges"]
        .as_array()
        .expect("probe edges")
        .iter()
        .filter(|edge| edge["kind"] == "calls" && edge["label"] == "compact_label")
        .collect::<Vec<_>>();
    assert_eq!(compact_definitions, 2);
    assert_eq!(compact_calls.len(), 10);
    assert_eq!(
        compact_calls
            .iter()
            .filter(|edge| edge["confidence"] == "resolved")
            .count(),
        0,
        "formerly ambiguous compact_label calls must never claim resolution"
    );

    // This public-surface proof is paired with deslop-parse's AST-sentinel truth table
    // (`selects_javascript_typescript_and_tsx_grammars_by_dialect`), which also proves
    // wrong-grammar rejection and the .mts/.cts dialect aliases.
    let grammar_scan = deslop()
        .args(["scan", "--format", "json"])
        .arg(repo_path("tests/fixtures/typescript/typed.ts"))
        .arg(repo_path("tests/fixtures/typescript/component.tsx"))
        .arg(repo_path("tests/fixtures/typescript/component.jsx"))
        .output()
        .expect("run grammar selection scan");
    assert_success(&grammar_scan, "grammar selection scan");
    let grammar_report: Value =
        serde_json::from_slice(&grammar_scan.stdout).expect("grammar scan JSON");
    let grammar_reports = grammar_report["reports"]
        .as_array()
        .expect("grammar reports");
    assert_eq!(grammar_report["status"], "complete");
    assert_eq!(grammar_reports.len(), 3);
    assert_eq!(
        grammar_reports
            .iter()
            .filter(|report| report["lang"] == "type-script")
            .count(),
        2
    );
    assert_eq!(
        grammar_reports
            .iter()
            .filter(|report| report["lang"] == "java-script")
            .count(),
        1
    );
    assert!(grammar_reports.iter().all(|report| {
        report["analysis"]["status"] == "complete"
            && report["analysis"]["diagnostics"]
                .as_array()
                .is_some_and(Vec::is_empty)
    }));

    let malformed_scan = deslop()
        .args(["scan", "--format", "json"])
        .arg(repo_path("tests/fixtures/typescript/malformed.ts"))
        .arg(repo_path("tests/fixtures/typescript/malformed.tsx"))
        .output()
        .expect("run malformed typed scan");
    assert_eq!(malformed_scan.status.code(), Some(2));
    let malformed_report: Value =
        serde_json::from_slice(&malformed_scan.stdout).expect("malformed scan JSON");
    let malformed_reports = malformed_report["reports"]
        .as_array()
        .expect("malformed reports");
    assert_eq!(malformed_report["status"], "partial");
    assert_eq!(malformed_reports.len(), 2);
    assert!(malformed_reports.iter().all(|report| {
        report["analysis"]["status"] == "partial"
            && !report["analysis"]["diagnostics"]
                .as_array()
                .expect("diagnostics")
                .is_empty()
            && report["findings"].as_array().is_some_and(Vec::is_empty)
    }));

    let malformed_metrics = deslop()
        .arg("metrics")
        .arg(repo_path("tests/fixtures/typescript/malformed.ts"))
        .arg(repo_path("tests/fixtures/typescript/malformed.tsx"))
        .args(["--format", "json"])
        .output()
        .expect("run malformed metrics");
    assert_eq!(malformed_metrics.status.code(), Some(2));
    let metrics_report: Value =
        serde_json::from_slice(&malformed_metrics.stdout).expect("malformed metrics JSON");
    assert_eq!(metrics_report["status"], "partial");
    assert_eq!(metrics_report["analyses"].as_array().unwrap().len(), 2);
    assert!(
        metrics_report["analyses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|analysis| analysis["analysis"]["status"] == "partial")
    );
    assert!(metrics_report["functions"].as_array().unwrap().is_empty());
    assert!(
        metrics_report["heuristic_outliers"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let malformed_graph = deslop()
        .arg("graph")
        .arg(repo_path("tests/fixtures/typescript/malformed.ts"))
        .arg(repo_path("tests/fixtures/typescript/malformed.tsx"))
        .args(["--format", "json"])
        .output()
        .expect("run malformed graph");
    assert_eq!(malformed_graph.status.code(), Some(2));
    let graph_report: Value =
        serde_json::from_slice(&malformed_graph.stdout).expect("malformed graph JSON");
    assert_eq!(graph_report["status"], "partial");
    assert_eq!(graph_report["analyses"].as_array().unwrap().len(), 2);
    assert!(
        graph_report["analyses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|analysis| analysis["analysis"]["status"] == "partial")
    );
    assert_eq!(graph_report["summary"]["files"], 2);
    assert_eq!(graph_report["summary"]["symbols"], 0);
    assert_eq!(graph_report["summary"]["edges"], 0);

    let temp = tempfile::tempdir().expect("capability tempdir");
    let empty_project = temp.path().join("empty-julia-environment");
    std::fs::create_dir(&empty_project).expect("empty Julia environment");
    std::fs::write(empty_project.join("Project.toml"), "[deps]\n").expect("empty Julia project");
    let julia_source = temp.path().join("slop_julia.jl");
    std::fs::copy(
        repo_path("tests/corpus/sloppy/slop_julia.jl"),
        &julia_source,
    )
    .expect("copy Julia corpus fixture");
    let capability_probe = deslop()
        .arg("propose")
        .arg("--julia-external=jet")
        .arg("--julia-project")
        .arg(&empty_project)
        .arg(&julia_source)
        .env("JULIA_LOAD_PATH", "@:@stdlib")
        .output()
        .expect("run unavailable capability probe");
    assert_success(&capability_probe, "unavailable capability probe");
    let capability_orders = parse_workorders(&capability_probe);
    assert_eq!(capability_orders.len(), 3);
    let capabilities = &capability_orders[0].proposal_context.external_capabilities;
    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0].analyzer, "JET.jl");
    assert!(!capabilities[0].available);
    assert_eq!(capabilities[0].covered_rules, ["julia-jet"]);
    assert!(
        capability_orders.iter().all(|work_order| {
            work_order.proposal_context.external_capabilities == *capabilities
        })
    );

    eprintln!(
        "M0 DoD: workorders=30 unique_ids=30 unique_targets=30 grouped_findings=65; \
         corpus_graph=21_files/74_symbols/197_edges/123_syntactic/0_false_resolved; \
         ambiguity=1_edge/0_false_resolved; compact_label=2_defs/10_calls/0_resolved; \
         grammars=3_complete; malformed=2_partial/0_metric_regions/0_graph_symbols; \
         JET_capabilities=1_unavailable"
    );
}

fn deslop() -> Command {
    Command::new(env!("CARGO_BIN_EXE_deslop"))
}

fn graph(path: &Path) -> Value {
    let output = deslop()
        .arg("graph")
        .arg(path)
        .args(["--format", "json"])
        .output()
        .expect("run graph");
    assert_success(&output, "graph");
    serde_json::from_slice(&output.stdout).expect("graph JSON")
}

fn parse_workorders(output: &Output) -> Vec<WorkOrder> {
    String::from_utf8(output.stdout.clone())
        .expect("UTF-8 workorders")
        .lines()
        .map(|line| serde_json::from_str(line).expect("workorder JSONL"))
        .collect()
}

fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}
