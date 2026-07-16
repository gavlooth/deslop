use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Mutex;

use anyhow::{Result, bail};
use deslop_core::{RevisionGuard, SafetyClass, Span};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    SHARED_WORK_ORDER_SCHEMA, SharedWorkOrder, SharedWorkOrderId, WORK_ORDER_HANDLE_SCHEMA,
    WORK_ORDER_PLAN_SCHEMA, WorkOrderAccess, WorkOrderEvidence, WorkOrderHandle, WorkOrderImpact,
    WorkOrderPatchBudget, WorkOrderPlan, WorkOrderPlanId, WorkOrderPlannerConstraints,
    WorkOrderProvenance, WorkOrderRecipe, WorkOrderResource, WorkOrderTarget, WorkOrderUnknown,
    WorkOrderVerification, plan_work_orders,
};

pub const WORK_ORDER_SERVICE_SCHEMA: &str = "deslop.work-order-service/1";
pub const WORK_ORDER_CURSOR_SCHEMA: &str = "deslop.work-order-cursor/1";
pub const WORK_ORDER_PATCH_PROPOSAL_SCHEMA: &str = "deslop.work-order-patch-proposal/1";
pub const WORK_ORDER_VERIFICATION_SCHEMA: &str = "deslop.work-order-verification/1";
pub const WORK_ORDER_APPLY_AUTHORIZATION_SCHEMA: &str = "deslop.work-order-apply-authorization/1";
pub const WORK_ORDER_APPLY_RECEIPT_SCHEMA: &str = "deslop.work-order-apply-receipt/1";

const SERVICE_ID_DOMAIN: &str = "deslop work order service v1";
const CURSOR_ID_DOMAIN: &str = "deslop work order cursor v1";
const PATCH_ID_DOMAIN: &str = "deslop work order patch proposal v1";
const VERIFICATION_ID_DOMAIN: &str = "deslop work order verification v1";
const AUTHORIZATION_ID_DOMAIN: &str = "deslop work order apply authorization v1";
const APPLY_RECEIPT_ID_DOMAIN: &str = "deslop work order apply receipt v1";

pub const MAX_QUERY_ITEMS: usize = 256;
pub const MAX_QUERY_EVIDENCE: usize = 512;
pub const MAX_QUERY_BYTES: usize = 1_048_576;

macro_rules! digest_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                validate_digest_id(&value, $prefix).map_err(serde::de::Error::custom)?;
                Ok(Self(value))
            }
        }
    };
}

digest_id!(WorkOrderServiceId, "wos1_");
digest_id!(WorkOrderCursorId, "woc1_");
digest_id!(WorkOrderPatchProposalId, "wpp1_");
digest_id!(WorkOrderVerificationId, "wvr1_");
digest_id!(WorkOrderApplyAuthorizationId, "waa1_");
digest_id!(WorkOrderApplyReceiptId, "war1_");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderServiceMetadata {
    pub capabilities: Vec<String>,
    pub parse_gaps: Vec<String>,
    pub architecture_summary: Vec<String>,
    pub cache_state: Vec<String>,
    pub provenance: Vec<String>,
    pub unknowns: Vec<WorkOrderUnknown>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderProtocolInput {
    pub orders: Vec<SharedWorkOrder>,
    pub constraints: WorkOrderPlannerConstraints,
    pub metadata: WorkOrderServiceMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderOperation {
    Index,
    Triage,
    Explain,
    Plan,
    ProposePatch,
    Verify,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryBudget {
    pub maximum_items: usize,
    pub maximum_evidence: usize,
    pub maximum_bytes: usize,
}

impl QueryBudget {
    pub fn validate(self) -> Result<()> {
        if self.maximum_items == 0
            || self.maximum_evidence == 0
            || self.maximum_bytes == 0
            || self.maximum_items > MAX_QUERY_ITEMS
            || self.maximum_evidence > MAX_QUERY_EVIDENCE
            || self.maximum_bytes > MAX_QUERY_BYTES
        {
            bail!("query budget is zero or exceeds a hard protocol ceiling");
        }
        Ok(())
    }
}

impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            maximum_items: 50,
            maximum_evidence: 100,
            maximum_bytes: 131_072,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaOffer {
    pub family: String,
    pub supported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaNegotiation {
    pub schema: String,
    pub selected: Vec<SchemaSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaSelection {
    pub family: String,
    pub schema: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationProvenance {
    pub service: WorkOrderServiceId,
    pub plan: WorkOrderPlanId,
    pub operation: WorkOrderOperation,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderIndexResponse {
    pub schema: String,
    pub total_orders: usize,
    pub blocked_groups: usize,
    pub schedule_waves: usize,
    pub capabilities: Vec<String>,
    pub parse_gaps: Vec<String>,
    pub architecture_summary: Vec<String>,
    pub cache_state: Vec<String>,
    pub provenance: OperationProvenance,
    pub unknowns: Vec<WorkOrderUnknown>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderCursor {
    pub schema: String,
    pub id: WorkOrderCursorId,
    pub service: WorkOrderServiceId,
    pub plan: WorkOrderPlanId,
    pub operation: WorkOrderOperation,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    pub cursor: Option<WorkOrderCursor>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageItem {
    pub order: SharedWorkOrderId,
    pub recipe: String,
    pub safety: SafetyClass,
    pub unknown_count: usize,
    pub blocked: bool,
    pub schedule_wave: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageResponse {
    pub schema: String,
    pub items: Vec<TriageItem>,
    pub next: Option<WorkOrderCursor>,
    pub total: usize,
    pub provenance: OperationProvenance,
    pub unknowns: Vec<WorkOrderUnknown>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplainResponse {
    pub schema: String,
    pub order: SharedWorkOrderId,
    pub target: WorkOrderTarget,
    pub recipe: WorkOrderRecipe,
    pub evidence: Vec<WorkOrderEvidence>,
    pub impact: WorkOrderImpact,
    pub safety: SafetyClass,
    pub patch_budget: WorkOrderPatchBudget,
    pub verification: WorkOrderVerification,
    pub access: WorkOrderAccess,
    pub subject_provenance: WorkOrderProvenance,
    pub provenance: OperationProvenance,
    pub unknowns: Vec<WorkOrderUnknown>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatchIntentKind {
    RecipeGrounded,
    LlmProposed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedWorkOrderEdit {
    pub path: std::path::PathBuf,
    pub span: Span,
    pub revision_guard: RevisionGuard,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchIntent {
    pub kind: PatchIntentKind,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderPatchProposal {
    pub schema: String,
    pub id: WorkOrderPatchProposalId,
    pub service: WorkOrderServiceId,
    pub handle: WorkOrderHandle,
    pub intent: PatchIntent,
    pub edits: Vec<ProposedWorkOrderEdit>,
    pub by: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationCheckStatus {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationObservation {
    pub key: String,
    pub status: VerificationCheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderVerificationStatus {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderVerificationReceipt {
    pub schema: String,
    pub id: WorkOrderVerificationId,
    pub service: WorkOrderServiceId,
    pub plan: WorkOrderPlanId,
    pub order: SharedWorkOrderId,
    pub patch: WorkOrderPatchProposalId,
    pub status: WorkOrderVerificationStatus,
    pub observations: Vec<VerificationObservation>,
    pub unknowns: Vec<WorkOrderUnknown>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyPolicy {
    pub maximum_safety: SafetyClass,
    pub allow_unknowns: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderApplyAuthorization {
    pub schema: String,
    pub id: WorkOrderApplyAuthorizationId,
    pub service: WorkOrderServiceId,
    pub plan: WorkOrderPlanId,
    pub order: SharedWorkOrderId,
    pub patch: WorkOrderPatchProposalId,
    pub verification: WorkOrderVerificationId,
    pub policy: ApplyPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyExecution {
    pub changed_resources: Vec<WorkOrderResource>,
    pub undo_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderApplyReceipt {
    pub schema: String,
    pub id: WorkOrderApplyReceiptId,
    pub authorization: WorkOrderApplyAuthorization,
    pub execution: ApplyExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "request", rename_all = "snake_case")]
pub enum WorkOrderProtocolRequest {
    Index,
    Triage {
        page: PageRequest,
        budget: QueryBudget,
    },
    Explain {
        handle: WorkOrderHandle,
        budget: QueryBudget,
    },
    Plan {
        budget: QueryBudget,
    },
    ProposePatch {
        handle: WorkOrderHandle,
        intent: PatchIntent,
        edits: Vec<ProposedWorkOrderEdit>,
        by: String,
    },
    Verify {
        patch: WorkOrderPatchProposal,
        observations: Vec<VerificationObservation>,
    },
    Apply {
        patch: WorkOrderPatchProposal,
        verification: WorkOrderVerificationReceipt,
        policy: ApplyPolicy,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "response", rename_all = "snake_case")]
pub enum WorkOrderProtocolResponse {
    Index(WorkOrderIndexResponse),
    Triage(TriageResponse),
    Explain(Box<ExplainResponse>),
    Plan(WorkOrderPlan),
    ProposePatch(WorkOrderPatchProposal),
    Verify(WorkOrderVerificationReceipt),
    Apply(WorkOrderApplyAuthorization),
}

#[derive(Default)]
pub struct ApplyLedger {
    receipts: Mutex<BTreeMap<WorkOrderApplyAuthorizationId, WorkOrderApplyReceipt>>,
}

#[derive(Debug, Clone)]
pub struct WorkOrderService {
    id: WorkOrderServiceId,
    plan: WorkOrderPlan,
    metadata: WorkOrderServiceMetadata,
}

impl WorkOrderService {
    pub fn new(
        orders: Vec<SharedWorkOrder>,
        constraints: WorkOrderPlannerConstraints,
        mut metadata: WorkOrderServiceMetadata,
    ) -> Result<Self> {
        canonicalize_strings(&mut metadata.capabilities, "capabilities")?;
        canonicalize_strings(&mut metadata.parse_gaps, "parse gaps")?;
        canonicalize_strings(&mut metadata.architecture_summary, "architecture summary")?;
        canonicalize_strings(&mut metadata.cache_state, "cache state")?;
        canonicalize_strings(&mut metadata.provenance, "provenance")?;
        metadata.unknowns.sort();
        metadata.unknowns.dedup();
        let plan = plan_work_orders(orders, constraints)?;
        let digest = crate::digest_json(SERVICE_ID_DOMAIN, &(plan.id(), &metadata))?;
        Ok(Self {
            id: WorkOrderServiceId(format!("wos1_{}", &digest[4..])),
            plan,
            metadata,
        })
    }

    pub fn id(&self) -> &WorkOrderServiceId {
        &self.id
    }

    pub fn from_input(input: WorkOrderProtocolInput) -> Result<Self> {
        Self::new(input.orders, input.constraints, input.metadata)
    }

    pub fn execute(&self, request: WorkOrderProtocolRequest) -> Result<WorkOrderProtocolResponse> {
        Ok(match request {
            WorkOrderProtocolRequest::Index => WorkOrderProtocolResponse::Index(self.index()),
            WorkOrderProtocolRequest::Triage { page, budget } => {
                WorkOrderProtocolResponse::Triage(self.triage(page, budget)?)
            }
            WorkOrderProtocolRequest::Explain { handle, budget } => {
                WorkOrderProtocolResponse::Explain(Box::new(self.explain(&handle, budget)?))
            }
            WorkOrderProtocolRequest::Plan { budget } => {
                WorkOrderProtocolResponse::Plan(self.plan(budget)?)
            }
            WorkOrderProtocolRequest::ProposePatch {
                handle,
                intent,
                edits,
                by,
            } => WorkOrderProtocolResponse::ProposePatch(
                self.propose_patch(handle, intent, edits, by)?,
            ),
            WorkOrderProtocolRequest::Verify {
                patch,
                observations,
            } => WorkOrderProtocolResponse::Verify(self.verify(&patch, observations)?),
            WorkOrderProtocolRequest::Apply {
                patch,
                verification,
                policy,
            } => WorkOrderProtocolResponse::Apply(self.authorize_apply(
                &patch,
                &verification,
                policy,
            )?),
        })
    }

    pub fn plan_value(&self) -> &WorkOrderPlan {
        &self.plan
    }

    pub fn negotiate(&self, mut offers: Vec<SchemaOffer>) -> Result<SchemaNegotiation> {
        for offer in &mut offers {
            validate_text("schema family", &offer.family)?;
            canonicalize_strings(&mut offer.supported, "schema offer")?;
        }
        offers.sort_by(|left, right| left.family.cmp(&right.family));
        if offers
            .windows(2)
            .any(|pair| pair[0].family == pair[1].family)
        {
            bail!("schema negotiation contains duplicate families");
        }
        let required = [
            ("service", WORK_ORDER_SERVICE_SCHEMA),
            ("work-order", SHARED_WORK_ORDER_SCHEMA),
            ("plan", WORK_ORDER_PLAN_SCHEMA),
            ("handle", WORK_ORDER_HANDLE_SCHEMA),
        ];
        let mut selected = Vec::new();
        for (family, schema) in required {
            let Some(offer) = offers.iter().find(|offer| offer.family == family) else {
                bail!("schema negotiation omitted required family `{family}`");
            };
            if !offer.supported.iter().any(|candidate| candidate == schema) {
                bail!("no supported schema for required family `{family}`");
            }
            selected.push(SchemaSelection {
                family: family.into(),
                schema: schema.into(),
            });
        }
        Ok(SchemaNegotiation {
            schema: "deslop.schema-negotiation/1".into(),
            selected,
        })
    }

    pub fn index(&self) -> WorkOrderIndexResponse {
        WorkOrderIndexResponse {
            schema: "deslop.work-order-index/1".into(),
            total_orders: self.plan.orders().len(),
            blocked_groups: self.plan.blocked().len(),
            schedule_waves: self.plan.waves().len(),
            capabilities: self.metadata.capabilities.clone(),
            parse_gaps: self.metadata.parse_gaps.clone(),
            architecture_summary: self.metadata.architecture_summary.clone(),
            cache_state: self.metadata.cache_state.clone(),
            provenance: self.provenance(
                WorkOrderOperation::Index,
                self.metadata.parse_gaps.is_empty(),
            ),
            unknowns: self.metadata.unknowns.clone(),
        }
    }

    pub fn triage(&self, request: PageRequest, budget: QueryBudget) -> Result<TriageResponse> {
        budget.validate()?;
        if request.limit == 0
            || request.limit > budget.maximum_items
            || request.limit > MAX_QUERY_ITEMS
        {
            bail!("triage page limit is zero or exceeds the query budget");
        }
        let offset = self.cursor_offset(request.cursor.as_ref(), WorkOrderOperation::Triage)?;
        let blocked_groups = self
            .plan
            .blocked()
            .iter()
            .map(|block| &block.group)
            .collect::<BTreeSet<_>>();
        let group_by_order = self
            .plan
            .groups()
            .iter()
            .flat_map(|group| group.members.iter().map(move |order| (order, &group.id)))
            .collect::<BTreeMap<_, _>>();
        let wave_by_group = self
            .plan
            .waves()
            .iter()
            .flat_map(|wave| wave.groups.iter().map(move |group| (group, wave.ordinal)))
            .collect::<BTreeMap<_, _>>();
        let mut all = self
            .plan
            .orders()
            .iter()
            .map(|order| {
                let group = group_by_order[order.id()];
                TriageItem {
                    order: order.id().clone(),
                    recipe: order.recipe().name.clone(),
                    safety: order.safety(),
                    unknown_count: order.unknowns().len(),
                    blocked: blocked_groups.contains(group),
                    schedule_wave: wave_by_group.get(group).copied(),
                }
            })
            .collect::<Vec<_>>();
        all.sort_by(|left, right| {
            (
                left.blocked,
                left.unknown_count,
                safety_rank(left.safety),
                &left.recipe,
                &left.order,
            )
                .cmp(&(
                    right.blocked,
                    right.unknown_count,
                    safety_rank(right.safety),
                    &right.recipe,
                    &right.order,
                ))
        });
        if offset > all.len() {
            bail!("triage cursor offset exceeds the result set");
        }
        let end = (offset + request.limit).min(all.len());
        let items = all[offset..end].to_vec();
        let next = (end < all.len())
            .then(|| self.cursor(WorkOrderOperation::Triage, end))
            .transpose()?;
        let response = TriageResponse {
            schema: "deslop.work-order-triage/1".into(),
            items,
            next,
            total: all.len(),
            provenance: self.provenance(WorkOrderOperation::Triage, true),
            unknowns: self.metadata.unknowns.clone(),
        };
        ensure_response_budget(&response, budget.maximum_bytes)?;
        Ok(response)
    }

    pub fn explain(
        &self,
        handle: &WorkOrderHandle,
        budget: QueryBudget,
    ) -> Result<ExplainResponse> {
        budget.validate()?;
        let order = self.order_for_handle(handle)?;
        let mut evidence = order.evidence().to_vec();
        let mut unknowns = order.unknowns().to_vec();
        if evidence.len() > budget.maximum_evidence {
            evidence.truncate(budget.maximum_evidence);
            unknowns.push(WorkOrderUnknown {
                key: "evidence-budget".into(),
                reason: "evidence was truncated by the declared query budget".into(),
            });
        }
        let mut response = ExplainResponse {
            schema: "deslop.work-order-explain/1".into(),
            order: order.id().clone(),
            target: order.target().clone(),
            recipe: order.recipe().clone(),
            evidence,
            impact: order.impact().clone(),
            safety: order.safety(),
            patch_budget: order.patch_budget().clone(),
            verification: order.verification().clone(),
            access: order.access().clone(),
            subject_provenance: order.provenance().clone(),
            provenance: self.provenance(WorkOrderOperation::Explain, true),
            unknowns,
        };
        while serialized_len(&response)? > budget.maximum_bytes && !response.evidence.is_empty() {
            response.evidence.pop();
            if !response
                .unknowns
                .iter()
                .any(|unknown| unknown.key == "context-budget")
            {
                response.unknowns.push(WorkOrderUnknown {
                    key: "context-budget".into(),
                    reason: "evidence was truncated to satisfy the byte budget".into(),
                });
            }
        }
        response.provenance.complete = !response
            .unknowns
            .iter()
            .any(|unknown| unknown.key.ends_with("budget"));
        ensure_response_budget(&response, budget.maximum_bytes)?;
        Ok(response)
    }

    pub fn plan(&self, budget: QueryBudget) -> Result<WorkOrderPlan> {
        budget.validate()?;
        if self.plan.orders().len() > budget.maximum_items {
            bail!("work-order plan exceeds the item budget; use triage pagination first");
        }
        ensure_response_budget(&self.plan, budget.maximum_bytes)?;
        Ok(self.plan.clone())
    }

    pub fn propose_patch(
        &self,
        handle: WorkOrderHandle,
        intent: PatchIntent,
        mut edits: Vec<ProposedWorkOrderEdit>,
        by: String,
    ) -> Result<WorkOrderPatchProposal> {
        let order = self.order_for_handle(&handle)?;
        validate_text("patch intent", &intent.summary)?;
        validate_text("patch author", &by)?;
        if edits.is_empty() || edits.len() > order.patch_budget().maximum_edits {
            bail!("patch edit count is empty or exceeds the work-order budget");
        }
        edits.sort_by(|left, right| {
            (&left.path, left.span.start_byte, left.span.end_byte).cmp(&(
                &right.path,
                right.span.start_byte,
                right.span.end_byte,
            ))
        });
        for edit in &edits {
            if edit.path != order.target().path
                || edit.span.start_byte < order.target().span.start_byte
                || edit.span.end_byte > order.target().span.end_byte
                || edit.span.start_byte >= edit.span.end_byte
            {
                bail!("patch edit is outside the pinned target identity");
            }
            if intent.kind == PatchIntentKind::LlmProposed
                && edit.revision_guard != order.target().revision_guard
            {
                bail!("LLM patch edit does not bind the pinned target revision");
            }
        }
        if edits.windows(2).any(|pair| {
            pair[0].path == pair[1].path && pair[1].span.start_byte < pair[0].span.end_byte
        }) {
            bail!("patch edits overlap");
        }
        let removed = edits.iter().map(|edit| edit.before.len()).sum::<usize>();
        let added = edits.iter().map(|edit| edit.after.len()).sum::<usize>();
        if removed > order.patch_budget().maximum_removed_bytes
            || added > order.patch_budget().maximum_added_bytes
        {
            bail!("patch byte counts exceed the work-order budget");
        }
        if intent.kind == PatchIntentKind::RecipeGrounded {
            let crate::WorkOrderSubject::Transformation { candidate } = order.subject() else {
                bail!("recipe-grounded patch requires a transformation subject");
            };
            let declared = candidate
                .edits()
                .iter()
                .map(|edit| ProposedWorkOrderEdit {
                    path: edit.target.file().path.clone(),
                    span: edit.span,
                    revision_guard: edit.revision_guard.clone(),
                    before: edit.before.clone(),
                    after: edit.after.clone(),
                })
                .collect::<Vec<_>>();
            if edits != declared {
                bail!("recipe-grounded patch differs from the candidate's exact edits");
            }
        }
        let payload = PatchPayload {
            service: &self.id,
            handle: &handle,
            intent: &intent,
            edits: &edits,
            by: &by,
        };
        let digest = crate::digest_json(PATCH_ID_DOMAIN, &payload)?;
        Ok(WorkOrderPatchProposal {
            schema: WORK_ORDER_PATCH_PROPOSAL_SCHEMA.into(),
            id: WorkOrderPatchProposalId(format!("wpp1_{}", &digest[4..])),
            service: self.id.clone(),
            handle,
            intent,
            edits,
            by,
        })
    }

    pub fn verify(
        &self,
        patch: &WorkOrderPatchProposal,
        mut observations: Vec<VerificationObservation>,
    ) -> Result<WorkOrderVerificationReceipt> {
        self.validate_patch(patch)?;
        observations.sort();
        if observations
            .windows(2)
            .any(|pair| pair[0].key == pair[1].key)
        {
            bail!("verification contains duplicate check keys");
        }
        for observation in &observations {
            validate_text("verification key", &observation.key)?;
            validate_text("verification detail", &observation.detail)?;
        }
        let order = self.order_for_handle(&patch.handle)?;
        let required = required_verification_keys(order.verification());
        let observed = observations
            .iter()
            .map(|observation| (observation.key.as_str(), observation.status))
            .collect::<BTreeMap<_, _>>();
        let mut unknowns = Vec::new();
        for key in &required {
            if !observed.contains_key(key.as_str()) {
                unknowns.push(WorkOrderUnknown {
                    key: key.clone(),
                    reason: "required verification observation is absent".into(),
                });
            }
        }
        let status = if observations
            .iter()
            .any(|observation| observation.status == VerificationCheckStatus::Failed)
        {
            WorkOrderVerificationStatus::Failed
        } else if !unknowns.is_empty()
            || observations
                .iter()
                .any(|observation| observation.status == VerificationCheckStatus::Unknown)
        {
            WorkOrderVerificationStatus::Unknown
        } else {
            WorkOrderVerificationStatus::Passed
        };
        let payload = VerificationPayload {
            service: &self.id,
            plan: self.plan.id(),
            order: order.id(),
            patch: &patch.id,
            status,
            observations: &observations,
            unknowns: &unknowns,
        };
        let digest = crate::digest_json(VERIFICATION_ID_DOMAIN, &payload)?;
        Ok(WorkOrderVerificationReceipt {
            schema: WORK_ORDER_VERIFICATION_SCHEMA.into(),
            id: WorkOrderVerificationId(format!("wvr1_{}", &digest[4..])),
            service: self.id.clone(),
            plan: self.plan.id().clone(),
            order: order.id().clone(),
            patch: patch.id.clone(),
            status,
            observations,
            unknowns,
        })
    }

    pub fn authorize_apply(
        &self,
        patch: &WorkOrderPatchProposal,
        verification: &WorkOrderVerificationReceipt,
        policy: ApplyPolicy,
    ) -> Result<WorkOrderApplyAuthorization> {
        self.validate_patch(patch)?;
        let rebuilt_verification = self.verify(patch, verification.observations.clone())?;
        if &rebuilt_verification != verification {
            bail!("verification receipt is stale or noncanonical");
        }
        if verification.schema != WORK_ORDER_VERIFICATION_SCHEMA
            || verification.service != self.id
            || verification.plan != *self.plan.id()
            || verification.order != patch.handle.order
            || verification.patch != patch.id
        {
            bail!("verification receipt does not bind the current patch and plan");
        }
        if verification.status != WorkOrderVerificationStatus::Passed
            || (!policy.allow_unknowns && !verification.unknowns.is_empty())
        {
            bail!("apply policy requires a passed verification with permitted unknowns");
        }
        let order = self.order_for_handle(&patch.handle)?;
        if safety_rank(order.safety()) > safety_rank(policy.maximum_safety) {
            bail!("work-order safety exceeds the apply policy");
        }
        let payload = AuthorizationPayload {
            service: &self.id,
            plan: self.plan.id(),
            order: order.id(),
            patch: &patch.id,
            verification: &verification.id,
            policy,
        };
        let digest = crate::digest_json(AUTHORIZATION_ID_DOMAIN, &payload)?;
        Ok(WorkOrderApplyAuthorization {
            schema: WORK_ORDER_APPLY_AUTHORIZATION_SCHEMA.into(),
            id: WorkOrderApplyAuthorizationId(format!("waa1_{}", &digest[4..])),
            service: self.id.clone(),
            plan: self.plan.id().clone(),
            order: order.id().clone(),
            patch: patch.id.clone(),
            verification: verification.id.clone(),
            policy,
        })
    }

    pub fn apply_with(
        &self,
        ledger: &ApplyLedger,
        patch: &WorkOrderPatchProposal,
        verification: &WorkOrderVerificationReceipt,
        policy: ApplyPolicy,
        execute: impl FnOnce(&WorkOrderPatchProposal) -> Result<ApplyExecution>,
    ) -> Result<WorkOrderApplyReceipt> {
        let authorization = self.authorize_apply(patch, verification, policy)?;
        let mut receipts = ledger
            .receipts
            .lock()
            .map_err(|_| anyhow::anyhow!("apply retry ledger is poisoned"))?;
        if let Some(receipt) = receipts.get(&authorization.id) {
            return Ok(receipt.clone());
        }
        let mut execution = execute(patch)?;
        validate_text("undo identity", &execution.undo_identity)?;
        execution.changed_resources.sort();
        execution.changed_resources.dedup();
        let digest = crate::digest_json(APPLY_RECEIPT_ID_DOMAIN, &(&authorization, &execution))?;
        let receipt = WorkOrderApplyReceipt {
            schema: WORK_ORDER_APPLY_RECEIPT_SCHEMA.into(),
            id: WorkOrderApplyReceiptId(format!("war1_{}", &digest[4..])),
            authorization,
            execution,
        };
        receipts.insert(receipt.authorization.id.clone(), receipt.clone());
        Ok(receipt)
    }

    fn provenance(&self, operation: WorkOrderOperation, complete: bool) -> OperationProvenance {
        OperationProvenance {
            service: self.id.clone(),
            plan: self.plan.id().clone(),
            operation,
            complete,
        }
    }

    fn cursor(&self, operation: WorkOrderOperation, offset: usize) -> Result<WorkOrderCursor> {
        let digest = crate::digest_json(
            CURSOR_ID_DOMAIN,
            &(&self.id, self.plan.id(), operation, offset),
        )?;
        Ok(WorkOrderCursor {
            schema: WORK_ORDER_CURSOR_SCHEMA.into(),
            id: WorkOrderCursorId(format!("woc1_{}", &digest[4..])),
            service: self.id.clone(),
            plan: self.plan.id().clone(),
            operation,
            offset,
        })
    }

    fn cursor_offset(
        &self,
        cursor: Option<&WorkOrderCursor>,
        operation: WorkOrderOperation,
    ) -> Result<usize> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        if cursor.schema != WORK_ORDER_CURSOR_SCHEMA
            || cursor.service != self.id
            || cursor.plan != *self.plan.id()
            || cursor.operation != operation
            || cursor.id != self.cursor(operation, cursor.offset)?.id
        {
            bail!("stale or foreign pagination cursor");
        }
        Ok(cursor.offset)
    }

    fn order_for_handle(&self, handle: &WorkOrderHandle) -> Result<&SharedWorkOrder> {
        if handle.schema != WORK_ORDER_HANDLE_SCHEMA || handle.plan != *self.plan.id() {
            bail!("stale or foreign work-order handle");
        }
        let order = self
            .plan
            .orders()
            .iter()
            .find(|order| order.id() == &handle.order)
            .ok_or_else(|| anyhow::anyhow!("work-order handle target is absent"))?;
        if handle.revision_guard != order.target().revision_guard {
            bail!("work-order handle revision is stale");
        }
        Ok(order)
    }

    fn validate_patch(&self, patch: &WorkOrderPatchProposal) -> Result<()> {
        if patch.schema != WORK_ORDER_PATCH_PROPOSAL_SCHEMA || patch.service != self.id {
            bail!("patch proposal belongs to a foreign service");
        }
        let rebuilt = self.propose_patch(
            patch.handle.clone(),
            patch.intent.clone(),
            patch.edits.clone(),
            patch.by.clone(),
        )?;
        if &rebuilt != patch {
            bail!("patch proposal is stale or noncanonical");
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct PatchPayload<'a> {
    service: &'a WorkOrderServiceId,
    handle: &'a WorkOrderHandle,
    intent: &'a PatchIntent,
    edits: &'a [ProposedWorkOrderEdit],
    by: &'a str,
}

#[derive(Serialize)]
struct VerificationPayload<'a> {
    service: &'a WorkOrderServiceId,
    plan: &'a WorkOrderPlanId,
    order: &'a SharedWorkOrderId,
    patch: &'a WorkOrderPatchProposalId,
    status: WorkOrderVerificationStatus,
    observations: &'a [VerificationObservation],
    unknowns: &'a [WorkOrderUnknown],
}

#[derive(Serialize)]
struct AuthorizationPayload<'a> {
    service: &'a WorkOrderServiceId,
    plan: &'a WorkOrderPlanId,
    order: &'a SharedWorkOrderId,
    patch: &'a WorkOrderPatchProposalId,
    verification: &'a WorkOrderVerificationId,
    policy: ApplyPolicy,
}

fn required_verification_keys(verification: &WorkOrderVerification) -> Vec<String> {
    match verification {
        WorkOrderVerification::FindingProposal { contract } => {
            let mut keys = vec![
                "parse".into(),
                "public-api".into(),
                "error-handling".into(),
                "growth".into(),
            ];
            if contract.check_cmd.is_some() {
                keys.push("check-command".into());
            }
            keys
        }
        WorkOrderVerification::Transformation { required_steps, .. } => required_steps
            .iter()
            .filter(|step| step.required)
            .map(|step| step.key.clone())
            .collect(),
    }
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

fn canonicalize_strings(values: &mut Vec<String>, label: &str) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("service {label} contains an empty value");
    }
    values.sort();
    values.dedup();
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value != value.trim() {
        bail!("{label} must be nonempty and trimmed");
    }
    Ok(())
}

fn serialized_len(value: &impl Serialize) -> Result<usize> {
    Ok(serde_json::to_vec(value)?.len())
}

fn ensure_response_budget(value: &impl Serialize, maximum_bytes: usize) -> Result<()> {
    let size = serialized_len(value)?;
    if size > maximum_bytes {
        bail!("protocol response requires {size} bytes, exceeding the {maximum_bytes}-byte budget");
    }
    Ok(())
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<()> {
    let digest = value
        .strip_prefix(prefix)
        .filter(|digest| digest.len() == 64)
        .ok_or_else(|| anyhow::anyhow!("invalid protocol identity"))?;
    if !digest
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("invalid protocol identity");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use deslop_recipes::detect_rust_recipes;
    use tempfile::tempdir;

    use super::*;
    use crate::{WorkOrderSubject, shared_transformation_work_orders};

    fn service() -> WorkOrderService {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("sample.rs"),
            "fn sample() { return; 7; }\nfn other() { return; 8; }\n",
        )
        .unwrap();
        let candidates = detect_rust_recipes(temp.path(), &[PathBuf::from("sample.rs")]).unwrap();
        WorkOrderService::new(
            shared_transformation_work_orders(candidates).unwrap(),
            Default::default(),
            WorkOrderServiceMetadata {
                capabilities: vec!["rust-s4".into()],
                parse_gaps: Vec::new(),
                architecture_summary: vec!["two callable owners".into()],
                cache_state: vec!["cold".into()],
                provenance: vec!["fixture".into()],
                unknowns: Vec::new(),
            },
        )
        .unwrap()
    }

    fn handle(service: &WorkOrderService) -> WorkOrderHandle {
        WorkOrderHandle::for_order(service.plan_value(), &service.plan_value().orders()[0]).unwrap()
    }

    fn recipe_patch(service: &WorkOrderService) -> WorkOrderPatchProposal {
        let order = &service.plan_value().orders()[0];
        let WorkOrderSubject::Transformation { candidate } = order.subject() else {
            panic!("candidate")
        };
        let edits = candidate
            .edits()
            .iter()
            .map(|edit| ProposedWorkOrderEdit {
                path: edit.target.file().path.clone(),
                span: edit.span,
                revision_guard: edit.revision_guard.clone(),
                before: edit.before.clone(),
                after: edit.after.clone(),
            })
            .collect();
        service
            .propose_patch(
                handle(service),
                PatchIntent {
                    kind: PatchIntentKind::RecipeGrounded,
                    summary: "apply exact recipe".into(),
                },
                edits,
                "test-client".into(),
            )
            .unwrap()
    }

    fn passing_verification(
        service: &WorkOrderService,
        patch: &WorkOrderPatchProposal,
    ) -> WorkOrderVerificationReceipt {
        let order = service.order_for_handle(&patch.handle).unwrap();
        let observations = required_verification_keys(order.verification())
            .into_iter()
            .map(|key| VerificationObservation {
                key,
                status: VerificationCheckStatus::Passed,
                detail: "fixture passed".into(),
            })
            .collect();
        service.verify(patch, observations).unwrap()
    }

    #[test]
    fn negotiation_index_pagination_and_context_budgets_are_explicit() {
        let service = service();
        let negotiation = service
            .negotiate(vec![
                SchemaOffer {
                    family: "service".into(),
                    supported: vec![WORK_ORDER_SERVICE_SCHEMA.into()],
                },
                SchemaOffer {
                    family: "work-order".into(),
                    supported: vec![SHARED_WORK_ORDER_SCHEMA.into()],
                },
                SchemaOffer {
                    family: "plan".into(),
                    supported: vec![WORK_ORDER_PLAN_SCHEMA.into()],
                },
                SchemaOffer {
                    family: "handle".into(),
                    supported: vec![WORK_ORDER_HANDLE_SCHEMA.into()],
                },
            ])
            .unwrap();
        assert_eq!(negotiation.selected.len(), 4);
        assert_eq!(service.index().total_orders, 2);

        let budget = QueryBudget {
            maximum_items: 1,
            maximum_evidence: 1,
            maximum_bytes: MAX_QUERY_BYTES,
        };
        let first = service
            .triage(
                PageRequest {
                    cursor: None,
                    limit: 1,
                },
                budget,
            )
            .unwrap();
        let second = service
            .triage(
                PageRequest {
                    cursor: first.next.clone(),
                    limit: 1,
                },
                budget,
            )
            .unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(second.items.len(), 1);
        assert_ne!(first.items[0].order, second.items[0].order);

        let explanation = service
            .explain(
                &handle(&service),
                QueryBudget {
                    maximum_items: 1,
                    maximum_evidence: 1,
                    maximum_bytes: MAX_QUERY_BYTES,
                },
            )
            .unwrap();
        assert!(explanation.evidence.len() <= 1);
        assert!(!explanation.provenance.complete);
        assert!(
            QueryBudget {
                maximum_items: MAX_QUERY_ITEMS + 1,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn stale_handles_and_overlapping_or_out_of_scope_edits_fail_closed() {
        let service = service();
        let mut stale = handle(&service);
        stale.revision_guard = RevisionGuard::from("stale");
        assert!(service.explain(&stale, Default::default()).is_err());

        let order = &service.plan_value().orders()[0];
        let edit = ProposedWorkOrderEdit {
            path: order.target().path.clone(),
            span: order.target().span,
            revision_guard: order.target().revision_guard.clone(),
            before: "x".into(),
            after: "y".into(),
        };
        assert!(
            service
                .propose_patch(
                    handle(&service),
                    PatchIntent {
                        kind: PatchIntentKind::LlmProposed,
                        summary: "overlap".into()
                    },
                    vec![edit.clone(), edit],
                    "client".into(),
                )
                .is_err()
        );
    }

    #[test]
    fn concurrent_clients_are_deterministic_and_apply_retries_execute_once() {
        let service = Arc::new(service());
        let patch = recipe_patch(&service);
        let verification = passing_verification(&service, &patch);
        let ledger = Arc::new(ApplyLedger::default());
        let executions = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let service = Arc::clone(&service);
            let ledger = Arc::clone(&ledger);
            let executions = Arc::clone(&executions);
            let patch = patch.clone();
            let verification = verification.clone();
            threads.push(thread::spawn(move || {
                let triage = service
                    .triage(
                        PageRequest {
                            cursor: None,
                            limit: 2,
                        },
                        Default::default(),
                    )
                    .unwrap();
                let receipt = service
                    .apply_with(
                        &ledger,
                        &patch,
                        &verification,
                        ApplyPolicy {
                            maximum_safety: SafetyClass::SafeAuto,
                            allow_unknowns: false,
                        },
                        |_| {
                            executions.fetch_add(1, Ordering::SeqCst);
                            Ok(ApplyExecution {
                                changed_resources: Vec::new(),
                                undo_identity: "undo-1".into(),
                            })
                        },
                    )
                    .unwrap();
                (triage.items, receipt.id)
            }));
        }
        let results = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(executions.load(Ordering::SeqCst), 1);
    }
}
