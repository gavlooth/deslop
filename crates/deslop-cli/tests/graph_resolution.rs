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
