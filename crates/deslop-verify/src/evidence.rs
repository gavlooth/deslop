use std::collections::BTreeMap;

use anyhow::{Result, bail};
use deslop_core::SafetyClass;
use deslop_protocol::SharedWorkOrder;
use serde::{Deserialize, Serialize};

use crate::{VerificationCheckKind, VerifierPlan, VerifierPlanStatus};

pub const PRE_CHANGE_CHARACTERIZATION_SCHEMA: &str = "deslop.pre-change-characterization/1";
pub const VERIFICATION_EVIDENCE_SCHEMA: &str = "deslop.verification-evidence/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceKind {
    Parse,
    Format,
    Build,
    Lint,
    Type,
    TargetedTest,
    Coverage,
    Characterization,
    Differential,
    Mutation,
    GraphDelta,
}

impl From<VerificationCheckKind> for EvidenceKind {
    fn from(value: VerificationCheckKind) -> Self {
        match value {
            VerificationCheckKind::Parse => Self::Parse,
            VerificationCheckKind::Format => Self::Format,
            VerificationCheckKind::Build => Self::Build,
            VerificationCheckKind::Lint => Self::Lint,
            VerificationCheckKind::Type => Self::Type,
            VerificationCheckKind::TargetedTest => Self::TargetedTest,
            VerificationCheckKind::Coverage => Self::Coverage,
            VerificationCheckKind::Characterization => Self::Characterization,
            VerificationCheckKind::Differential => Self::Differential,
            VerificationCheckKind::Mutation => Self::Mutation,
            VerificationCheckKind::GraphDelta => Self::GraphDelta,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceOutcome {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEvidence {
    pub schema: String,
    pub check: String,
    pub kind: EvidenceKind,
    pub snapshot: String,
    pub artifact: String,
    pub outcome: EvidenceOutcome,
    pub detail: String,
}

impl VerificationEvidence {
    pub fn new(
        check: impl Into<String>,
        kind: EvidenceKind,
        snapshot: impl Into<String>,
        artifact: impl Into<String>,
        outcome: EvidenceOutcome,
        detail: impl Into<String>,
    ) -> Result<Self> {
        let evidence = Self {
            schema: VERIFICATION_EVIDENCE_SCHEMA.into(),
            check: check.into(),
            kind,
            snapshot: snapshot.into(),
            artifact: artifact.into(),
            outcome,
            detail: detail.into(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != VERIFICATION_EVIDENCE_SCHEMA {
            bail!("unsupported verification-evidence schema `{}`", self.schema);
        }
        for (label, value) in [
            ("evidence check", self.check.as_str()),
            ("evidence snapshot", self.snapshot.as_str()),
            ("evidence artifact", self.artifact.as_str()),
            ("evidence detail", self.detail.as_str()),
        ] {
            validate_text(label, value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreChangeCharacterization {
    schema: String,
    id: String,
    work_order: String,
    snapshot: String,
    test_artifact: String,
    observed_behavior: String,
    approved_by: String,
    captured_sequence: u64,
    approved_sequence: u64,
}

impl PreChangeCharacterization {
    pub fn capture(
        order: &SharedWorkOrder,
        test_artifact: impl Into<String>,
        observed_behavior: impl Into<String>,
        approved_by: impl Into<String>,
        captured_sequence: u64,
        approved_sequence: u64,
    ) -> Result<Self> {
        order.validate()?;
        let snapshot = order
            .provenance()
            .project_snapshot
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("characterization requires a pinned project snapshot")
            })?;
        let mut characterization = Self {
            schema: PRE_CHANGE_CHARACTERIZATION_SCHEMA.into(),
            id: String::new(),
            work_order: order.id().as_str().into(),
            snapshot: snapshot.clone(),
            test_artifact: test_artifact.into(),
            observed_behavior: observed_behavior.into(),
            approved_by: approved_by.into(),
            captured_sequence,
            approved_sequence,
        };
        characterization.id = derive_characterization_id(&characterization)?;
        characterization.validate()?;
        Ok(characterization)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn validate_for(
        &self,
        order: &SharedWorkOrder,
        patch_authored_sequence: u64,
    ) -> Result<()> {
        self.validate()?;
        let snapshot = order
            .provenance()
            .project_snapshot
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("work order lacks a project snapshot"))?;
        if self.work_order != order.id().as_str() || self.snapshot != snapshot {
            bail!("characterization is stale or belongs to another work order");
        }
        if self.approved_sequence >= patch_authored_sequence {
            bail!("risky characterization must be approved before the rewrite is authored");
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.schema != PRE_CHANGE_CHARACTERIZATION_SCHEMA {
            bail!(
                "unsupported pre-change-characterization schema `{}`",
                self.schema
            );
        }
        for (label, value) in [
            ("characterization work order", self.work_order.as_str()),
            ("characterization snapshot", self.snapshot.as_str()),
            (
                "characterization test artifact",
                self.test_artifact.as_str(),
            ),
            ("characterization behavior", self.observed_behavior.as_str()),
            ("characterization approver", self.approved_by.as_str()),
        ] {
            validate_text(label, value)?;
        }
        if self.captured_sequence == 0
            || self.approved_sequence == 0
            || self.captured_sequence > self.approved_sequence
        {
            bail!("characterization capture must precede or equal nonzero approval sequence");
        }
        if self.id != derive_characterization_id(self)? {
            bail!("pre-change-characterization identity is stale");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreChangeCharacterizationWire {
    schema: String,
    id: String,
    work_order: String,
    snapshot: String,
    test_artifact: String,
    observed_behavior: String,
    approved_by: String,
    captured_sequence: u64,
    approved_sequence: u64,
}

impl<'de> Deserialize<'de> for PreChangeCharacterization {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PreChangeCharacterizationWire::deserialize(deserializer)?;
        let characterization = Self {
            schema: wire.schema,
            id: wire.id,
            work_order: wire.work_order,
            snapshot: wire.snapshot,
            test_artifact: wire.test_artifact,
            observed_behavior: wire.observed_behavior,
            approved_by: wire.approved_by,
            captured_sequence: wire.captured_sequence,
            approved_sequence: wire.approved_sequence,
        };
        characterization
            .validate()
            .map_err(serde::de::Error::custom)?;
        Ok(characterization)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationDisposition {
    Automatic,
    ReviewOnly,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDecision {
    pub disposition: VerificationDisposition,
    pub evidence: Vec<VerificationEvidence>,
    pub residual_uncertainty: Vec<String>,
}

pub fn evaluate_evidence(
    order: &SharedWorkOrder,
    plan: &VerifierPlan,
    mut evidence: Vec<VerificationEvidence>,
    characterization: Option<&PreChangeCharacterization>,
    patch_authored_sequence: u64,
) -> Result<EvidenceDecision> {
    order.validate()?;
    plan.validate()?;
    if plan.work_order() != order.id().as_str()
        || order.provenance().project_snapshot.as_deref() != Some(plan.snapshot())
    {
        bail!("verifier plan is foreign or stale for the work order");
    }
    evidence.sort();
    if evidence.windows(2).any(|pair| pair[0] == pair[1]) {
        bail!("duplicate verification evidence");
    }
    let mut by_check = BTreeMap::new();
    for observation in &evidence {
        observation.validate()?;
        if observation.snapshot != plan.snapshot() {
            bail!("verification evidence is stale for the verifier plan");
        }
        if by_check
            .insert(observation.check.as_str(), observation)
            .is_some()
        {
            bail!(
                "multiple evidence records for check `{}`",
                observation.check
            );
        }
    }
    let mut uncertainty = plan.residual_uncertainty().to_vec();
    let mut required_failed = plan.status() != VerifierPlanStatus::Ready;
    for check in plan.checks() {
        match by_check.get(check.id.as_str()) {
            Some(observation) if observation.kind != EvidenceKind::from(check.kind) => {
                required_failed = true;
                uncertainty.push(format!(
                    "check `{}` returned the wrong evidence kind",
                    check.id
                ));
            }
            Some(observation) if observation.outcome == EvidenceOutcome::Passed => {}
            Some(observation) if observation.outcome == EvidenceOutcome::Failed => {
                required_failed = true;
                uncertainty.push(format!(
                    "check `{}` failed: {}",
                    check.id, observation.detail
                ));
            }
            Some(observation) => {
                required_failed = true;
                uncertainty.push(format!(
                    "check `{}` remains unknown: {}",
                    check.id, observation.detail
                ));
            }
            None => {
                required_failed = true;
                uncertainty.push(format!("check `{}` has no evidence", check.id));
            }
        }
    }

    let risky = matches!(
        order.safety(),
        SafetyClass::RiskySuggest | SafetyClass::LlmOnly
    );
    if risky {
        required_failed |= validate_risky_characterization(
            order,
            &evidence,
            characterization,
            patch_authored_sequence,
            &mut uncertainty,
        )?;
    }

    let mandatory_dynamic = [
        EvidenceKind::TargetedTest,
        EvidenceKind::Coverage,
        EvidenceKind::Differential,
        EvidenceKind::Mutation,
    ];
    for kind in mandatory_dynamic {
        if !evidence.iter().any(|observation| observation.kind == kind) {
            uncertainty.push(format!(
                "{} evidence was not selected",
                evidence_kind_name(kind)
            ));
        }
    }
    uncertainty.sort();
    uncertainty.dedup();

    let (disposition, safety_uncertainty) =
        decide_safety_disposition(order.safety(), required_failed);
    uncertainty.extend(safety_uncertainty);
    uncertainty.sort();
    uncertainty.dedup();
    Ok(EvidenceDecision {
        disposition,
        evidence,
        residual_uncertainty: uncertainty,
    })
}

pub fn decide_safety_disposition(
    safety: SafetyClass,
    verification_failed: bool,
) -> (VerificationDisposition, Vec<String>) {
    if verification_failed {
        return (
            VerificationDisposition::Rejected,
            vec!["one or more required verification obligations failed or remain unknown".into()],
        );
    }
    match safety {
        SafetyClass::SafeAuto => (VerificationDisposition::Automatic, Vec::new()),
        SafetyClass::NeverAuto => (
            VerificationDisposition::Rejected,
            vec!["never-auto evidence has no rewrite authority".into()],
        ),
        SafetyClass::AnalyzerConfirmed
        | SafetyClass::SafeWithPrecondition
        | SafetyClass::RiskySuggest
        | SafetyClass::LlmOnly => (
            VerificationDisposition::ReviewOnly,
            vec![format!(
                "safety class {:?} retains review-only semantic uncertainty",
                safety
            )],
        ),
    }
}

fn validate_risky_characterization(
    order: &SharedWorkOrder,
    evidence: &[VerificationEvidence],
    characterization: Option<&PreChangeCharacterization>,
    patch_authored_sequence: u64,
    uncertainty: &mut Vec<String>,
) -> Result<bool> {
    match characterization {
        Some(characterization) => {
            characterization.validate_for(order, patch_authored_sequence)?;
            let has_characterization_evidence = evidence.iter().any(|observation| {
                observation.kind == EvidenceKind::Characterization
                    && observation.artifact == characterization.id()
                    && observation.outcome == EvidenceOutcome::Passed
            });
            if !has_characterization_evidence {
                uncertainty.push(
                    "risky rewrite lacks passing evidence for its approved pre-change characterization"
                        .into(),
                );
            }
            Ok(!has_characterization_evidence)
        }
        None => {
            uncertainty.push(
                "risky rewrite lacks approved characterization captured on the pinned pre-change snapshot"
                    .into(),
            );
            Ok(true)
        }
    }
}

fn evidence_kind_name(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Parse => "parse",
        EvidenceKind::Format => "format",
        EvidenceKind::Build => "build",
        EvidenceKind::Lint => "lint",
        EvidenceKind::Type => "type",
        EvidenceKind::TargetedTest => "targeted-test",
        EvidenceKind::Coverage => "coverage",
        EvidenceKind::Characterization => "characterization",
        EvidenceKind::Differential => "differential",
        EvidenceKind::Mutation => "mutation",
        EvidenceKind::GraphDelta => "graph-delta",
    }
}

fn derive_characterization_id(characterization: &PreChangeCharacterization) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        schema: &'a str,
        work_order: &'a str,
        snapshot: &'a str,
        test_artifact: &'a str,
        observed_behavior: &'a str,
        approved_by: &'a str,
        captured_sequence: u64,
        approved_sequence: u64,
    }
    let payload = serde_json::to_vec(&Identity {
        schema: &characterization.schema,
        work_order: &characterization.work_order,
        snapshot: &characterization.snapshot,
        test_artifact: &characterization.test_artifact,
        observed_behavior: &characterization.observed_behavior,
        approved_by: &characterization.approved_by,
        captured_sequence: characterization.captured_sequence,
        approved_sequence: characterization.approved_sequence,
    })?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop pre-change characterization v1\0");
    hasher.update(&payload);
    Ok(format!("pc1_{}", hasher.finalize().to_hex()))
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 16_384 || value.chars().any(char::is_control) {
        bail!("{label} must be nonempty bounded printable text");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthorityObservation, AuthorityProvider, AuthorityRequirement, AuthorityState,
        VerificationCatalog, VerificationCheck, VerifierExecutionPolicy,
    };
    use deslop_protocol::SharedWorkOrder;
    use deslop_recipes::detect_rust_recipes;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, SharedWorkOrder, VerifierPlan) {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("fixture.rs"), "fn run() { return; 1; }\n").unwrap();
        let candidate = detect_rust_recipes(root.path(), &[PathBuf::from("fixture.rs")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let order = SharedWorkOrder::from_candidate(candidate).unwrap();
        let snapshot = order.provenance().project_snapshot.clone().unwrap();
        let resource = order.access().writes[0].clone();
        let requirement = AuthorityRequirement {
            key: "binding".into(),
            accepted_providers: vec![AuthorityProvider::Compiler],
        };
        let kinds = [
            ("build", VerificationCheckKind::Build),
            ("coverage", VerificationCheckKind::Coverage),
            ("differential", VerificationCheckKind::Differential),
            ("lint", VerificationCheckKind::Lint),
            ("mutation", VerificationCheckKind::Mutation),
            ("targeted-test", VerificationCheckKind::TargetedTest),
            ("type", VerificationCheckKind::Type),
        ];
        let checks = kinds
            .into_iter()
            .map(|(id, kind)| VerificationCheck {
                id: id.into(),
                kind,
                command: Some("true".into()),
                covers: vec![resource.clone()],
                dependencies: Vec::new(),
                authority: vec![requirement.clone()],
                always_required: false,
            })
            .collect();
        let plan = VerifierPlan::build(
            &order,
            VerificationCatalog {
                snapshot: snapshot.clone(),
                impact_coverage_complete: true,
                checks,
            },
            vec![AuthorityObservation {
                key: "binding".into(),
                provider: AuthorityProvider::Compiler,
                snapshot,
                artifact: "compiler-artifact".into(),
                state: AuthorityState::Proven,
                detail: "compiler proves binding".into(),
            }],
            VerifierExecutionPolicy::hermetic_workspace(),
        )
        .unwrap();
        (root, order, plan)
    }

    fn passing_evidence(plan: &VerifierPlan) -> Vec<VerificationEvidence> {
        plan.checks()
            .iter()
            .map(|check| {
                VerificationEvidence::new(
                    check.id.clone(),
                    check.kind.into(),
                    plan.snapshot(),
                    format!("artifact-{}", check.id),
                    EvidenceOutcome::Passed,
                    "check passed",
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn safe_auto_requires_every_selected_check_and_retains_dynamic_evidence() {
        let (_root, order, plan) = fixture();
        let decision = evaluate_evidence(&order, &plan, passing_evidence(&plan), None, 10).unwrap();
        assert_eq!(decision.disposition, VerificationDisposition::Automatic);
        assert!(decision.residual_uncertainty.is_empty());

        let mut missing = passing_evidence(&plan);
        missing.retain(|evidence| evidence.kind != EvidenceKind::Mutation);
        let decision = evaluate_evidence(&order, &plan, missing, None, 10).unwrap();
        assert_eq!(decision.disposition, VerificationDisposition::Rejected);
        assert!(
            decision
                .residual_uncertainty
                .iter()
                .any(|reason| reason.contains("mutation"))
        );
    }

    #[test]
    fn characterization_is_identity_bound_and_must_precede_patch_authorship() {
        let (_root, order, _plan) = fixture();
        let characterization = PreChangeCharacterization::capture(
            &order,
            "test-artifact",
            "behavior-artifact",
            "reviewer",
            1,
            2,
        )
        .unwrap();
        characterization.validate_for(&order, 3).unwrap();
        assert!(characterization.validate_for(&order, 2).is_err());

        let mut wire = serde_json::to_value(&characterization).unwrap();
        wire["snapshot"] = serde_json::json!("ps1_stale");
        assert!(serde_json::from_value::<PreChangeCharacterization>(wire).is_err());
    }

    #[test]
    fn risky_gate_requires_prechange_approval_and_matching_passing_artifact() {
        let (_root, order, _plan) = fixture();
        let mut uncertainty = Vec::new();
        assert!(validate_risky_characterization(&order, &[], None, 3, &mut uncertainty).unwrap());
        let characterization = PreChangeCharacterization::capture(
            &order,
            "test-artifact",
            "behavior-artifact",
            "reviewer",
            1,
            2,
        )
        .unwrap();
        let evidence = vec![
            VerificationEvidence::new(
                "characterization",
                EvidenceKind::Characterization,
                order.provenance().project_snapshot.clone().unwrap(),
                characterization.id(),
                EvidenceOutcome::Passed,
                "pre-change and patched behavior match",
            )
            .unwrap(),
        ];
        let mut uncertainty = Vec::new();
        assert!(
            !validate_risky_characterization(
                &order,
                &evidence,
                Some(&characterization),
                3,
                &mut uncertainty,
            )
            .unwrap()
        );
        assert!(uncertainty.is_empty());
    }

    #[test]
    fn stale_or_wrong_kind_evidence_rejects() {
        let (_root, order, plan) = fixture();
        let mut evidence = passing_evidence(&plan);
        evidence[0].snapshot = "ps1_stale".into();
        assert!(evaluate_evidence(&order, &plan, evidence, None, 10).is_err());

        let mut evidence = passing_evidence(&plan);
        evidence[0].kind = EvidenceKind::Parse;
        let decision = evaluate_evidence(&order, &plan, evidence, None, 10).unwrap();
        assert_eq!(decision.disposition, VerificationDisposition::Rejected);
    }

    #[test]
    fn evidence_schema_is_strict() {
        let evidence = VerificationEvidence::new(
            "test",
            EvidenceKind::TargetedTest,
            "ps1_snapshot",
            "artifact",
            EvidenceOutcome::Unknown,
            "not run",
        )
        .unwrap();
        let mut wire = serde_json::to_value(evidence).unwrap();
        wire["extra"] = serde_json::json!(true);
        assert!(serde_json::from_value::<VerificationEvidence>(wire).is_err());
    }

    #[test]
    fn every_weaker_safety_class_retains_explicit_uncertainty() {
        assert_eq!(
            decide_safety_disposition(SafetyClass::SafeAuto, false),
            (VerificationDisposition::Automatic, Vec::new())
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
    }
}
