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
    assert_eq!(orders[0]["schema"], "deslop.work-order/1");

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
    let candidate = &orders[0]["subject"]["candidate"];
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
    let candidate = &orders[0]["subject"]["candidate"];
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
fn branch_split_reports_unknown_slices_and_cannot_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("split.rs");
    let orders_path = root.path().join("split-orders.json");
    let original = "fn a() {}\nfn b() {}\nfn run(flag: bool) { if flag { a(); b(); } }\n";
    fs::write(&source, original).unwrap();
    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "split.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-split-independent-branch-actions",
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
    let candidate = &orders[0]["subject"]["candidate"];
    assert_eq!(candidate["disposition"], "review-required");
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["condition"] == "distinct-dependence-slices"
                && result["state"] == "unknown")
    );
    assert!(
        candidate["edits"][0]["after"]
            .as_str()
            .unwrap()
            .contains("let __deslop_m57_condition = flag")
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
fn guard_clause_reports_pst_exit_evidence_and_cannot_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("guards.rs");
    let orders_path = root.path().join("guard-orders.json");
    let original = "fn run(flag: bool) { if flag { let _value = 1; } else { return; } }\n";
    fs::write(&source, original).unwrap();
    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "guards.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-invert-guard-clause",
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
    let candidate = &orders[0]["subject"]["candidate"];
    assert_eq!(candidate["disposition"], "review-required");
    assert_eq!(candidate["safety"], "safe-with-precondition");
    assert_eq!(
        candidate["edits"][0]["after"],
        "if !(flag) { return; } let _value = 1;"
    );
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "guard-arm-abrupt-exit-exact"
                    && result["state"] == "proven"
            )
    );
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "pst-continuation-boundary-exact"
                    && result["state"] == "proven"
            )
    );
    assert!(
        candidate["forbidden_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "binding-lifetime-or-drop-change"
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
fn terminal_branch_recipes_report_graph_evidence_and_cannot_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("terminal.rs");
    let original = "enum Mode { A, B }\n\
                    fn dead() -> i32 { if true { 1 } else { 2 } }\n\
                    fn dispatch(mode: Mode) -> i32 {\n\
                        if mode == Mode::A { 1 } else if mode == Mode::B { 2 } else { 3 }\n\
                    }\n";
    fs::write(&source, original).unwrap();

    for (recipe, expected_edit, required_condition) in [
        (
            "rust-remove-literal-dead-arm",
            "{ 1 }",
            "literal-predicate-outcome-exact",
        ),
        (
            "rust-convert-exhaustive-chain-to-match",
            "match mode { Mode::A => { 1 }, Mode::B => { 2 }, _ => { 3 } }",
            "explicit-fallback-exhaustive",
        ),
    ] {
        let orders_path = root.path().join(format!("{recipe}.json"));
        let detected = deslop()
            .args([
                "recipes",
                "detect",
                "terminal.rs",
                "--root",
                root.path().to_str().unwrap(),
                "--recipe",
                recipe,
                "--format",
                "workorders",
                "--output",
                orders_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(detected.status.success(), "{:?}", detected.stderr);
        let orders: Vec<Value> = serde_json::from_slice(&fs::read(&orders_path).unwrap()).unwrap();
        assert_eq!(orders.len(), 1, "{recipe}");
        let candidate = &orders[0]["subject"]["candidate"];
        assert_eq!(candidate["disposition"], "review-required");
        assert_eq!(candidate["safety"], "safe-with-precondition");
        assert_eq!(candidate["edits"][0]["after"], expected_edit);
        assert!(
            candidate["required_results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["condition"] == required_condition
                    && result["state"] == "proven")
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
        assert!(!rejected.status.success(), "{recipe}");
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("is not automatic"));
        assert_eq!(fs::read_to_string(&source).unwrap(), original);
    }
}

#[test]
fn extract_method_reports_exact_compiling_edit_and_cannot_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("extract.rs");
    let orders_path = root.path().join("extract-orders.json");
    let original = "fn run(flag: bool, value: &mut i32) {\n\
                    \x20   if flag {\n\
                    \x20       *value += 1;\n\
                    \x20       *value += 2;\n\
                    \x20   } else {\n\
                    \x20       *value -= 1;\n\
                    \x20       *value -= 2;\n\
                    \x20   }\n\
                    }\n";
    fs::write(&source, original).unwrap();

    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "extract.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-extract-sese-branch-method",
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
    let candidate = &orders[0]["subject"]["candidate"];
    assert_eq!(
        candidate["recipe"]["name"],
        "rust-extract-sese-branch-method"
    );
    assert_eq!(candidate["disposition"], "review-required");
    assert_eq!(candidate["safety"], "safe-with-precondition");
    assert_eq!(candidate["edits"].as_array().unwrap().len(), 1);
    let after = candidate["edits"][0]["after"].as_str().unwrap();
    assert!(after.starts_with("fn __deslop_extract_branch_"));
    assert!(after.contains("(flag, value);"));
    assert!(
        candidate["required_results"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |result| result["condition"] == "complete-computation-object-state-slice"
                    && result["state"] == "unknown"
            )
    );
    for (condition, state) in [
        ("exact-extraction-inputs", "proven"),
        ("exact-extraction-outputs", "proven"),
        ("exact-extraction-mutation-frontier", "unknown"),
        ("exact-extraction-exits", "proven"),
        ("exact-extraction-exceptions", "unknown"),
        ("exact-extraction-captures", "proven"),
        ("exact-extraction-async-ownership", "proven"),
    ] {
        assert!(
            candidate["required_results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["condition"] == condition && result["state"] == state),
            "missing {condition}={state}"
        );
    }

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
fn responsibility_split_reports_one_atomic_multi_helper_workorder() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("split.rs");
    let orders_path = root.path().join("split-orders.json");
    let original = "fn run(left: bool, right: bool, v: &mut i32, w: &mut i32) {\n\
                    \x20   if left { *v += 1; *v += 2; } else { *v -= 1; *v -= 2; }\n\
                    \x20   if right { *w *= 2; *w *= 3; } else { *w += 4; *w += 5; }\n\
                    }\n";
    fs::write(&source, original).unwrap();

    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "split.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-split-dependence-cohesive-callable",
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
    let candidate = &orders[0]["subject"]["candidate"];
    assert_eq!(
        candidate["recipe"]["name"],
        "rust-split-dependence-cohesive-callable"
    );
    assert_eq!(candidate["disposition"], "review-required");
    assert_eq!(candidate["edits"].as_array().unwrap().len(), 1);
    let after = candidate["edits"][0]["after"].as_str().unwrap();
    assert_eq!(after.matches("fn __deslop_extract_branch_").count(), 2);
    for (condition, state) in [
        ("multiple-dependence-cohesive-action-clusters", "proven"),
        ("disjoint-action-cluster-frontiers", "unknown"),
        ("exact-action-cluster-signatures", "proven"),
    ] {
        assert!(
            candidate["required_results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["condition"] == condition && result["state"] == state),
            "missing {condition}={state}"
        );
    }
}

#[test]
fn inline_selector_is_available_and_fails_closed_without_binding_authority() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("inline.rs");
    let orders_path = root.path().join("inline-orders.json");
    fs::write(&source, "fn helper() { 1 + 2; }\nfn run() { helper(); }\n").unwrap();

    let detected = deslop()
        .args([
            "recipes",
            "detect",
            "inline.rs",
            "--root",
            root.path().to_str().unwrap(),
            "--recipe",
            "rust-inline-exact-single-use-helper",
            "--format",
            "workorders",
            "--output",
            orders_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(detected.status.success(), "{:?}", detected.stderr);
    let orders: Vec<Value> = serde_json::from_slice(&fs::read(&orders_path).unwrap()).unwrap();
    assert!(orders.is_empty());
    assert!(detected.stderr.is_empty());
}

#[test]
fn local_cleanup_selectors_are_available_and_fail_closed_without_data_authority() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("cleanup.rs"),
        "fn run() -> i32 { let temporary = 1 + 2; let result = temporary * 3; 99; let unused = 7; result }\n",
    )
    .unwrap();

    for recipe in [
        "rust-inline-exact-single-use-temporary",
        "rust-remove-unused-pure-literal-expression",
        "rust-remove-independent-unused-literal-local",
    ] {
        let detected = deslop()
            .args([
                "recipes",
                "detect",
                "cleanup.rs",
                "--root",
                root.path().to_str().unwrap(),
                "--recipe",
                recipe,
                "--format",
                "candidates",
            ])
            .output()
            .unwrap();
        assert!(detected.status.success(), "{recipe}: {:?}", detected.stderr);
        let candidates: Vec<Value> = serde_json::from_slice(&detected.stdout).unwrap();
        assert!(candidates.is_empty(), "{recipe}");
        assert!(detected.stderr.is_empty(), "{recipe}");
    }
}

#[test]
fn ordering_selectors_are_available_and_fail_closed_without_scope_resolution_authority() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("ordering.rs"),
        "use std::vec::Vec;\nuse std::collections::BTreeMap;\nfn zebra() { alpha(); }\nfn alpha() {}\nfn main() {}\n",
    )
    .unwrap();

    for recipe in [
        "rust-sort-simple-import-block",
        "rust-sort-hoisted-private-function-block",
    ] {
        let detected = deslop()
            .args([
                "recipes",
                "detect",
                "ordering.rs",
                "--root",
                root.path().to_str().unwrap(),
                "--recipe",
                recipe,
                "--format",
                "candidates",
            ])
            .output()
            .unwrap();
        assert!(detected.status.success(), "{recipe}: {:?}", detected.stderr);
        let candidates: Vec<Value> = serde_json::from_slice(&detected.stdout).unwrap();
        assert!(candidates.is_empty(), "{recipe}");
        assert!(detected.stderr.is_empty(), "{recipe}");
    }
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
