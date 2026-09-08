use anyhow::{Result, ensure};
use deslop_protocol::{Patch, propose_work_orders, workorder_revision_guard};
use deslop_verify::{CoverageConfig, MutationConfig, VerifyOptions, apply_patches, verify_patches};
use std::{fs, process::Command};

#[test]
fn composed_failure_preserves_sources_and_creates_no_backups() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let original = "fn value() -> i32 {\n    return 1;\n}\n";
    for name in ["a.rs", "b.rs"] {
        fs::write(temp.path().join(name), original)?;
    }
    let batch = propose_work_orders(temp.path(), &[], Default::default())?;
    let patches: Vec<_> = batch
        .work_orders
        .iter()
        .filter(|order| order.region.text.contains("return 1;"))
        .map(|order| Patch {
            schema: "deslop.patch/3".into(),
            workorder_id: order.id.clone(),
            revision_guard: workorder_revision_guard(order).clone(),
            proposal_context: order.proposal_context.clone(),
            replacement: order.region.text.replace("return 1;", "1"),
            by: "composed-regression".into(),
        })
        .collect();
    ensure!(patches.len() == 2);
    // This trusted fixture's selected contract permits either change, not both.
    let options = VerifyOptions {
        root: temp.path().into(),
        scope: None,
        check_cmd: Some("grep -q 'return 1;' a.rs b.rs".into()),
        coverage: CoverageConfig::Disabled,
        mutation: MutationConfig::Disabled,
        characterization_tests: vec![],
        allow_non_removable: true,
    };
    let sandbox_available = Command::new("/usr/bin/systemd-run")
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--property=MemoryMax=2147483648",
            "--property=MemorySwapMax=0",
            "--property=TasksMax=256",
            "--",
            "/usr/bin/prlimit",
            "--fsize=16777216",
            "--",
            "/usr/bin/bwrap",
        ])
        .args([
            "--unshare-all",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--",
            "/usr/bin/true",
        ])
        .output()
        .is_ok_and(|output| output.status.success());
    for patch in &patches {
        let result = verify_patches(std::slice::from_ref(patch), &options)?;
        if sandbox_available {
            ensure!(
                result.failed_count() == 0,
                "individual candidate: {result:?}"
            );
        } else {
            ensure!(
                result.failed_count() == 1,
                "unavailable sandbox must reject"
            );
        }
    }
    let report = apply_patches(&patches, &options, true)?;
    ensure!(report.verified.failed_count() == 2 && report.written.is_empty());
    for name in ["a.rs", "b.rs"] {
        ensure!(fs::read_to_string(temp.path().join(name))? == original);
        ensure!(!temp.path().join(format!("{name}.deslop.bak")).exists());
    }
    Ok(())
}
