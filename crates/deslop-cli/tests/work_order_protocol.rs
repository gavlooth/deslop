use std::fs;
use std::process::Command;

use deslop_protocol::{SharedWorkOrder, WorkOrderProtocolResponse};
use serde_json::json;

#[test]
fn cli_executes_shared_bounded_index_request() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(
        temp.path().join("sample.rs"),
        "fn sample() { let value = 42; println!(\"{}\", value); }\n",
    )
    .expect("source");
    let proposed = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .current_dir(temp.path())
        .args(["propose", "sample.rs"])
        .output()
        .expect("propose");
    assert!(proposed.status.success());
    let order: SharedWorkOrder = serde_json::from_slice(
        proposed
            .stdout
            .split(|byte| *byte == b'\n')
            .find(|line| !line.is_empty())
            .expect("work order"),
    )
    .expect("shared order");
    fs::write(
        temp.path().join("input.json"),
        serde_json::to_vec_pretty(&json!({
            "orders": [order],
            "constraints": {
                "prerequisites": [],
                "atomic_groups": [],
                "mutually_exclusive_recipes": []
            },
            "metadata": {
                "capabilities": ["cli"],
                "parse_gaps": [],
                "architecture_summary": ["one source"],
                "cache_state": ["cold"],
                "provenance": ["cli-test"],
                "unknowns": []
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        temp.path().join("request.json"),
        b"{\"operation\":\"index\"}",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .current_dir(temp.path())
        .args([
            "work-orders",
            "--input",
            "input.json",
            "--request",
            "request.json",
        ])
        .output()
        .expect("work order protocol");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: WorkOrderProtocolResponse =
        serde_json::from_slice(&output.stdout).expect("response");
    let WorkOrderProtocolResponse::Index(index) = response else {
        panic!("index response")
    };
    assert_eq!(index.total_orders, 1);
    assert_eq!(
        index.provenance.operation,
        deslop_protocol::WorkOrderOperation::Index
    );
}
