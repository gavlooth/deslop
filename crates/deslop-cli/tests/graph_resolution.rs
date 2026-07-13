use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

#[test]
fn graph_cli_keeps_import_alias_calls_unresolved() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("origin.rs"), "pub fn helper() {}\n").expect("origin");
    std::fs::write(temp.path().join("chosen.rs"), "pub fn chosen() {}\n").expect("unrelated");
    std::fs::write(
        temp.path().join("caller.rs"),
        "use crate::origin::helper as chosen;\nfn run() { chosen(); }\n",
    )
    .expect("caller");

    let json = graph_command(temp.path(), "json");
    let graph: Value = serde_json::from_slice(&json.stdout).expect("graph JSON");
    let call = graph["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .find(|edge| edge["kind"] == "calls" && edge["label"] == "chosen")
        .expect("chosen call");
    let target = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == call["to"])
        .expect("call target");

    assert_eq!(graph["schema"], "deslop.graph/2");
    assert_eq!(call["confidence"], "syntactic");
    assert_eq!(target["kind"], "external-symbol");
    assert!(
        graph["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .all(|edge| { edge["kind"] == "contains" || edge["confidence"] != "resolved" })
    );
    assert!(
        graph["agent_notes"]
            .as_array()
            .expect("agent notes")
            .iter()
            .any(|note| note
                .as_str()
                .is_some_and(|note| note.contains("unresolved placeholder")))
    );

    let dot = graph_command(temp.path(), "dot");
    assert!(
        String::from_utf8(dot.stdout)
            .expect("DOT UTF-8")
            .contains("calls: chosen (syntactic)")
    );
}

#[test]
fn graph_cli_locks_the_compact_label_false_resolution_probe() {
    let graph_source =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../deslop-graph/src");
    let json = graph_command(&graph_source, "json");
    let graph: Value = serde_json::from_slice(&json.stdout).expect("graph JSON");
    let nodes = graph["nodes"].as_array().expect("nodes");
    let definitions = nodes
        .iter()
        .filter(|node| node["name"] == "compact_label")
        .collect::<Vec<_>>();
    let definition_ids = definitions
        .iter()
        .map(|node| node["id"].as_str().expect("definition id"))
        .collect::<BTreeSet<_>>();
    let calls = graph["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|edge| edge["kind"] == "calls" && edge["label"] == "compact_label")
        .collect::<Vec<_>>();

    assert_eq!(definitions.len(), 2);
    assert_eq!(calls.len(), 10);
    for call in calls {
        assert_eq!(call["confidence"], "syntactic");
        assert!(definition_ids.contains(call["to"].as_str().expect("call target id")));
        let from = nodes
            .iter()
            .find(|node| node["id"] == call["from"])
            .expect("call source");
        let to = nodes
            .iter()
            .find(|node| node["id"] == call["to"])
            .expect("call target");
        assert_eq!(from["path"], to["path"], "{call:#?}");
    }
}

#[test]
fn graph_cli_corpus_has_no_false_resolved_or_require_calls() {
    let corpus = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let json = graph_command(&corpus, "json");
    let graph: Value = serde_json::from_slice(&json.stdout).expect("graph JSON");
    let edges = graph["edges"].as_array().expect("edges");

    assert_eq!(graph["summary"]["files"], 21);
    assert_eq!(graph["summary"]["symbols"], 74);
    assert_eq!(graph["summary"]["edges"], 197);
    assert!(
        edges
            .iter()
            .all(|edge| edge["kind"] == "contains" || edge["confidence"] != "resolved")
    );
    assert!(edges.iter().all(|edge| {
        edge["kind"] != "calls" || !matches!(edge["label"].as_str(), Some("require" | ":require"))
    }));
}

fn graph_command(path: &std::path::Path, format: &str) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .arg("graph")
        .arg(path)
        .arg("--format")
        .arg(format)
        .output()
        .expect("run deslop graph");
    assert!(
        output.status.success(),
        "graph failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
