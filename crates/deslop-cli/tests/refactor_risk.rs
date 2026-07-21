//! Fixture-driven evaluation of `deslop refactor-risk` against the golden
//! refactor-history corpus (`tests/refactor-history/manifest.json`).
//!
//! Every manifest case runs the CLI over its ordered snapshot directories
//! (`--from`/`--to`, extra revisions via `--then`) and checks each
//! expectation: a rule with `should_fire: true` must appear in the report's
//! findings; `should_fire: false` must not. An expectation with
//! `"summary": true` is checked against `summaries` instead — summary
//! findings must never leak into `findings` (no double counting).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn deslop() -> Command {
    Command::new(env!("CARGO_BIN_EXE_deslop"))
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/refactor-history")
}

#[test]
fn refactor_history_manifest_expectations_hold() {
    let root = corpus_root();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(root.join("manifest.json")).expect("read refactor-history manifest"),
    )
    .expect("parse refactor-history manifest");
    assert_eq!(manifest["schema"], "deslop.refactor-history-manifest/1");

    let cases = manifest["cases"].as_array().expect("manifest cases");
    assert!(
        cases.len() >= 6,
        "the golden corpus must keep at least the six Phase 0 cases"
    );

    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let revisions = case["revisions"].as_array().expect("case revisions");
        assert!(
            revisions.len() >= 2,
            "case {name}: refactor-risk compares at least two revisions"
        );
        let mut args = vec![
            "refactor-risk".to_string(),
            "--from".to_string(),
            root.join(name)
                .join(revisions[0].as_str().unwrap())
                .display()
                .to_string(),
            "--to".to_string(),
            root.join(name)
                .join(revisions[1].as_str().unwrap())
                .display()
                .to_string(),
        ];
        for revision in &revisions[2..] {
            args.push("--then".to_string());
            args.push(
                root.join(name)
                    .join(revision.as_str().unwrap())
                    .display()
                    .to_string(),
            );
        }

        let output = deslop()
            .args(&args)
            .output()
            .expect("run deslop refactor-risk");
        assert!(
            output.status.success(),
            "case {name}: refactor-risk failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_slice(&output.stdout).expect("case {name}: parse report JSON");
        assert_eq!(report["schema"], "deslop.refactor-risk/1");
        let fired: Vec<&str> = report["findings"]
            .as_array()
            .expect("report findings")
            .iter()
            .map(|finding| finding["rule"].as_str().expect("finding rule"))
            .collect();
        let summaries: Vec<&str> = report["summaries"]
            .as_array()
            .expect("report summaries")
            .iter()
            .map(|summary| summary["rule"].as_str().expect("summary rule"))
            .collect();
        assert!(
            !fired.contains(&"adoption-chain-incomplete"),
            "case {name}: summaries must not appear in findings (double counting)"
        );
        for finding in report["findings"].as_array().unwrap() {
            assert_eq!(
                finding["safety"], "never-auto",
                "case {name}: refactor-defect findings are always review-only"
            );
        }
        for summary in report["summaries"].as_array().unwrap() {
            assert_eq!(
                summary["safety"], "never-auto",
                "case {name}: refactor-defect summaries are always review-only"
            );
        }

        for expectation in case["expectations"].as_array().expect("case expectations") {
            let rule = expectation["rule"].as_str().expect("expectation rule");
            let should_fire = expectation["should_fire"]
                .as_bool()
                .expect("expectation should_fire");
            let is_summary = expectation["summary"].as_bool().unwrap_or(false);
            let note = expectation["note"].as_str().unwrap_or("");
            let surface = if is_summary { &summaries } else { &fired };
            assert_eq!(
                surface.contains(&rule),
                should_fire,
                "case {name}: rule {rule} should_fire={should_fire} summary={is_summary} \
                 ({note}); findings: {fired:?}; summaries: {summaries:?}"
            );
        }
    }
}
