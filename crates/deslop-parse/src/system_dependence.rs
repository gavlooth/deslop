use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, DataFlowAccess, DataFlowAccessKey,
    DataFlowAccessKind, DataFlowBoundary, DataFlowBoundaryKey, DataFlowBoundaryKind,
    DataFlowDefinition, DataFlowDefinitionKey, DataFlowGraph, FactCoverage, ProgramDependenceGraph,
    ProgramDependenceGraphKey, ProgramDependenceNodeKey, ProgramDependencePolicyId,
    ProgramDependenceProjection, ProjectionId, ResolutionEndpoint, ResolutionStatus, ScopeFactData,
    ScopeFactKey, ScopeKind,
};

pub const SYSTEM_DEPENDENCE_SCHEMA: &str = "deslop.system-dependence/1";
pub const SYSTEM_DEPENDENCE_POLICY_SCHEMA: &str = "deslop.system-dependence-policy/1";

const POLICY_DOMAIN: &str = "deslop system-dependence policy v1";
const SUMMARY_DOMAIN: &str = "deslop callable summary v1";
const GLOBAL_DOMAIN: &str = "deslop global summary v1";
const CALL_DOMAIN: &str = "deslop call-site summary v1";
const EDGE_DOMAIN: &str = "deslop system-dependence edge v1";
const GAP_DOMAIN: &str = "deslop system-dependence gap v1";

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

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                validate_digest(&value, $prefix).map_err(D::Error::custom)?;
                Ok(Self(value))
            }
        }
    };
}

digest_id!(SystemDependencePolicyId, "sdp1_");
digest_id!(CallableSummaryKey, "css1_");
digest_id!(GlobalSummaryKey, "gss1_");
digest_id!(CallSiteKey, "cst1_");
digest_id!(SystemDependenceEdgeKey, "sde1_");
digest_id!(SystemDependenceGapKey, "sdx1_");

impl SystemDependencePolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, SystemDependenceBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(SystemDependenceBuildError::Invalid(
                "system-dependence policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_id(POLICY_DOMAIN, "sdp1_", parts)))
    }
}

#[derive(Debug, Clone)]
pub struct GlobalSummaryDraft {
    pub declaration: ScopeFactKey,
    pub reads: Vec<DataFlowAccessKey>,
    pub writes: Vec<DataFlowDefinitionKey>,
    pub mutation_outputs: Vec<DataFlowBoundaryKey>,
}

#[derive(Debug, Clone)]
pub struct CallableSummaryDraft {
    pub program_dependence_graph: ProgramDependenceGraphKey,
    pub formal_inputs: Vec<DataFlowBoundaryKey>,
    pub outputs: Vec<DataFlowBoundaryKey>,
    pub globals: Vec<GlobalSummaryDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterBinding {
    actual: DataFlowAccessKey,
    formal: DataFlowBoundaryKey,
}

impl ParameterBinding {
    pub fn actual(&self) -> &DataFlowAccessKey {
        &self.actual
    }

    pub fn formal(&self) -> &DataFlowBoundaryKey {
        &self.formal
    }
}

#[derive(Debug, Clone)]
pub struct ParameterBindingDraft {
    pub actual: DataFlowAccessKey,
    pub formal: DataFlowBoundaryKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputBinding {
    formal: DataFlowBoundaryKey,
    receiving_definition: Option<DataFlowDefinitionKey>,
}

impl OutputBinding {
    pub fn formal(&self) -> &DataFlowBoundaryKey {
        &self.formal
    }

    pub fn receiving_definition(&self) -> Option<&DataFlowDefinitionKey> {
        self.receiving_definition.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct OutputBindingDraft {
    pub formal: DataFlowBoundaryKey,
    pub receiving_definition: Option<DataFlowDefinitionKey>,
}

#[derive(Debug, Clone)]
pub struct CallSiteDraft {
    pub caller: ProgramDependenceGraphKey,
    pub call: DataFlowAccessKey,
    pub parameter_bindings: Vec<ParameterBindingDraft>,
    pub output_bindings: Vec<OutputBindingDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalSummary {
    key: GlobalSummaryKey,
    declaration: ScopeFactKey,
    reads: Vec<DataFlowAccessKey>,
    writes: Vec<DataFlowDefinitionKey>,
    mutation_outputs: Vec<DataFlowBoundaryKey>,
}

impl GlobalSummary {
    pub fn key(&self) -> &GlobalSummaryKey {
        &self.key
    }

    pub fn declaration(&self) -> &ScopeFactKey {
        &self.declaration
    }

    pub fn reads(&self) -> &[DataFlowAccessKey] {
        &self.reads
    }

    pub fn writes(&self) -> &[DataFlowDefinitionKey] {
        &self.writes
    }

    pub fn mutation_outputs(&self) -> &[DataFlowBoundaryKey] {
        &self.mutation_outputs
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallableSummary {
    key: CallableSummaryKey,
    program_dependence_graph: ProgramDependenceGraphKey,
    formal_inputs: Vec<DataFlowBoundaryKey>,
    outputs: Vec<DataFlowBoundaryKey>,
    globals: Vec<GlobalSummary>,
}

impl CallableSummary {
    pub fn key(&self) -> &CallableSummaryKey {
        &self.key
    }

    pub fn program_dependence_graph(&self) -> &ProgramDependenceGraphKey {
        &self.program_dependence_graph
    }

    pub fn formal_inputs(&self) -> &[DataFlowBoundaryKey] {
        &self.formal_inputs
    }

    pub fn outputs(&self) -> &[DataFlowBoundaryKey] {
        &self.outputs
    }

    pub fn globals(&self) -> &[GlobalSummary] {
        &self.globals
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDependenceEndpoint {
    graph: ProgramDependenceGraphKey,
    node: ProgramDependenceNodeKey,
}

impl SystemDependenceEndpoint {
    pub fn graph(&self) -> &ProgramDependenceGraphKey {
        &self.graph
    }

    pub fn node(&self) -> &ProgramDependenceNodeKey {
        &self.node
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "evidence", rename_all = "kebab-case")]
pub enum SystemDependenceEdgeKind {
    Call {
        call_site: CallSiteKey,
        call: DataFlowAccessKey,
    },
    ParameterIn {
        call_site: CallSiteKey,
        actual: DataFlowAccessKey,
        formal: DataFlowBoundaryKey,
    },
    Return {
        call_site: CallSiteKey,
        formal: DataFlowBoundaryKey,
        receiving_definition: Option<DataFlowDefinitionKey>,
    },
    ParameterOut {
        call_site: CallSiteKey,
        formal: DataFlowBoundaryKey,
        receiving_definition: Option<DataFlowDefinitionKey>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDependenceEdge {
    key: SystemDependenceEdgeKey,
    from: SystemDependenceEndpoint,
    to: SystemDependenceEndpoint,
    kind: SystemDependenceEdgeKind,
}

impl SystemDependenceEdge {
    pub fn key(&self) -> &SystemDependenceEdgeKey {
        &self.key
    }

    pub fn from(&self) -> &SystemDependenceEndpoint {
        &self.from
    }

    pub fn to(&self) -> &SystemDependenceEndpoint {
        &self.to
    }

    pub fn kind(&self) -> &SystemDependenceEdgeKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "evidence", rename_all = "kebab-case")]
pub enum SystemDependenceGapKind {
    UnresolvedOrNonLocalCallee {
        call: DataFlowAccessKey,
    },
    MissingParameterBinding {
        call: DataFlowAccessKey,
        formal: DataFlowBoundaryKey,
    },
    MissingOutputBinding {
        call: DataFlowAccessKey,
        formal: DataFlowBoundaryKey,
    },
    UnsupportedOutputKind {
        call: DataFlowAccessKey,
        formal: DataFlowBoundaryKey,
        output_kind: DataFlowBoundaryKind,
    },
    CapabilityUnavailable {
        call: DataFlowAccessKey,
        graph: ProgramDependenceGraphKey,
        capability: AdapterCapability,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDependenceGap {
    key: SystemDependenceGapKey,
    kind: SystemDependenceGapKind,
}

impl SystemDependenceGap {
    pub fn key(&self) -> &SystemDependenceGapKey {
        &self.key
    }

    pub fn kind(&self) -> &SystemDependenceGapKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallSite {
    key: CallSiteKey,
    caller: ProgramDependenceGraphKey,
    call: DataFlowAccessKey,
    call_node: ProgramDependenceNodeKey,
    callee: Option<ProgramDependenceGraphKey>,
    parameter_bindings: Vec<ParameterBinding>,
    output_bindings: Vec<OutputBinding>,
    uncertainty: Option<String>,
}

impl CallSite {
    pub fn key(&self) -> &CallSiteKey {
        &self.key
    }

    pub fn caller(&self) -> &ProgramDependenceGraphKey {
        &self.caller
    }

    pub fn call(&self) -> &DataFlowAccessKey {
        &self.call
    }

    pub fn call_node(&self) -> &ProgramDependenceNodeKey {
        &self.call_node
    }

    pub fn callee(&self) -> Option<&ProgramDependenceGraphKey> {
        self.callee.as_ref()
    }

    pub fn parameter_bindings(&self) -> &[ParameterBinding] {
        &self.parameter_bindings
    }

    pub fn output_bindings(&self) -> &[OutputBinding] {
        &self.output_bindings
    }

    pub fn uncertainty(&self) -> Option<&str> {
        self.uncertainty.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDependenceCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDependenceCapabilityEvidence {
    graph: ProgramDependenceGraphKey,
    call_graph_support: CapabilitySupport,
    call_graph_authority: Option<CapabilityAuthority>,
    sdg_support: CapabilitySupport,
    sdg_authority: Option<CapabilityAuthority>,
}

impl SystemDependenceCapabilityEvidence {
    pub fn graph(&self) -> &ProgramDependenceGraphKey {
        &self.graph
    }

    pub fn call_graph_support(&self) -> CapabilitySupport {
        self.call_graph_support
    }

    pub fn call_graph_authority(&self) -> Option<CapabilityAuthority> {
        self.call_graph_authority
    }

    pub fn sdg_support(&self) -> CapabilitySupport {
        self.sdg_support
    }

    pub fn sdg_authority(&self) -> Option<CapabilityAuthority> {
        self.sdg_authority
    }

    fn validate(&self) -> Result<(), SystemDependenceBuildError> {
        validate_capability_authority(
            "CallGraph",
            self.call_graph_support,
            self.call_graph_authority,
        )?;
        validate_capability_authority("Sdg", self.sdg_support, self.sdg_authority)
    }
}

impl SystemDependenceCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    fn validate(&self) -> Result<(), SystemDependenceBuildError> {
        validate_canonical("system-dependence coverage reasons", &self.reasons)?;
        for reason in &self.reasons {
            validate_text(reason)?;
        }
        match (self.status, self.reasons.is_empty()) {
            (FactCoverage::Complete, true) => Ok(()),
            (FactCoverage::Complete, false) => Err(SystemDependenceBuildError::Invalid(
                "Complete system-dependence coverage cannot carry reasons".into(),
            )),
            (_, false) => Ok(()),
            (_, true) => Err(SystemDependenceBuildError::Invalid(
                "incomplete system-dependence coverage requires an exact reason".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDependenceDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    program_dependence_projection_id: ProjectionId,
    program_dependence_policy: ProgramDependencePolicyId,
    data_flow_projection_id: ProjectionId,
    resolution_projection_id: ProjectionId,
    policy: SystemDependencePolicyId,
    capabilities: Vec<SystemDependenceCapabilityEvidence>,
    coverage: SystemDependenceCoverageEvidence,
    summaries: Vec<CallableSummary>,
    calls: Vec<CallSite>,
    edges: Vec<SystemDependenceEdge>,
    gaps: Vec<SystemDependenceGap>,
}

impl SystemDependenceDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn policy(&self) -> &SystemDependencePolicyId {
        &self.policy
    }

    pub fn program_dependence_projection_id(&self) -> &ProjectionId {
        &self.program_dependence_projection_id
    }

    pub fn program_dependence_policy(&self) -> &ProgramDependencePolicyId {
        &self.program_dependence_policy
    }

    pub fn data_flow_projection_id(&self) -> &ProjectionId {
        &self.data_flow_projection_id
    }

    pub fn resolution_projection_id(&self) -> &ProjectionId {
        &self.resolution_projection_id
    }

    pub fn capabilities(&self) -> &[SystemDependenceCapabilityEvidence] {
        &self.capabilities
    }

    pub fn coverage(&self) -> &SystemDependenceCoverageEvidence {
        &self.coverage
    }

    pub fn summaries(&self) -> &[CallableSummary] {
        &self.summaries
    }

    pub fn calls(&self) -> &[CallSite] {
        &self.calls
    }

    pub fn edges(&self) -> &[SystemDependenceEdge] {
        &self.edges
    }

    pub fn gaps(&self) -> &[SystemDependenceGap] {
        &self.gaps
    }

    fn validate(&self) -> Result<(), SystemDependenceBuildError> {
        if self.schema != SYSTEM_DEPENDENCE_SCHEMA {
            return Err(SystemDependenceBuildError::Invalid(format!(
                "unsupported system-dependence schema {}",
                self.schema
            )));
        }
        validate_digest(self.projection_id.as_str(), "pj1_")?;
        validate_digest(&self.analysis_id, "pa1_")?;
        validate_digest(self.program_dependence_projection_id.as_str(), "pj1_")?;
        validate_digest(self.data_flow_projection_id.as_str(), "pj1_")?;
        validate_digest(self.resolution_projection_id.as_str(), "pj1_")?;
        validate_sorted_by(
            "system-dependence capabilities",
            &self.capabilities,
            |evidence| evidence.graph.as_str(),
        )?;
        for evidence in &self.capabilities {
            evidence.validate()?;
        }
        self.coverage.validate()?;
        validate_sorted_by("callable summaries", &self.summaries, |summary| {
            summary.key.as_str()
        })?;
        validate_sorted_by("call sites", &self.calls, |call| call.key.as_str())?;
        validate_sorted_by("system-dependence edges", &self.edges, |edge| {
            edge.key.as_str()
        })?;
        validate_sorted_by("system-dependence gaps", &self.gaps, |gap| gap.key.as_str())?;
        if self.coverage.status == FactCoverage::Complete && !self.gaps.is_empty() {
            return Err(SystemDependenceBuildError::Invalid(
                "Complete system-dependence coverage cannot carry gaps".into(),
            ));
        }
        validate_document_payloads(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemDependenceDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    program_dependence_projection_id: ProjectionId,
    program_dependence_policy: ProgramDependencePolicyId,
    data_flow_projection_id: ProjectionId,
    resolution_projection_id: ProjectionId,
    policy: SystemDependencePolicyId,
    capabilities: Vec<SystemDependenceCapabilityEvidence>,
    coverage: SystemDependenceCoverageEvidence,
    summaries: Vec<CallableSummary>,
    calls: Vec<CallSite>,
    edges: Vec<SystemDependenceEdge>,
    gaps: Vec<SystemDependenceGap>,
}

impl<'de> Deserialize<'de> for SystemDependenceDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SystemDependenceDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            program_dependence_projection_id: wire.program_dependence_projection_id,
            program_dependence_policy: wire.program_dependence_policy,
            data_flow_projection_id: wire.data_flow_projection_id,
            resolution_projection_id: wire.resolution_projection_id,
            policy: wire.policy,
            capabilities: wire.capabilities,
            coverage: wire.coverage,
            summaries: wire.summaries,
            calls: wire.calls,
            edges: wire.edges,
            gaps: wire.gaps,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct SystemDependenceProjection {
    id: ProjectionId,
    program_dependence: Arc<ProgramDependenceProjection>,
    policy: SystemDependencePolicyId,
    document: SystemDependenceDocument,
}

impl SystemDependenceProjection {
    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn program_dependence(&self) -> &Arc<ProgramDependenceProjection> {
        &self.program_dependence
    }

    pub fn policy(&self) -> &SystemDependencePolicyId {
        &self.policy
    }

    pub fn document(&self) -> &SystemDependenceDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemDependenceBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for SystemDependenceBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => {
                write!(formatter, "invalid system-dependence evidence: {detail}")
            }
            Self::Identity(detail) => {
                write!(formatter, "system-dependence identity error: {detail}")
            }
        }
    }
}

impl std::error::Error for SystemDependenceBuildError {}

#[derive(Debug)]
pub struct SystemDependenceBuilder {
    program_dependence: Arc<ProgramDependenceProjection>,
    policy: SystemDependencePolicyId,
    summaries: Vec<CallableSummaryDraft>,
    calls: Vec<CallSiteDraft>,
}

impl SystemDependenceBuilder {
    pub fn new(
        program_dependence: Arc<ProgramDependenceProjection>,
        policy: SystemDependencePolicyId,
    ) -> Self {
        Self {
            program_dependence,
            policy,
            summaries: Vec::new(),
            calls: Vec::new(),
        }
    }

    pub fn add_summary(
        &mut self,
        draft: CallableSummaryDraft,
    ) -> Result<(), SystemDependenceBuildError> {
        if self
            .summaries
            .iter()
            .any(|summary| summary.program_dependence_graph == draft.program_dependence_graph)
        {
            return Err(SystemDependenceBuildError::Invalid(
                "duplicate callable summary draft".into(),
            ));
        }
        self.summaries.push(draft);
        Ok(())
    }

    pub fn add_call_site(&mut self, draft: CallSiteDraft) {
        self.calls.push(draft);
    }

    pub fn build(self) -> Result<SystemDependenceProjection, SystemDependenceBuildError> {
        let source_graphs = self.program_dependence.document().graphs();
        if self.summaries.len() != source_graphs.len() {
            return Err(SystemDependenceBuildError::Invalid(
                "system dependence requires one callable summary per local PDG".into(),
            ));
        }
        let mut summaries = self
            .summaries
            .into_iter()
            .map(|draft| derive_summary(&self.program_dependence, &self.policy, draft))
            .collect::<Result<Vec<_>, _>>()?;
        summaries.sort_by(|left, right| left.key.cmp(&right.key));
        let summary_by_graph = summaries
            .iter()
            .map(|summary| (summary.program_dependence_graph.clone(), summary))
            .collect::<BTreeMap<_, _>>();
        if summary_by_graph.len() != source_graphs.len()
            || source_graphs
                .iter()
                .any(|graph| !summary_by_graph.contains_key(graph.key()))
        {
            return Err(SystemDependenceBuildError::Invalid(
                "callable summaries do not close over the local PDG set".into(),
            ));
        }

        let mut calls = Vec::new();
        let mut edges = Vec::new();
        let mut gaps = Vec::new();
        for draft in self.calls {
            derive_call_site(
                &self.program_dependence,
                &self.policy,
                &summary_by_graph,
                draft,
                &mut calls,
                &mut edges,
                &mut gaps,
            )?;
        }
        calls.sort_by(|left, right| left.key.cmp(&right.key));
        edges.sort_by(|left, right| left.key.cmp(&right.key));
        gaps.sort_by(|left, right| left.key.cmp(&right.key));

        let mut capabilities = source_graphs
            .iter()
            .map(|graph| graph_capabilities(&self.program_dependence, graph))
            .collect::<Result<Vec<_>, _>>()?;
        capabilities.sort_by(|left, right| left.graph.cmp(&right.graph));
        let mut reasons = Vec::new();
        for graph in source_graphs {
            reasons.extend(
                graph
                    .coverage()
                    .reasons()
                    .iter()
                    .map(|reason| format!("local PDG {}: {reason}", graph.key().as_str())),
            );
            let evidence = capabilities
                .iter()
                .find(|evidence| evidence.graph == *graph.key())
                .expect("capability evidence was derived for every source graph");
            if evidence.call_graph_support != CapabilitySupport::Provided {
                reasons.push(format!(
                    "graph {} adapter CallGraph capability is {}",
                    graph.key().as_str(),
                    evidence.call_graph_support.as_str()
                ));
            }
            if evidence.sdg_support != CapabilitySupport::Provided {
                reasons.push(format!(
                    "graph {} adapter Sdg capability is {}",
                    graph.key().as_str(),
                    evidence.sdg_support.as_str()
                ));
            }
        }
        reasons.extend(gaps.iter().map(|gap| gap_reason(&gap.kind)));
        reasons.sort();
        reasons.dedup();
        let status = if reasons.is_empty()
            && source_graphs
                .iter()
                .all(|graph| graph.coverage().status() == FactCoverage::Complete)
        {
            FactCoverage::Complete
        } else if source_graphs
            .iter()
            .any(|graph| graph.coverage().status() == FactCoverage::Failed)
        {
            FactCoverage::Failed
        } else if capabilities.iter().any(|evidence| {
            evidence.call_graph_support == CapabilitySupport::Unsupported
                || evidence.sdg_support == CapabilitySupport::Unsupported
        }) {
            FactCoverage::Unsupported
        } else {
            FactCoverage::Partial
        };
        let coverage = SystemDependenceCoverageEvidence { status, reasons };
        coverage.validate()?;
        let data_flow = self.program_dependence.data_flow();
        let resolution = data_flow.resolution();
        let payload = serde_json::to_vec(&(
            self.program_dependence.id(),
            self.program_dependence.policy(),
            &self.policy,
            &capabilities,
            &coverage,
            &summaries,
            &calls,
            &edges,
            &gaps,
        ))
        .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
        let analysis = data_flow.control_regions().control_flow().analysis();
        let id = analysis
            .derive_projection_id(
                SYSTEM_DEPENDENCE_SCHEMA,
                &payload,
                self.program_dependence.id().as_str().as_bytes(),
            )
            .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
        let document = SystemDependenceDocument {
            schema: SYSTEM_DEPENDENCE_SCHEMA.into(),
            projection_id: id.clone(),
            analysis_id: analysis.id().as_str().into(),
            program_dependence_projection_id: self.program_dependence.id().clone(),
            program_dependence_policy: self.program_dependence.policy().clone(),
            data_flow_projection_id: data_flow.id().clone(),
            resolution_projection_id: resolution.id().clone(),
            policy: self.policy.clone(),
            capabilities,
            coverage,
            summaries,
            calls,
            edges,
            gaps,
        };
        document.validate()?;
        Ok(SystemDependenceProjection {
            id,
            program_dependence: self.program_dependence,
            policy: self.policy,
            document,
        })
    }
}

fn derive_summary(
    projection: &ProgramDependenceProjection,
    policy: &SystemDependencePolicyId,
    draft: CallableSummaryDraft,
) -> Result<CallableSummary, SystemDependenceBuildError> {
    let pdg = find_pdg(projection, &draft.program_dependence_graph)?;
    let data = find_data_graph(projection, pdg)?;
    let boundaries = data
        .boundaries()
        .iter()
        .map(|boundary| (boundary.key().clone(), boundary))
        .collect::<BTreeMap<_, _>>();
    validate_distinct_ordered_keys("formal inputs", &draft.formal_inputs)?;
    validate_distinct_ordered_keys("callable outputs", &draft.outputs)?;
    for key in &draft.formal_inputs {
        let boundary = boundaries.get(key).ok_or_else(|| {
            SystemDependenceBuildError::Invalid("formal input boundary is missing".into())
        })?;
        if boundary.kind() != DataFlowBoundaryKind::ParameterInput {
            return Err(SystemDependenceBuildError::Invalid(
                "formal input is not a ParameterInput boundary".into(),
            ));
        }
    }
    for key in &draft.outputs {
        let boundary = boundaries.get(key).ok_or_else(|| {
            SystemDependenceBuildError::Invalid("callable output boundary is missing".into())
        })?;
        if boundary.kind() == DataFlowBoundaryKind::ParameterInput {
            return Err(SystemDependenceBuildError::Invalid(
                "callable output cannot be a ParameterInput boundary".into(),
            ));
        }
    }
    let expected_inputs = data
        .boundaries()
        .iter()
        .filter(|boundary| boundary.kind() == DataFlowBoundaryKind::ParameterInput)
        .map(|boundary| boundary.key().clone())
        .collect::<BTreeSet<_>>();
    let expected_outputs = data
        .boundaries()
        .iter()
        .filter(|boundary| boundary.kind() != DataFlowBoundaryKind::ParameterInput)
        .map(|boundary| boundary.key().clone())
        .collect::<BTreeSet<_>>();
    if draft.formal_inputs.iter().cloned().collect::<BTreeSet<_>>() != expected_inputs
        || draft.outputs.iter().cloned().collect::<BTreeSet<_>>() != expected_outputs
    {
        return Err(SystemDependenceBuildError::Invalid(
            "callable summary does not enumerate every formal input and output exactly once".into(),
        ));
    }
    let mut globals = draft
        .globals
        .into_iter()
        .map(|global| derive_global(projection, policy, data, global))
        .collect::<Result<Vec<_>, _>>()?;
    globals.sort_by(|left, right| left.key.cmp(&right.key));
    let payload = serde_json::to_vec(&(
        &draft.program_dependence_graph,
        &draft.formal_inputs,
        &draft.outputs,
        &globals,
    ))
    .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
    Ok(CallableSummary {
        key: CallableSummaryKey(derive_id(
            SUMMARY_DOMAIN,
            "css1_",
            &[policy.as_str().as_bytes(), &payload],
        )),
        program_dependence_graph: draft.program_dependence_graph,
        formal_inputs: draft.formal_inputs,
        outputs: draft.outputs,
        globals,
    })
}

fn derive_global(
    projection: &ProgramDependenceProjection,
    policy: &SystemDependencePolicyId,
    data: &DataFlowGraph,
    mut draft: GlobalSummaryDraft,
) -> Result<GlobalSummary, SystemDependenceBuildError> {
    validate_global_declaration(projection, &draft.declaration)?;
    draft.reads.sort();
    draft.writes.sort();
    draft.mutation_outputs.sort();
    validate_canonical("global reads", &draft.reads)?;
    validate_canonical("global writes", &draft.writes)?;
    validate_canonical("global mutation outputs", &draft.mutation_outputs)?;
    let symbols = data
        .symbols()
        .iter()
        .map(|symbol| (symbol.key(), symbol.declaration()))
        .collect::<BTreeMap<_, _>>();
    let accesses = data
        .accesses()
        .iter()
        .map(|access| (access.key(), access))
        .collect::<BTreeMap<_, _>>();
    let definitions = data
        .definitions()
        .iter()
        .map(|definition| (definition.key(), definition))
        .collect::<BTreeMap<_, _>>();
    let boundaries = data
        .boundaries()
        .iter()
        .map(|boundary| (boundary.key(), boundary))
        .collect::<BTreeMap<_, _>>();
    for key in &draft.reads {
        let access = accesses.get(key).ok_or_else(|| {
            SystemDependenceBuildError::Invalid("global read access is missing".into())
        })?;
        if !matches!(
            access.kind(),
            DataFlowAccessKind::Read
                | DataFlowAccessKind::ReadWrite
                | DataFlowAccessKind::Call
                | DataFlowAccessKind::Borrow
                | DataFlowAccessKind::Capture
        ) || access
            .symbol()
            .and_then(|symbol| symbols.get(symbol).copied())
            != Some(&draft.declaration)
        {
            return Err(SystemDependenceBuildError::Invalid(
                "global read does not resolve to its declaration".into(),
            ));
        }
    }
    for key in &draft.writes {
        let definition = definitions.get(key).ok_or_else(|| {
            SystemDependenceBuildError::Invalid("global write definition is missing".into())
        })?;
        if symbols.get(definition.symbol()).copied() != Some(&draft.declaration) {
            return Err(SystemDependenceBuildError::Invalid(
                "global write does not resolve to its declaration".into(),
            ));
        }
    }
    for key in &draft.mutation_outputs {
        let boundary = boundaries.get(key).ok_or_else(|| {
            SystemDependenceBuildError::Invalid("global mutation output is missing".into())
        })?;
        if boundary.kind() != DataFlowBoundaryKind::MutationOutput
            || boundary
                .symbol()
                .and_then(|symbol| symbols.get(symbol).copied())
                != Some(&draft.declaration)
        {
            return Err(SystemDependenceBuildError::Invalid(
                "global mutation output does not resolve to its declaration".into(),
            ));
        }
    }
    let payload = serde_json::to_vec(&(
        &draft.declaration,
        &draft.reads,
        &draft.writes,
        &draft.mutation_outputs,
    ))
    .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
    Ok(GlobalSummary {
        key: GlobalSummaryKey(derive_id(
            GLOBAL_DOMAIN,
            "gss1_",
            &[policy.as_str().as_bytes(), &payload],
        )),
        declaration: draft.declaration,
        reads: draft.reads,
        writes: draft.writes,
        mutation_outputs: draft.mutation_outputs,
    })
}

#[allow(clippy::too_many_arguments)]
fn derive_call_site(
    projection: &ProgramDependenceProjection,
    policy: &SystemDependencePolicyId,
    summaries: &BTreeMap<ProgramDependenceGraphKey, &CallableSummary>,
    draft: CallSiteDraft,
    calls: &mut Vec<CallSite>,
    edges: &mut Vec<SystemDependenceEdge>,
    gaps: &mut Vec<SystemDependenceGap>,
) -> Result<(), SystemDependenceBuildError> {
    let caller_pdg = find_pdg(projection, &draft.caller)?;
    let caller_data = find_data_graph(projection, caller_pdg)?;
    let call = find_access(caller_data, &draft.call)?;
    if call.kind() != DataFlowAccessKind::Call {
        return Err(SystemDependenceBuildError::Invalid(
            "call-site draft does not cite a Call access".into(),
        ));
    }
    let call_node = node_for_point(caller_pdg, call.point())?.clone();
    let callee = resolve_local_callee(projection, call)?;
    if callee.is_none()
        && (!draft.parameter_bindings.is_empty() || !draft.output_bindings.is_empty())
    {
        return Err(SystemDependenceBuildError::Invalid(
            "unresolved/non-local call cannot carry local binding drafts".into(),
        ));
    }
    let mut parameter_bindings = draft
        .parameter_bindings
        .into_iter()
        .map(|binding| ParameterBinding {
            actual: binding.actual,
            formal: binding.formal,
        })
        .collect::<Vec<_>>();
    let mut output_bindings = draft
        .output_bindings
        .into_iter()
        .map(|binding| OutputBinding {
            formal: binding.formal,
            receiving_definition: binding.receiving_definition,
        })
        .collect::<Vec<_>>();
    validate_distinct_bindings(&parameter_bindings, &output_bindings)?;
    parameter_bindings.sort_by(|left, right| left.formal.cmp(&right.formal));
    output_bindings.sort_by(|left, right| left.formal.cmp(&right.formal));
    let uncertainty = callee
        .is_none()
        .then(|| "call does not resolve to one exact local CFG owner".to_string());
    let call_payload = serde_json::to_vec(&(
        &draft.caller,
        &draft.call,
        &call_node,
        &callee,
        &parameter_bindings,
        &output_bindings,
        &uncertainty,
    ))
    .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
    let call_site = CallSite {
        key: CallSiteKey(derive_id(
            CALL_DOMAIN,
            "cst1_",
            &[policy.as_str().as_bytes(), &call_payload],
        )),
        caller: draft.caller,
        call: draft.call,
        call_node,
        callee: callee.clone(),
        parameter_bindings,
        output_bindings,
        uncertainty,
    };
    if callee.is_none() {
        gaps.push(make_gap(
            policy,
            SystemDependenceGapKind::UnresolvedOrNonLocalCallee {
                call: call_site.call.clone(),
            },
        )?);
        calls.push(call_site);
        return Ok(());
    }
    let callee_key = callee.expect("checked above");
    let callee_pdg = find_pdg(projection, &callee_key)?;
    let callee_data = find_data_graph(projection, callee_pdg)?;
    let callee_summary = summaries[&callee_key];
    let caller_capabilities = graph_capabilities(projection, caller_pdg)?;
    let callee_capabilities = graph_capabilities(projection, callee_pdg)?;
    let mut unavailable = BTreeSet::new();
    if caller_capabilities.call_graph_support != CapabilitySupport::Provided {
        unavailable.insert((caller_pdg.key().clone(), AdapterCapability::CallGraph));
    }
    if caller_capabilities.sdg_support != CapabilitySupport::Provided {
        unavailable.insert((caller_pdg.key().clone(), AdapterCapability::Sdg));
    }
    if callee_capabilities.sdg_support != CapabilitySupport::Provided {
        unavailable.insert((callee_pdg.key().clone(), AdapterCapability::Sdg));
    }
    for (graph, capability) in unavailable {
        gaps.push(make_gap(
            policy,
            SystemDependenceGapKind::CapabilityUnavailable {
                call: call_site.call.clone(),
                graph,
                capability,
            },
        )?);
    }
    let capability_ready = caller_capabilities.call_graph_support == CapabilitySupport::Provided
        && caller_capabilities.sdg_support == CapabilitySupport::Provided
        && callee_capabilities.sdg_support == CapabilitySupport::Provided;
    let callee_entry =
        node_for_point(callee_pdg, callee_data_point_entry(projection, callee_pdg)?)?;
    if capability_ready {
        edges.push(make_edge(
            policy,
            SystemDependenceEndpoint {
                graph: call_site.caller.clone(),
                node: call_site.call_node.clone(),
            },
            SystemDependenceEndpoint {
                graph: callee_key.clone(),
                node: callee_entry.clone(),
            },
            SystemDependenceEdgeKind::Call {
                call_site: call_site.key.clone(),
                call: call_site.call.clone(),
            },
        )?);
    }
    let formal_boundaries = callee_data
        .boundaries()
        .iter()
        .map(|boundary| (boundary.key(), boundary))
        .collect::<BTreeMap<_, _>>();
    let actual_accesses = caller_data
        .accesses()
        .iter()
        .map(|access| (access.key(), access))
        .collect::<BTreeMap<_, _>>();
    let actual_definitions = caller_data
        .definitions()
        .iter()
        .map(|definition| (definition.key(), definition))
        .collect::<BTreeMap<_, _>>();
    let bound_formals = call_site
        .parameter_bindings
        .iter()
        .map(|binding| &binding.formal)
        .collect::<BTreeSet<_>>();
    for formal in &callee_summary.formal_inputs {
        if !bound_formals.contains(formal) {
            gaps.push(make_gap(
                policy,
                SystemDependenceGapKind::MissingParameterBinding {
                    call: call_site.call.clone(),
                    formal: formal.clone(),
                },
            )?);
        }
    }
    for binding in &call_site.parameter_bindings {
        if !callee_summary.formal_inputs.contains(&binding.formal) {
            return Err(SystemDependenceBuildError::Invalid(
                "parameter binding cites a non-formal callee boundary".into(),
            ));
        }
        let actual = actual_accesses.get(&binding.actual).ok_or_else(|| {
            SystemDependenceBuildError::Invalid("parameter actual access is missing".into())
        })?;
        let formal = formal_boundaries[&binding.formal];
        if capability_ready {
            edges.push(make_edge(
                policy,
                endpoint_for_access(caller_pdg, actual)?,
                endpoint_for_boundary(callee_pdg, formal)?,
                SystemDependenceEdgeKind::ParameterIn {
                    call_site: call_site.key.clone(),
                    actual: binding.actual.clone(),
                    formal: binding.formal.clone(),
                },
            )?);
        }
    }
    let bound_outputs = call_site
        .output_bindings
        .iter()
        .map(|binding| &binding.formal)
        .collect::<BTreeSet<_>>();
    for formal in &callee_summary.outputs {
        if !bound_outputs.contains(formal) {
            gaps.push(make_gap(
                policy,
                SystemDependenceGapKind::MissingOutputBinding {
                    call: call_site.call.clone(),
                    formal: formal.clone(),
                },
            )?);
        }
    }
    for binding in &call_site.output_bindings {
        if !callee_summary.outputs.contains(&binding.formal) {
            return Err(SystemDependenceBuildError::Invalid(
                "output binding cites a non-output callee boundary".into(),
            ));
        }
        let formal = formal_boundaries[&binding.formal];
        let kind = match formal.kind() {
            DataFlowBoundaryKind::ReturnOutput => Some(SystemDependenceEdgeKind::Return {
                call_site: call_site.key.clone(),
                formal: binding.formal.clone(),
                receiving_definition: binding.receiving_definition.clone(),
            }),
            DataFlowBoundaryKind::MutationOutput => Some(SystemDependenceEdgeKind::ParameterOut {
                call_site: call_site.key.clone(),
                formal: binding.formal.clone(),
                receiving_definition: binding.receiving_definition.clone(),
            }),
            kind => {
                gaps.push(make_gap(
                    policy,
                    SystemDependenceGapKind::UnsupportedOutputKind {
                        call: call_site.call.clone(),
                        formal: binding.formal.clone(),
                        output_kind: kind,
                    },
                )?);
                None
            }
        };
        let to = if let Some(definition) = &binding.receiving_definition {
            let definition = actual_definitions.get(definition).ok_or_else(|| {
                SystemDependenceBuildError::Invalid(
                    "output receiving definition is missing from caller".into(),
                )
            })?;
            endpoint_for_definition(caller_pdg, definition)?
        } else {
            SystemDependenceEndpoint {
                graph: call_site.caller.clone(),
                node: call_site.call_node.clone(),
            }
        };
        if capability_ready && let Some(kind) = kind {
            edges.push(make_edge(
                policy,
                endpoint_for_boundary(callee_pdg, formal)?,
                to,
                kind,
            )?);
        }
    }
    calls.push(call_site);
    Ok(())
}

fn find_pdg<'a>(
    projection: &'a ProgramDependenceProjection,
    key: &ProgramDependenceGraphKey,
) -> Result<&'a ProgramDependenceGraph, SystemDependenceBuildError> {
    projection
        .document()
        .graphs()
        .iter()
        .find(|graph| graph.key() == key)
        .ok_or_else(|| SystemDependenceBuildError::Invalid("local PDG is missing".into()))
}

fn find_data_graph<'a>(
    projection: &'a ProgramDependenceProjection,
    pdg: &ProgramDependenceGraph,
) -> Result<&'a DataFlowGraph, SystemDependenceBuildError> {
    projection
        .data_flow()
        .document()
        .graphs()
        .iter()
        .find(|graph| graph.key() == pdg.data_flow_graph())
        .ok_or_else(|| {
            SystemDependenceBuildError::Invalid("local PDG dataflow graph is missing".into())
        })
}

fn find_access<'a>(
    data: &'a DataFlowGraph,
    key: &DataFlowAccessKey,
) -> Result<&'a DataFlowAccess, SystemDependenceBuildError> {
    data.accesses()
        .iter()
        .find(|access| access.key() == key)
        .ok_or_else(|| SystemDependenceBuildError::Invalid("call access is missing".into()))
}

fn resolve_local_callee(
    projection: &ProgramDependenceProjection,
    access: &DataFlowAccess,
) -> Result<Option<ProgramDependenceGraphKey>, SystemDependenceBuildError> {
    let resolution = projection.data_flow().resolution();
    let Some(result) = resolution
        .results()
        .iter()
        .find(|result| result.wire().key() == access.resolution())
        .map(|result| result.wire())
    else {
        return Err(SystemDependenceBuildError::Invalid(
            "call access resolution result is missing".into(),
        ));
    };
    if result.status() != ResolutionStatus::Unique
        || result.coverage().status() != FactCoverage::Complete
    {
        return Ok(None);
    }
    let Some(preferred) = result.preferred() else {
        return Ok(None);
    };
    if preferred.status() != ResolutionStatus::Unique || preferred.endpoints().len() != 1 {
        return Ok(None);
    }
    let fact_key = match &preferred.endpoints()[0] {
        ResolutionEndpoint::Declaration(key) | ResolutionEndpoint::Definition(key) => key,
        _ => return Ok(None),
    };
    let scope_graph = resolution.scope_graph();
    let Some(fact) = scope_graph
        .facts()
        .iter()
        .find(|fact| fact.key() == fact_key)
    else {
        return Err(SystemDependenceBuildError::Invalid(
            "resolved call endpoint fact is missing".into(),
        ));
    };
    let owner = scope_graph
        .analysis()
        .node_key(fact.node())
        .map_err(|_| SystemDependenceBuildError::Invalid("callee owner node is missing".into()))?;
    let matches = projection
        .document()
        .graphs()
        .iter()
        .filter(|graph| graph.owner() == owner)
        .map(|graph| graph.key().clone())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [key] => Ok(Some(key.clone())),
        [] => Ok(None),
        _ => Err(SystemDependenceBuildError::Invalid(
            "resolved call endpoint matches multiple local CFG owners".into(),
        )),
    }
}

fn validate_global_declaration(
    projection: &ProgramDependenceProjection,
    declaration: &ScopeFactKey,
) -> Result<(), SystemDependenceBuildError> {
    let scope_graph = projection.data_flow().resolution().scope_graph();
    let fact = scope_graph
        .facts()
        .iter()
        .find(|fact| fact.key() == declaration)
        .ok_or_else(|| {
            SystemDependenceBuildError::Invalid("global declaration fact is missing".into())
        })?;
    let ScopeFactData::Declaration { scope, .. } = fact.data() else {
        return Err(SystemDependenceBuildError::Invalid(
            "global summary key is not a Declaration fact".into(),
        ));
    };
    let scope = scope_graph
        .facts()
        .iter()
        .find(|fact| fact.key() == scope)
        .ok_or_else(|| {
            SystemDependenceBuildError::Invalid("global declaration scope is missing".into())
        })?;
    match scope.data() {
        ScopeFactData::Scope {
            scope_kind:
                ScopeKind::Project
                | ScopeKind::Package
                | ScopeKind::BuildTarget
                | ScopeKind::Module
                | ScopeKind::File
                | ScopeKind::Namespace,
            ..
        } => Ok(()),
        _ => Err(SystemDependenceBuildError::Invalid(
            "global summary declaration is not owned by a project/module/file scope".into(),
        )),
    }
}

fn graph_capabilities(
    projection: &ProgramDependenceProjection,
    pdg: &ProgramDependenceGraph,
) -> Result<SystemDependenceCapabilityEvidence, SystemDependenceBuildError> {
    let flow = projection.data_flow().control_regions().control_flow();
    let graph = flow
        .document()
        .graphs()
        .iter()
        .find(|graph| graph.key() == pdg.control_flow_graph())
        .ok_or_else(|| {
            SystemDependenceBuildError::Invalid("local PDG control-flow graph is missing".into())
        })?;
    let call_graph = graph
        .adapter()
        .capabilities()
        .declaration(AdapterCapability::CallGraph);
    let sdg = graph
        .adapter()
        .capabilities()
        .declaration(AdapterCapability::Sdg);
    let evidence = SystemDependenceCapabilityEvidence {
        graph: pdg.key().clone(),
        call_graph_support: call_graph.support(),
        call_graph_authority: call_graph.authority(),
        sdg_support: sdg.support(),
        sdg_authority: sdg.authority(),
    };
    evidence.validate()?;
    Ok(evidence)
}

fn callee_data_point_entry<'a>(
    projection: &'a ProgramDependenceProjection,
    pdg: &ProgramDependenceGraph,
) -> Result<&'a crate::ControlPointKey, SystemDependenceBuildError> {
    let flow = projection.data_flow().control_regions().control_flow();
    flow.document()
        .graphs()
        .iter()
        .find(|graph| graph.key() == pdg.control_flow_graph())
        .map(|graph| graph.entry())
        .ok_or_else(|| SystemDependenceBuildError::Invalid("callee CFG is missing".into()))
}

fn node_for_point<'a>(
    pdg: &'a ProgramDependenceGraph,
    point: &crate::ControlPointKey,
) -> Result<&'a ProgramDependenceNodeKey, SystemDependenceBuildError> {
    pdg.nodes()
        .iter()
        .find(|node| node.point() == point)
        .map(|node| node.key())
        .ok_or_else(|| {
            SystemDependenceBuildError::Invalid("local PDG point node is missing".into())
        })
}

fn endpoint_for_access(
    pdg: &ProgramDependenceGraph,
    access: &DataFlowAccess,
) -> Result<SystemDependenceEndpoint, SystemDependenceBuildError> {
    Ok(SystemDependenceEndpoint {
        graph: pdg.key().clone(),
        node: node_for_point(pdg, access.point())?.clone(),
    })
}

fn endpoint_for_definition(
    pdg: &ProgramDependenceGraph,
    definition: &DataFlowDefinition,
) -> Result<SystemDependenceEndpoint, SystemDependenceBuildError> {
    Ok(SystemDependenceEndpoint {
        graph: pdg.key().clone(),
        node: node_for_point(pdg, definition.point())?.clone(),
    })
}

fn endpoint_for_boundary(
    pdg: &ProgramDependenceGraph,
    boundary: &DataFlowBoundary,
) -> Result<SystemDependenceEndpoint, SystemDependenceBuildError> {
    Ok(SystemDependenceEndpoint {
        graph: pdg.key().clone(),
        node: node_for_point(pdg, boundary.point())?.clone(),
    })
}

fn validate_distinct_bindings(
    parameters: &[ParameterBinding],
    outputs: &[OutputBinding],
) -> Result<(), SystemDependenceBuildError> {
    if parameters
        .iter()
        .map(|binding| &binding.actual)
        .collect::<BTreeSet<_>>()
        .len()
        != parameters.len()
        || parameters
            .iter()
            .map(|binding| &binding.formal)
            .collect::<BTreeSet<_>>()
            .len()
            != parameters.len()
        || outputs
            .iter()
            .map(|binding| &binding.formal)
            .collect::<BTreeSet<_>>()
            .len()
            != outputs.len()
    {
        return Err(SystemDependenceBuildError::Invalid(
            "call-site bindings must be one-to-one and distinct".into(),
        ));
    }
    Ok(())
}

fn make_edge(
    policy: &SystemDependencePolicyId,
    from: SystemDependenceEndpoint,
    to: SystemDependenceEndpoint,
    kind: SystemDependenceEdgeKind,
) -> Result<SystemDependenceEdge, SystemDependenceBuildError> {
    let payload = serde_json::to_vec(&(&from, &to, &kind))
        .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
    Ok(SystemDependenceEdge {
        key: SystemDependenceEdgeKey(derive_id(
            EDGE_DOMAIN,
            "sde1_",
            &[policy.as_str().as_bytes(), &payload],
        )),
        from,
        to,
        kind,
    })
}

fn make_gap(
    policy: &SystemDependencePolicyId,
    kind: SystemDependenceGapKind,
) -> Result<SystemDependenceGap, SystemDependenceBuildError> {
    let payload = serde_json::to_vec(&kind)
        .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
    Ok(SystemDependenceGap {
        key: SystemDependenceGapKey(derive_id(
            GAP_DOMAIN,
            "sdx1_",
            &[policy.as_str().as_bytes(), &payload],
        )),
        kind,
    })
}

fn validate_document_payloads(
    document: &SystemDependenceDocument,
) -> Result<(), SystemDependenceBuildError> {
    let summary_graphs = document
        .summaries
        .iter()
        .map(|summary| (&summary.program_dependence_graph, summary))
        .collect::<BTreeMap<_, _>>();
    if summary_graphs.len() != document.summaries.len() {
        return Err(SystemDependenceBuildError::Invalid(
            "system-dependence document repeats a callable summary graph".into(),
        ));
    }
    let capability_graphs = document
        .capabilities
        .iter()
        .map(|evidence| &evidence.graph)
        .collect::<BTreeSet<_>>();
    if capability_graphs.len() != document.capabilities.len()
        || capability_graphs != summary_graphs.keys().copied().collect()
    {
        return Err(SystemDependenceBuildError::Invalid(
            "system-dependence capability evidence does not close over callable summaries".into(),
        ));
    }
    if document.coverage.status == FactCoverage::Complete
        && document.capabilities.iter().any(|evidence| {
            evidence.call_graph_support != CapabilitySupport::Provided
                || evidence.sdg_support != CapabilitySupport::Provided
        })
    {
        return Err(SystemDependenceBuildError::Invalid(
            "Complete system-dependence coverage requires Provided CallGraph and Sdg capabilities"
                .into(),
        ));
    }
    let mut global_keys = BTreeSet::new();
    for summary in &document.summaries {
        validate_distinct_ordered_keys("formal inputs", &summary.formal_inputs)?;
        validate_distinct_ordered_keys("callable outputs", &summary.outputs)?;
        validate_sorted_by("global summaries", &summary.globals, |global| {
            global.key.as_str()
        })?;
        for global in &summary.globals {
            validate_canonical("global reads", &global.reads)?;
            validate_canonical("global writes", &global.writes)?;
            validate_canonical("global mutation outputs", &global.mutation_outputs)?;
            let payload = serde_json::to_vec(&(
                &global.declaration,
                &global.reads,
                &global.writes,
                &global.mutation_outputs,
            ))
            .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
            let expected = GlobalSummaryKey(derive_id(
                GLOBAL_DOMAIN,
                "gss1_",
                &[document.policy.as_str().as_bytes(), &payload],
            ));
            if global.key != expected || !global_keys.insert(&global.key) {
                return Err(SystemDependenceBuildError::Invalid(
                    "global summary key does not bind a distinct payload".into(),
                ));
            }
        }
        let payload = serde_json::to_vec(&(
            &summary.program_dependence_graph,
            &summary.formal_inputs,
            &summary.outputs,
            &summary.globals,
        ))
        .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
        let expected = CallableSummaryKey(derive_id(
            SUMMARY_DOMAIN,
            "css1_",
            &[document.policy.as_str().as_bytes(), &payload],
        ));
        if summary.key != expected {
            return Err(SystemDependenceBuildError::Invalid(
                "callable summary key does not bind its payload".into(),
            ));
        }
    }
    let calls = document
        .calls
        .iter()
        .map(|call| (&call.key, call))
        .collect::<BTreeMap<_, _>>();
    let calls_by_access = document
        .calls
        .iter()
        .map(|call| (&call.call, call))
        .collect::<BTreeMap<_, _>>();
    if calls_by_access.len() != document.calls.len() {
        return Err(SystemDependenceBuildError::Invalid(
            "system-dependence document repeats a call access".into(),
        ));
    }
    for call in &document.calls {
        if !summary_graphs.contains_key(&call.caller)
            || call
                .callee
                .as_ref()
                .is_some_and(|callee| !summary_graphs.contains_key(callee))
        {
            return Err(SystemDependenceBuildError::Invalid(
                "call site cites a missing caller or callee summary".into(),
            ));
        }
        validate_distinct_bindings(&call.parameter_bindings, &call.output_bindings)?;
        if call.callee.is_none()
            && (!call.parameter_bindings.is_empty() || !call.output_bindings.is_empty())
        {
            return Err(SystemDependenceBuildError::Invalid(
                "unresolved call site carries local bindings".into(),
            ));
        }
        if let Some(callee) = &call.callee {
            let summary = summary_graphs[callee];
            if call
                .parameter_bindings
                .iter()
                .any(|binding| !summary.formal_inputs.contains(&binding.formal))
                || call
                    .output_bindings
                    .iter()
                    .any(|binding| !summary.outputs.contains(&binding.formal))
            {
                return Err(SystemDependenceBuildError::Invalid(
                    "call-site bindings do not belong to the cited callee summary".into(),
                ));
            }
        }
        match (&call.callee, &call.uncertainty) {
            (Some(_), None) => {}
            (None, Some(reason)) => validate_text(reason)?,
            _ => {
                return Err(SystemDependenceBuildError::Invalid(
                    "call-site callee and uncertainty disagree".into(),
                ));
            }
        }
        let payload = serde_json::to_vec(&(
            &call.caller,
            &call.call,
            &call.call_node,
            &call.callee,
            &call.parameter_bindings,
            &call.output_bindings,
            &call.uncertainty,
        ))
        .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
        let expected = CallSiteKey(derive_id(
            CALL_DOMAIN,
            "cst1_",
            &[document.policy.as_str().as_bytes(), &payload],
        ));
        if call.key != expected {
            return Err(SystemDependenceBuildError::Invalid(
                "call-site key does not bind its payload".into(),
            ));
        }
    }
    for edge in &document.edges {
        let call_site = match &edge.kind {
            SystemDependenceEdgeKind::Call { call_site, .. }
            | SystemDependenceEdgeKind::ParameterIn { call_site, .. }
            | SystemDependenceEdgeKind::Return { call_site, .. }
            | SystemDependenceEdgeKind::ParameterOut { call_site, .. } => call_site,
        };
        if !calls.contains_key(call_site)
            || !summary_graphs.contains_key(&edge.from.graph)
            || !summary_graphs.contains_key(&edge.to.graph)
        {
            return Err(SystemDependenceBuildError::Invalid(
                "system-dependence edge has dangling call or graph evidence".into(),
            ));
        }
        let call = calls[call_site];
        validate_edge_semantics(call, edge)?;
        let payload = serde_json::to_vec(&(&edge.from, &edge.to, &edge.kind))
            .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
        let expected = SystemDependenceEdgeKey(derive_id(
            EDGE_DOMAIN,
            "sde1_",
            &[document.policy.as_str().as_bytes(), &payload],
        ));
        if edge.key != expected {
            return Err(SystemDependenceBuildError::Invalid(
                "system-dependence edge key does not bind its payload".into(),
            ));
        }
    }
    for gap in &document.gaps {
        let call_access = gap_call(&gap.kind);
        let Some(call) = calls_by_access.get(call_access).copied() else {
            return Err(SystemDependenceBuildError::Invalid(
                "system-dependence gap cites a missing call access".into(),
            ));
        };
        validate_gap_semantics(document, call, &gap.kind)?;
        let payload = serde_json::to_vec(&gap.kind)
            .map_err(|error| SystemDependenceBuildError::Identity(error.to_string()))?;
        let expected = SystemDependenceGapKey(derive_id(
            GAP_DOMAIN,
            "sdx1_",
            &[document.policy.as_str().as_bytes(), &payload],
        ));
        if gap.key != expected
            || document
                .coverage
                .reasons
                .binary_search(&gap_reason(&gap.kind))
                .is_err()
        {
            return Err(SystemDependenceBuildError::Invalid(
                "system-dependence gap key/reason does not bind its payload".into(),
            ));
        }
    }
    Ok(())
}

fn validate_edge_semantics(
    call: &CallSite,
    edge: &SystemDependenceEdge,
) -> Result<(), SystemDependenceBuildError> {
    let Some(callee) = call.callee.as_ref() else {
        return Err(SystemDependenceBuildError::Invalid(
            "unresolved call site cannot own an interprocedural edge".into(),
        ));
    };
    let graph_direction_is_call = edge.from.graph == call.caller && edge.to.graph == *callee;
    let graph_direction_is_output = edge.from.graph == *callee && edge.to.graph == call.caller;
    let valid = match &edge.kind {
        SystemDependenceEdgeKind::Call { call: access, .. } => {
            access == &call.call && graph_direction_is_call && edge.from.node == call.call_node
        }
        SystemDependenceEdgeKind::ParameterIn { actual, formal, .. } => {
            graph_direction_is_call
                && call
                    .parameter_bindings
                    .iter()
                    .any(|binding| binding.actual == *actual && binding.formal == *formal)
        }
        SystemDependenceEdgeKind::Return {
            formal,
            receiving_definition,
            ..
        }
        | SystemDependenceEdgeKind::ParameterOut {
            formal,
            receiving_definition,
            ..
        } => {
            graph_direction_is_output
                && call.output_bindings.iter().any(|binding| {
                    binding.formal == *formal
                        && binding.receiving_definition == *receiving_definition
                })
        }
    };
    if valid {
        Ok(())
    } else {
        Err(SystemDependenceBuildError::Invalid(
            "system-dependence edge contradicts its call-site evidence".into(),
        ))
    }
}

fn gap_call(kind: &SystemDependenceGapKind) -> &DataFlowAccessKey {
    match kind {
        SystemDependenceGapKind::UnresolvedOrNonLocalCallee { call }
        | SystemDependenceGapKind::MissingParameterBinding { call, .. }
        | SystemDependenceGapKind::MissingOutputBinding { call, .. }
        | SystemDependenceGapKind::UnsupportedOutputKind { call, .. }
        | SystemDependenceGapKind::CapabilityUnavailable { call, .. } => call,
    }
}

fn validate_gap_semantics(
    document: &SystemDependenceDocument,
    call: &CallSite,
    kind: &SystemDependenceGapKind,
) -> Result<(), SystemDependenceBuildError> {
    let valid = match kind {
        SystemDependenceGapKind::UnresolvedOrNonLocalCallee { .. } => call.callee.is_none(),
        SystemDependenceGapKind::MissingParameterBinding { formal, .. } => call
            .callee
            .as_ref()
            .and_then(|callee| {
                document
                    .summaries
                    .iter()
                    .find(|summary| summary.program_dependence_graph == *callee)
            })
            .is_some_and(|summary| {
                summary.formal_inputs.contains(formal)
                    && !call
                        .parameter_bindings
                        .iter()
                        .any(|binding| binding.formal == *formal)
            }),
        SystemDependenceGapKind::MissingOutputBinding { formal, .. } => call
            .callee
            .as_ref()
            .and_then(|callee| {
                document
                    .summaries
                    .iter()
                    .find(|summary| summary.program_dependence_graph == *callee)
            })
            .is_some_and(|summary| {
                summary.outputs.contains(formal)
                    && !call
                        .output_bindings
                        .iter()
                        .any(|binding| binding.formal == *formal)
            }),
        SystemDependenceGapKind::UnsupportedOutputKind { formal, .. } => call
            .callee
            .as_ref()
            .and_then(|callee| {
                document
                    .summaries
                    .iter()
                    .find(|summary| summary.program_dependence_graph == *callee)
            })
            .is_some_and(|summary| {
                summary.outputs.contains(formal)
                    && call
                        .output_bindings
                        .iter()
                        .any(|binding| binding.formal == *formal)
            }),
        SystemDependenceGapKind::CapabilityUnavailable {
            graph, capability, ..
        } => {
            let graph_is_participant = match capability {
                AdapterCapability::CallGraph => graph == &call.caller,
                AdapterCapability::Sdg => {
                    graph == &call.caller || call.callee.as_ref() == Some(graph)
                }
                _ => false,
            };
            graph_is_participant
                && document
                    .capabilities
                    .iter()
                    .find(|evidence| evidence.graph == *graph)
                    .is_some_and(|evidence| match capability {
                        AdapterCapability::CallGraph => {
                            evidence.call_graph_support != CapabilitySupport::Provided
                        }
                        AdapterCapability::Sdg => {
                            evidence.sdg_support != CapabilitySupport::Provided
                        }
                        _ => false,
                    })
        }
    };
    if valid {
        Ok(())
    } else {
        Err(SystemDependenceBuildError::Invalid(
            "system-dependence gap contradicts its call-site evidence".into(),
        ))
    }
}

fn gap_reason(kind: &SystemDependenceGapKind) -> String {
    match kind {
        SystemDependenceGapKind::UnresolvedOrNonLocalCallee { call } => {
            format!("call {} is unresolved or non-local", call.as_str())
        }
        SystemDependenceGapKind::MissingParameterBinding { call, formal } => format!(
            "call {} lacks parameter binding for {}",
            call.as_str(),
            formal.as_str()
        ),
        SystemDependenceGapKind::MissingOutputBinding { call, formal } => format!(
            "call {} lacks output binding for {}",
            call.as_str(),
            formal.as_str()
        ),
        SystemDependenceGapKind::UnsupportedOutputKind {
            call,
            formal,
            output_kind,
        } => format!(
            "call {} output {} has unsupported kind {:?}",
            call.as_str(),
            formal.as_str(),
            output_kind
        ),
        SystemDependenceGapKind::CapabilityUnavailable {
            call,
            graph,
            capability,
        } => format!(
            "call {} graph {} capability {} is unavailable",
            call.as_str(),
            graph.as_str(),
            capability.as_str()
        ),
    }
}

fn validate_capability_authority(
    capability: &str,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
) -> Result<(), SystemDependenceBuildError> {
    match (support, authority) {
        (CapabilitySupport::Provided, Some(_))
        | (CapabilitySupport::Unsupported | CapabilitySupport::Unknown, None) => Ok(()),
        _ => Err(SystemDependenceBuildError::Invalid(format!(
            "{capability} capability support and authority disagree"
        ))),
    }
}

fn validate_distinct_ordered_keys<T: Ord>(
    label: &str,
    values: &[T],
) -> Result<(), SystemDependenceBuildError> {
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        Err(SystemDependenceBuildError::Invalid(format!(
            "{label} contain duplicate keys"
        )))
    } else {
        Ok(())
    }
}

fn validate_sorted_by<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), SystemDependenceBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        Err(SystemDependenceBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )))
    } else {
        Ok(())
    }
}

fn validate_canonical<T: Ord>(label: &str, values: &[T]) -> Result<(), SystemDependenceBuildError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(SystemDependenceBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )))
    } else {
        Ok(())
    }
}

fn validate_text(value: &str) -> Result<(), SystemDependenceBuildError> {
    if value.trim().is_empty() || value.trim() != value {
        Err(SystemDependenceBuildError::Invalid(
            "system-dependence text must be canonical and nonempty".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), SystemDependenceBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(SystemDependenceBuildError::Invalid(format!(
            "identity must start with {prefix}"
        )));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SystemDependenceBuildError::Invalid(
            "identity must contain a canonical 32-byte hexadecimal digest".into(),
        ));
    }
    Ok(())
}

fn derive_id(domain: &str, prefix: &str, parts: &[&[u8]]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(domain.len() as u64).to_le_bytes());
    hasher.update(domain.as_bytes());
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    format!("{prefix}{}", hasher.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;

    use super::*;
    use crate::{
        BindingDraft, BindingForm, BindingTargetDraft, BuildContextId, ControlEdgeDraft,
        ControlEdgeKind, ControlEdgePrecision, ControlExitOutcome, ControlFlowBuilder,
        ControlFlowCoverageEvidence, ControlFlowGraphDraft, ControlFlowOwnerKind,
        ControlFlowPolicyId, ControlPointDraft, ControlPointKind, DataFlowAccessDraft,
        DataFlowBoundaryDraft, DataFlowBuilder, DataFlowDefinitionDraft, DataFlowEffectDraft,
        DataFlowEffectKind, DataFlowGraphDraft, DataFlowPolicyId, FactCoverageEvidence, Mutability,
        NameNamespace, NamespacePolicy, NonStructuredControlPolicyId, ProgramDependencePolicyId,
        ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft, ReferenceRole, RepositoryId,
        ResolutionPolicyId, ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind,
        VisibilityDraft, VisibilityKind, derive_control_regions,
        derive_non_structured_control_regions, derive_program_dependence,
    };

    fn analysis() -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let mut registry = deslop_lang::Registry::default();
        registry.register(&crate::data_flow::tests::DATA_FLOW_TEST_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("system-dependence-integration-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay(
            "system.dflowrs",
            b"fn inc(p: i32) -> i32 { p + 1 }\nfn run(x: i32) -> i32 { let y = inc(x); y }\n"
                .to_vec(),
        )
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn nodes_by_kind(analysis: &ProjectAnalysis, kind: &str) -> Vec<crate::NodeId> {
        analysis
            .node_ids()
            .filter(|node| analysis.node(*node).unwrap().raw_kind() == kind)
            .collect()
    }

    fn nodes_by_text(analysis: &ProjectAnalysis, value: &str) -> Vec<crate::NodeId> {
        let mut nodes = analysis
            .node_ids()
            .filter(|node| {
                let view = analysis.node(*node).unwrap();
                view.raw_kind() == "identifier" && view.text() == value
            })
            .collect::<Vec<_>>();
        nodes.sort_by_key(|node| analysis.node(*node).unwrap().span().start_byte());
        nodes
    }

    fn roles(
        analysis: &Arc<ProjectAnalysis>,
        node: crate::NodeId,
    ) -> deslop_lang::CanonicalRoleSet {
        let path = analysis.node(node).unwrap().path().to_path_buf();
        analysis
            .canonical_role_projection(&path)
            .unwrap()
            .facts()
            .iter()
            .find(|fact| fact.node() == node)
            .unwrap()
            .roles()
    }

    fn visibility(scope: crate::ScopeFactId) -> VisibilityDraft {
        VisibilityDraft {
            kind: VisibilityKind::Scope,
            boundary: Some(scope),
            adapter_rule: None,
        }
    }

    fn copy_summaries(document: &SystemDependenceDocument, builder: &mut SystemDependenceBuilder) {
        for summary in document.summaries() {
            builder
                .add_summary(CallableSummaryDraft {
                    program_dependence_graph: summary.program_dependence_graph().clone(),
                    formal_inputs: summary.formal_inputs().to_vec(),
                    outputs: summary.outputs().to_vec(),
                    globals: summary
                        .globals()
                        .iter()
                        .map(|global| GlobalSummaryDraft {
                            declaration: global.declaration().clone(),
                            reads: global.reads().to_vec(),
                            writes: global.writes().to_vec(),
                            mutation_outputs: global.mutation_outputs().to_vec(),
                        })
                        .collect(),
                })
                .unwrap();
        }
    }

    fn digest<T: DeserializeOwned>(prefix: &str, ordinal: usize) -> T {
        serde_json::from_str(&format!("\"{prefix}{:064x}\"", ordinal)).unwrap()
    }

    #[test]
    fn m4_7_policy_identity_is_input_sensitive() {
        let first = SystemDependencePolicyId::from_parts(&[b"one"]).unwrap();
        let repeated = SystemDependencePolicyId::from_parts(&[b"one"]).unwrap();
        let changed = SystemDependencePolicyId::from_parts(&[b"two"]).unwrap();
        assert_eq!(first, repeated);
        assert_ne!(first, changed);
        assert!(SystemDependencePolicyId::from_parts(&[]).is_err());
    }

    #[test]
    fn m4_7_capability_evidence_requires_matching_authority() {
        let valid = SystemDependenceCapabilityEvidence {
            graph: digest("pdg1_", 1),
            call_graph_support: CapabilitySupport::Provided,
            call_graph_authority: Some(CapabilityAuthority::Adapter),
            sdg_support: CapabilitySupport::Unknown,
            sdg_authority: None,
        };
        assert!(valid.validate().is_ok());
        let missing_authority = SystemDependenceCapabilityEvidence {
            call_graph_authority: None,
            ..valid.clone()
        };
        assert!(missing_authority.validate().is_err());
        let spurious_authority = SystemDependenceCapabilityEvidence {
            sdg_authority: Some(CapabilityAuthority::Adapter),
            ..valid
        };
        assert!(spurious_authority.validate().is_err());
    }

    #[test]
    fn m4_7_capability_gap_identity_names_participant_graph() {
        let policy = SystemDependencePolicyId::from_parts(&[b"capability-gap"]).unwrap();
        let call: DataFlowAccessKey = digest("dfa1_", 1);
        let first = make_gap(
            &policy,
            SystemDependenceGapKind::CapabilityUnavailable {
                call: call.clone(),
                graph: digest("pdg1_", 1),
                capability: AdapterCapability::Sdg,
            },
        )
        .unwrap();
        let second = make_gap(
            &policy,
            SystemDependenceGapKind::CapabilityUnavailable {
                call,
                graph: digest("pdg1_", 2),
                capability: AdapterCapability::Sdg,
            },
        )
        .unwrap();
        assert_ne!(first.key(), second.key());
        assert_ne!(gap_reason(first.kind()), gap_reason(second.kind()));
    }

    #[test]
    fn m4_7_call_bindings_are_one_to_one() {
        let actual: DataFlowAccessKey = digest("dfa1_", 1);
        let parameters = vec![
            ParameterBinding {
                actual: actual.clone(),
                formal: digest("dfb1_", 1),
            },
            ParameterBinding {
                actual,
                formal: digest("dfb1_", 2),
            },
        ];
        assert!(validate_distinct_bindings(&parameters, &[]).is_err());
        let outputs = vec![
            OutputBinding {
                formal: digest("dfb1_", 3),
                receiving_definition: None,
            },
            OutputBinding {
                formal: digest("dfb1_", 3),
                receiving_definition: Some(digest("dfd1_", 1)),
            },
        ];
        assert!(validate_distinct_bindings(&[], &outputs).is_err());
    }

    #[test]
    fn m4_7_interprocedural_edge_identity_is_directional() {
        let policy = SystemDependencePolicyId::from_parts(&[b"edge-direction"]).unwrap();
        let from = SystemDependenceEndpoint {
            graph: digest("pdg1_", 1),
            node: digest("pdn1_", 1),
        };
        let to = SystemDependenceEndpoint {
            graph: digest("pdg1_", 2),
            node: digest("pdn1_", 2),
        };
        let kind = SystemDependenceEdgeKind::Call {
            call_site: digest("cst1_", 1),
            call: digest("dfa1_", 1),
        };
        let forward = make_edge(&policy, from.clone(), to.clone(), kind.clone()).unwrap();
        let reverse = make_edge(&policy, to, from, kind).unwrap();
        assert_ne!(forward.key(), reverse.key());
    }

    #[test]
    fn m4_7_coverage_status_and_reasons_fail_closed() {
        assert!(
            SystemDependenceCoverageEvidence {
                status: FactCoverage::Complete,
                reasons: vec!["unexpected gap".into()],
            }
            .validate()
            .is_err()
        );
        assert!(
            SystemDependenceCoverageEvidence {
                status: FactCoverage::Partial,
                reasons: vec![],
            }
            .validate()
            .is_err()
        );
        assert!(
            SystemDependenceCoverageEvidence {
                status: FactCoverage::Partial,
                reasons: vec!["exact gap".into()],
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn m4_7_digest_wire_rejects_malformed_identity() {
        assert!(serde_json::from_str::<SystemDependencePolicyId>("\"sdp1_bad\"").is_err());
        assert!(
            serde_json::from_str::<CallableSummaryKey>(&format!("\"wrong_{:064x}\"", 1)).is_err()
        );
    }

    #[test]
    fn m4_7_two_callable_summaries_emit_exact_call_parameter_return_and_global_facts() {
        let analysis = analysis();
        let root = nodes_by_kind(&analysis, "source_file")[0];
        let functions = nodes_by_kind(&analysis, "function_item");
        let blocks = nodes_by_kind(&analysis, "block");
        let call_node = nodes_by_kind(&analysis, "call_expression")[0];
        let inc_expression = nodes_by_kind(&analysis, "binary_expression")[0];
        let incs = nodes_by_text(&analysis, "inc");
        let ps = nodes_by_text(&analysis, "p");
        let xs = nodes_by_text(&analysis, "x");
        let ys = nodes_by_text(&analysis, "y");
        assert_eq!(functions.len(), 2);
        assert_eq!(blocks.len(), 2);
        assert_eq!(incs.len(), 2);
        assert_eq!(ps.len(), 2);
        assert_eq!(xs.len(), 2);
        assert_eq!(ys.len(), 2);

        let complete = FactCoverageEvidence::complete();
        let namespaces = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let mut scope_builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"system-dependence-target"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"system-dependence-scope/1"]).unwrap(),
        )
        .unwrap();
        let file_scope = scope_builder
            .add_scope(
                root,
                roles(&analysis, root),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let inc_scope = scope_builder
            .add_scope(
                functions[0],
                roles(&analysis, functions[0]),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let run_scope = scope_builder
            .add_scope(
                functions[1],
                roles(&analysis, functions[1]),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();
        let declaration = |name: &str, scope| crate::DeclarationDraft {
            original_name: name.into(),
            lookup_key: name.into(),
            namespace: NameNamespace::Value,
            scope,
            visibility: visibility(scope),
            modifiers: vec![],
        };
        let inc_declaration = scope_builder
            .add_declaration(
                functions[0],
                roles(&analysis, functions[0]),
                complete.clone(),
                declaration("inc", file_scope),
            )
            .unwrap();
        scope_builder
            .add_binding(
                functions[0],
                roles(&analysis, functions[0]),
                complete.clone(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(inc_declaration),
                    form: BindingForm::Declaration,
                    timing: crate::BindingTiming::AtDeclaration,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        scope_builder
            .add_declaration(
                functions[1],
                roles(&analysis, functions[1]),
                complete.clone(),
                declaration("run", file_scope),
            )
            .unwrap();
        let global_declaration = scope_builder
            .add_declaration(
                functions[1],
                roles(&analysis, functions[1]),
                complete.clone(),
                declaration("global", file_scope),
            )
            .unwrap();
        scope_builder
            .add_binding(
                functions[1],
                roles(&analysis, functions[1]),
                complete.clone(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(global_declaration),
                    form: BindingForm::Declaration,
                    timing: crate::BindingTiming::AtDeclaration,
                    mutability: Mutability::Mutable,
                },
            )
            .unwrap();
        let p_declaration = scope_builder
            .add_declaration(
                ps[0],
                roles(&analysis, ps[0]),
                complete.clone(),
                declaration("p", inc_scope),
            )
            .unwrap();
        let p_binding = scope_builder
            .add_binding(
                ps[0],
                roles(&analysis, ps[0]),
                complete.clone(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(p_declaration),
                    form: BindingForm::Parameter,
                    timing: crate::BindingTiming::ScopeEntry,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let x_declaration = scope_builder
            .add_declaration(
                xs[0],
                roles(&analysis, xs[0]),
                complete.clone(),
                declaration("x", run_scope),
            )
            .unwrap();
        let x_binding = scope_builder
            .add_binding(
                xs[0],
                roles(&analysis, xs[0]),
                complete.clone(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(x_declaration),
                    form: BindingForm::Parameter,
                    timing: crate::BindingTiming::ScopeEntry,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let y_declaration = scope_builder
            .add_declaration(
                ys[0],
                roles(&analysis, ys[0]),
                complete.clone(),
                declaration("y", run_scope),
            )
            .unwrap();
        let y_binding = scope_builder
            .add_binding(
                ys[0],
                roles(&analysis, ys[0]),
                complete.clone(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(y_declaration),
                    form: BindingForm::Declaration,
                    timing: crate::BindingTiming::AfterInitializer,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let reference = |name: &str, scope, role| ReferenceDraft {
            original_spelling: name.into(),
            segments: vec![name.into()],
            namespace: NameNamespace::Value,
            scope,
            role,
        };
        let p_read = scope_builder
            .add_reference(
                ps[1],
                roles(&analysis, ps[1]),
                complete.clone(),
                reference("p", inc_scope, ReferenceRole::Read),
            )
            .unwrap();
        let inc_call = scope_builder
            .add_reference(
                incs[1],
                roles(&analysis, incs[1]),
                complete.clone(),
                reference("inc", run_scope, ReferenceRole::Call),
            )
            .unwrap();
        let x_read = scope_builder
            .add_reference(
                xs[1],
                roles(&analysis, xs[1]),
                complete.clone(),
                reference("x", run_scope, ReferenceRole::Read),
            )
            .unwrap();
        let y_read = scope_builder
            .add_reference(
                ys[1],
                roles(&analysis, ys[1]),
                complete.clone(),
                reference("y", run_scope, ReferenceRole::Read),
            )
            .unwrap();
        let global_read = scope_builder
            .add_reference(
                ys[1],
                roles(&analysis, ys[1]),
                complete,
                reference("global", run_scope, ReferenceRole::Read),
            )
            .unwrap();
        let scope_graph = Arc::new(scope_builder.build().unwrap());
        let key = |id| scope_graph.fact(id).unwrap().key().clone();
        let inc_declaration_key = key(inc_declaration);
        let global_declaration_key = key(global_declaration);
        let p_declaration_key = key(p_declaration);
        let p_binding_key = key(p_binding);
        let x_declaration_key = key(x_declaration);
        let x_binding_key = key(x_binding);
        let y_declaration_key = key(y_declaration);
        let y_binding_key = key(y_binding);
        let p_read_key = key(p_read);
        let inc_call_key = key(inc_call);
        let x_read_key = key(x_read);
        let y_read_key = key(y_read);
        let global_read_key = key(global_read);
        let resolution = Arc::new(
            crate::ResolutionProjection::build(
                scope_graph,
                ResolutionPolicyId::from_parts(&[b"system-dependence-resolution/1"]).unwrap(),
            )
            .unwrap(),
        );
        for result in resolution.results() {
            assert_eq!(
                result.wire().status(),
                ResolutionStatus::Unique,
                "non-unique fixture resolution: {:#?}",
                result.wire()
            );
        }

        let mut flow_builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"system-dependence-cfg/1"]).unwrap(),
        );
        for (owner, syntax, block) in [
            (functions[0], inc_expression, blocks[0]),
            (functions[1], call_node, blocks[1]),
        ] {
            flow_builder
                .add_graph(ControlFlowGraphDraft {
                    owner,
                    owner_kind: ControlFlowOwnerKind::Callable,
                    coverage: ControlFlowCoverageEvidence::complete(),
                    points: vec![
                        ControlPointDraft {
                            kind: ControlPointKind::Entry,
                            source: None,
                            ordinal: 0,
                        },
                        ControlPointDraft {
                            kind: ControlPointKind::Syntax,
                            source: Some(syntax),
                            ordinal: 0,
                        },
                        ControlPointDraft {
                            kind: ControlPointKind::Syntax,
                            source: Some(if owner == functions[1] { ys[1] } else { syntax }),
                            ordinal: 1,
                        },
                        ControlPointDraft {
                            kind: ControlPointKind::Synthetic(
                                crate::ControlSyntheticPointKind::ExitDispatch,
                            ),
                            source: Some(block),
                            ordinal: 0,
                        },
                        ControlPointDraft {
                            kind: ControlPointKind::Exit,
                            source: None,
                            ordinal: 0,
                        },
                    ],
                    edges: vec![
                        ControlEdgeDraft {
                            from: 0,
                            to: 1,
                            kind: ControlEdgeKind::Entry,
                            source: owner,
                            predicate: None,
                            precision: ControlEdgePrecision::Exact,
                        },
                        ControlEdgeDraft {
                            from: 1,
                            to: 2,
                            kind: ControlEdgeKind::Normal,
                            source: owner,
                            predicate: None,
                            precision: ControlEdgePrecision::Exact,
                        },
                        ControlEdgeDraft {
                            from: 2,
                            to: 3,
                            kind: ControlEdgeKind::Normal,
                            source: owner,
                            predicate: None,
                            precision: ControlEdgePrecision::Exact,
                        },
                        ControlEdgeDraft {
                            from: 3,
                            to: 4,
                            kind: ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                            source: owner,
                            predicate: None,
                            precision: ControlEdgePrecision::Exact,
                        },
                    ],
                })
                .unwrap();
        }
        let flow = Arc::new(flow_builder.build().unwrap());
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                crate::ControlRegionPolicyId::from_parts(&[b"system-dependence-regions/1"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let graph_for_owner = |owner| {
            flow.document()
                .graphs()
                .iter()
                .find(|graph| graph.owner() == analysis.node_key(owner).unwrap())
                .unwrap()
        };
        let point_for_source = |graph: &crate::ControlFlowGraph, node| {
            graph
                .points()
                .iter()
                .find(|point| point.source() == Some(analysis.node_key(node).unwrap()))
                .unwrap()
                .key()
                .clone()
        };
        let inc_flow = graph_for_owner(functions[0]);
        let run_flow = graph_for_owner(functions[1]);
        let inc_expression_point = point_for_source(inc_flow, inc_expression);
        let call_point = point_for_source(run_flow, call_node);
        let tail_point = point_for_source(run_flow, ys[1]);
        let effects = |graph: &crate::ControlFlowGraph| {
            graph
                .points()
                .iter()
                .map(|point| DataFlowEffectDraft {
                    point: point.key().clone(),
                    effects: (point.key() == graph.exit())
                        .then_some(vec![DataFlowEffectKind::Returns])
                        .unwrap_or_default(),
                    uncertainty: None,
                })
                .collect::<Vec<_>>()
        };
        let mut data_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"system-dependence-dataflow/1"]).unwrap(),
        )
        .unwrap();
        data_builder
            .add_graph(DataFlowGraphDraft {
                control_flow_graph: inc_flow.key().clone(),
                definitions: vec![DataFlowDefinitionDraft {
                    point: inc_flow.entry().clone(),
                    declaration: p_declaration_key.clone(),
                    source_fact: p_binding_key.clone(),
                    ordinal: 0,
                }],
                accesses: vec![DataFlowAccessDraft {
                    point: inc_expression_point,
                    reference: p_read_key.clone(),
                    kind: DataFlowAccessKind::Read,
                    ordinal: 0,
                }],
                boundaries: vec![
                    DataFlowBoundaryDraft {
                        point: inc_flow.entry().clone(),
                        kind: DataFlowBoundaryKind::ParameterInput,
                        declaration: Some(p_declaration_key.clone()),
                        source_fact: p_binding_key,
                    },
                    DataFlowBoundaryDraft {
                        point: inc_flow.exit().clone(),
                        kind: DataFlowBoundaryKind::ReturnOutput,
                        declaration: Some(p_declaration_key.clone()),
                        source_fact: p_read_key.clone(),
                    },
                    DataFlowBoundaryDraft {
                        point: inc_flow.exit().clone(),
                        kind: DataFlowBoundaryKind::MutationOutput,
                        declaration: Some(p_declaration_key),
                        source_fact: p_read_key,
                    },
                ],
                effects: effects(inc_flow),
            })
            .unwrap();
        data_builder
            .add_graph(DataFlowGraphDraft {
                control_flow_graph: run_flow.key().clone(),
                definitions: vec![
                    DataFlowDefinitionDraft {
                        point: run_flow.entry().clone(),
                        declaration: x_declaration_key.clone(),
                        source_fact: x_binding_key.clone(),
                        ordinal: 0,
                    },
                    DataFlowDefinitionDraft {
                        point: run_flow.entry().clone(),
                        declaration: global_declaration_key.clone(),
                        source_fact: global_declaration_key.clone(),
                        ordinal: 1,
                    },
                    DataFlowDefinitionDraft {
                        point: call_point.clone(),
                        declaration: y_declaration_key.clone(),
                        source_fact: y_binding_key,
                        ordinal: 2,
                    },
                ],
                accesses: vec![
                    DataFlowAccessDraft {
                        point: call_point.clone(),
                        reference: x_read_key.clone(),
                        kind: DataFlowAccessKind::Read,
                        ordinal: 0,
                    },
                    DataFlowAccessDraft {
                        point: call_point,
                        reference: inc_call_key.clone(),
                        kind: DataFlowAccessKind::Call,
                        ordinal: 1,
                    },
                    DataFlowAccessDraft {
                        point: tail_point.clone(),
                        reference: y_read_key.clone(),
                        kind: DataFlowAccessKind::Read,
                        ordinal: 0,
                    },
                    DataFlowAccessDraft {
                        point: tail_point,
                        reference: global_read_key.clone(),
                        kind: DataFlowAccessKind::Read,
                        ordinal: 1,
                    },
                ],
                boundaries: vec![
                    DataFlowBoundaryDraft {
                        point: run_flow.entry().clone(),
                        kind: DataFlowBoundaryKind::ParameterInput,
                        declaration: Some(x_declaration_key),
                        source_fact: x_binding_key,
                    },
                    DataFlowBoundaryDraft {
                        point: run_flow.exit().clone(),
                        kind: DataFlowBoundaryKind::ReturnOutput,
                        declaration: Some(y_declaration_key.clone()),
                        source_fact: y_read_key,
                    },
                    DataFlowBoundaryDraft {
                        point: run_flow.exit().clone(),
                        kind: DataFlowBoundaryKind::MutationOutput,
                        declaration: Some(global_declaration_key.clone()),
                        source_fact: global_read_key.clone(),
                    },
                ],
                effects: effects(run_flow),
            })
            .unwrap();
        let data = Arc::new(data_builder.build().unwrap());
        let non_structured = Arc::new(
            derive_non_structured_control_regions(
                regions,
                NonStructuredControlPolicyId::from_parts(&[b"system-dependence-non-structured/1"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let pdg = Arc::new(
            derive_program_dependence(
                Arc::clone(&data),
                non_structured,
                ProgramDependencePolicyId::from_parts(&[b"system-dependence-pdg/1"]).unwrap(),
            )
            .unwrap(),
        );
        assert!(
            pdg.document()
                .graphs()
                .iter()
                .all(|graph| graph.coverage().status() == FactCoverage::Complete)
        );
        let pdg_for_flow = |flow_key| {
            pdg.document()
                .graphs()
                .iter()
                .find(|graph| graph.control_flow_graph() == flow_key)
                .unwrap()
        };
        let inc_pdg = pdg_for_flow(inc_flow.key());
        let run_pdg = pdg_for_flow(run_flow.key());
        let data_for_flow = |flow_key| {
            data.document()
                .graphs()
                .iter()
                .find(|graph| graph.control_flow_graph() == flow_key)
                .unwrap()
        };
        let inc_data = data_for_flow(inc_flow.key());
        let run_data = data_for_flow(run_flow.key());
        let inc_parameter = inc_data
            .boundaries()
            .iter()
            .find(|boundary| boundary.kind() == DataFlowBoundaryKind::ParameterInput)
            .unwrap()
            .key()
            .clone();
        let inc_return = inc_data
            .boundaries()
            .iter()
            .find(|boundary| boundary.kind() == DataFlowBoundaryKind::ReturnOutput)
            .unwrap()
            .key()
            .clone();
        let inc_mutation = inc_data
            .boundaries()
            .iter()
            .find(|boundary| boundary.kind() == DataFlowBoundaryKind::MutationOutput)
            .unwrap()
            .key()
            .clone();
        let run_parameter = run_data
            .boundaries()
            .iter()
            .find(|boundary| boundary.kind() == DataFlowBoundaryKind::ParameterInput)
            .unwrap()
            .key()
            .clone();
        let run_return = run_data
            .boundaries()
            .iter()
            .find(|boundary| boundary.kind() == DataFlowBoundaryKind::ReturnOutput)
            .unwrap()
            .key()
            .clone();
        let global_mutation = run_data
            .boundaries()
            .iter()
            .find(|boundary| boundary.kind() == DataFlowBoundaryKind::MutationOutput)
            .unwrap()
            .key()
            .clone();
        let actual_x = run_data
            .accesses()
            .iter()
            .find(|access| access.reference() == &x_read_key)
            .unwrap()
            .key()
            .clone();
        let call = run_data
            .accesses()
            .iter()
            .find(|access| access.reference() == &inc_call_key)
            .unwrap()
            .key()
            .clone();
        let global_access = run_data
            .accesses()
            .iter()
            .find(|access| access.reference() == &global_read_key)
            .unwrap()
            .key()
            .clone();
        let global_definition = run_data
            .definitions()
            .iter()
            .find(|definition| {
                run_data
                    .symbols()
                    .iter()
                    .find(|symbol| symbol.key() == definition.symbol())
                    .is_some_and(|symbol| symbol.declaration() == &global_declaration_key)
            })
            .unwrap()
            .key()
            .clone();
        let y_definition = run_data
            .definitions()
            .iter()
            .find(|definition| {
                run_data
                    .symbols()
                    .iter()
                    .find(|symbol| symbol.key() == definition.symbol())
                    .is_some_and(|symbol| symbol.declaration() == &y_declaration_key)
            })
            .unwrap()
            .key()
            .clone();

        let mut builder = SystemDependenceBuilder::new(
            Arc::clone(&pdg),
            SystemDependencePolicyId::from_parts(&[b"system-dependence/1"]).unwrap(),
        );
        builder
            .add_summary(CallableSummaryDraft {
                program_dependence_graph: inc_pdg.key().clone(),
                formal_inputs: vec![inc_parameter.clone()],
                outputs: vec![inc_return.clone(), inc_mutation.clone()],
                globals: vec![],
            })
            .unwrap();
        builder
            .add_summary(CallableSummaryDraft {
                program_dependence_graph: run_pdg.key().clone(),
                formal_inputs: vec![run_parameter],
                outputs: vec![run_return, global_mutation.clone()],
                globals: vec![GlobalSummaryDraft {
                    declaration: global_declaration_key,
                    reads: vec![global_access],
                    writes: vec![global_definition],
                    mutation_outputs: vec![global_mutation],
                }],
            })
            .unwrap();
        builder.add_call_site(CallSiteDraft {
            caller: run_pdg.key().clone(),
            call,
            parameter_bindings: vec![ParameterBindingDraft {
                actual: actual_x,
                formal: inc_parameter,
            }],
            output_bindings: vec![
                OutputBindingDraft {
                    formal: inc_return,
                    receiving_definition: Some(y_definition),
                },
                OutputBindingDraft {
                    formal: inc_mutation,
                    receiving_definition: None,
                },
            ],
        });
        let system = builder.build().unwrap();
        assert_eq!(
            system.document().coverage().status(),
            FactCoverage::Complete
        );
        assert_eq!(system.document().summaries().len(), 2);
        assert_eq!(system.document().calls().len(), 1);
        assert!(system.document().gaps().is_empty());
        assert_eq!(system.document().edges().len(), 4);
        assert_eq!(
            system
                .document()
                .edges()
                .iter()
                .filter(|edge| matches!(edge.kind(), SystemDependenceEdgeKind::Call { .. }))
                .count(),
            1
        );
        assert_eq!(
            system
                .document()
                .edges()
                .iter()
                .filter(|edge| matches!(edge.kind(), SystemDependenceEdgeKind::ParameterOut { .. }))
                .count(),
            1
        );
        assert_eq!(
            system
                .document()
                .edges()
                .iter()
                .filter(|edge| matches!(edge.kind(), SystemDependenceEdgeKind::ParameterIn { .. }))
                .count(),
            1
        );
        assert_eq!(
            system
                .document()
                .edges()
                .iter()
                .filter(|edge| matches!(edge.kind(), SystemDependenceEdgeKind::Return { .. }))
                .count(),
            1
        );
        assert_eq!(
            system
                .document()
                .summaries()
                .iter()
                .flat_map(|summary| summary.globals())
                .count(),
            1
        );
        let bytes = serde_json::to_vec(system.document()).unwrap();
        let decoded: SystemDependenceDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);

        let mut corrupted: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        corrupted["edges"][0]["to"]["node"] = corrupted["edges"][0]["from"]["node"].clone();
        assert!(serde_json::from_value::<SystemDependenceDocument>(corrupted).is_err());

        let mut wrong_schema: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        wrong_schema["schema"] = "deslop.system-dependence/999".into();
        assert!(serde_json::from_value::<SystemDependenceDocument>(wrong_schema).is_err());

        let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("untrusted".into(), true.into());
        assert!(serde_json::from_value::<SystemDependenceDocument>(unknown).is_err());

        let changed_policy =
            SystemDependencePolicyId::from_parts(&[b"system-dependence/2"]).unwrap();
        assert_ne!(changed_policy, *system.document().policy());
        let call_site = &system.document().calls()[0];
        let mut changed_builder =
            SystemDependenceBuilder::new(Arc::clone(&pdg), changed_policy.clone());
        copy_summaries(system.document(), &mut changed_builder);
        changed_builder.add_call_site(CallSiteDraft {
            caller: call_site.caller().clone(),
            call: call_site.call().clone(),
            parameter_bindings: call_site
                .parameter_bindings()
                .iter()
                .map(|binding| ParameterBindingDraft {
                    actual: binding.actual().clone(),
                    formal: binding.formal().clone(),
                })
                .collect(),
            output_bindings: call_site
                .output_bindings()
                .iter()
                .map(|binding| OutputBindingDraft {
                    formal: binding.formal().clone(),
                    receiving_definition: binding.receiving_definition().cloned(),
                })
                .collect(),
        });
        let changed = changed_builder.build().unwrap();
        assert_ne!(changed.id(), system.id());
        assert_ne!(
            changed.document().summaries()[0].key(),
            system.document().summaries()[0].key()
        );

        let mut missing_parameter = SystemDependenceBuilder::new(
            Arc::clone(&pdg),
            SystemDependencePolicyId::from_parts(&[b"system-dependence/missing-parameter"])
                .unwrap(),
        );
        copy_summaries(system.document(), &mut missing_parameter);
        missing_parameter.add_call_site(CallSiteDraft {
            caller: call_site.caller().clone(),
            call: call_site.call().clone(),
            parameter_bindings: vec![],
            output_bindings: call_site
                .output_bindings()
                .iter()
                .map(|binding| OutputBindingDraft {
                    formal: binding.formal().clone(),
                    receiving_definition: binding.receiving_definition().cloned(),
                })
                .collect(),
        });
        let missing_parameter = missing_parameter.build().unwrap();
        assert_eq!(
            missing_parameter.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(
            missing_parameter
                .document()
                .gaps()
                .iter()
                .any(|gap| matches!(
                    gap.kind(),
                    SystemDependenceGapKind::MissingParameterBinding { .. }
                ))
        );
        assert!(
            !missing_parameter
                .document()
                .edges()
                .iter()
                .any(|edge| matches!(edge.kind(), SystemDependenceEdgeKind::ParameterIn { .. }))
        );

        let mut missing_output = SystemDependenceBuilder::new(
            pdg,
            SystemDependencePolicyId::from_parts(&[b"system-dependence/missing-output"]).unwrap(),
        );
        copy_summaries(system.document(), &mut missing_output);
        missing_output.add_call_site(CallSiteDraft {
            caller: call_site.caller().clone(),
            call: call_site.call().clone(),
            parameter_bindings: call_site
                .parameter_bindings()
                .iter()
                .map(|binding| ParameterBindingDraft {
                    actual: binding.actual().clone(),
                    formal: binding.formal().clone(),
                })
                .collect(),
            output_bindings: vec![],
        });
        let missing_output = missing_output.build().unwrap();
        assert_eq!(
            missing_output.document().coverage().status(),
            FactCoverage::Partial
        );
        assert_eq!(
            missing_output
                .document()
                .gaps()
                .iter()
                .filter(|gap| matches!(
                    gap.kind(),
                    SystemDependenceGapKind::MissingOutputBinding { .. }
                ))
                .count(),
            2
        );
        assert!(
            !missing_output
                .document()
                .edges()
                .iter()
                .any(|edge| matches!(
                    edge.kind(),
                    SystemDependenceEdgeKind::Return { .. }
                        | SystemDependenceEdgeKind::ParameterOut { .. }
                ))
        );
        let inc_resolution = resolution
            .results()
            .iter()
            .find(|result| result.wire().reference() == &inc_call_key)
            .unwrap();
        assert!(matches!(
            inc_resolution.wire().preferred().unwrap().endpoints(),
            [crate::ResolutionEndpoint::Declaration(endpoint)] if endpoint == &inc_declaration_key
        ));
    }
}
