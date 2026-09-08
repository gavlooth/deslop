use anyhow::{Result, ensure};
use deslop_eval::trajectory::*;
use deslop_protocol::{Patch, propose_work_orders, workorder_revision_guard};
use deslop_verify::{CoverageConfig, MutationConfig, VerifyOptions};
use std::{collections::BTreeMap, fs};

#[test]
fn public_export_replays_but_does_not_grant_write_authority() -> Result<()> {
    let root = tempfile::tempdir()?;
    let clean = "fn value() -> i32 {\n    1\n}\n";
    let verbose = "fn value() -> i32 {\n    return 1;\n}\n";
    let base = Snapshot::new("base", BTreeMap::from([("sample.rs".into(), clean.into())]))?;
    let final_snapshot = Snapshot::new(
        "final",
        BTreeMap::from([("sample.rs".into(), verbose.into())]),
    )?;
    let export = serde_json::to_vec(&serde_json::json!({
        "info":{"id":"fictional-engineering-fixture"},
        "messages":[{"info":{"id":"message-1"},"parts":[{
            "id":"edit-1", "type":"tool", "tool":"edit",
            "state":{"status":"completed", "input":{
                "filePath":"sample.rs", "oldString":"    1", "newString":"    return 1;"
            }}
        }]}]
    }))?;
    let trajectory = import_opencode_export(
        &export,
        base,
        final_snapshot,
        LicenseGate {
            spdx: Some("MIT".into()),
            approved: true,
            source_uri: Some("fictional-engineering-fixture:not-study-data".into()),
        },
    )?;
    ensure!(replay_trajectory(&trajectory)?.status == ReplayStatus::Complete);
    let candidates = generate_candidates(&trajectory, &MinimizationBudget::default())?;
    ensure!(candidates.len() == 1);
    fs::write(root.path().join("sample.rs"), verbose)?;
    let proposals = propose_work_orders(root.path(), &[], Default::default())?;
    let order = proposals
        .work_orders
        .iter()
        .find(|order| order.region.text.contains("return 1;"))
        .expect("actual source proposal");
    let patch = Patch {
        schema: "deslop.patch/3".into(),
        workorder_id: order.id.clone(),
        revision_guard: workorder_revision_guard(order).clone(),
        proposal_context: order.proposal_context.clone(),
        replacement: order.region.text.replace("return 1;", "1"),
        by: "engineering-fixture".into(),
    };
    let mut options = VerifyOptions {
        root: root.path().into(),
        scope: None,
        check_cmd: None,
        coverage: CoverageConfig::Disabled,
        mutation: MutationConfig::Disabled,
        characterization_tests: vec![],
        allow_non_removable: false,
    };
    ensure!(
        verify_and_apply_candidate(
            &candidates[0],
            std::slice::from_ref(&patch),
            &options,
            false
        )
        .is_err()
    );
    ensure!(fs::read_to_string(root.path().join("sample.rs"))? == verbose);
    // Explicit test-owner approval for this hand-checked fictional rewrite,
    // not an authority inferred from imported observations or size reduction.
    options.allow_non_removable = true;
    let result = verify_and_apply_candidate(&candidates[0], &[patch], &options, false)?;
    ensure!(result.applied.verified.failed_count() == 0 && result.applied.written.len() == 1);
    ensure!(fs::read_to_string(root.path().join("sample.rs"))? == clean);
    ensure!(result.candidate_hash == trajectory.base.identity.tree_hash);
    Ok(())
}
