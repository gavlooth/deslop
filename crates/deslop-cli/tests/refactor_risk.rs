//! Fixture-driven evaluation of `deslop refactor-risk` against the golden
//! refactor-history corpus (`tests/refactor-history/manifest.json`).
//!
//! Every manifest case runs the CLI over its ordered snapshot directories
//! and checks each expectation: a rule with `should_fire: true` must appear
//! in the report's findings; `should_fire: false` must not.

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
        assert_eq!(
            revisions.len(),
            2,
            "case {name}: the Phase 1 detector compares exactly two revisions"
        );
        let from = root.join(name).join(revisions[0].as_str().unwrap());
        let to = root.join(name).join(revisions[1].as_str().unwrap());

        let output = deslop()
            .args([
                "refactor-risk",
                "--from",
                from.to_str().unwrap(),
                "--to",
                to.to_str().unwrap(),
            ])
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
        for finding in report["findings"].as_array().unwrap() {
            assert_eq!(
                finding["safety"], "never-auto",
                "case {name}: refactor-defect findings are always review-only"
            );
        }

        for expectation in case["expectations"].as_array().expect("case expectations") {
            let rule = expectation["rule"].as_str().expect("expectation rule");
            let should_fire = expectation["should_fire"]
                .as_bool()
                .expect("expectation should_fire");
            let note = expectation["note"].as_str().unwrap_or("");
            assert_eq!(
                fired.contains(&rule),
                should_fire,
                "case {name}: rule {rule} should_fire={should_fire} ({note}); fired: {fired:?}"
            );
        }
    }
}
