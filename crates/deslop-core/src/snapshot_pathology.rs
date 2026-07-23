//! Present-state contract pathology findings from one exact source snapshot.
//!
//! Unlike `refactor_defect`, this schema does not claim that an owner moved,
//! a mechanism was retired, or a condition persisted. History can enrich the
//! same neutral pathology later, but absence of history is not represented as
//! a synthetic revision pair or zero persistence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::refactor_defect::{ContractNodeRef, ContractStep, CoverageGap, EvidenceItem};
use crate::{SafetyClass, Span};

pub const SNAPSHOT_PATHOLOGY_SCHEMA: &str = "deslop.snapshot-pathology/1";
pub const SNAPSHOT_REFACTOR_RISK_SCHEMA: &str = "deslop.snapshot-refactor-risk/1";

pub mod rule_names {
    pub const OWNER_CONSUMER_CONTRACT_SPLIT: &str = "owner-consumer-contract-split";
    pub const PARTITION_BOUNDARY_NOT_PRESERVED: &str = "partition-boundary-not-preserved";
    pub const MECHANISM_GATE_CONTRACT_SPLIT: &str = "mechanism-gate-contract-split";
    pub const PRODUCER_VERIFIER_SCHEMA_MISMATCH: &str = "producer-verifier-schema-mismatch";
    pub const ACCEPTED_CONFIG_NO_BEHAVIORAL_REACH: &str = "accepted-config-no-behavioral-reach";
    pub const CONFIDENCE_DERIVED_AFTER_LOSSY_COMMIT: &str = "confidence-derived-after-lossy-commit";
    pub const TELEMETRY_CLAIM_UNBOUND: &str = "telemetry-claim-unbound";
    pub const TEST_CONTRACT_DIMENSION_UNCOVERED: &str = "test-contract-dimension-uncovered";
    pub const SAME_PATH_EXPENSIVE_WORK_REPEATED: &str = "same-path-expensive-work-repeated";
    pub const PUBLISHED_IDENTITY_NOT_LIVE: &str = "published-identity-not-live";
    pub const CONTRACT_CHAIN_INCOMPLETE: &str = "contract-chain-incomplete";
    pub const SIBLING_ADMISSION_GUARDS_ASYMMETRIC: &str = "sibling-admission-guards-asymmetric";

    pub const ALL: &[&str] = &[
        OWNER_CONSUMER_CONTRACT_SPLIT,
        PARTITION_BOUNDARY_NOT_PRESERVED,
        MECHANISM_GATE_CONTRACT_SPLIT,
        PRODUCER_VERIFIER_SCHEMA_MISMATCH,
        ACCEPTED_CONFIG_NO_BEHAVIORAL_REACH,
        CONFIDENCE_DERIVED_AFTER_LOSSY_COMMIT,
        TELEMETRY_CLAIM_UNBOUND,
        TEST_CONTRACT_DIMENSION_UNCOVERED,
        SAME_PATH_EXPENSIVE_WORK_REPEATED,
        PUBLISHED_IDENTITY_NOT_LIVE,
        CONTRACT_CHAIN_INCOMPLETE,
        SIBLING_ADMISSION_GUARDS_ASYMMETRIC,
    ];
}

/// Map the causal history vocabulary onto the neutral end-state family.
pub fn neutral_family_for_history_rule(rule: &str) -> Option<&'static str> {
    use crate::refactor_defect::rule_names as history;
    Some(match rule {
        history::OWNER_MOVED_CONSUMER_STALE => rule_names::OWNER_CONSUMER_CONTRACT_SPLIT,
        history::SCOPE_COLLAPSE_AFTER_REFACTOR => rule_names::PARTITION_BOUNDARY_NOT_PRESERVED,
        history::MECHANISM_LIVE_GATE_RETIRED => rule_names::MECHANISM_GATE_CONTRACT_SPLIT,
        history::PRODUCER_VERIFIER_SCHEMA_DRIFT => rule_names::PRODUCER_VERIFIER_SCHEMA_MISMATCH,
        history::ACCEPTED_CONFIG_INERT => rule_names::ACCEPTED_CONFIG_NO_BEHAVIORAL_REACH,
        history::CONFIDENCE_PROVENANCE_LOST => rule_names::CONFIDENCE_DERIVED_AFTER_LOSSY_COMMIT,
        history::TELEMETRY_NOT_BOUND_TO_CLAIM => rule_names::TELEMETRY_CLAIM_UNBOUND,
        history::TEST_ORACLE_LAG => rule_names::TEST_CONTRACT_DIMENSION_UNCOVERED,
        history::HOT_PATH_WORK_DUPLICATED => rule_names::SAME_PATH_EXPENSIVE_WORK_REPEATED,
        history::OPERATIONAL_IDENTITY_STALE => rule_names::PUBLISHED_IDENTITY_NOT_LIVE,
        history::ADOPTION_CHAIN_INCOMPLETE => rule_names::CONTRACT_CHAIN_INCOMPLETE,
        history::SIBLING_ADMISSION_GATES_DIVERGED => {
            rule_names::SIBLING_ADMISSION_GUARDS_ASYMMETRIC
        }
        _ => return None,
    })
}

/// One present-state review finding. Every required field is evidence about
/// the current snapshot; historical transition evidence is intentionally not
/// part of this schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotPathology {
    pub schema: String,
    pub rule: String,
    /// Stable neutral family used to join optional historical enrichment.
    pub family: String,
    /// Caller-visible identity of the exact analyzed snapshot.
    pub snapshot: String,
    /// Nodes that define the current contract component or split.
    pub anchors: Vec<ContractNodeRef>,
    pub conflicting_edges: Vec<ContractStep>,
    pub causal_path: Vec<ContractStep>,
    pub evidence: Vec<EvidenceItem>,
    pub counter_evidence: Vec<EvidenceItem>,
    pub coverage_gaps: Vec<CoverageGap>,
    pub priority_inputs: BTreeMap<String, i64>,
    pub safety: SafetyClass,
    pub suggested_verification: String,
}

impl SnapshotPathology {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SNAPSHOT_PATHOLOGY_SCHEMA {
            return Err(format!(
                "expected schema `{SNAPSHOT_PATHOLOGY_SCHEMA}`, got `{}`",
                self.schema
            ));
        }
        if !rule_names::ALL.contains(&self.rule.as_str()) || self.family != self.rule {
            return Err(format!(
                "unknown or mismatched snapshot pathology rule `{}`",
                self.rule
            ));
        }
        if self.anchors.is_empty() {
            return Err(format!(
                "snapshot pathology `{}` has no current-state anchor",
                self.rule
            ));
        }
        if self.causal_path.is_empty() {
            return Err(format!(
                "snapshot pathology `{}` has no causal path",
                self.rule
            ));
        }
        if self.safety != SafetyClass::NeverAuto {
            return Err(format!(
                "snapshot pathology `{}` is not never-auto",
                self.rule
            ));
        }
        if self.suggested_verification.trim().is_empty() {
            return Err(format!(
                "snapshot pathology `{}` has no suggested verification",
                self.rule
            ));
        }
        Ok(())
    }

    /// Identity of the end-state pathology, independent of snapshot label or
    /// future history-window enrichment.
    pub fn pathology_identity(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"deslop snapshot pathology identity v1");
        let mut part = |bytes: &[u8]| {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        };
        part(self.family.as_bytes());
        let mut anchors: Vec<_> = self
            .anchors
            .iter()
            .map(|node| {
                (
                    node.path.to_string_lossy().to_string(),
                    node.fingerprint.as_str(),
                )
            })
            .collect();
        anchors.sort_unstable();
        anchors.dedup();
        for (path, fingerprint) in anchors {
            part(path.as_bytes());
            part(fingerprint.as_bytes());
        }
        format!("rsp1_{}", hasher.finalize().to_hex())
    }

    pub fn to_finding(&self) -> crate::Finding {
        let location = self
            .conflicting_edges
            .first()
            .map(|step| &step.node)
            .or_else(|| self.anchors.first())
            .or_else(|| self.causal_path.first().map(|step| &step.node));
        let (path, span) = location
            .map(|node| (node.path.clone(), node.span))
            .unwrap_or_else(|| (std::path::PathBuf::new(), Span::new(1, 1, 0, 0)));
        let mut message = format!(
            "[snapshot {}] {}",
            self.snapshot,
            self.causal_path
                .iter()
                .map(|step| step.detail.as_str())
                .collect::<Vec<_>>()
                .join(" -> ")
        );
        if let Some(first) = self.counter_evidence.first() {
            message.push_str(&format!(
                " | counter-evidence ({} item(s)): {}",
                self.counter_evidence.len(),
                first.detail
            ));
        }
        if let Some(first) = self.coverage_gaps.first() {
            message.push_str(&format!(
                " | coverage gaps ({}): {}",
                self.coverage_gaps.len(),
                first.reason
            ));
        }
        crate::Finding {
            path,
            span,
            rule: self.rule.clone(),
            severity: crate::Severity::Info,
            safety: SafetyClass::NeverAuto,
            detected_by: crate::DetectedBy::RefactorSnapshot,
            message,
            suggestion: self.suggested_verification.clone(),
            precondition: None,
            edit: None,
            fingerprint: self.pathology_identity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refactor_defect::{CapabilityLevel, ContractEdgeKind, ContractRole, FactProvider};
    use std::path::PathBuf;

    fn node() -> ContractNodeRef {
        ContractNodeRef {
            role: ContractRole::Consumer,
            path: PathBuf::from("src/current.py"),
            span: Span::new(2, 3, 10, 30),
            fingerprint: "current-fp".to_string(),
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Partial,
        }
    }

    #[test]
    fn identity_ignores_snapshot_label() {
        let step = ContractStep {
            edge: ContractEdgeKind::Consumes,
            node: node(),
            token: Some("score".to_string()),
            detail: "current consumer path reaches score".to_string(),
        };
        let mut finding = SnapshotPathology {
            schema: SNAPSHOT_PATHOLOGY_SCHEMA.to_string(),
            rule: rule_names::OWNER_CONSUMER_CONTRACT_SPLIT.to_string(),
            family: rule_names::OWNER_CONSUMER_CONTRACT_SPLIT.to_string(),
            snapshot: "one".to_string(),
            anchors: vec![node()],
            conflicting_edges: vec![step.clone()],
            causal_path: vec![step],
            evidence: Vec::new(),
            counter_evidence: Vec::new(),
            coverage_gaps: Vec::new(),
            priority_inputs: BTreeMap::new(),
            safety: SafetyClass::NeverAuto,
            suggested_verification: "compare both paths".to_string(),
        };
        finding.validate().unwrap();
        let first = finding.pathology_identity();
        finding.snapshot = "two".to_string();
        assert_eq!(first, finding.pathology_identity());
    }
}
