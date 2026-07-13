use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

use deslop_protocol::WorkOrder;

fn deslop() -> Command {
    Command::new(env!("CARGO_BIN_EXE_deslop"))
}

fn propose(paths: &[PathBuf]) -> Vec<WorkOrder> {
    let output = deslop()
        .arg("propose")
        .args(paths)
        .output()
        .expect("run deslop propose");
    assert!(
        output.status.success(),
        "propose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .expect("UTF-8 propose output")
        .lines()
        .map(|line| serde_json::from_str::<WorkOrder>(line).expect("work order JSONL"))
        .collect()
}

#[test]
fn never_auto_findings_are_reported_but_never_proposed() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("settings.toml"), "phantom_knob = 4\n")
        .expect("write config artifact");
    std::fs::write(
        temp.path().join("driver.jl"),
        concat!(
            "phantom_knob = get(options, \"phantom-knob\")\n",
            "println(phantom_knob)\n",
        ),
    )
    .expect("write parser-supported source");

    let scanned = deslop()
        .args(["scan", "--format", "json"])
        .arg(temp.path())
        .output()
        .expect("scan report-only finding");
    assert!(
        scanned.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&scanned.stderr)
    );
    let reports: serde_json::Value = serde_json::from_slice(&scanned.stdout).expect("scan JSON");
    let findings = reports["reports"]
        .as_array()
        .expect("reports")
        .iter()
        .flat_map(|file| file["findings"].as_array().expect("findings"));
    assert!(findings.into_iter().any(|finding| {
        finding["rule"] == "config-key-unconsumed" && finding["safety"] == "never-auto"
    }));

    assert!(propose(&[temp.path().to_path_buf()]).is_empty());

    let agent_scan = deslop()
        .args(["scan", "--format", "agent"])
        .arg(temp.path())
        .output()
        .expect("scan agent output");
    assert!(agent_scan.status.success());
    assert!(agent_scan.stdout.is_empty());
}

fn assert_unique_ids(work_orders: &[WorkOrder]) {
    let unique_ids = work_orders
        .iter()
        .map(|work_order| work_order.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(unique_ids.len(), work_orders.len());
}

fn finding_count(work_orders: &[WorkOrder]) -> usize {
    work_orders
        .iter()
        .map(|work_order| work_order.findings.len())
        .sum()
}

fn target_keys(work_orders: &[WorkOrder]) -> BTreeSet<String> {
    work_orders
        .iter()
        .map(|work_order| {
            let value = serde_json::to_value(work_order).expect("work order JSON");
            format!("{}|{}|{}", value["path"], value["kind"], value["region"])
        })
        .collect()
}

fn sloppy_rust_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/sloppy/slop_rust.rs")
}

#[test]
fn propose_groups_the_multi_finding_rust_corpus_into_unique_regions() {
    let work_orders = propose(&[sloppy_rust_fixture()]);
    let largest_group = work_orders
        .iter()
        .map(|work_order| work_order.findings.len())
        .max();

    assert_eq!(work_orders.len(), 3);
    assert_unique_ids(&work_orders);
    assert_eq!(finding_count(&work_orders), 13);
    assert_eq!(largest_group, Some(11));
    let largest = work_orders
        .iter()
        .find(|work_order| work_order.findings.len() == 11)
        .expect("largest grouped region");
    let rule_counts =
        largest
            .findings
            .iter()
            .fold(BTreeMap::<&str, usize>::new(), |mut counts, finding| {
                *counts.entry(&finding.rule).or_default() += 1;
                counts
            });
    assert_eq!(
        rule_counts,
        BTreeMap::from([
            ("let-and-return", 1),
            ("long-method", 1),
            ("near-duplicate", 9),
        ])
    );
}

#[test]
fn propose_deduplicates_repeated_and_overlapping_input_paths() {
    let fixture = sloppy_rust_fixture();
    let corpus_dir = fixture.parent().expect("corpus directory").to_path_buf();

    let full_corpus = propose(std::slice::from_ref(&corpus_dir));
    assert_eq!(full_corpus.len(), 28);
    assert_unique_ids(&full_corpus);
    assert_eq!(finding_count(&full_corpus), 62);
    assert_eq!(target_keys(&full_corpus).len(), 28);

    let repeated = propose(&[fixture.clone(), fixture.clone()]);
    assert_eq!(repeated.len(), 3);
    assert_unique_ids(&repeated);
    assert_eq!(finding_count(&repeated), 13);

    let overlapping = propose(&[fixture, corpus_dir]);
    assert_eq!(overlapping.len(), 28);
    assert_unique_ids(&overlapping);
    assert_eq!(finding_count(&overlapping), 62);
    assert_eq!(target_keys(&overlapping).len(), 28);
    assert_eq!(
        serde_json::to_value(full_corpus).expect("full corpus JSON"),
        serde_json::to_value(overlapping).expect("overlapping JSON")
    );
}

#[test]
fn propose_keeps_identical_content_in_distinct_files_separate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let text = std::fs::read_to_string(sloppy_rust_fixture()).expect("fixture text");
    let left = temp.path().join("left.rs");
    let right = temp.path().join("right.rs");
    std::fs::write(&left, &text).expect("write left fixture");
    std::fs::write(&right, text).expect("write right fixture");

    let work_orders = propose(&[left, right]);

    assert_eq!(work_orders.len(), 6);
    assert_unique_ids(&work_orders);
}

#[test]
fn propose_output_is_invariant_to_equivalent_path_order_and_spelling() {
    let cwd = std::env::current_dir().expect("current directory");
    let temp = tempfile::tempdir_in(&cwd).expect("tempdir in current directory");
    let absolute = temp.path().join("sample.rs");
    let text = std::fs::read_to_string(sloppy_rust_fixture()).expect("fixture text");
    std::fs::write(&absolute, text).expect("write fixture");
    let relative = absolute
        .strip_prefix(&cwd)
        .expect("relative fixture path")
        .to_path_buf();
    let dotted = PathBuf::from(".").join(&relative);

    let forward = propose(&[absolute.clone(), dotted]);
    let reversed = propose(&[relative, absolute]);

    assert_eq!(
        serde_json::to_value(forward).expect("forward JSON"),
        serde_json::to_value(reversed).expect("reversed JSON")
    );
}

#[test]
fn malformed_propose_is_atomic_and_preserves_existing_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output_path = temp.path().join("workorders.jsonl");
    std::fs::write(&output_path, "keep-me\n").expect("sentinel");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/typescript/malformed.ts");

    let output = deslop()
        .arg("propose")
        .arg(&fixture)
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("run malformed propose");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("tree-sitter-error"));
    assert_eq!(std::fs::read_to_string(output_path).unwrap(), "keep-me\n");
}
