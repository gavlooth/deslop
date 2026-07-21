//! Contracts for refactor-defect accumulation detection (Phase 0).
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md`. These types define the two
//! versioned wire schemas — `deslop.refactor-history/1` (ordered exact-byte
//! revision snapshots) and `deslop.refactor-defect/1` (review findings) — plus
//! the canonical contract roles and edge kinds shared by language adapters,
//! the contract graph projection, and the analyzer rules.
//!
//! Evidence discipline: every provider fact is bound to the exact revision it
//! analyzed, provider disagreements stay visible, and missing facts remain
//! unknown — they are never converted into negative facts. All findings in
//! this family are [`SafetyClass::NeverAuto`]: the analysis diagnoses an
//! incomplete contract migration and suggests a verification; it never
//! generates a rewrite.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{SafetyClass, Span};

/// Wire schema identifier for an ordered revision history bundle.
pub const REFACTOR_HISTORY_SCHEMA: &str = "deslop.refactor-history/1";

/// Wire schema identifier for one refactor-defect review finding.
pub const REFACTOR_DEFECT_SCHEMA: &str = "deslop.refactor-defect/1";

/// Detector-family rule names, mirrored in the canonical registry
/// (`crate::rules`). Keep the two in sync: the registry is the user-facing
/// source of truth, these constants are the typed one.
pub mod rule_names {
    pub const OWNER_MOVED_CONSUMER_STALE: &str = "owner-moved-consumer-stale";
    pub const SCOPE_COLLAPSE_AFTER_REFACTOR: &str = "scope-collapse-after-refactor";
    pub const MECHANISM_LIVE_GATE_RETIRED: &str = "mechanism-live-gate-retired";
    pub const PRODUCER_VERIFIER_SCHEMA_DRIFT: &str = "producer-verifier-schema-drift";
    pub const ACCEPTED_CONFIG_INERT: &str = "accepted-config-inert";
    pub const CONFIDENCE_PROVENANCE_LOST: &str = "confidence-provenance-lost";
    pub const TELEMETRY_NOT_BOUND_TO_CLAIM: &str = "telemetry-not-bound-to-claim";
    pub const TEST_ORACLE_LAG: &str = "test-oracle-lag";
    pub const HOT_PATH_WORK_DUPLICATED: &str = "hot-path-work-duplicated";
    pub const OPERATIONAL_IDENTITY_STALE: &str = "operational-identity-stale";
    pub const ADOPTION_CHAIN_INCOMPLETE: &str = "adoption-chain-incomplete";

    /// Every refactor-defect detector family, including the summary rule.
    pub const ALL: &[&str] = &[
        OWNER_MOVED_CONSUMER_STALE,
        SCOPE_COLLAPSE_AFTER_REFACTOR,
        MECHANISM_LIVE_GATE_RETIRED,
        PRODUCER_VERIFIER_SCHEMA_DRIFT,
        ACCEPTED_CONFIG_INERT,
        CONFIDENCE_PROVENANCE_LOST,
        TELEMETRY_NOT_BOUND_TO_CLAIM,
        TEST_ORACLE_LAG,
        HOT_PATH_WORK_DUPLICATED,
        OPERATIONAL_IDENTITY_STALE,
        ADOPTION_CHAIN_INCOMPLETE,
    ];
}

kebab_data_enum! {
// Language-neutral node roles in the contract graph. Adapters map
// grammar-specific node kinds into these roles with distinct provenance.
pub enum ContractRole {
    Owner,
    ConfigParameter,
    Producer,
    Consumer,
    PersistenceSurface,
    Verifier,
    TestEntryPoint,
    Assertion,
    TelemetrySurface,
    RuntimeIdentity,
}
}

kebab_data_enum! {
// Relationships between contract nodes, described semantically rather than
// as language syntax.
pub enum ContractEdgeKind {
    Declares,
    Configures,
    Reads,
    Governs,
    Produces,
    Transforms,
    Consumes,
    Persists,
    Reloads,
    Verifies,
    Exercises,
    Asserts,
    Observes,
    Publishes,
}
}

kebab_data_enum! {
// Who attested a fact. Provider results keep independent authority:
// disagreements are surfaced, never merged away.
pub enum FactProvider {
    TreeSitter,
    Lsp,
    SemanticTokens,
    VcsHistory,
    Runtime,
    DomainSpecific,
}
}

kebab_data_enum! {
// How much of its contract a provider could attest for one fact or adapter.
pub enum CapabilityLevel {
    Complete,
    Partial,
    Unknown,
}
}

kebab_data_enum! {
// How an entity in one revision was matched to an entity in another,
// strongest evidence first.
pub enum EntityMatchKind {
    StableSymbol,
    PathSpan,
    StructuralFingerprint,
    RenameEvidence,
    NeighborhoodSimilarity,
}
}

/// One file in a revision snapshot: exact source bytes plus their digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionFile {
    pub path: PathBuf,
    /// Hex digest of the exact source bytes in `contents`.
    pub digest: String,
    /// Exact source bytes (UTF-8).
    pub contents: String,
}

impl RevisionFile {
    /// Capture a file, computing the digest from the exact bytes given.
    pub fn new(path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        let contents = contents.into();
        let digest = blake3::hash(contents.as_bytes()).to_hex().to_string();
        Self {
            path: path.into(),
            digest,
            contents,
        }
    }

    /// Whether `digest` still matches `contents`.
    pub fn digest_matches(&self) -> bool {
        blake3::hash(self.contents.as_bytes()).to_hex().as_str() == self.digest
    }
}

/// A provider-supplied artifact bound to the exact revision it analyzed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderArtifact {
    pub provider: FactProvider,
    /// Revision this artifact was computed from; must match the owning snapshot.
    pub revision: String,
    pub capability: CapabilityLevel,
    /// Provider-defined payload (for example serialized LSP results).
    pub payload: String,
}

/// One immutable revision in the analysis window, built from exact source
/// bytes. Timestamps appear only when the history provider knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionSnapshot {
    /// Provider-specific revision identity (commit id, change id, editor
    /// snapshot id, ...). The analyzer does not interpret it.
    pub revision: String,
    pub parents: Vec<String>,
    /// Present only when known; never guessed.
    pub timestamp: Option<String>,
    pub files: Vec<RevisionFile>,
    pub provider_artifacts: Vec<ProviderArtifact>,
}

/// An ordered revision window (`deslop.refactor-history/1`). Git, Jujutsu,
/// editor-local history, or an external review system can all produce this
/// bundle; no repository model is embedded in the analyzer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefactorHistoryBundle {
    pub schema: String,
    /// Oldest first; adjacent pairs are compared in order.
    pub revisions: Vec<RevisionSnapshot>,
}

impl RefactorHistoryBundle {
    pub fn new(revisions: Vec<RevisionSnapshot>) -> Self {
        Self {
            schema: REFACTOR_HISTORY_SCHEMA.to_string(),
            revisions,
        }
    }

    /// Structural integrity of the bundle: correct schema, at least one
    /// revision, matching digests, and provider artifacts pinned to the
    /// revision that carries them.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REFACTOR_HISTORY_SCHEMA {
            return Err(format!(
                "expected schema `{REFACTOR_HISTORY_SCHEMA}`, got `{}`",
                self.schema
            ));
        }
        if self.revisions.is_empty() {
            return Err("history bundle contains no revisions".to_string());
        }
        for snapshot in &self.revisions {
            for file in &snapshot.files {
                if !file.digest_matches() {
                    return Err(format!(
                        "digest mismatch for {} at revision {}",
                        file.path.display(),
                        snapshot.revision
                    ));
                }
            }
            for artifact in &snapshot.provider_artifacts {
                if artifact.revision != snapshot.revision {
                    return Err(format!(
                        "provider artifact for {:?} is pinned to revision {} but carried by {}",
                        artifact.provider, artifact.revision, snapshot.revision
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Reference to one contract node at one revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractNodeRef {
    pub role: ContractRole,
    pub path: PathBuf,
    pub span: Span,
    /// Structural fingerprint of the node within its revision.
    pub fingerprint: String,
    pub provider: FactProvider,
    pub capability: CapabilityLevel,
}

/// How a before/after entity match was established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMatchEvidence {
    pub kind: EntityMatchKind,
    pub detail: String,
}

/// The before/after owner change a finding is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerMigration {
    pub before: ContractNodeRef,
    pub after: ContractNodeRef,
    pub match_evidence: Vec<EntityMatchEvidence>,
}

/// One contract edge with the node it reaches. Used both for stale edges
/// (downstream edges still attached to the former owner) and for the steps of
/// the auditable causal path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractStep {
    pub edge: ContractEdgeKind,
    pub node: ContractNodeRef,
    /// The contract token evidencing this step (reference, schema field,
    /// config key, or call text), when one token identifies it. Typed so
    /// review tooling and provider-fact joins never parse `detail`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub detail: String,
}

/// One positive-evidence or counter-evidence item. Counter-evidence stays
/// visible and suppresses or downgrades promotion; it is never deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceItem {
    pub provider: FactProvider,
    pub detail: String,
    pub node: Option<ContractNodeRef>,
}

/// A fact the analysis could not establish. Gaps are explicit output, never a
/// clean result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageGap {
    pub provider: FactProvider,
    pub capability: CapabilityLevel,
    pub reason: String,
}

/// The before/after revisions a finding compares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionPair {
    pub before: String,
    pub after: String,
}

/// How long the stale state has survived, as triage input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persistence {
    /// Revisions the stale edge has survived.
    pub revisions: u64,
    /// Edits that touched the new owner and the stale consumer independently.
    pub independent_edits: u64,
}

/// One refactor-defect review finding (`deslop.refactor-defect/1`). This is
/// the full review payload; on the scan path the same finding travels as a
/// registry-named `Finding` with the causal-path summary in `message` and the
/// suggested verification in `suggestion`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefactorDefect {
    pub schema: String,
    /// Detector-family rule name; must be one of `rule_names::ALL`.
    pub rule: String,
    pub revisions: RevisionPair,
    pub owner: Option<OwnerMigration>,
    pub stale_edges: Vec<ContractStep>,
    pub causal_path: Vec<ContractStep>,
    pub evidence: Vec<EvidenceItem>,
    pub counter_evidence: Vec<EvidenceItem>,
    pub coverage_gaps: Vec<CoverageGap>,
    pub persistence: Persistence,
    /// Transparent triage inputs (owner-change evidence, stale edge count,
    /// missing oracle, persistence, independent churn, boundary distance).
    /// Triage only — never confidence and never fix safety.
    pub priority_inputs: BTreeMap<String, i64>,
    /// Always [`SafetyClass::NeverAuto`]: this analysis diagnoses; it never
    /// rewrites.
    pub safety: SafetyClass,
    pub suggested_verification: String,
}

impl RefactorDefect {
    /// The invariants every emitted refactor-defect finding must satisfy.
    /// A finding without a reviewable causal path or a suggested
    /// verification is not honest review output (acceptance gate 7).
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REFACTOR_DEFECT_SCHEMA {
            return Err(format!(
                "expected schema `{REFACTOR_DEFECT_SCHEMA}`, got `{}`",
                self.schema
            ));
        }
        if !rule_names::ALL.contains(&self.rule.as_str()) {
            return Err(format!(
                "unknown refactor-defect rule `{}` (known: {})",
                self.rule,
                rule_names::ALL.join(", ")
            ));
        }
        if self.safety != SafetyClass::NeverAuto {
            return Err(format!(
                "refactor-defect findings are review-only; rule `{}` carried safety {:?}",
                self.rule, self.safety
            ));
        }
        if self.causal_path.is_empty() {
            return Err(format!(
                "rule `{}` finding carries no causal path; findings must be reviewable",
                self.rule
            ));
        }
        if self.suggested_verification.trim().is_empty() {
            return Err(format!(
                "rule `{}` finding carries no suggested verification",
                self.rule
            ));
        }
        Ok(())
    }

    /// History-aware finding identity (acceptance gate 10): a BLAKE3 digest
    /// over the rule, the owner identity, and the causal-path structure —
    /// content-bound fingerprints and paths, never revision labels, spans,
    /// or window size. The same defect detected through a different history
    /// window keeps the same identity, so baselines neither churn falsely
    /// nor silently accept a changed defect. This is deliberately not the
    /// scan-path `baseline_fingerprint`, which is a reporting-suppression
    /// fingerprint over path/rule/span/text.
    pub fn stable_identity(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"deslop refactor-defect identity v1");
        let mut part = |bytes: &[u8]| {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        };
        part(self.rule.as_bytes());
        match &self.owner {
            Some(owner) => {
                part(owner.before.path.to_string_lossy().as_bytes());
                part(owner.before.fingerprint.as_bytes());
                part(owner.after.path.to_string_lossy().as_bytes());
                part(owner.after.fingerprint.as_bytes());
            }
            None => part(b"no-owner"),
        }
        for step in self.stale_edges.iter().chain(&self.causal_path) {
            part(format!("{:?}", step.edge).as_bytes());
            part(step.node.path.to_string_lossy().as_bytes());
            part(step.node.fingerprint.as_bytes());
        }
        format!("rdf1_{}", hasher.finalize().to_hex())
    }

    /// Project this finding into the registry-named scan-path [`Finding`]
    /// form: the causal-path summary travels in `message`, the suggested
    /// verification in `suggestion`, and the history-aware identity in
    /// `fingerprint`. `NeverAuto` safety and the absent edit keep the
    /// existing report, SARIF, and LSP surfaces review-only with no format
    /// changes.
    pub fn to_finding(&self) -> crate::Finding {
        let location = self
            .stale_edges
            .first()
            .map(|step| &step.node)
            .or_else(|| self.owner.as_ref().map(|owner| &owner.after))
            .or_else(|| self.causal_path.first().map(|step| &step.node));
        let (path, span) = match location {
            Some(node) => (node.path.clone(), node.span),
            None => (PathBuf::new(), Span::new(1, 1, 0, 0)),
        };
        let mut message = format!(
            "[{} -> {}] {}",
            self.revisions.before,
            self.revisions.after,
            self.causal_path
                .iter()
                .map(|step| step.detail.as_str())
                .collect::<Vec<_>>()
                .join(" -> ")
        );
        // Disagreement and capability gaps stay visible in every output
        // format, including the compact scan-path form.
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
            detected_by: crate::DetectedBy::RefactorHistory,
            message,
            suggestion: self.suggested_verification.clone(),
            precondition: None,
            edit: None,
            fingerprint: self.stable_identity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(role: ContractRole) -> ContractNodeRef {
        ContractNodeRef {
            role,
            path: PathBuf::from("src/lib.py"),
            span: Span::new(10, 12, 200, 260),
            fingerprint: "fp:abc".to_string(),
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Complete,
        }
    }

    fn defect() -> RefactorDefect {
        RefactorDefect {
            schema: REFACTOR_DEFECT_SCHEMA.to_string(),
            rule: rule_names::OWNER_MOVED_CONSUMER_STALE.to_string(),
            revisions: RevisionPair {
                before: "rev-a".to_string(),
                after: "rev-b".to_string(),
            },
            owner: Some(OwnerMigration {
                before: node(ContractRole::Owner),
                after: node(ContractRole::Owner),
                match_evidence: vec![EntityMatchEvidence {
                    kind: EntityMatchKind::RenameEvidence,
                    detail: "symbol renamed in same edit".to_string(),
                }],
            }),
            stale_edges: vec![ContractStep {
                token: None,
                edge: ContractEdgeKind::Consumes,
                node: node(ContractRole::Consumer),
                detail: "score reconstructed from committed logits".to_string(),
            }],
            causal_path: vec![ContractStep {
                token: None,
                edge: ContractEdgeKind::Produces,
                node: node(ContractRole::Producer),
                detail: "commit-time decision reads new owner".to_string(),
            }],
            evidence: vec![EvidenceItem {
                provider: FactProvider::TreeSitter,
                detail: "new owner read on decision path".to_string(),
                node: Some(node(ContractRole::Producer)),
            }],
            counter_evidence: Vec::new(),
            coverage_gaps: vec![CoverageGap {
                provider: FactProvider::Lsp,
                capability: CapabilityLevel::Unknown,
                reason: "no server available for revision".to_string(),
            }],
            persistence: Persistence {
                revisions: 3,
                independent_edits: 2,
            },
            priority_inputs: BTreeMap::from([("stale-edges".to_string(), 1)]),
            safety: SafetyClass::NeverAuto,
            suggested_verification: "compare public score against commit-time posterior"
                .to_string(),
        }
    }

    #[test]
    fn schema_constants_match_house_convention() {
        assert_eq!(REFACTOR_HISTORY_SCHEMA, "deslop.refactor-history/1");
        assert_eq!(REFACTOR_DEFECT_SCHEMA, "deslop.refactor-defect/1");
    }

    #[test]
    fn defect_roundtrips_and_validates() {
        let defect = defect();
        defect.validate().unwrap();
        let json = serde_json::to_string(&defect).unwrap();
        let back: RefactorDefect = serde_json::from_str(&json).unwrap();
        assert_eq!(defect, back);
    }

    #[test]
    fn defect_rejects_non_never_auto_safety() {
        let mut defect = defect();
        defect.safety = SafetyClass::RiskySuggest;
        assert!(defect.validate().unwrap_err().contains("review-only"));
    }

    #[test]
    fn defect_rejects_unknown_rule() {
        let mut defect = defect();
        defect.rule = "magic-number".to_string();
        assert!(defect.validate().unwrap_err().contains("unknown"));
    }

    #[test]
    fn history_bundle_roundtrips_and_validates() {
        let bundle = RefactorHistoryBundle::new(vec![
            RevisionSnapshot {
                revision: "rev-a".to_string(),
                parents: vec![],
                timestamp: None,
                files: vec![RevisionFile::new("src/lib.py", "def f():\n    return 1\n")],
                provider_artifacts: vec![],
            },
            RevisionSnapshot {
                revision: "rev-b".to_string(),
                parents: vec!["rev-a".to_string()],
                timestamp: Some("2026-07-21T00:00:00Z".to_string()),
                files: vec![RevisionFile::new("src/lib.py", "def f():\n    return 2\n")],
                provider_artifacts: vec![ProviderArtifact {
                    provider: FactProvider::Lsp,
                    revision: "rev-b".to_string(),
                    capability: CapabilityLevel::Partial,
                    payload: "{}".to_string(),
                }],
            },
        ]);
        bundle.validate().unwrap();
        let json = serde_json::to_string(&bundle).unwrap();
        let back: RefactorHistoryBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(bundle, back);
    }

    #[test]
    fn bundle_rejects_digest_mismatch() {
        let mut file = RevisionFile::new("src/lib.py", "x = 1\n");
        file.contents = "x = 2\n".to_string();
        let bundle = RefactorHistoryBundle::new(vec![RevisionSnapshot {
            revision: "rev-a".to_string(),
            parents: vec![],
            timestamp: None,
            files: vec![file],
            provider_artifacts: vec![],
        }]);
        assert!(bundle.validate().unwrap_err().contains("digest mismatch"));
    }

    #[test]
    fn bundle_rejects_mispinned_provider_artifact() {
        let bundle = RefactorHistoryBundle::new(vec![RevisionSnapshot {
            revision: "rev-a".to_string(),
            parents: vec![],
            timestamp: None,
            files: vec![],
            provider_artifacts: vec![ProviderArtifact {
                provider: FactProvider::Lsp,
                revision: "rev-other".to_string(),
                capability: CapabilityLevel::Complete,
                payload: "{}".to_string(),
            }],
        }]);
        assert!(bundle.validate().unwrap_err().contains("pinned"));
    }

    #[test]
    fn stable_identity_ignores_revision_labels_and_window_facts() {
        let base = defect();
        let mut other_window = defect();
        other_window.revisions = RevisionPair {
            before: "rev-x".to_string(),
            after: "rev-y".to_string(),
        };
        other_window.persistence = Persistence {
            revisions: 9,
            independent_edits: 4,
        };
        other_window.coverage_gaps.clear();
        assert_eq!(base.stable_identity(), other_window.stable_identity());
        assert!(base.stable_identity().starts_with("rdf1_"));
    }

    #[test]
    fn stable_identity_distinguishes_rule_and_owner() {
        let base = defect();
        let mut other_rule = defect();
        other_rule.rule = rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT.to_string();
        assert_ne!(base.stable_identity(), other_rule.stable_identity());
        let mut other_owner = defect();
        other_owner.owner.as_mut().unwrap().after.fingerprint = "fp:changed".to_string();
        assert_ne!(base.stable_identity(), other_owner.stable_identity());
    }

    #[test]
    fn to_finding_projects_review_only_scan_form() {
        let defect = defect();
        let finding = defect.to_finding();
        assert_eq!(finding.rule, defect.rule);
        assert_eq!(finding.safety, SafetyClass::NeverAuto);
        assert!(finding.edit.is_none());
        assert_eq!(finding.fingerprint, defect.stable_identity());
        assert!(finding.message.contains("rev-a -> rev-b"));
        assert!(finding.message.contains("commit-time decision"));
        assert_eq!(finding.suggestion, defect.suggested_verification);
        assert_eq!(finding.path, defect.stale_edges[0].node.path);
    }

    #[test]
    fn validate_requires_causal_path_and_verification() {
        let mut no_path = defect();
        no_path.causal_path.clear();
        assert!(no_path.validate().unwrap_err().contains("causal path"));
        let mut no_verification = defect();
        no_verification.suggested_verification = "  ".to_string();
        assert!(
            no_verification
                .validate()
                .unwrap_err()
                .contains("suggested verification")
        );
    }

    #[test]
    fn registry_covers_every_refactor_defect_rule() {
        for name in rule_names::ALL {
            assert!(
                crate::rules::is_known(name),
                "rule `{name}` missing from canonical registry"
            );
        }
    }

    #[test]
    fn rule_family_names_are_kebab_case() {
        for name in rule_names::ALL {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "rule name `{name}` is not kebab-case"
            );
        }
    }
}
