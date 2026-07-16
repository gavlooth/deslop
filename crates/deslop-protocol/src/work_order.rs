use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use anyhow::{Result, bail};
use deslop_core::{AnalysisStatus, RevisionGuard, SafetyClass, Severity, Span};
use deslop_parse::{GraphEvidenceLayer, NodeKey};
use deslop_recipes::{ImpactCone, ProofState, TransformationCandidate, ValidationStep};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    Contract, RecipeResourceKind, RecipeWorkOrder, WorkOrder as FindingWorkOrder,
    validate_workorder_identity,
};

pub const SHARED_WORK_ORDER_SCHEMA: &str = "deslop.work-order/1";
const SHARED_WORK_ORDER_ID_DOMAIN: &str = "deslop shared work order v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SharedWorkOrderId(String);

impl SharedWorkOrderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SharedWorkOrderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SharedWorkOrderId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest(&value, "wo1_").map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum WorkOrderSubject {
    FindingProposal {
        order: Box<FindingWorkOrder>,
    },
    Transformation {
        candidate: Box<TransformationCandidate>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderTarget {
    pub path: PathBuf,
    pub span: Span,
    pub revision_guard: RevisionGuard,
    pub node: Option<NodeKey>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderRecipe {
    pub identity: String,
    pub name: String,
    pub version: String,
    pub parameters: Vec<WorkOrderParameter>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderParameter {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderEvidenceKind {
    Required,
    CounterEvidence,
    Finding,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderEvidence {
    pub kind: WorkOrderEvidenceKind,
    pub key: String,
    pub state: ProofState,
    pub layer: Option<GraphEvidenceLayer>,
    pub entity: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum WorkOrderImpact {
    Region { path: PathBuf, span: Span },
    Graph { cone: ImpactCone },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderPatchBudget {
    pub maximum_files: usize,
    pub maximum_edits: usize,
    pub maximum_removed_bytes: usize,
    pub maximum_added_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum WorkOrderVerification {
    FindingProposal {
        contract: Contract,
    },
    Transformation {
        required_steps: Vec<ValidationStep>,
        protect_undeclared_bytes: bool,
        protect_undeclared_files: bool,
        protect_public_api: bool,
        rollback_checks: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderResourceKind {
    Candidate,
    Node,
    Symbol,
    File,
    Api,
    Build,
    Test,
    SourceBytes,
    SourceSpan,
    GraphProjection,
    ProjectSnapshot,
    ProposalContext,
    Recipe,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderResource {
    pub kind: WorkOrderResourceKind,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderAccess {
    pub reads: Vec<WorkOrderResource>,
    pub writes: Vec<WorkOrderResource>,
    pub requires: Vec<WorkOrderResource>,
    pub invalidates: Vec<WorkOrderResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderProvenance {
    pub subject_schema: String,
    pub subject_identity: String,
    pub proposal_context: Option<String>,
    pub project_snapshot: Option<String>,
    pub analysis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderUnknown {
    pub key: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SharedWorkOrder {
    schema: String,
    id: SharedWorkOrderId,
    subject: WorkOrderSubject,
    target: WorkOrderTarget,
    recipe: WorkOrderRecipe,
    evidence: Vec<WorkOrderEvidence>,
    impact: WorkOrderImpact,
    safety: SafetyClass,
    patch_budget: WorkOrderPatchBudget,
    verification: WorkOrderVerification,
    access: WorkOrderAccess,
    provenance: WorkOrderProvenance,
    unknowns: Vec<WorkOrderUnknown>,
}

impl SharedWorkOrder {
    pub fn from_finding_order(order: FindingWorkOrder) -> Result<Self> {
        validate_workorder_identity(&order).map_err(anyhow::Error::msg)?;
        Self::from_subject(WorkOrderSubject::FindingProposal {
            order: Box::new(order),
        })
    }

    pub fn from_candidate(candidate: TransformationCandidate) -> Result<Self> {
        Self::from_subject(WorkOrderSubject::Transformation {
            candidate: Box::new(candidate),
        })
    }

    fn from_subject(subject: WorkOrderSubject) -> Result<Self> {
        let canonical = CanonicalWorkOrder::from_subject(&subject)?;
        let id = derive_id(&canonical)?;
        let order = Self {
            schema: SHARED_WORK_ORDER_SCHEMA.into(),
            id,
            subject,
            target: canonical.target,
            recipe: canonical.recipe,
            evidence: canonical.evidence,
            impact: canonical.impact,
            safety: canonical.safety,
            patch_budget: canonical.patch_budget,
            verification: canonical.verification,
            access: canonical.access,
            provenance: canonical.provenance,
            unknowns: canonical.unknowns,
        };
        order.validate()?;
        Ok(order)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &SharedWorkOrderId {
        &self.id
    }

    pub fn subject(&self) -> &WorkOrderSubject {
        &self.subject
    }

    pub fn target(&self) -> &WorkOrderTarget {
        &self.target
    }

    pub fn recipe(&self) -> &WorkOrderRecipe {
        &self.recipe
    }

    pub fn evidence(&self) -> &[WorkOrderEvidence] {
        &self.evidence
    }

    pub fn impact(&self) -> &WorkOrderImpact {
        &self.impact
    }

    pub fn safety(&self) -> SafetyClass {
        self.safety
    }

    pub fn patch_budget(&self) -> &WorkOrderPatchBudget {
        &self.patch_budget
    }

    pub fn verification(&self) -> &WorkOrderVerification {
        &self.verification
    }

    pub fn access(&self) -> &WorkOrderAccess {
        &self.access
    }

    pub fn provenance(&self) -> &WorkOrderProvenance {
        &self.provenance
    }

    pub fn unknowns(&self) -> &[WorkOrderUnknown] {
        &self.unknowns
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SHARED_WORK_ORDER_SCHEMA {
            bail!("unsupported shared work-order schema `{}`", self.schema);
        }
        let canonical = CanonicalWorkOrder::from_subject(&self.subject)?;
        if self.target != canonical.target
            || self.recipe != canonical.recipe
            || self.evidence != canonical.evidence
            || self.impact != canonical.impact
            || self.safety != canonical.safety
            || self.patch_budget != canonical.patch_budget
            || self.verification != canonical.verification
            || self.access != canonical.access
            || self.provenance != canonical.provenance
            || self.unknowns != canonical.unknowns
        {
            bail!("shared work order diverges from its authoritative subject");
        }
        if self.id != derive_id(&canonical)? {
            bail!("shared work-order identity is stale");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedWorkOrderWire {
    schema: String,
    id: SharedWorkOrderId,
    subject: WorkOrderSubject,
    target: WorkOrderTarget,
    recipe: WorkOrderRecipe,
    evidence: Vec<WorkOrderEvidence>,
    impact: WorkOrderImpact,
    safety: SafetyClass,
    patch_budget: WorkOrderPatchBudget,
    verification: WorkOrderVerification,
    access: WorkOrderAccess,
    provenance: WorkOrderProvenance,
    unknowns: Vec<WorkOrderUnknown>,
}

impl<'de> Deserialize<'de> for SharedWorkOrder {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SharedWorkOrderWire::deserialize(deserializer)?;
        let order = Self {
            schema: wire.schema,
            id: wire.id,
            subject: wire.subject,
            target: wire.target,
            recipe: wire.recipe,
            evidence: wire.evidence,
            impact: wire.impact,
            safety: wire.safety,
            patch_budget: wire.patch_budget,
            verification: wire.verification,
            access: wire.access,
            provenance: wire.provenance,
            unknowns: wire.unknowns,
        };
        order.validate().map_err(serde::de::Error::custom)?;
        Ok(order)
    }
}

pub fn shared_finding_work_orders(
    orders: impl IntoIterator<Item = FindingWorkOrder>,
) -> Result<Vec<SharedWorkOrder>> {
    shared_work_order_batch(orders.into_iter().map(SharedWorkOrder::from_finding_order))
}

pub fn shared_transformation_work_orders(
    candidates: impl IntoIterator<Item = TransformationCandidate>,
) -> Result<Vec<SharedWorkOrder>> {
    shared_work_order_batch(candidates.into_iter().map(SharedWorkOrder::from_candidate))
}

fn shared_work_order_batch(
    orders: impl IntoIterator<Item = Result<SharedWorkOrder>>,
) -> Result<Vec<SharedWorkOrder>> {
    let mut orders = orders.into_iter().collect::<Result<Vec<_>>>()?;
    orders.sort_by(|left, right| left.id.cmp(&right.id));
    if orders.windows(2).any(|pair| pair[0].id == pair[1].id) {
        bail!("distinct subjects produced a duplicate shared work-order identity");
    }
    Ok(orders)
}

#[derive(Serialize)]
struct CanonicalWorkOrder {
    target: WorkOrderTarget,
    recipe: WorkOrderRecipe,
    evidence: Vec<WorkOrderEvidence>,
    impact: WorkOrderImpact,
    safety: SafetyClass,
    patch_budget: WorkOrderPatchBudget,
    verification: WorkOrderVerification,
    access: WorkOrderAccess,
    provenance: WorkOrderProvenance,
    unknowns: Vec<WorkOrderUnknown>,
}

impl CanonicalWorkOrder {
    fn from_subject(subject: &WorkOrderSubject) -> Result<Self> {
        match subject {
            WorkOrderSubject::FindingProposal { order } => Self::from_finding(order),
            WorkOrderSubject::Transformation { candidate } => Self::from_candidate(candidate),
        }
    }

    fn from_finding(order: &FindingWorkOrder) -> Result<Self> {
        validate_workorder_identity(order).map_err(anyhow::Error::msg)?;
        let span = Span::new(
            order.region.start_line,
            order.region.end_line,
            order.region.start_byte,
            order.region.end_byte,
        );
        let target = WorkOrderTarget {
            path: order.path.clone(),
            span,
            revision_guard: order.revision_guard.clone(),
            node: None,
            fingerprint: order.region_fingerprint.clone(),
        };
        let recipe = WorkOrderRecipe {
            identity: "implicit:rewrite-region/v1".into(),
            name: "rewrite-region".into(),
            version: "1".into(),
            parameters: vec![WorkOrderParameter {
                name: "kind".into(),
                value: serde_json::to_value(order.kind)?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            }],
        };
        let mut evidence = order
            .findings
            .iter()
            .map(|finding| WorkOrderEvidence {
                kind: WorkOrderEvidenceKind::Finding,
                key: finding.rule.clone(),
                state: ProofState::Proven,
                layer: None,
                entity: format!("severity:{}", severity_name(finding.severity)),
                detail: finding.message.clone(),
            })
            .collect::<Vec<_>>();
        evidence.sort();
        let safety = order
            .findings
            .iter()
            .map(|finding| finding.safety)
            .max_by_key(|safety| safety_rank(*safety))
            .unwrap_or(SafetyClass::NeverAuto);
        let maximum_added_bytes =
            ((order.region.text.len() as f64) * f64::from(order.contract.max_growth_ratio)).ceil();
        let patch_budget = WorkOrderPatchBudget {
            maximum_files: 1,
            maximum_edits: 1,
            maximum_removed_bytes: order.region.text.len(),
            maximum_added_bytes: maximum_added_bytes.min(usize::MAX as f64) as usize,
        };
        let verification = WorkOrderVerification::FindingProposal {
            contract: order.contract.clone(),
        };
        let access = canonical_access(
            vec![
                resource(WorkOrderResourceKind::File, &order.path.to_string_lossy()),
                resource(
                    WorkOrderResourceKind::SourceBytes,
                    order.revision_guard.as_str(),
                ),
                resource(
                    WorkOrderResourceKind::ProposalContext,
                    &order.proposal_context.context_id,
                ),
            ],
            vec![resource(
                WorkOrderResourceKind::SourceSpan,
                &span_identity(&order.path, span, &order.revision_guard),
            )],
            vec![
                resource(WorkOrderResourceKind::Recipe, "implicit:rewrite-region/v1"),
                resource(
                    WorkOrderResourceKind::ProposalContext,
                    &order.proposal_context.context_id,
                ),
            ],
            vec![
                resource(WorkOrderResourceKind::File, &order.path.to_string_lossy()),
                resource(
                    WorkOrderResourceKind::ProposalContext,
                    &order.proposal_context.context_id,
                ),
            ],
        )?;
        let unknowns = order
            .proposal_context
            .sources
            .iter()
            .filter_map(
                |source| match source.analysis.as_ref().map(|analysis| analysis.status) {
                    Some(AnalysisStatus::Complete) => None,
                    Some(status) => Some(WorkOrderUnknown {
                        key: format!("analysis:{}", source.path.display()),
                        reason: format!("source analysis is {status:?}").to_lowercase(),
                    }),
                    None => Some(WorkOrderUnknown {
                        key: format!("analysis:{}", source.path.display()),
                        reason: "source analysis provenance is absent".into(),
                    }),
                },
            )
            .collect();
        Ok(Self {
            target,
            recipe,
            evidence,
            impact: WorkOrderImpact::Region {
                path: order.path.clone(),
                span,
            },
            safety,
            patch_budget,
            verification,
            access,
            provenance: WorkOrderProvenance {
                subject_schema: order.schema.clone(),
                subject_identity: order.id.clone(),
                proposal_context: Some(order.proposal_context.context_id.clone()),
                project_snapshot: None,
                analysis: None,
            },
            unknowns,
        })
    }

    fn from_candidate(candidate: &TransformationCandidate) -> Result<Self> {
        let recipe_order = RecipeWorkOrder::from_candidate(candidate.clone())?;
        let first_edit = candidate
            .edits()
            .first()
            .ok_or_else(|| anyhow::anyhow!("transformation work order requires an edit"))?;
        let target = WorkOrderTarget {
            path: candidate.target().node.file().path.clone(),
            span: first_edit.span,
            revision_guard: first_edit.revision_guard.clone(),
            node: Some(candidate.target().node.clone()),
            fingerprint: candidate
                .target()
                .subtree_fingerprint
                .as_ref()
                .map(|fingerprint| fingerprint.exact().as_str().to_string())
                .unwrap_or_else(|| format!("node:{:?}", candidate.target().node)),
        };
        let recipe = WorkOrderRecipe {
            identity: candidate.recipe().id().as_str().into(),
            name: candidate.recipe().name().into(),
            version: candidate.recipe().version().into(),
            parameters: Vec::new(),
        };
        let required_layers = candidate
            .recipe()
            .required_conditions()
            .iter()
            .map(|condition| (condition.key.as_str(), condition.layer))
            .collect::<std::collections::BTreeMap<_, _>>();
        let forbidden_layers = candidate
            .recipe()
            .forbidden_conditions()
            .iter()
            .map(|condition| (condition.key.as_str(), condition.layer))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut evidence = Vec::new();
        for result in candidate.required_results() {
            append_condition_evidence(
                &mut evidence,
                WorkOrderEvidenceKind::Required,
                result.condition.as_str(),
                result.state,
                required_layers.get(result.condition.as_str()).copied(),
                &result.evidence,
            );
        }
        for result in candidate.forbidden_results() {
            append_condition_evidence(
                &mut evidence,
                WorkOrderEvidenceKind::CounterEvidence,
                result.condition.as_str(),
                result.state,
                forbidden_layers.get(result.condition.as_str()).copied(),
                &result.evidence,
            );
        }
        evidence.sort();
        let patch_budget = recipe_order.patch_budget();
        let verification = recipe_order.verification();
        let access = canonical_access(
            map_recipe_resources(recipe_order.reads()),
            map_recipe_resources(recipe_order.writes()),
            map_recipe_resources(recipe_order.requires()),
            map_recipe_resources(recipe_order.invalidates()),
        )?;
        let mut unknowns = candidate
            .required_results()
            .iter()
            .chain(candidate.forbidden_results())
            .filter(|result| result.state == ProofState::Unknown)
            .map(|result| WorkOrderUnknown {
                key: result.condition.clone(),
                reason: "recipe condition remains unknown".into(),
            })
            .collect::<Vec<_>>();
        unknowns.sort();
        Ok(Self {
            target,
            recipe,
            evidence,
            impact: WorkOrderImpact::Graph {
                cone: candidate.impact().clone(),
            },
            safety: candidate.safety(),
            patch_budget: WorkOrderPatchBudget {
                maximum_files: patch_budget.maximum_files,
                maximum_edits: patch_budget.maximum_edits,
                maximum_removed_bytes: patch_budget.maximum_removed_bytes,
                maximum_added_bytes: patch_budget.maximum_added_bytes,
            },
            verification: WorkOrderVerification::Transformation {
                required_steps: verification.required_steps.clone(),
                protect_undeclared_bytes: verification.protect_undeclared_bytes,
                protect_undeclared_files: verification.protect_undeclared_files,
                protect_public_api: verification.protect_public_api,
                rollback_checks: verification.rollback_checks.clone(),
            },
            access,
            provenance: WorkOrderProvenance {
                subject_schema: candidate.schema().into(),
                subject_identity: candidate.id().as_str().into(),
                proposal_context: None,
                project_snapshot: Some(candidate.source().project_snapshot.clone()),
                analysis: Some(candidate.source().analysis.clone()),
            },
            unknowns,
        })
    }
}

fn append_condition_evidence(
    output: &mut Vec<WorkOrderEvidence>,
    kind: WorkOrderEvidenceKind,
    key: &str,
    state: ProofState,
    layer: Option<GraphEvidenceLayer>,
    evidence: &[deslop_recipes::ConditionEvidence],
) {
    if evidence.is_empty() {
        output.push(WorkOrderEvidence {
            kind,
            key: key.into(),
            state,
            layer,
            entity: "none".into(),
            detail: "no retained evidence item".into(),
        });
    } else {
        output.extend(evidence.iter().map(|item| WorkOrderEvidence {
            kind,
            key: key.into(),
            state,
            layer,
            entity: format!("{:?}", item.entity),
            detail: item.detail.clone(),
        }));
    }
}

fn map_recipe_resources(resources: &[crate::RecipeResource]) -> Vec<WorkOrderResource> {
    resources
        .iter()
        .map(|item| WorkOrderResource {
            kind: match item.kind {
                RecipeResourceKind::Candidate => WorkOrderResourceKind::Candidate,
                RecipeResourceKind::SourceBytes => WorkOrderResourceKind::SourceBytes,
                RecipeResourceKind::GraphProjection => WorkOrderResourceKind::GraphProjection,
                RecipeResourceKind::SourceSpan => WorkOrderResourceKind::SourceSpan,
                RecipeResourceKind::ProjectSnapshot => WorkOrderResourceKind::ProjectSnapshot,
            },
            identity: item.identity.clone(),
        })
        .collect()
}

fn canonical_access(
    mut reads: Vec<WorkOrderResource>,
    mut writes: Vec<WorkOrderResource>,
    mut requires: Vec<WorkOrderResource>,
    mut invalidates: Vec<WorkOrderResource>,
) -> Result<WorkOrderAccess> {
    for resources in [&mut reads, &mut writes, &mut requires, &mut invalidates] {
        resources.sort();
        if resources.iter().any(|item| item.identity.trim().is_empty()) {
            bail!("work-order resources require nonempty identities");
        }
        if resources.iter().collect::<BTreeSet<_>>().len() != resources.len() {
            bail!("work-order access set contains duplicate resources");
        }
    }
    Ok(WorkOrderAccess {
        reads,
        writes,
        requires,
        invalidates,
    })
}

fn resource(kind: WorkOrderResourceKind, identity: &str) -> WorkOrderResource {
    WorkOrderResource {
        kind,
        identity: identity.into(),
    }
}

fn span_identity(path: &std::path::Path, span: Span, guard: &RevisionGuard) -> String {
    format!(
        "{}:{}..{}:{}",
        path.display(),
        span.start_byte,
        span.end_byte,
        guard
    )
}

fn safety_rank(safety: SafetyClass) -> u8 {
    match safety {
        SafetyClass::SafeAuto => 0,
        SafetyClass::AnalyzerConfirmed => 1,
        SafetyClass::SafeWithPrecondition => 2,
        SafetyClass::RiskySuggest => 3,
        SafetyClass::LlmOnly => 4,
        SafetyClass::NeverAuto => 5,
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "info",
        Severity::Minor => "minor",
        Severity::Major => "major",
    }
}

fn derive_id(canonical: &CanonicalWorkOrder) -> Result<SharedWorkOrderId> {
    let digest = crate::digest_json(SHARED_WORK_ORDER_ID_DOMAIN, canonical)?;
    Ok(SharedWorkOrderId(format!("wo1_{}", &digest[4..])))
}

fn validate_digest(value: &str, prefix: &str) -> Result<()> {
    let digest = value
        .strip_prefix(prefix)
        .filter(|digest| digest.len() == 64)
        .ok_or_else(|| anyhow::anyhow!("invalid shared work-order identity"))?;
    if !digest
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("invalid shared work-order identity");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use deslop_analyzer::AnalyzerConfig;
    use deslop_recipes::detect_rust_recipes;
    use serde_json::Value;
    use tempfile::tempdir;

    use super::*;
    fn finding_order() -> FindingWorkOrder {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sample.rs");
        fs::write(
            &path,
            "fn sample() { let value = 42; println!(\"{}\", value); }\n",
        )
        .unwrap();
        crate::propose_work_orders(
            temp.path(),
            &[PathBuf::from("sample.rs")],
            AnalyzerConfig::default(),
        )
        .unwrap()
        .work_orders
        .into_iter()
        .next()
        .expect("fixture must produce one proposal")
    }

    fn recipe_candidate() -> TransformationCandidate {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sample.rs");
        fs::write(&path, "fn sample() { return; 7; }\n").unwrap();
        detect_rust_recipes(temp.path(), &[PathBuf::from("sample.rs")])
            .unwrap()
            .into_iter()
            .next()
            .expect("fixture must produce one transformation candidate")
    }

    #[test]
    fn finding_and_transformation_use_one_strict_shared_schema() {
        let finding = SharedWorkOrder::from_finding_order(finding_order()).unwrap();
        let transformation = SharedWorkOrder::from_candidate(recipe_candidate()).unwrap();
        assert_eq!(finding.schema(), SHARED_WORK_ORDER_SCHEMA);
        assert_eq!(transformation.schema(), SHARED_WORK_ORDER_SCHEMA);
        assert_ne!(finding.id(), transformation.id());
        assert!(!finding.access().reads.is_empty());
        assert!(!transformation.access().writes.is_empty());
        assert!(!transformation.evidence().is_empty());

        for order in [finding, transformation] {
            let encoded = serde_json::to_value(&order).unwrap();
            let decoded: SharedWorkOrder = serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded, order);
        }
    }

    #[test]
    fn summary_safety_budget_access_and_identity_tampering_fail_closed() {
        let order = SharedWorkOrder::from_candidate(recipe_candidate()).unwrap();
        for field in ["safety", "patch_budget", "access", "provenance"] {
            let mut value = serde_json::to_value(&order).unwrap();
            match field {
                "safety" => value[field] = Value::String("never-auto".into()),
                "patch_budget" => value[field]["maximum_edits"] = Value::from(999),
                "access" => value[field]["writes"] = Value::Array(Vec::new()),
                "provenance" => value[field]["analysis"] = Value::String("pa1_forged".into()),
                _ => unreachable!(),
            }
            assert!(serde_json::from_value::<SharedWorkOrder>(value).is_err());
        }
        let mut stale = serde_json::to_value(order).unwrap();
        stale["id"] = Value::String(format!("wo1_{}", "0".repeat(64)));
        assert!(serde_json::from_value::<SharedWorkOrder>(stale).is_err());
    }

    #[test]
    fn shared_batches_are_deterministic_and_reject_duplicate_subjects() {
        let candidate = recipe_candidate();
        assert!(
            shared_transformation_work_orders([candidate.clone(), candidate])
                .unwrap_err()
                .to_string()
                .contains("duplicate shared work-order identity")
        );
        let first = finding_order();
        let mut second = finding_order();
        second.id = format!("{}_other", second.id);
        assert!(SharedWorkOrder::from_finding_order(second).is_err());
        let batch = shared_finding_work_orders([first]).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].schema(), SHARED_WORK_ORDER_SCHEMA);
    }
}
