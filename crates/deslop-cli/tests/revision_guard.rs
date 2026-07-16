use std::fs;
use std::process::{Command, Output};

use deslop_protocol::{SharedWorkOrder, WorkOrderSubject};
use serde_json::json;

#[test]
fn cli_rejects_boundary_stale_and_legacy_patches_without_writing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("sample.clj");
    fs::write(&source, "(= (count xs) 0)\n").expect("original source");

    let proposed = cli(temp.path(), &["propose", "sample.clj"]);
    assert!(proposed.status.success(), "{}", stderr(&proposed));
    let shared: SharedWorkOrder = serde_json::from_slice(
        proposed
            .stdout
            .split(|byte| *byte == b'\n')
            .find(|line| !line.is_empty())
            .expect("workorder JSONL"),
    )
    .expect("shared workorder");
    assert_eq!(shared.schema(), "deslop.work-order/1");
    assert!(shared.id().as_str().starts_with("wo1_"));
    let work_order = match shared.subject() {
        WorkOrderSubject::FindingProposal { order } => (**order).clone(),
        WorkOrderSubject::Transformation { .. } => panic!("expected finding proposal"),
    };
    assert!(work_order.id.starts_with("wo3_"));

    let patch_path = temp.path().join("patch.jsonl");
    fs::write(
        &patch_path,
        format!(
            "{}\n",
            json!({
                "schema": "deslop.patch/3",
                "workorder_id": work_order.id,
                "revision_guard": work_order.revision_guard,
                "proposal_context": work_order.proposal_context,
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
    assert!(stderr(&verified).contains("proposal context no longer matches"));

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
    assert!(stderr(&applied).contains("proposal context no longer matches"));
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
    assert!(stderr(&legacy).contains("regenerate as `deslop.patch/3`"));
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
