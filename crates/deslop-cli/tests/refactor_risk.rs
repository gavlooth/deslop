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

fn case_revision(case: &str, revision: &str) -> String {
    corpus_root()
        .join(case)
        .join(revision)
        .display()
        .to_string()
}

#[test]
fn text_format_renders_review_findings_and_coverage() {
    let output = deslop()
        .args([
            "refactor-risk",
            "--from",
            &case_revision("py-owner-moved-stale", "01-before"),
            "--to",
            &case_revision("py-owner-moved-stale", "02-after"),
            "--format",
            "text",
        ])
        .output()
        .expect("run refactor-risk --format text");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("owner-moved-consumer-stale"), "{text}");
    assert!(text.contains("NeverAuto"), "{text}");
    assert!(text.contains("coverage gaps"), "{text}");
    assert!(text.contains("suggestion:"), "{text}");
}

#[test]
fn sarif_format_marks_findings_report_only() {
    let output = deslop()
        .args([
            "refactor-risk",
            "--from",
            &case_revision("py-owner-moved-stale", "01-before"),
            "--to",
            &case_revision("py-owner-moved-stale", "02-after"),
            "--format",
            "sarif",
        ])
        .output()
        .expect("run refactor-risk --format sarif");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sarif: Value = serde_json::from_slice(&output.stdout).expect("parse SARIF");
    let results = sarif["runs"][0]["results"].as_array().expect("results");
    assert!(!results.is_empty());
    for result in results {
        assert_eq!(
            result["properties"]["reportOnly"], true,
            "refactor-defect findings must be report-only in SARIF: {result}"
        );
    }
}

/// Acceptance gate 10: the history-aware identity is stable across
/// history-window changes, so a baseline written from a two-revision window
/// suppresses the same defect detected through a three-revision window.
#[test]
fn baseline_identity_is_stable_across_window_changes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let baseline_path = temp.path().join("refactor-baseline.json");
    let write = deslop()
        .args([
            "refactor-risk",
            "--from",
            &case_revision("py-persistence-window", "01-before"),
            "--to",
            &case_revision("py-persistence-window", "02-after"),
            "--write-baseline",
            &baseline_path.display().to_string(),
        ])
        .output()
        .expect("write refactor baseline");
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    let suppressed = deslop()
        .args([
            "refactor-risk",
            "--from",
            &case_revision("py-persistence-window", "01-before"),
            "--to",
            &case_revision("py-persistence-window", "02-after"),
            "--then",
            &case_revision("py-persistence-window", "03-later"),
            "--baseline",
            &baseline_path.display().to_string(),
        ])
        .output()
        .expect("run with refactor baseline");
    assert!(suppressed.status.success());
    let report: Value = serde_json::from_slice(&suppressed.stdout).expect("parse report");
    assert_eq!(
        report["findings"].as_array().expect("findings").len(),
        0,
        "the same defect through a longer window must keep its identity: {report}"
    );
}

/// The pluggable history provider resolves Git revisions into exact-byte
/// snapshots; labels are the revision specs.
#[test]
fn git_revisions_resolve_through_the_history_provider() {
    let temp = tempfile::tempdir().expect("temp dir");
    let repo = temp.path();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(
        repo.join("scoring.py"),
        std::fs::read(
            corpus_root()
                .join("py-owner-moved-stale/01-before")
                .join("scoring.py"),
        )
        .expect("before fixture"),
    )
    .expect("write before");
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "before"]);
    std::fs::write(
        repo.join("scoring.py"),
        std::fs::read(
            corpus_root()
                .join("py-owner-moved-stale/02-after")
                .join("scoring.py"),
        )
        .expect("after fixture"),
    )
    .expect("write after");
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "after"]);

    let output = deslop()
        .args(["refactor-risk", "--from", "HEAD~1", "--to", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("run refactor-risk over git revisions");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse report");
    assert_eq!(report["before"], "HEAD~1");
    assert_eq!(report["after"], "HEAD");
    assert!(
        report["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["rule"] == "owner-moved-consumer-stale"),
        "{report}"
    );
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

        // Optional coverage pin: incomplete coverage must be an explicit
        // gap with the expected reason, never a clean result.
        if let Some(coverage) = case.get("coverage") {
            let expected = coverage["expect"].as_str().expect("coverage expect");
            assert_eq!(
                report["coverage"].as_str().expect("report coverage"),
                expected,
                "case {name}: coverage mismatch"
            );
            if let Some(substring) = coverage["reason_contains"].as_str() {
                let reasons = report["coverage_reasons"]
                    .as_array()
                    .expect("coverage reasons");
                assert!(
                    reasons
                        .iter()
                        .any(|reason| reason.as_str().unwrap_or("").contains(substring)),
                    "case {name}: no coverage reason contains {substring:?}: {reasons:?}"
                );
            }
        }
    }
}
