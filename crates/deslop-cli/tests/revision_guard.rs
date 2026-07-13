use std::fs;
use std::process::{Command, Output};

use deslop_protocol::WorkOrder;
use serde_json::{Value, json};

#[test]
fn cli_rejects_boundary_stale_and_legacy_patches_without_writing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("sample.clj");
    fs::write(&source, "(= (count xs) 0)\n").expect("original source");

    let proposed = cli(temp.path(), &["propose", "sample.clj"]);
    assert!(proposed.status.success(), "{}", stderr(&proposed));
    let work_order: WorkOrder = serde_json::from_slice(
        proposed
            .stdout
            .split(|byte| *byte == b'\n')
            .find(|line| !line.is_empty())
            .expect("workorder JSONL"),
    )
    .expect("workorder");
    assert_eq!(work_order.schema, "deslop.workorder/2");
    assert!(work_order.id.starts_with("wo2_"));

    let patch_path = temp.path().join("patch.jsonl");
    fs::write(
        &patch_path,
        format!(
            "{}\n",
            json!({
                "schema": "deslop.patch/2",
                "workorder_id": work_order.id,
                "revision_guard": work_order.revision_guard,
                "replacement": "(empty? xs)\n",
                "by": "cli-regression"
            })
        ),
    )
    .expect("patch");

    let changed = " (= (count xs) 0)\n";
    fs::write(&source, changed).expect("boundary whitespace mutation");
    let verified = cli(
        temp.path(),
        &["verify", "--patches", "patch.jsonl", "--check-cmd", "true"],
    );
    assert_eq!(verified.status.code(), Some(1), "{}", stderr(&verified));
    let verify_report: Value = serde_json::from_slice(&verified.stdout).expect("verify report");
    assert_eq!(verify_report["results"][0]["verdict"], "rejected");
    assert_eq!(
        verify_report["results"][0]["reasons"],
        json!(["stale revision_guard"])
    );

    let applied = cli(
        temp.path(),
        &[
            "apply",
            "--patches",
            "patch.jsonl",
            "--check-cmd",
            "true",
            "--allow-non-removable",
            "--no-backup",
        ],
    );
    assert_eq!(applied.status.code(), Some(1), "{}", stderr(&applied));
    let apply_report: Value = serde_json::from_slice(&applied.stdout).expect("apply report");
    assert_eq!(apply_report["written"], json!([]));
    assert_eq!(fs::read_to_string(&source).unwrap(), changed);

    let legacy_path = temp.path().join("legacy.jsonl");
    fs::write(
        &legacy_path,
        format!(
            "{}\n",
            json!({
                "schema": "deslop.patch/1",
                "workorder_id": work_order.id,
                "region_fingerprint": work_order.region_fingerprint,
                "replacement": "(empty? xs)\n",
                "by": "legacy"
            })
        ),
    )
    .expect("legacy patch");
    let legacy = cli(temp.path(), &["verify", "--patches", "legacy.jsonl"]);
    assert!(!legacy.status.success());
    assert!(stderr(&legacy).contains("regenerate as `deslop.patch/2`"));
    assert_eq!(fs::read_to_string(source).unwrap(), changed);
}

fn cli(cwd: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deslop"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run deslop")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
