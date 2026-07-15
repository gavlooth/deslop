use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ArchitectureComponentKey, ArchitectureGapKey, ArchitectureLevel, ArchitectureProjection,
    DataFlowAccessKey, DataFlowDefinitionKey, DataFlowGraphKey, DataFlowPolicyId,
    DataFlowProjection, DependencyEdgeKey, DependencyEdgeKind, DependencyNodeKey,
    DependencyNodeKind, FactCoverage, ProjectionId, ResolutionResultKey,
};

pub const CYCLE_SEAM_SCHEMA: &str = "deslop.cycle-seams/1";
pub const CYCLE_SEAM_POLICY_SCHEMA: &str = "deslop.cycle-seam-policy/1";

const POLICY_DOMAIN: &str = "deslop cycle seam policy v1";
const CANDIDATE_DOMAIN: &str = "deslop cycle seam candidate v1";
const GAP_DOMAIN: &str = "deslop cycle seam gap v1";

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

digest_id!(CycleSeamPolicyId, "csp1_");
digest_id!(CycleSeamCandidateKey, "csc1_");
digest_id!(CycleSeamGapKey, "csg1_");

impl CycleSeamPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, CycleSeamBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(invalid(
                "cycle-seam policy identity requires nonempty parts",
            ));
        }
        Ok(Self(derive_id(POLICY_DOMAIN, "csp1_", parts)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CycleSeamDisposition {
    ReviewRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CycleSeamAction {
    ExtractTargetApiBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleSeamCost {
    authority_penalty: usize,
    api_surface: usize,
    reaching_definitions: usize,
    data_flow_accesses: usize,
    resolutions: usize,
}

impl CycleSeamCost {
    pub fn authority_penalty(&self) -> usize {
        self.authority_penalty
    }
    pub fn api_surface(&self) -> usize {
        self.api_surface
    }
    pub fn reaching_definitions(&self) -> usize {
        self.reaching_definitions
    }
    pub fn data_flow_accesses(&self) -> usize {
        self.data_flow_accesses
    }
    pub fn resolutions(&self) -> usize {
        self.resolutions
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleSeamCandidate {
    key: CycleSeamCandidateKey,
    component: ArchitectureComponentKey,
    level: ArchitectureLevel,
    cut_edge: DependencyEdgeKey,
    from: DependencyNodeKey,
    to: DependencyNodeKey,
    action: CycleSeamAction,
    disposition: CycleSeamDisposition,
    api_use_edges: Vec<DependencyEdgeKey>,
    api_nodes: Vec<DependencyNodeKey>,
    resolutions: Vec<ResolutionResultKey>,
    data_flow_accesses: Vec<DataFlowAccessKey>,
    reaching_definitions: Vec<DataFlowDefinitionKey>,
    evidence_coverage: FactCoverage,
    cost: CycleSeamCost,
    rank: u32,
    review_obligations: Vec<String>,
}

impl CycleSeamCandidate {
    pub fn key(&self) -> &CycleSeamCandidateKey {
        &self.key
    }
    pub fn component(&self) -> &ArchitectureComponentKey {
        &self.component
    }
    pub fn level(&self) -> ArchitectureLevel {
        self.level
    }
    pub fn cut_edge(&self) -> &DependencyEdgeKey {
        &self.cut_edge
    }
    pub fn from(&self) -> &DependencyNodeKey {
        &self.from
    }
    pub fn to(&self) -> &DependencyNodeKey {
        &self.to
    }
    pub fn action(&self) -> CycleSeamAction {
        self.action
    }
    pub fn disposition(&self) -> CycleSeamDisposition {
        self.disposition
    }
    pub fn api_use_edges(&self) -> &[DependencyEdgeKey] {
        &self.api_use_edges
    }
    pub fn api_nodes(&self) -> &[DependencyNodeKey] {
        &self.api_nodes
    }
    pub fn resolutions(&self) -> &[ResolutionResultKey] {
        &self.resolutions
    }
    pub fn data_flow_accesses(&self) -> &[DataFlowAccessKey] {
        &self.data_flow_accesses
    }
    pub fn reaching_definitions(&self) -> &[DataFlowDefinitionKey] {
        &self.reaching_definitions
    }
    pub fn evidence_coverage(&self) -> FactCoverage {
        self.evidence_coverage
    }
    pub fn cost(&self) -> &CycleSeamCost {
        &self.cost
    }
    pub fn rank(&self) -> u32 {
        self.rank
    }
    pub fn review_obligations(&self) -> &[String] {
        &self.review_obligations
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CycleSeamGapKind {
    SourceArchitecture {
        gap: ArchitectureGapKey,
    },
    MissingDataFlowProjection,
    DataFlowSourceMismatch {
        expected: ProjectionId,
        actual: ProjectionId,
    },
    TopologyWithoutApiEvidence {
        component: ArchitectureComponentKey,
        edge: DependencyEdgeKey,
    },
    EdgeEvidenceWithoutResolution {
        component: ArchitectureComponentKey,
        edge: DependencyEdgeKey,
    },
    ResolutionWithoutApiUse {
        component: ArchitectureComponentKey,
        edge: DependencyEdgeKey,
        resolution: ResolutionResultKey,
    },
    ResolutionWithoutDataFlowAccess {
        component: ArchitectureComponentKey,
        edge: DependencyEdgeKey,
        resolution: ResolutionResultKey,
    },
    IncompleteDataFlowGraph {
        graph: DataFlowGraphKey,
        status: FactCoverage,
    },
    UncertainDataFlowAccess {
        access: DataFlowAccessKey,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleSeamGap {
    key: CycleSeamGapKey,
    kind: CycleSeamGapKind,
}

impl CycleSeamGap {
    pub fn key(&self) -> &CycleSeamGapKey {
        &self.key
    }
    pub fn kind(&self) -> &CycleSeamGapKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleSeamCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

impl CycleSeamCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CycleSeamDocument {
    schema: String,
    projection_id: ProjectionId,
    architecture_projection_id: ProjectionId,
    dependency_projection_id: ProjectionId,
    resolution_projection_id: ProjectionId,
    data_flow_projection_id: Option<ProjectionId>,
    data_flow_policy: Option<DataFlowPolicyId>,
    policy: CycleSeamPolicyId,
    coverage: CycleSeamCoverageEvidence,
    candidates: Vec<CycleSeamCandidate>,
    gaps: Vec<CycleSeamGap>,
}

impl CycleSeamDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }
    pub fn architecture_projection_id(&self) -> &ProjectionId {
        &self.architecture_projection_id
    }
    pub fn dependency_projection_id(&self) -> &ProjectionId {
        &self.dependency_projection_id
    }
    pub fn resolution_projection_id(&self) -> &ProjectionId {
        &self.resolution_projection_id
    }
    pub fn data_flow_projection_id(&self) -> Option<&ProjectionId> {
        self.data_flow_projection_id.as_ref()
    }
    pub fn data_flow_policy(&self) -> Option<&DataFlowPolicyId> {
        self.data_flow_policy.as_ref()
    }
    pub fn policy(&self) -> &CycleSeamPolicyId {
        &self.policy
    }
    pub fn coverage(&self) -> &CycleSeamCoverageEvidence {
        &self.coverage
    }
    pub fn candidates(&self) -> &[CycleSeamCandidate] {
        &self.candidates
    }
    pub fn gaps(&self) -> &[CycleSeamGap] {
        &self.gaps
    }

    fn validate(&self) -> Result<(), CycleSeamBuildError> {
        if self.schema != CYCLE_SEAM_SCHEMA {
            return Err(invalid("unsupported cycle-seam schema"));
        }
        for id in [
            &self.projection_id,
            &self.architecture_projection_id,
            &self.dependency_projection_id,
            &self.resolution_projection_id,
        ] {
            validate_digest(id.as_str(), "pj1_")?;
        }
        if let Some(id) = &self.data_flow_projection_id {
            validate_digest(id.as_str(), "pj1_")?;
        }
        if self.data_flow_projection_id.is_some() != self.data_flow_policy.is_some() {
            return Err(invalid("cycle-seam data-flow identity is incomplete"));
        }
        validate_digest(self.policy.as_str(), "csp1_")?;
        validate_sorted("cycle-seam candidates", &self.candidates, |item| {
            item.key.as_str()
        })?;
        validate_sorted("cycle-seam gaps", &self.gaps, |item| item.key.as_str())?;
        let mut cuts = BTreeSet::new();
        let mut ranks = BTreeMap::<&ArchitectureComponentKey, BTreeSet<u32>>::new();
        for candidate in &self.candidates {
            if candidate.disposition != CycleSeamDisposition::ReviewRequired
                || candidate.action != CycleSeamAction::ExtractTargetApiBoundary
                || candidate.from == candidate.to
                || candidate.rank == 0
                || candidate.api_use_edges.is_empty()
                || candidate.api_nodes.is_empty()
                || candidate.resolutions.is_empty()
                || !matches!(
                    candidate.evidence_coverage,
                    FactCoverage::Complete | FactCoverage::Partial
                )
                || (candidate.evidence_coverage == FactCoverage::Complete
                    && candidate.data_flow_accesses.is_empty())
            {
                return Err(invalid("cycle-seam candidate contract is invalid"));
            }
            for values in [
                candidate
                    .api_use_edges
                    .iter()
                    .map(|key| key.as_str())
                    .collect::<Vec<_>>(),
                candidate.api_nodes.iter().map(|key| key.as_str()).collect(),
                candidate
                    .resolutions
                    .iter()
                    .map(|key| key.as_str())
                    .collect(),
                candidate
                    .data_flow_accesses
                    .iter()
                    .map(|key| key.as_str())
                    .collect(),
                candidate
                    .reaching_definitions
                    .iter()
                    .map(|key| key.as_str())
                    .collect(),
            ] {
                if !strictly_sorted(&values) && values.len() > 1 {
                    return Err(invalid("cycle-seam evidence is not canonical"));
                }
            }
            if !strictly_sorted(&candidate.review_obligations)
                || candidate.review_obligations != review_obligations()
            {
                return Err(invalid("cycle-seam review obligations are invalid"));
            }
            if candidate.cost.api_surface != candidate.api_nodes.len()
                || candidate.cost.reaching_definitions != candidate.reaching_definitions.len()
                || candidate.cost.data_flow_accesses != candidate.data_flow_accesses.len()
                || candidate.cost.resolutions != candidate.resolutions.len()
                || (candidate.cost.authority_penalty == 0)
                    != (candidate.evidence_coverage == FactCoverage::Complete)
                || candidate.cost.authority_penalty > 1
            {
                return Err(invalid("cycle-seam cost does not match evidence"));
            }
            if !cuts.insert((&candidate.component, &candidate.cut_edge))
                || !ranks
                    .entry(&candidate.component)
                    .or_default()
                    .insert(candidate.rank)
            {
                return Err(invalid("cycle-seam candidate cut or rank repeats"));
            }
            if candidate.key
                != make_candidate_key(&self.architecture_projection_id, &self.policy, candidate)?
            {
                return Err(invalid("cycle-seam candidate key does not match payload"));
            }
        }
        for (component, actual) in ranks {
            let expected = (1..=actual.len() as u32).collect::<BTreeSet<_>>();
            if actual != expected {
                return Err(invalid(format!(
                    "cycle-seam ranks are not contiguous for {}",
                    component.as_str()
                )));
            }
        }
        let mut candidates_by_component =
            BTreeMap::<&ArchitectureComponentKey, Vec<&CycleSeamCandidate>>::new();
        for candidate in &self.candidates {
            candidates_by_component
                .entry(&candidate.component)
                .or_default()
                .push(candidate);
        }
        for candidates in candidates_by_component.values_mut() {
            candidates.sort_by(|left, right| {
                candidate_wire_order(left).cmp(&candidate_wire_order(right))
            });
            if candidates
                .iter()
                .enumerate()
                .any(|(index, candidate)| candidate.rank != index as u32 + 1)
            {
                return Err(invalid(
                    "cycle-seam rank does not match canonical cost order",
                ));
            }
        }
        let mut has_missing_data_flow = false;
        let mut has_mismatched_data_flow = false;
        for gap in &self.gaps {
            match &gap.kind {
                CycleSeamGapKind::MissingDataFlowProjection => has_missing_data_flow = true,
                CycleSeamGapKind::DataFlowSourceMismatch { expected, actual } => {
                    if expected == actual {
                        return Err(invalid("cycle-seam data-flow mismatch is not a mismatch"));
                    }
                    has_mismatched_data_flow = true;
                }
                CycleSeamGapKind::IncompleteDataFlowGraph { status, .. }
                    if *status == FactCoverage::Complete =>
                {
                    return Err(invalid("cycle-seam incomplete graph gap is complete"));
                }
                CycleSeamGapKind::SourceArchitecture { .. }
                | CycleSeamGapKind::TopologyWithoutApiEvidence { .. }
                | CycleSeamGapKind::EdgeEvidenceWithoutResolution { .. }
                | CycleSeamGapKind::ResolutionWithoutApiUse { .. }
                | CycleSeamGapKind::ResolutionWithoutDataFlowAccess { .. }
                | CycleSeamGapKind::IncompleteDataFlowGraph { .. }
                | CycleSeamGapKind::UncertainDataFlowAccess { .. } => {}
            }
            if gap.key != make_gap_key(&self.architecture_projection_id, &self.policy, &gap.kind)? {
                return Err(invalid("cycle-seam gap key does not match payload"));
            }
        }
        if has_missing_data_flow && has_mismatched_data_flow {
            return Err(invalid("cycle-seam data-flow gaps contradict each other"));
        }
        if self.coverage != make_coverage(&self.candidates, &self.gaps) {
            return Err(invalid("cycle-seam coverage does not match evidence"));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CycleSeamDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    architecture_projection_id: ProjectionId,
    dependency_projection_id: ProjectionId,
    resolution_projection_id: ProjectionId,
    data_flow_projection_id: Option<ProjectionId>,
    data_flow_policy: Option<DataFlowPolicyId>,
    policy: CycleSeamPolicyId,
    coverage: CycleSeamCoverageEvidence,
    candidates: Vec<CycleSeamCandidate>,
    gaps: Vec<CycleSeamGap>,
}

impl<'de> Deserialize<'de> for CycleSeamDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CycleSeamDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            architecture_projection_id: wire.architecture_projection_id,
            dependency_projection_id: wire.dependency_projection_id,
            resolution_projection_id: wire.resolution_projection_id,
            data_flow_projection_id: wire.data_flow_projection_id,
            data_flow_policy: wire.data_flow_policy,
            policy: wire.policy,
            coverage: wire.coverage,
            candidates: wire.candidates,
            gaps: wire.gaps,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct CycleSeamProjection {
    id: ProjectionId,
    architecture: Arc<ArchitectureProjection>,
    data_flow: Option<Arc<DataFlowProjection>>,
    policy: CycleSeamPolicyId,
    document: CycleSeamDocument,
}

impl CycleSeamProjection {
    pub fn id(&self) -> &ProjectionId {
        &self.id
    }
    pub fn architecture(&self) -> &Arc<ArchitectureProjection> {
        &self.architecture
    }
    pub fn data_flow(&self) -> Option<&Arc<DataFlowProjection>> {
        self.data_flow.as_ref()
    }
    pub fn policy(&self) -> &CycleSeamPolicyId {
        &self.policy
    }
    pub fn document(&self) -> &CycleSeamDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleSeamBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for CycleSeamBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid cycle-seam evidence: {detail}"),
            Self::Identity(detail) => write!(formatter, "cycle-seam identity error: {detail}"),
        }
    }
}

impl std::error::Error for CycleSeamBuildError {}

#[derive(Debug)]
struct CandidateDraft {
    component: ArchitectureComponentKey,
    level: ArchitectureLevel,
    cut_edge: DependencyEdgeKey,
    from: DependencyNodeKey,
    to: DependencyNodeKey,
    api_use_edges: BTreeSet<DependencyEdgeKey>,
    api_nodes: BTreeSet<DependencyNodeKey>,
    resolutions: BTreeSet<ResolutionResultKey>,
    data_flow_accesses: BTreeSet<DataFlowAccessKey>,
    reaching_definitions: BTreeSet<DataFlowDefinitionKey>,
    complete: bool,
}

pub fn derive_cycle_seams(
    architecture: Arc<ArchitectureProjection>,
    data_flow: Option<Arc<DataFlowProjection>>,
    policy: CycleSeamPolicyId,
) -> Result<CycleSeamProjection, CycleSeamBuildError> {
    let dependency = architecture.dependency();
    let source = dependency.document();
    let expected_resolution = dependency.resolution().id();
    let data_flow_matches = data_flow.as_ref().is_some_and(|projection| {
        projection.document().resolution_projection_id() == expected_resolution
    });
    let mut gap_kinds = architecture
        .document()
        .gaps()
        .iter()
        .map(|gap| CycleSeamGapKind::SourceArchitecture {
            gap: gap.key().clone(),
        })
        .collect::<BTreeSet<_>>();

    let cyclic = architecture
        .document()
        .components()
        .iter()
        .filter(|component| component.cyclic())
        .collect::<Vec<_>>();
    if !cyclic.is_empty() {
        match &data_flow {
            None => {
                gap_kinds.insert(CycleSeamGapKind::MissingDataFlowProjection);
            }
            Some(projection) if !data_flow_matches => {
                gap_kinds.insert(CycleSeamGapKind::DataFlowSourceMismatch {
                    expected: expected_resolution.clone(),
                    actual: projection.document().resolution_projection_id().clone(),
                });
            }
            Some(_) => {}
        }
    }

    let local_apis = source
        .nodes()
        .iter()
        .filter_map(|node| {
            matches!(node.kind(), DependencyNodeKind::LocalApi { .. }).then_some(node.key().clone())
        })
        .collect::<BTreeSet<_>>();
    let mut api_by_resolution =
        BTreeMap::<ResolutionResultKey, BTreeSet<(DependencyEdgeKey, DependencyNodeKey)>>::new();
    for edge in source
        .edges()
        .iter()
        .filter(|edge| edge.kind() == DependencyEdgeKind::ApiUse)
    {
        if !local_apis.contains(edge.to()) {
            continue;
        }
        for evidence in edge.evidence() {
            if let Some(resolution) = &evidence.resolution {
                api_by_resolution
                    .entry(resolution.clone())
                    .or_default()
                    .insert((edge.key().clone(), edge.to().clone()));
            }
        }
    }

    let mut access_by_resolution = BTreeMap::<
        ResolutionResultKey,
        Vec<(
            DataFlowGraphKey,
            FactCoverage,
            DataFlowAccessKey,
            Vec<DataFlowDefinitionKey>,
            bool,
        )>,
    >::new();
    if data_flow_matches {
        for graph in data_flow
            .as_ref()
            .expect("matching data flow")
            .document()
            .graphs()
        {
            for access in graph.accesses() {
                access_by_resolution
                    .entry(access.resolution().clone())
                    .or_default()
                    .push((
                        graph.key().clone(),
                        graph.coverage().status(),
                        access.key().clone(),
                        access.reaching_definitions().to_vec(),
                        access.uncertainty().is_some(),
                    ));
            }
        }
    }

    let edges_by_key = source
        .edges()
        .iter()
        .map(|edge| (edge.key(), edge))
        .collect::<BTreeMap<_, _>>();
    let mut drafts = Vec::new();
    for component in cyclic {
        let members = component.members().iter().collect::<BTreeSet<_>>();
        for edge in source.edges().iter().filter(|edge| {
            edge_level(edge.kind()) == Some(component.level())
                && members.contains(edge.from())
                && members.contains(edge.to())
        }) {
            let mut draft = CandidateDraft {
                component: component.key().clone(),
                level: component.level(),
                cut_edge: edge.key().clone(),
                from: edge.from().clone(),
                to: edge.to().clone(),
                api_use_edges: BTreeSet::new(),
                api_nodes: BTreeSet::new(),
                resolutions: BTreeSet::new(),
                data_flow_accesses: BTreeSet::new(),
                reaching_definitions: BTreeSet::new(),
                complete: architecture.document().coverage().status() == FactCoverage::Complete,
            };
            for evidence in edge.evidence() {
                if evidence.coverage != FactCoverage::Complete || evidence.authority.is_none() {
                    draft.complete = false;
                }
                let Some(resolution) = &evidence.resolution else {
                    draft.complete = false;
                    gap_kinds.insert(CycleSeamGapKind::EdgeEvidenceWithoutResolution {
                        component: component.key().clone(),
                        edge: edge.key().clone(),
                    });
                    continue;
                };
                draft.resolutions.insert(resolution.clone());
                match api_by_resolution.get(resolution) {
                    Some(api_evidence) => {
                        for (api_edge, api_node) in api_evidence {
                            draft.api_use_edges.insert(api_edge.clone());
                            draft.api_nodes.insert(api_node.clone());
                        }
                    }
                    None => {
                        draft.complete = false;
                        gap_kinds.insert(CycleSeamGapKind::ResolutionWithoutApiUse {
                            component: component.key().clone(),
                            edge: edge.key().clone(),
                            resolution: resolution.clone(),
                        });
                    }
                }
                match access_by_resolution.get(resolution) {
                    Some(accesses) => {
                        for (graph, status, access, definitions, uncertain) in accesses {
                            draft.data_flow_accesses.insert(access.clone());
                            draft
                                .reaching_definitions
                                .extend(definitions.iter().cloned());
                            if *status != FactCoverage::Complete {
                                draft.complete = false;
                                gap_kinds.insert(CycleSeamGapKind::IncompleteDataFlowGraph {
                                    graph: graph.clone(),
                                    status: *status,
                                });
                            }
                            if *uncertain {
                                draft.complete = false;
                                gap_kinds.insert(CycleSeamGapKind::UncertainDataFlowAccess {
                                    access: access.clone(),
                                });
                            }
                        }
                    }
                    None => {
                        draft.complete = false;
                        if data_flow_matches {
                            gap_kinds.insert(CycleSeamGapKind::ResolutionWithoutDataFlowAccess {
                                component: component.key().clone(),
                                edge: edge.key().clone(),
                                resolution: resolution.clone(),
                            });
                        }
                    }
                }
            }
            if draft.api_nodes.is_empty() {
                gap_kinds.insert(CycleSeamGapKind::TopologyWithoutApiEvidence {
                    component: component.key().clone(),
                    edge: edge.key().clone(),
                });
            } else {
                if !data_flow_matches {
                    draft.complete = false;
                }
                drafts.push(draft);
            }
        }
    }
    if edges_by_key.len() != source.edges().len() {
        return Err(invalid("dependency edge identities are not unique"));
    }

    let mut grouped = BTreeMap::<ArchitectureComponentKey, Vec<CandidateDraft>>::new();
    for draft in drafts {
        grouped
            .entry(draft.component.clone())
            .or_default()
            .push(draft);
    }
    let mut candidates = Vec::new();
    for drafts in grouped.values_mut() {
        drafts.sort_by(|left, right| candidate_order(left).cmp(&candidate_order(right)));
        for (index, draft) in drafts.iter().enumerate() {
            let cost = draft_cost(draft);
            let mut candidate = CycleSeamCandidate {
                key: CycleSeamCandidateKey(String::new()),
                component: draft.component.clone(),
                level: draft.level,
                cut_edge: draft.cut_edge.clone(),
                from: draft.from.clone(),
                to: draft.to.clone(),
                action: CycleSeamAction::ExtractTargetApiBoundary,
                disposition: CycleSeamDisposition::ReviewRequired,
                api_use_edges: draft.api_use_edges.iter().cloned().collect(),
                api_nodes: draft.api_nodes.iter().cloned().collect(),
                resolutions: draft.resolutions.iter().cloned().collect(),
                data_flow_accesses: draft.data_flow_accesses.iter().cloned().collect(),
                reaching_definitions: draft.reaching_definitions.iter().cloned().collect(),
                evidence_coverage: if draft.complete {
                    FactCoverage::Complete
                } else {
                    FactCoverage::Partial
                },
                cost,
                rank: u32::try_from(index + 1)
                    .map_err(|_| invalid("cycle-seam rank exceeds u32"))?,
                review_obligations: review_obligations(),
            };
            candidate.key = make_candidate_key(architecture.id(), &policy, &candidate)?;
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|left, right| left.key.cmp(&right.key));
    let mut gaps = gap_kinds
        .into_iter()
        .map(|kind| make_gap(architecture.id(), &policy, kind))
        .collect::<Result<Vec<_>, _>>()?;
    gaps.sort_by(|left, right| left.key.cmp(&right.key));
    let coverage = make_coverage(&candidates, &gaps);
    let data_flow_projection_id = data_flow.as_ref().map(|projection| projection.id().clone());
    let data_flow_policy = data_flow
        .as_ref()
        .map(|projection| projection.policy().clone());
    let payload = serde_json::to_vec(&(
        architecture.id(),
        dependency.id(),
        expected_resolution,
        &data_flow_projection_id,
        &data_flow_policy,
        &policy,
        &coverage,
        &candidates,
        &gaps,
    ))
    .map_err(|error| CycleSeamBuildError::Identity(error.to_string()))?;
    let analysis = dependency.resolution().scope_graph().analysis();
    let id = analysis
        .derive_projection_id(CYCLE_SEAM_SCHEMA, &payload, policy.as_str().as_bytes())
        .map_err(|error| CycleSeamBuildError::Identity(error.to_string()))?;
    let document = CycleSeamDocument {
        schema: CYCLE_SEAM_SCHEMA.into(),
        projection_id: id.clone(),
        architecture_projection_id: architecture.id().clone(),
        dependency_projection_id: dependency.id().clone(),
        resolution_projection_id: expected_resolution.clone(),
        data_flow_projection_id,
        data_flow_policy,
        policy: policy.clone(),
        coverage,
        candidates,
        gaps,
    };
    document.validate()?;
    Ok(CycleSeamProjection {
        id,
        architecture,
        data_flow,
        policy,
        document,
    })
}

fn candidate_order(
    draft: &CandidateDraft,
) -> (
    CycleSeamCost,
    &DependencyNodeKey,
    &DependencyNodeKey,
    &DependencyEdgeKey,
) {
    (draft_cost(draft), &draft.from, &draft.to, &draft.cut_edge)
}

fn candidate_wire_order(
    candidate: &CycleSeamCandidate,
) -> (
    &CycleSeamCost,
    &DependencyNodeKey,
    &DependencyNodeKey,
    &DependencyEdgeKey,
) {
    (
        &candidate.cost,
        &candidate.from,
        &candidate.to,
        &candidate.cut_edge,
    )
}

fn draft_cost(draft: &CandidateDraft) -> CycleSeamCost {
    CycleSeamCost {
        authority_penalty: usize::from(!draft.complete),
        api_surface: draft.api_nodes.len(),
        reaching_definitions: draft.reaching_definitions.len(),
        data_flow_accesses: draft.data_flow_accesses.len(),
        resolutions: draft.resolutions.len(),
    }
}

fn review_obligations() -> Vec<String> {
    vec![
        "confirm neutral boundary ownership".into(),
        "preserve API and data-flow semantics".into(),
        "run behavior-preservation verification".into(),
        "update build and dependency declarations".into(),
    ]
}

fn edge_level(kind: DependencyEdgeKind) -> Option<ArchitectureLevel> {
    match kind {
        DependencyEdgeKind::FileDependency => Some(ArchitectureLevel::File),
        DependencyEdgeKind::ModuleDependency => Some(ArchitectureLevel::Module),
        DependencyEdgeKind::PackageDependency => Some(ArchitectureLevel::Package),
        DependencyEdgeKind::BuildTargetDependency => Some(ArchitectureLevel::BuildTarget),
        DependencyEdgeKind::PackageContainsTarget
        | DependencyEdgeKind::TargetContainsModule
        | DependencyEdgeKind::ModuleContainsFile
        | DependencyEdgeKind::ApiUse => None,
    }
}

fn make_candidate_key(
    architecture: &ProjectionId,
    policy: &CycleSeamPolicyId,
    candidate: &CycleSeamCandidate,
) -> Result<CycleSeamCandidateKey, CycleSeamBuildError> {
    let payload = serde_json::to_vec(&(
        &candidate.component,
        candidate.level,
        &candidate.cut_edge,
        &candidate.from,
        &candidate.to,
        candidate.action,
        candidate.disposition,
        &candidate.api_use_edges,
        &candidate.api_nodes,
        &candidate.resolutions,
        &candidate.data_flow_accesses,
        &candidate.reaching_definitions,
        candidate.evidence_coverage,
        &candidate.cost,
        candidate.rank,
        &candidate.review_obligations,
    ))
    .map_err(|error| CycleSeamBuildError::Identity(error.to_string()))?;
    Ok(CycleSeamCandidateKey(derive_id(
        CANDIDATE_DOMAIN,
        "csc1_",
        &[
            architecture.as_str().as_bytes(),
            policy.as_str().as_bytes(),
            &payload,
        ],
    )))
}

fn make_gap(
    architecture: &ProjectionId,
    policy: &CycleSeamPolicyId,
    kind: CycleSeamGapKind,
) -> Result<CycleSeamGap, CycleSeamBuildError> {
    let payload = serde_json::to_vec(&kind)
        .map_err(|error| CycleSeamBuildError::Identity(error.to_string()))?;
    Ok(CycleSeamGap {
        key: CycleSeamGapKey(derive_id(
            GAP_DOMAIN,
            "csg1_",
            &[
                architecture.as_str().as_bytes(),
                policy.as_str().as_bytes(),
                &payload,
            ],
        )),
        kind,
    })
}

fn make_gap_key(
    architecture: &ProjectionId,
    policy: &CycleSeamPolicyId,
    kind: &CycleSeamGapKind,
) -> Result<CycleSeamGapKey, CycleSeamBuildError> {
    Ok(make_gap(architecture, policy, kind.clone())?.key)
}

fn make_coverage(
    candidates: &[CycleSeamCandidate],
    gaps: &[CycleSeamGap],
) -> CycleSeamCoverageEvidence {
    let status = if gaps.is_empty()
        && candidates
            .iter()
            .all(|candidate| candidate.evidence_coverage == FactCoverage::Complete)
    {
        FactCoverage::Complete
    } else {
        FactCoverage::Partial
    };
    let mut reasons = gaps
        .iter()
        .map(|gap| format!("cycle-seam gap {}", gap.key.as_str()))
        .collect::<Vec<_>>();
    reasons.sort();
    reasons.dedup();
    CycleSeamCoverageEvidence { status, reasons }
}

fn validate_sorted<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), CycleSeamBuildError> {
    let keys = values.iter().map(key).collect::<Vec<_>>();
    if !strictly_sorted(&keys) && keys.len() > 1 {
        Err(invalid(format!("{label} are not canonical and distinct")))
    } else {
        Ok(())
    }
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), CycleSeamBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(invalid(format!("identity must start with {prefix}")));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Err(invalid(
            "identity must contain a canonical 32-byte hexadecimal digest",
        ))
    } else {
        Ok(())
    }
}

fn invalid(detail: impl Into<String>) -> CycleSeamBuildError {
    CycleSeamBuildError::Invalid(detail.into())
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
pub(crate) mod tests {
    use std::path::Path;

    use super::*;
    use crate::dependency::tests::{
        FixtureEndpoint, cycle_dependency_fixture, dependency_fixture,
        topology_only_cycle_dependency_fixture,
    };
    use crate::{
        ArchitecturePolicy, ControlEdgeDraft, ControlEdgeKind, ControlEdgePrecision,
        ControlExitOutcome, ControlFlowBuilder, ControlFlowCoverageEvidence, ControlFlowGraphDraft,
        ControlFlowOwnerKind, ControlFlowPolicyId, ControlPointDraft, ControlPointKind,
        ControlSyntheticPointKind, DataFlowAccessDraft, DataFlowAccessKind, DataFlowBuilder,
        DataFlowEffectDraft, DataFlowGraphDraft, DataFlowPolicyId, FactCoverageEvidence,
        ProjectAnalysis, derive_architecture, derive_control_regions,
    };

    fn policy() -> CycleSeamPolicyId {
        CycleSeamPolicyId::from_parts(&[b"cycle-seam-test-policy/1"]).unwrap()
    }

    pub(crate) fn cycle_architecture() -> Arc<ArchitectureProjection> {
        let dependency = Arc::new(cycle_dependency_fixture());
        let projection =
            derive_architecture(dependency, ArchitecturePolicy::new(vec![], vec![]).unwrap())
                .unwrap();
        assert_eq!(
            projection
                .document()
                .components()
                .iter()
                .filter(|component| component.cyclic())
                .count(),
            4
        );
        Arc::new(projection)
    }

    fn node_by_text(analysis: &ProjectAnalysis, path: &str, text: &str) -> crate::NodeId {
        analysis
            .node_ids()
            .find(|node| {
                let view = analysis.node(*node).unwrap();
                view.path() == Path::new(path)
                    && view.raw_kind() == "identifier"
                    && view.text() == text
            })
            .unwrap()
    }

    fn containing_function(
        analysis: &ProjectAnalysis,
        path: &str,
        child: crate::NodeId,
    ) -> crate::NodeId {
        let child_span = analysis.node(child).unwrap().span();
        analysis
            .node_ids()
            .filter(|node| {
                let view = analysis.node(*node).unwrap();
                let span = view.span();
                view.path() == Path::new(path)
                    && view.raw_kind() == "function_item"
                    && span.start_byte() <= child_span.start_byte()
                    && span.end_byte() >= child_span.end_byte()
            })
            .min_by_key(|node| {
                let span = analysis.node(*node).unwrap().span();
                span.end_byte() - span.start_byte()
            })
            .unwrap()
    }

    fn cycle_data_flow(
        architecture: &ArchitectureProjection,
        omit_one_access: bool,
    ) -> Arc<DataFlowProjection> {
        let resolution = Arc::clone(architecture.dependency().resolution());
        let analysis = Arc::clone(resolution.scope_graph().analysis());
        let references = [
            (
                "consumer.resolutionrs",
                node_by_text(&analysis, "consumer.resolutionrs", "imported"),
            ),
            (
                "provider.resolutionrs",
                node_by_text(&analysis, "provider.resolutionrs", "consumed"),
            ),
        ];
        let mut flow_builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"cycle-seam-cfg/1"]).unwrap(),
        );
        for (path, reference) in references {
            let owner = containing_function(&analysis, path, reference);
            let edge = |from, to, kind| ControlEdgeDraft {
                from,
                to,
                kind,
                source: owner,
                predicate: None,
                precision: ControlEdgePrecision::Exact,
            };
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
                            source: Some(reference),
                            ordinal: 0,
                        },
                        ControlPointDraft {
                            kind: ControlPointKind::Synthetic(
                                ControlSyntheticPointKind::ExitDispatch,
                            ),
                            source: Some(owner),
                            ordinal: 0,
                        },
                        ControlPointDraft {
                            kind: ControlPointKind::Exit,
                            source: None,
                            ordinal: 0,
                        },
                    ],
                    edges: vec![
                        edge(0, 1, ControlEdgeKind::Entry),
                        edge(1, 2, ControlEdgeKind::Normal),
                        edge(2, 3, ControlEdgeKind::Exit(ControlExitOutcome::Normal)),
                    ],
                })
                .unwrap();
        }
        let flow = Arc::new(flow_builder.build().unwrap());
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                crate::ControlRegionPolicyId::from_parts(&[b"cycle-seam-regions/1"]).unwrap(),
            )
            .unwrap(),
        );
        let mut builder = DataFlowBuilder::new(
            regions,
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"cycle-seam-data-flow/1"]).unwrap(),
        )
        .unwrap();
        for (index, graph) in flow.document().graphs().iter().enumerate() {
            let point = graph
                .points()
                .iter()
                .find(|point| matches!(point.kind(), ControlPointKind::Syntax))
                .unwrap();
            let source = point.source().unwrap();
            let accesses = if omit_one_access && index == 1 {
                vec![]
            } else {
                let result = resolution
                    .results()
                    .iter()
                    .find(|result| result.wire().reference_evidence().node_key == *source)
                    .unwrap()
                    .wire();
                vec![DataFlowAccessDraft {
                    point: point.key().clone(),
                    reference: result.reference().clone(),
                    kind: DataFlowAccessKind::Read,
                    ordinal: 0,
                }]
            };
            builder
                .add_graph(DataFlowGraphDraft {
                    control_flow_graph: graph.key().clone(),
                    definitions: vec![],
                    accesses,
                    boundaries: vec![],
                    effects: graph
                        .points()
                        .iter()
                        .map(|point| DataFlowEffectDraft {
                            point: point.key().clone(),
                            effects: vec![],
                            uncertainty: None,
                        })
                        .collect(),
                })
                .unwrap();
        }
        let projection = builder.build().unwrap();
        assert!(
            projection
                .document()
                .graphs()
                .iter()
                .all(|graph| graph.coverage().status() == FactCoverage::Complete)
        );
        Arc::new(projection)
    }

    pub(crate) fn complete_cycle_data_flow(
        architecture: &ArchitectureProjection,
    ) -> Arc<DataFlowProjection> {
        cycle_data_flow(architecture, false)
    }

    #[test]
    fn exact_cycle_emits_two_ranked_review_cuts_at_every_level() {
        let architecture = cycle_architecture();
        let data_flow = complete_cycle_data_flow(&architecture);
        let projection =
            derive_cycle_seams(Arc::clone(&architecture), Some(data_flow), policy()).unwrap();
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Complete);
        assert!(document.gaps().is_empty(), "{:#?}", document.gaps());
        assert_eq!(document.candidates().len(), 8);
        for level in [
            ArchitectureLevel::File,
            ArchitectureLevel::Module,
            ArchitectureLevel::Package,
            ArchitectureLevel::BuildTarget,
        ] {
            let candidates = document
                .candidates()
                .iter()
                .filter(|candidate| candidate.level() == level)
                .collect::<Vec<_>>();
            assert_eq!(candidates.len(), 2);
            assert_eq!(
                candidates
                    .iter()
                    .map(|candidate| candidate.rank())
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([1, 2])
            );
        }
        for candidate in document.candidates() {
            assert_eq!(
                candidate.disposition(),
                CycleSeamDisposition::ReviewRequired
            );
            assert_eq!(candidate.api_nodes().len(), 1);
            assert_eq!(candidate.api_use_edges().len(), 1);
            assert_eq!(candidate.resolutions().len(), 1);
            assert_eq!(candidate.data_flow_accesses().len(), 1);
            assert!(candidate.reaching_definitions().is_empty());
            assert_eq!(candidate.evidence_coverage(), FactCoverage::Complete);
            assert_eq!(candidate.cost().authority_penalty(), 0);
            assert_eq!(candidate.cost().api_surface(), 1);
        }
    }

    #[test]
    fn missing_data_flow_retains_api_grounded_review_candidates_as_partial() {
        let architecture = cycle_architecture();
        let projection = derive_cycle_seams(architecture, None, policy()).unwrap();
        assert_eq!(projection.document().candidates().len(), 8);
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(
            projection
                .document()
                .gaps()
                .iter()
                .any(|gap| matches!(gap.kind(), CycleSeamGapKind::MissingDataFlowProjection))
        );
        assert!(projection.document().candidates().iter().all(|candidate| {
            candidate.evidence_coverage() == FactCoverage::Partial
                && candidate.cost().authority_penalty() == 1
                && candidate.data_flow_accesses().is_empty()
        }));
    }

    #[test]
    fn complete_data_flow_with_missing_resolution_access_downgrades_only_affected_cuts() {
        let architecture = cycle_architecture();
        let data_flow = cycle_data_flow(&architecture, true);
        let projection = derive_cycle_seams(architecture, Some(data_flow), policy()).unwrap();
        assert_eq!(projection.document().candidates().len(), 8);
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert_eq!(
            projection
                .document()
                .gaps()
                .iter()
                .filter(|gap| matches!(
                    gap.kind(),
                    CycleSeamGapKind::ResolutionWithoutDataFlowAccess { .. }
                ))
                .count(),
            4
        );
        assert_eq!(
            projection
                .document()
                .candidates()
                .iter()
                .filter(|candidate| candidate.evidence_coverage() == FactCoverage::Complete)
                .count(),
            4
        );
    }

    #[test]
    fn topology_only_cycle_abstains_instead_of_selecting_an_edge() {
        let dependency = Arc::new(topology_only_cycle_dependency_fixture());
        let architecture = Arc::new(
            derive_architecture(dependency, ArchitecturePolicy::new(vec![], vec![]).unwrap())
                .unwrap(),
        );
        assert_eq!(
            architecture
                .document()
                .components()
                .iter()
                .filter(|component| component.cyclic())
                .count(),
            3
        );
        let projection = derive_cycle_seams(architecture, None, policy()).unwrap();
        assert!(projection.document().candidates().is_empty());
        assert_eq!(
            projection
                .document()
                .gaps()
                .iter()
                .filter(|gap| matches!(
                    gap.kind(),
                    CycleSeamGapKind::TopologyWithoutApiEvidence { .. }
                ))
                .count(),
            6
        );
    }

    #[test]
    fn foreign_data_flow_projection_is_not_joined_by_project_shape() {
        let architecture = cycle_architecture();
        let foreign_pdg = crate::data_flow::tests::ambiguous_capture_pdg_fixture();
        let foreign_data_flow = Arc::clone(foreign_pdg.data_flow());
        let projection =
            derive_cycle_seams(architecture, Some(foreign_data_flow), policy()).unwrap();
        assert_eq!(projection.document().candidates().len(), 8);
        assert!(projection.document().gaps().iter().any(|gap| matches!(
            gap.kind(),
            CycleSeamGapKind::DataFlowSourceMismatch { expected, actual }
                if expected != actual
        )));
        assert!(projection.document().candidates().iter().all(|candidate| {
            candidate.evidence_coverage() == FactCoverage::Partial
                && candidate.data_flow_accesses().is_empty()
        }));
    }

    #[test]
    fn partial_architecture_authority_is_inherited_even_with_complete_accesses() {
        let dependency = Arc::new(dependency_fixture(
            FactCoverageEvidence::partial("cycle fixture exports are incomplete").unwrap(),
            FixtureEndpoint::BidirectionalDeclarations,
            false,
        ));
        let architecture = Arc::new(
            derive_architecture(dependency, ArchitecturePolicy::new(vec![], vec![]).unwrap())
                .unwrap(),
        );
        let data_flow = complete_cycle_data_flow(&architecture);
        let projection = derive_cycle_seams(architecture, Some(data_flow), policy()).unwrap();
        assert_eq!(projection.document().candidates().len(), 8);
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(
            projection
                .document()
                .gaps()
                .iter()
                .any(|gap| matches!(gap.kind(), CycleSeamGapKind::SourceArchitecture { .. }))
        );
        assert!(
            projection
                .document()
                .candidates()
                .iter()
                .all(|candidate| candidate.evidence_coverage() == FactCoverage::Partial)
        );
    }

    #[test]
    fn acyclic_topology_needs_no_data_flow_and_emits_no_seam() {
        let dependency = Arc::new(dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            false,
        ));
        let architecture = Arc::new(
            derive_architecture(dependency, ArchitecturePolicy::new(vec![], vec![]).unwrap())
                .unwrap(),
        );
        let projection = derive_cycle_seams(architecture, None, policy()).unwrap();
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Complete
        );
        assert!(projection.document().candidates().is_empty());
        assert!(projection.document().gaps().is_empty());
    }

    #[test]
    fn deterministic_round_trip_and_candidate_tamper_rejection() {
        let architecture = cycle_architecture();
        let data_flow = complete_cycle_data_flow(&architecture);
        let first = derive_cycle_seams(
            Arc::clone(&architecture),
            Some(Arc::clone(&data_flow)),
            policy(),
        )
        .unwrap();
        let second = derive_cycle_seams(architecture, Some(data_flow), policy()).unwrap();
        assert_eq!(first.id(), second.id());
        let bytes = serde_json::to_vec(first.document()).unwrap();
        assert_eq!(bytes, serde_json::to_vec(second.document()).unwrap());
        let decoded: CycleSeamDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());

        let mut tampered = serde_json::to_value(first.document()).unwrap();
        tampered["candidates"][0]["rank"] = serde_json::json!(99);
        assert!(serde_json::from_value::<CycleSeamDocument>(tampered).is_err());
    }
}
