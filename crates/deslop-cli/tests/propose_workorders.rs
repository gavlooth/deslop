use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use deslop_protocol::WorkOrder;

fn propose(paths: &[PathBuf]) -> Vec<WorkOrder> {
    let output = Command::new(env!("CARGO_BIN_EXE_deslop"))
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
}

#[test]
fn propose_deduplicates_repeated_and_overlapping_input_paths() {
    let fixture = sloppy_rust_fixture();
    let corpus_dir = fixture.parent().expect("corpus directory").to_path_buf();

    let repeated = propose(&[fixture.clone(), fixture.clone()]);
    assert_eq!(repeated.len(), 3);
    assert_unique_ids(&repeated);
    assert_eq!(finding_count(&repeated), 13);

    let overlapping = propose(&[fixture, corpus_dir]);
    assert_eq!(overlapping.len(), 31);
    assert_unique_ids(&overlapping);
    assert_eq!(finding_count(&overlapping), 62);
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
