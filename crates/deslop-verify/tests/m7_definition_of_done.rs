use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use deslop_core::SafetyClass;
use deslop_protocol::SharedWorkOrder;
use deslop_recipes::{ExpectedGraphDelta, detect_rust_recipes};
use deslop_verify::{
    AuthorityObservation, AuthorityProvider, AuthorityRequirement, AuthorityState, EvidenceOutcome,
    GraphDeltaOracle, GraphReanalysisPhase, PolicyCommandRuntime, VerificationCatalog,
    VerificationCheck, VerificationCheckKind, VerificationDisposition, VerificationEvidence,
    VerificationRuntime, VerificationTransactionOptions, VerificationTransactionStatus,
    VerifierExecutionPolicy, VerifierFailure, VerifierFailureKind, VerifierPlan, VerifierStage,
    decide_safety_disposition, execute_verification_transaction, restore_committed_transaction,
};

struct BehaviorRuntime {
    expected_delta: ExpectedGraphDelta,
}

impl VerificationRuntime for BehaviorRuntime {
    fn format(
        &mut self,
        _root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        _policy: &VerifierExecutionPolicy,
    ) -> Result<VerificationEvidence, VerifierFailure> {
        passing(order, check, "formatter checked exact staged bytes")
    }

    fn reanalyze_graph_delta(
        &mut self,
        _root: &Path,
        _order: &SharedWorkOrder,
        phase: GraphReanalysisPhase,
        _policy: &VerifierExecutionPolicy,
    ) -> Result<ExpectedGraphDelta, VerifierFailure> {
        Ok(if phase == GraphReanalysisPhase::Rollback {
            ExpectedGraphDelta {
                changes: Vec::new(),
            }
        } else {
            self.expected_delta.clone()
        })
    }

    fn run_check(
        &mut self,
        root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        _policy: &VerifierExecutionPolicy,
    ) -> Result<VerificationEvidence, VerifierFailure> {
        let binary = root.join(format!("m7-{}", check.id));
        let compiled = Command::new("rustc")
            .arg("fixture.rs")
            .arg("-o")
            .arg(&binary)
            .current_dir(root)
            .status()
            .map_err(|error| command_failure(check, error.to_string()))?;
        if !compiled.success() {
            return Err(command_failure(check, format!("rustc exited {compiled}")));
        }
        let behavior = Command::new(&binary)
            .current_dir(root)
            .status()
            .map_err(|error| command_failure(check, error.to_string()))?;
        if !behavior.success() {
            return Err(command_failure(
                check,
                format!("behavior oracle exited {behavior}"),
            ));
        }
        passing(
            order,
            check,
            "fixture compiled and its behavior assertion passed",
        )
    }
}

impl GraphDeltaOracle for BehaviorRuntime {
    fn observe(
        &mut self,
        _root: &Path,
        _order: &SharedWorkOrder,
        phase: GraphReanalysisPhase,
    ) -> Result<ExpectedGraphDelta, VerifierFailure> {
        Ok(if phase == GraphReanalysisPhase::Rollback {
            ExpectedGraphDelta {
                changes: Vec::new(),
            }
        } else {
            self.expected_delta.clone()
        })
    }
}

fn passing(
    order: &SharedWorkOrder,
    check: &VerificationCheck,
    detail: &str,
) -> Result<VerificationEvidence, VerifierFailure> {
    VerificationEvidence::new(
        check.id.clone(),
        check.kind.into(),
        order.provenance().project_snapshot.clone().unwrap(),
        format!("m7-dod-artifact-{}", check.id),
        EvidenceOutcome::Passed,
        detail,
    )
    .map_err(|error| command_failure(check, error.to_string()))
}

fn command_failure(check: &VerificationCheck, detail: String) -> VerifierFailure {
    VerifierFailure {
        stage: VerifierStage::Command,
        kind: VerifierFailureKind::CommandFailed,
        check: Some(check.id.clone()),
        detail,
        retryable: false,
    }
}

fn plan(order: &SharedWorkOrder) -> VerifierPlan {
    let snapshot = order.provenance().project_snapshot.clone().unwrap();
    let resource = order.access().writes[0].clone();
    let requirement = AuthorityRequirement {
        key: "compiler-binding-and-type-preconditions".into(),
        accepted_providers: vec![AuthorityProvider::Compiler],
    };
    let mut checks = [
        ("build", VerificationCheckKind::Build),
        ("coverage", VerificationCheckKind::Coverage),
        ("differential", VerificationCheckKind::Differential),
        ("format", VerificationCheckKind::Format),
        ("graph-delta", VerificationCheckKind::GraphDelta),
        ("lint", VerificationCheckKind::Lint),
        ("mutation", VerificationCheckKind::Mutation),
        ("targeted-test", VerificationCheckKind::TargetedTest),
        ("type", VerificationCheckKind::Type),
    ]
    .into_iter()
    .map(|(id, kind)| VerificationCheck {
        id: id.into(),
        kind,
        command: (kind != VerificationCheckKind::GraphDelta).then(|| "rustc+behavior".into()),
        covers: vec![resource.clone()],
        dependencies: Vec::new(),
        authority: vec![requirement.clone()],
        always_required: false,
    })
    .collect::<Vec<_>>();
    checks.sort_by(|left, right| left.id.cmp(&right.id));
    VerifierPlan::build(
        order,
        VerificationCatalog {
            snapshot: snapshot.clone(),
            impact_coverage_complete: true,
            checks,
        },
        vec![AuthorityObservation {
            key: requirement.key,
            provider: AuthorityProvider::Compiler,
            snapshot,
            artifact: "rustc-m7-dod-artifact".into(),
            state: AuthorityState::Proven,
            detail: "rustc preconditions are current for the pinned fixture".into(),
        }],
        VerifierExecutionPolicy::hermetic_workspace(),
    )
    .unwrap()
}

#[test]
fn m7_safe_auto_is_behavior_checked_atomic_and_every_weaker_class_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    let source = "fn value() -> i32 { return 7; 1; }\nfn main() { assert_eq!(value(), 7); }\n";
    fs::write(root.path().join("fixture.rs"), source).unwrap();
    let candidate = detect_rust_recipes(root.path(), &[PathBuf::from("fixture.rs")])
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(candidate.safety(), SafetyClass::SafeAuto);
    let expected_delta = candidate.expected_delta().clone();
    let order = SharedWorkOrder::from_candidate(candidate).unwrap();
    let plan = plan(&order);
    let mut options = VerificationTransactionOptions::controlled(root.path().to_path_buf(), 2);
    options.authorize_write = true;
    let report = execute_verification_transaction(
        &order,
        &plan,
        None,
        &options,
        &mut BehaviorRuntime { expected_delta },
    )
    .unwrap();
    assert_eq!(report.status, VerificationTransactionStatus::Applied);
    assert_eq!(
        report.evidence.as_ref().unwrap().disposition,
        VerificationDisposition::Automatic
    );
    assert!(report.residual_uncertainty.is_empty());
    let patched = fs::read_to_string(root.path().join("fixture.rs")).unwrap();
    assert!(!patched.contains("1;"));

    let live_binary = root.path().join("m7-live");
    assert!(
        Command::new("rustc")
            .arg("fixture.rs")
            .arg("-o")
            .arg(&live_binary)
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    assert!(Command::new(&live_binary).status().unwrap().success());

    let manifest = report.undo_manifest.unwrap();
    restore_committed_transaction(root.path(), &manifest).unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("fixture.rs")).unwrap(),
        source
    );

    for safety in [
        SafetyClass::AnalyzerConfirmed,
        SafetyClass::SafeWithPrecondition,
        SafetyClass::RiskySuggest,
        SafetyClass::LlmOnly,
        SafetyClass::NeverAuto,
    ] {
        let (disposition, uncertainty) = decide_safety_disposition(safety, false);
        assert_ne!(disposition, VerificationDisposition::Automatic);
        assert!(!uncertainty.is_empty());
    }

    let _production_runtime_type = std::any::type_name::<PolicyCommandRuntime<BehaviorRuntime>>();
}
