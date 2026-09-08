use anyhow::Result;
use deslop_analyzer::revision_cleanup::FindingDisposition;
use deslop_protocol::revision_cleanup::revision_cleanup_proposals;
use std::{fs, path::PathBuf};

#[test]
fn nested_scope_uses_repository_paths_and_proposes_only_introduced_findings() -> Result<()> {
    let root = tempfile::tempdir()?;
    let base = root.path().join("base");
    let target = root.path().join("target");
    for tree in [&base, &target] {
        fs::create_dir_all(tree.join("src"))?;
    }
    let clean = "fn value() -> i32 {\n    1\n}\n";
    let verbose = "fn value() -> i32 {\n    return 1;\n}\n";
    fs::write(base.join("src/value.rs"), clean)?;
    fs::write(target.join("src/value.rs"), verbose)?;
    let scope = [PathBuf::from("src")];
    let report = revision_cleanup_proposals(
        &base,
        &target,
        &scope,
        Default::default(),
        "Preserve the return value",
    )?;
    assert!(report.comparison.comparable);
    assert_eq!(report.proposals.len(), 1);
    assert_eq!(report.comparison.findings.len(), 1);
    let attributed = &report.comparison.findings[0];
    assert_eq!(attributed.disposition, FindingDisposition::Introduced);
    let finding = attributed.finding.as_ref().unwrap();
    assert_eq!(finding.rule, "needless-return");
    assert_eq!(finding.path, PathBuf::from("src/value.rs"));
    assert_eq!(
        report.comparison.target_sources[&PathBuf::from("src/value.rs")],
        blake3::hash(verbose.as_bytes()).to_hex().to_string()
    );
    // Inherited findings are not attributed to the revision and yield no new
    // revision-linked proposals, while snapshot analysis remains available.
    let unchanged = revision_cleanup_proposals(
        &target,
        &target,
        &scope,
        Default::default(),
        "Preserve the return value",
    )?;
    assert!(unchanged.proposals.is_empty());
    assert_eq!(
        unchanged.comparison.findings[0].disposition,
        FindingDisposition::Inherited
    );
    Ok(())
}
