use std::fs;
use std::process::Command;

use serde_json::Value;

fn deslop() -> Command {
    Command::new(env!("CARGO_BIN_EXE_deslop"))
}

#[test]
fn recipe_detect_preview_and_workorder_are_read_only() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("fixture.rs");
    let original = "fn run() { return; 1; }\n";
    fs::write(&path, original).unwrap();

    let candidates = deslop()
        .args([
            "recipes",
            "detect",
            "fixture.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--format",
            "candidates",
        ])
        .output()
        .unwrap();
    assert!(candidates.status.success(), "{:?}", candidates.stderr);
    assert_eq!(
        serde_json::from_slice::<Vec<Value>>(&candidates.stdout)
            .unwrap()
            .len(),
        1
    );

    let workorders = deslop()
        .args([
            "recipes",
            "detect",
            "fixture.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--format",
            "workorders",
        ])
        .output()
        .unwrap();
    assert!(workorders.status.success(), "{:?}", workorders.stderr);
    let orders = serde_json::from_slice::<Vec<Value>>(&workorders.stdout).unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["schema"], "deslop.recipe-workorder/1");

    let diff = deslop()
        .args([
            "recipes",
            "detect",
            "fixture.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--format",
            "diff",
        ])
        .output()
        .unwrap();
    assert!(diff.status.success(), "{:?}", diff.stderr);
    let diff = String::from_utf8(diff.stdout).unwrap();
    assert!(diff.contains("--- fixture.rs"));
    assert!(diff.contains("+fn run() { return;  }"));
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn branch_factoring_is_reported_with_counter_evidence_and_cannot_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("branch.rs");
    let orders_path = root.path().join("branch-orders.json");
    let original =
        "fn run(flag: bool) -> i32 { if flag { side(); 1 } else { side(); 1 } }\nfn side() {}\n";
    fs::write(&source, original).unwrap();

    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "branch.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-factor-equivalent-branch-fragments",
            "--format",
            "workorders",
            "--output",
            orders_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(detected.status.success(), "{:?}", detected.stderr);
    let orders: Vec<Value> = serde_json::from_slice(&fs::read(&orders_path).unwrap()).unwrap();
    assert_eq!(orders.len(), 1);
    let candidate = &orders[0]["candidate"];
    assert_eq!(candidate["disposition"], "review-required");
    assert_eq!(candidate["safety"], "safe-with-precondition");
    assert_eq!(candidate["eligibility"]["eligible"], false);
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "effect-and-drop-order-preserved"
                    && result["state"] == "unknown"
            )
    );
    assert!(
        candidate["forbidden_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "binding-lifetime-or-drop-escape"
                    && result["state"] == "unknown"
            )
    );

    let rejected = deslop()
        .args([
            "recipes",
            "apply",
            "--root",
            root.path().to_str().unwrap(),
            "--workorders",
            orders_path.to_str().unwrap(),
            "--build-cmd",
            "true",
            "--test-cmd",
            "true",
            "--canary",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("is not automatic"));
    assert_eq!(fs::read_to_string(source).unwrap(), original);
}

#[test]
fn adjacent_condition_merge_retains_short_circuit_evidence_and_cannot_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("conditions.rs");
    let orders_path = root.path().join("condition-orders.json");
    let original = "fn run(a: bool, b: bool) { if a { if b { act(); } } }\nfn act() {}\n";
    fs::write(&source, original).unwrap();

    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "conditions.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-merge-adjacent-conditions",
            "--format",
            "workorders",
            "--output",
            orders_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(detected.status.success(), "{:?}", detected.stderr);
    let orders: Vec<Value> = serde_json::from_slice(&fs::read(&orders_path).unwrap()).unwrap();
    assert_eq!(orders.len(), 1);
    let candidate = &orders[0]["candidate"];
    assert_eq!(candidate["disposition"], "review-required");
    assert_eq!(candidate["edits"][0]["after"], "if (a) && (b) { act(); }");
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "left-to-right-evaluation-count"
                    && result["state"] == "proven"
                    && result["evidence"].as_array().unwrap().len() == 2
            )
    );
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "exception-suspension-order-preserved"
                    && result["state"] == "unknown"
            )
    );

    let rejected = deslop()
        .args([
            "recipes",
            "apply",
            "--root",
            root.path().to_str().unwrap(),
            "--workorders",
            orders_path.to_str().unwrap(),
            "--build-cmd",
            "true",
            "--test-cmd",
            "true",
            "--canary",
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("is not automatic"));
    assert_eq!(fs::read_to_string(source).unwrap(), original);
}

#[test]
fn recipe_cli_is_disabled_by_default_and_canary_rolls_back_live_failure() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("fixture.rs");
    let orders = root.path().join("orders.json");
    let original = "fn run() { return; 1; }\n";
    fs::write(&source, original).unwrap();

    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "fixture.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--format",
            "workorders",
            "--output",
            orders.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(detected.status.success(), "{:?}", detected.stderr);

    let disabled = deslop()
        .args([
            "recipes",
            "apply",
            "--root",
            root.path().to_str().unwrap(),
            "--workorders",
            orders.to_str().unwrap(),
            "--build-cmd",
            "true",
            "--test-cmd",
            "true",
            "--no-backup",
        ])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&disabled.stdout).unwrap();
    assert_eq!(report["status"], "rejected");
    assert_eq!(fs::read_to_string(&source).unwrap(), original);

    let rolled_back = deslop()
        .args([
            "recipes",
            "apply",
            "--root",
            root.path().to_str().unwrap(),
            "--workorders",
            orders.to_str().unwrap(),
            "--build-cmd",
            "true",
            "--test-cmd",
            "test \"$DESLOP_VALIDATION_PHASE\" != live",
            "--no-backup",
            "--canary",
        ])
        .output()
        .unwrap();
    assert_eq!(rolled_back.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&rolled_back.stdout).unwrap();
    assert_eq!(report["status"], "rolled-back");
    assert_eq!(report["rollback_verified"], true);
    assert_eq!(report["live_revision"], "original-rebuilt-and-tested");
    assert_eq!(fs::read_to_string(source).unwrap(), original);
}
