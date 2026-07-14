use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, ControlEdgeKey, ControlFlowGraph,
    ControlFlowGraphKey, ControlFlowPolicyId, ControlPointKey, ControlRegionGraph,
    ControlRegionGraphKey, ControlRegionPointKey, ControlRegionPolicyId, DataFlowAccessKey,
    DataFlowDefinitionKey, DataFlowGraph, DataFlowGraphKey, DataFlowPointKey, DataFlowPolicyId,
    DataFlowProjection, DataFlowSymbolKey, FactCoverage, NodeKey, NonStructuredControlFactKey,
    NonStructuredControlGraph, NonStructuredControlGraphKey, NonStructuredControlPolicyId,
    NonStructuredControlProjection, ProjectionId, ResolutionPolicyId,
};

pub const PROGRAM_DEPENDENCE_SCHEMA: &str = "deslop.program-dependence/1";
pub const PROGRAM_DEPENDENCE_POLICY_SCHEMA: &str = "deslop.program-dependence-policy/1";

const POLICY_DOMAIN: &str = "deslop program-dependence policy v1";
const GRAPH_DOMAIN: &str = "deslop program-dependence graph v1";
const NODE_DOMAIN: &str = "deslop program-dependence node v1";
const EDGE_DOMAIN: &str = "deslop program-dependence edge v1";
const GAP_DOMAIN: &str = "deslop program-dependence gap v1";

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

digest_id!(ProgramDependencePolicyId, "pdp1_");
digest_id!(ProgramDependenceGraphKey, "pdg1_");
digest_id!(ProgramDependenceNodeKey, "pdn1_");
digest_id!(ProgramDependenceEdgeKey, "pde1_");
digest_id!(ProgramDependenceGapKey, "pdx1_");

impl ProgramDependencePolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ProgramDependenceBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(ProgramDependenceBuildError::Invalid(
                "program-dependence policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_id(POLICY_DOMAIN, "pdp1_", parts)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDependenceCoverageEvidence {
    status: FactCoverage,
    local_pdg_support: CapabilitySupport,
    local_pdg_authority: Option<CapabilityAuthority>,
    reasons: Vec<String>,
}

impl ProgramDependenceCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    pub fn local_pdg_support(&self) -> CapabilitySupport {
        self.local_pdg_support
    }

    pub fn local_pdg_authority(&self) -> Option<CapabilityAuthority> {
        self.local_pdg_authority
    }

    fn validate(&self) -> Result<(), ProgramDependenceBuildError> {
        validate_canonical("program-dependence coverage reasons", &self.reasons)?;
        for reason in &self.reasons {
            validate_text(reason)?;
        }
        match (self.local_pdg_support, self.local_pdg_authority) {
            (CapabilitySupport::Provided, Some(_))
            | (CapabilitySupport::Unsupported | CapabilitySupport::Unknown, None) => {}
            _ => {
                return Err(ProgramDependenceBuildError::Invalid(
                    "LocalPdg capability support and authority disagree".into(),
                ));
            }
        }
        match (self.status, self.reasons.is_empty()) {
            (FactCoverage::Complete, true)
                if self.local_pdg_support == CapabilitySupport::Provided =>
            {
                Ok(())
            }
            (FactCoverage::Complete, true) => Err(ProgramDependenceBuildError::Invalid(
                "Complete program-dependence coverage requires Provided LocalPdg capability".into(),
            )),
            (FactCoverage::Complete, false) => Err(ProgramDependenceBuildError::Invalid(
                "Complete program-dependence coverage cannot carry uncertainty reasons".into(),
            )),
            (_, false) => Ok(()),
            (_, true) => Err(ProgramDependenceBuildError::Invalid(
                "incomplete program-dependence coverage requires an exact reason".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDependenceNode {
    key: ProgramDependenceNodeKey,
    point: ControlPointKey,
    control_region_point: ControlRegionPointKey,
    data_flow_point: DataFlowPointKey,
    source: Option<NodeKey>,
    reachable: bool,
    exit_reachable: bool,
}

impl ProgramDependenceNode {
    pub fn key(&self) -> &ProgramDependenceNodeKey {
        &self.key
    }

    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }

    pub fn control_region_point(&self) -> &ControlRegionPointKey {
        &self.control_region_point
    }

    pub fn data_flow_point(&self) -> &DataFlowPointKey {
        &self.data_flow_point
    }

    pub fn source(&self) -> Option<&NodeKey> {
        self.source.as_ref()
    }

    pub fn reachable(&self) -> bool {
        self.reachable
    }

    pub fn exit_reachable(&self) -> bool {
        self.exit_reachable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "evidence", rename_all = "kebab-case")]
pub enum ProgramDependenceEdgeKind {
    Control {
        inducing_edges: Vec<ControlEdgeKey>,
    },
    Flow {
        symbol: DataFlowSymbolKey,
        definition: DataFlowDefinitionKey,
        access: DataFlowAccessKey,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDependenceEdge {
    key: ProgramDependenceEdgeKey,
    from: ProgramDependenceNodeKey,
    to: ProgramDependenceNodeKey,
    kind: ProgramDependenceEdgeKind,
}

impl ProgramDependenceEdge {
    pub fn key(&self) -> &ProgramDependenceEdgeKey {
        &self.key
    }

    pub fn from(&self) -> &ProgramDependenceNodeKey {
        &self.from
    }

    pub fn to(&self) -> &ProgramDependenceNodeKey {
        &self.to
    }

    pub fn kind(&self) -> &ProgramDependenceEdgeKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "evidence", rename_all = "kebab-case")]
pub enum ProgramDependenceGapKind {
    UnresolvedAccess {
        access: DataFlowAccessKey,
    },
    ControlPostDominanceUnavailable {
        edge: ControlEdgeKey,
        from: ControlPointKey,
        to: ControlPointKey,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDependenceGap {
    key: ProgramDependenceGapKey,
    kind: ProgramDependenceGapKind,
}

impl ProgramDependenceGap {
    pub fn key(&self) -> &ProgramDependenceGapKey {
        &self.key
    }

    pub fn kind(&self) -> &ProgramDependenceGapKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDependenceGraph {
    key: ProgramDependenceGraphKey,
    control_flow_graph: ControlFlowGraphKey,
    control_region_graph: ControlRegionGraphKey,
    non_structured_control_graph: NonStructuredControlGraphKey,
    data_flow_graph: DataFlowGraphKey,
    owner: NodeKey,
    coverage: ProgramDependenceCoverageEvidence,
    control_edges: Vec<ControlEdgeKey>,
    symbols: Vec<DataFlowSymbolKey>,
    definitions: Vec<DataFlowDefinitionKey>,
    accesses: Vec<DataFlowAccessKey>,
    non_structured_facts: Vec<NonStructuredControlFactKey>,
    nodes: Vec<ProgramDependenceNode>,
    edges: Vec<ProgramDependenceEdge>,
    gaps: Vec<ProgramDependenceGap>,
}

impl ProgramDependenceGraph {
    pub fn key(&self) -> &ProgramDependenceGraphKey {
        &self.key
    }

    pub fn control_flow_graph(&self) -> &ControlFlowGraphKey {
        &self.control_flow_graph
    }

    pub fn control_region_graph(&self) -> &ControlRegionGraphKey {
        &self.control_region_graph
    }

    pub fn non_structured_control_graph(&self) -> &NonStructuredControlGraphKey {
        &self.non_structured_control_graph
    }

    pub fn data_flow_graph(&self) -> &DataFlowGraphKey {
        &self.data_flow_graph
    }

    pub fn owner(&self) -> &NodeKey {
        &self.owner
    }

    pub fn coverage(&self) -> &ProgramDependenceCoverageEvidence {
        &self.coverage
    }

    pub fn control_edges(&self) -> &[ControlEdgeKey] {
        &self.control_edges
    }

    pub fn symbols(&self) -> &[DataFlowSymbolKey] {
        &self.symbols
    }

    pub fn definitions(&self) -> &[DataFlowDefinitionKey] {
        &self.definitions
    }

    pub fn accesses(&self) -> &[DataFlowAccessKey] {
        &self.accesses
    }

    pub fn non_structured_facts(&self) -> &[NonStructuredControlFactKey] {
        &self.non_structured_facts
    }

    pub fn nodes(&self) -> &[ProgramDependenceNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[ProgramDependenceEdge] {
        &self.edges
    }

    pub fn gaps(&self) -> &[ProgramDependenceGap] {
        &self.gaps
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDependenceDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_flow_projection_id: ProjectionId,
    control_flow_policy: ControlFlowPolicyId,
    control_region_projection_id: ProjectionId,
    control_region_policy: ControlRegionPolicyId,
    non_structured_control_projection_id: ProjectionId,
    non_structured_control_policy: NonStructuredControlPolicyId,
    resolution_projection_id: ProjectionId,
    resolution_policy: ResolutionPolicyId,
    data_flow_projection_id: ProjectionId,
    data_flow_policy: DataFlowPolicyId,
    policy: ProgramDependencePolicyId,
    graphs: Vec<ProgramDependenceGraph>,
}

impl ProgramDependenceDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn control_flow_projection_id(&self) -> &ProjectionId {
        &self.control_flow_projection_id
    }

    pub fn control_flow_policy(&self) -> &ControlFlowPolicyId {
        &self.control_flow_policy
    }

    pub fn control_region_projection_id(&self) -> &ProjectionId {
        &self.control_region_projection_id
    }

    pub fn control_region_policy(&self) -> &ControlRegionPolicyId {
        &self.control_region_policy
    }

    pub fn non_structured_control_projection_id(&self) -> &ProjectionId {
        &self.non_structured_control_projection_id
    }

    pub fn non_structured_control_policy(&self) -> &NonStructuredControlPolicyId {
        &self.non_structured_control_policy
    }

    pub fn resolution_projection_id(&self) -> &ProjectionId {
        &self.resolution_projection_id
    }

    pub fn resolution_policy(&self) -> &ResolutionPolicyId {
        &self.resolution_policy
    }

    pub fn data_flow_projection_id(&self) -> &ProjectionId {
        &self.data_flow_projection_id
    }

    pub fn data_flow_policy(&self) -> &DataFlowPolicyId {
        &self.data_flow_policy
    }

    pub fn policy(&self) -> &ProgramDependencePolicyId {
        &self.policy
    }

    pub fn graphs(&self) -> &[ProgramDependenceGraph] {
        &self.graphs
    }

    fn validate(&self) -> Result<(), ProgramDependenceBuildError> {
        if self.schema != PROGRAM_DEPENDENCE_SCHEMA {
            return Err(ProgramDependenceBuildError::Invalid(format!(
                "unsupported program-dependence schema {}",
                self.schema
            )));
        }
        validate_digest(self.projection_id.as_str(), "pj1_")?;
        validate_digest(&self.analysis_id, "pa1_")?;
        for projection in [
            &self.control_flow_projection_id,
            &self.control_region_projection_id,
            &self.non_structured_control_projection_id,
            &self.resolution_projection_id,
            &self.data_flow_projection_id,
        ] {
            validate_digest(projection.as_str(), "pj1_")?;
        }
        if self.graphs.is_empty() {
            return Err(ProgramDependenceBuildError::Invalid(
                "program-dependence document cannot be empty".into(),
            ));
        }
        validate_sorted_by("program-dependence graphs", &self.graphs, |graph| {
            graph.key.as_str()
        })?;
        let mut sources = BTreeSet::new();
        for graph in &self.graphs {
            if !sources.insert(graph.data_flow_graph.clone()) {
                return Err(ProgramDependenceBuildError::Invalid(
                    "program-dependence document repeats a source dataflow graph".into(),
                ));
            }
            validate_graph(&self.policy, graph)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramDependenceDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_flow_projection_id: ProjectionId,
    control_flow_policy: ControlFlowPolicyId,
    control_region_projection_id: ProjectionId,
    control_region_policy: ControlRegionPolicyId,
    non_structured_control_projection_id: ProjectionId,
    non_structured_control_policy: NonStructuredControlPolicyId,
    resolution_projection_id: ProjectionId,
    resolution_policy: ResolutionPolicyId,
    data_flow_projection_id: ProjectionId,
    data_flow_policy: DataFlowPolicyId,
    policy: ProgramDependencePolicyId,
    graphs: Vec<ProgramDependenceGraph>,
}

impl<'de> Deserialize<'de> for ProgramDependenceDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ProgramDependenceDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            control_flow_projection_id: wire.control_flow_projection_id,
            control_flow_policy: wire.control_flow_policy,
            control_region_projection_id: wire.control_region_projection_id,
            control_region_policy: wire.control_region_policy,
            non_structured_control_projection_id: wire.non_structured_control_projection_id,
            non_structured_control_policy: wire.non_structured_control_policy,
            resolution_projection_id: wire.resolution_projection_id,
            resolution_policy: wire.resolution_policy,
            data_flow_projection_id: wire.data_flow_projection_id,
            data_flow_policy: wire.data_flow_policy,
            policy: wire.policy,
            graphs: wire.graphs,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct ProgramDependenceProjection {
    id: ProjectionId,
    data_flow: Arc<DataFlowProjection>,
    non_structured_control: Arc<NonStructuredControlProjection>,
    policy: ProgramDependencePolicyId,
    document: ProgramDependenceDocument,
}

impl ProgramDependenceProjection {
    pub fn schema(&self) -> &'static str {
        PROGRAM_DEPENDENCE_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn data_flow(&self) -> &Arc<DataFlowProjection> {
        &self.data_flow
    }

    pub fn non_structured_control(&self) -> &Arc<NonStructuredControlProjection> {
        &self.non_structured_control
    }

    pub fn policy(&self) -> &ProgramDependencePolicyId {
        &self.policy
    }

    pub fn document(&self) -> &ProgramDependenceDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramDependenceBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for ProgramDependenceBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => {
                write!(formatter, "invalid program-dependence evidence: {detail}")
            }
            Self::Identity(detail) => {
                write!(formatter, "program-dependence identity error: {detail}")
            }
        }
    }
}

impl std::error::Error for ProgramDependenceBuildError {}

#[derive(Debug, Clone)]
struct ControlFact {
    reachable: bool,
    exit_reachable: bool,
    post_dominators: BTreeSet<ControlPointKey>,
    immediate_post_dominator: Option<ControlPointKey>,
}

#[derive(Debug, Clone)]
struct ControlWitness {
    edge: ControlEdgeKey,
    from: ControlPointKey,
    to: ControlPointKey,
}

type ControlDependencies = BTreeMap<(ControlPointKey, ControlPointKey), BTreeSet<ControlEdgeKey>>;

#[derive(Debug, Clone)]
struct FlowDefinitionFact {
    point: ControlPointKey,
    symbol: DataFlowSymbolKey,
}

#[derive(Debug, Clone)]
struct FlowAccessFact {
    key: DataFlowAccessKey,
    point: ControlPointKey,
    symbol: Option<DataFlowSymbolKey>,
    reaching_definitions: Vec<DataFlowDefinitionKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlowDependency {
    from: ControlPointKey,
    to: ControlPointKey,
    symbol: DataFlowSymbolKey,
    definition: DataFlowDefinitionKey,
    access: DataFlowAccessKey,
}

fn derive_control_dependencies(
    facts: &BTreeMap<ControlPointKey, ControlFact>,
    witnesses: &[ControlWitness],
) -> (ControlDependencies, Vec<ControlWitness>) {
    let mut dependencies = BTreeMap::<_, BTreeSet<_>>::new();
    let mut gaps = Vec::new();
    for witness in witnesses {
        let Some(origin) = facts.get(&witness.from) else {
            gaps.push(witness.clone());
            continue;
        };
        let Some(target) = facts.get(&witness.to) else {
            gaps.push(witness.clone());
            continue;
        };
        if !origin.reachable {
            continue;
        }
        if !target.reachable {
            gaps.push(witness.clone());
            continue;
        }
        if origin.post_dominators.contains(&witness.to) {
            continue;
        }
        let Some(stop) = &origin.immediate_post_dominator else {
            gaps.push(witness.clone());
            continue;
        };
        let mut runner = witness.to.clone();
        let mut dependents = Vec::new();
        let mut visited = BTreeSet::new();
        let mut complete = true;
        while &runner != stop {
            if !visited.insert(runner.clone()) {
                complete = false;
                break;
            }
            let Some(fact) = facts.get(&runner) else {
                complete = false;
                break;
            };
            if !fact.exit_reachable {
                complete = false;
                break;
            }
            dependents.push(runner.clone());
            let Some(next) = &fact.immediate_post_dominator else {
                complete = false;
                break;
            };
            runner = next.clone();
        }
        if !complete {
            gaps.push(witness.clone());
            continue;
        }
        for dependent in dependents {
            dependencies
                .entry((witness.from.clone(), dependent))
                .or_default()
                .insert(witness.edge.clone());
        }
    }
    (dependencies, gaps)
}

fn derive_flow_dependencies(
    definitions: &BTreeMap<DataFlowDefinitionKey, FlowDefinitionFact>,
    accesses: &[FlowAccessFact],
) -> Result<(Vec<FlowDependency>, Vec<DataFlowAccessKey>), ProgramDependenceBuildError> {
    let mut dependencies = Vec::new();
    let mut gaps = Vec::new();
    for access in accesses {
        let Some(symbol) = &access.symbol else {
            gaps.push(access.key.clone());
            continue;
        };
        for definition_key in &access.reaching_definitions {
            let definition = definitions.get(definition_key).ok_or_else(|| {
                ProgramDependenceBuildError::Invalid(
                    "access reaches a definition missing from its dataflow graph".into(),
                )
            })?;
            if &definition.symbol != symbol {
                return Err(ProgramDependenceBuildError::Invalid(
                    "flow dependence joins different symbols".into(),
                ));
            }
            dependencies.push(FlowDependency {
                from: definition.point.clone(),
                to: access.point.clone(),
                symbol: symbol.clone(),
                definition: definition_key.clone(),
                access: access.key.clone(),
            });
        }
    }
    dependencies.sort_by(|left, right| {
        (&left.from, &left.to, &left.definition, &left.access).cmp(&(
            &right.from,
            &right.to,
            &right.definition,
            &right.access,
        ))
    });
    gaps.sort();
    Ok((dependencies, gaps))
}

pub fn derive_program_dependence(
    data_flow: Arc<DataFlowProjection>,
    non_structured_control: Arc<NonStructuredControlProjection>,
    policy: ProgramDependencePolicyId,
) -> Result<ProgramDependenceProjection, ProgramDependenceBuildError> {
    if data_flow.control_regions().id() != non_structured_control.control_regions().id() {
        return Err(ProgramDependenceBuildError::Invalid(
            "program-dependence sources use different control-region projections".into(),
        ));
    }
    let regions = data_flow.control_regions();
    let flow = regions.control_flow();
    let mut graphs = Vec::with_capacity(data_flow.document().graphs().len());
    for data_graph in data_flow.document().graphs() {
        let region_graph = regions
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == data_graph.control_region_graph())
            .ok_or_else(|| {
                ProgramDependenceBuildError::Invalid(
                    "dataflow graph references a missing control-region graph".into(),
                )
            })?;
        let flow_graph = flow
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == data_graph.control_flow_graph())
            .ok_or_else(|| {
                ProgramDependenceBuildError::Invalid(
                    "dataflow graph references a missing control-flow graph".into(),
                )
            })?;
        let non_structured_graph = non_structured_control
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.control_region_graph() == region_graph.key())
            .ok_or_else(|| {
                ProgramDependenceBuildError::Invalid(
                    "control-region graph has no non-structured classification graph".into(),
                )
            })?;
        graphs.push(derive_graph(
            flow_graph,
            region_graph,
            non_structured_graph,
            data_graph,
            &policy,
        )?);
    }
    graphs.sort_by(|left, right| left.key.cmp(&right.key));
    let resolution = data_flow.resolution();
    let payload = serde_json::to_vec(&(
        flow.id(),
        flow.policy(),
        regions.id(),
        regions.policy(),
        non_structured_control.id(),
        non_structured_control.policy(),
        resolution.id(),
        resolution.document().resolution_policy(),
        data_flow.id(),
        data_flow.policy(),
        &policy,
        &graphs,
    ))
    .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
    let id = flow
        .analysis()
        .derive_projection_id(
            PROGRAM_DEPENDENCE_SCHEMA,
            &payload,
            data_flow.id().as_str().as_bytes(),
        )
        .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
    let document = ProgramDependenceDocument {
        schema: PROGRAM_DEPENDENCE_SCHEMA.into(),
        projection_id: id.clone(),
        analysis_id: flow.analysis().id().as_str().into(),
        control_flow_projection_id: flow.id().clone(),
        control_flow_policy: flow.policy().clone(),
        control_region_projection_id: regions.id().clone(),
        control_region_policy: regions.policy().clone(),
        non_structured_control_projection_id: non_structured_control.id().clone(),
        non_structured_control_policy: non_structured_control.policy().clone(),
        resolution_projection_id: resolution.id().clone(),
        resolution_policy: resolution.document().resolution_policy().clone(),
        data_flow_projection_id: data_flow.id().clone(),
        data_flow_policy: data_flow.policy().clone(),
        policy: policy.clone(),
        graphs,
    };
    document.validate()?;
    Ok(ProgramDependenceProjection {
        id,
        data_flow,
        non_structured_control,
        policy,
        document,
    })
}

fn derive_graph(
    flow: &ControlFlowGraph,
    regions: &ControlRegionGraph,
    non_structured: &NonStructuredControlGraph,
    data: &DataFlowGraph,
    policy: &ProgramDependencePolicyId,
) -> Result<ProgramDependenceGraph, ProgramDependenceBuildError> {
    if flow.key() != regions.control_flow_graph()
        || flow.key() != data.control_flow_graph()
        || regions.key() != data.control_region_graph()
        || regions.key() != non_structured.control_region_graph()
        || flow.owner() != regions.owner()
        || flow.owner() != data.owner()
        || flow.owner() != non_structured.owner()
    {
        return Err(ProgramDependenceBuildError::Invalid(
            "program-dependence source graphs do not describe one owner".into(),
        ));
    }

    let region_points = regions
        .points()
        .iter()
        .map(|point| (point.point().clone(), point))
        .collect::<BTreeMap<_, _>>();
    let data_points = data
        .points()
        .iter()
        .map(|point| (point.point().clone(), point))
        .collect::<BTreeMap<_, _>>();
    if region_points.len() != flow.points().len() || data_points.len() != flow.points().len() {
        return Err(ProgramDependenceBuildError::Invalid(
            "program-dependence sources disagree on the CFG point set".into(),
        ));
    }

    let mut nodes = Vec::with_capacity(flow.points().len());
    let mut node_by_point = BTreeMap::new();
    for point in flow.points() {
        let region = region_points.get(point.key()).ok_or_else(|| {
            ProgramDependenceBuildError::Invalid("control-region point is missing".into())
        })?;
        let data_point = data_points.get(point.key()).ok_or_else(|| {
            ProgramDependenceBuildError::Invalid("dataflow point is missing".into())
        })?;
        if region.reachable() != data_point.reachable() {
            return Err(ProgramDependenceBuildError::Invalid(
                "control-region and dataflow reachability disagree".into(),
            ));
        }
        let payload = serde_json::to_vec(&(
            point.key(),
            region.key(),
            data_point.key(),
            point.source(),
            region.reachable(),
            region.exit_reachable(),
        ))
        .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
        let key = ProgramDependenceNodeKey(derive_id(
            NODE_DOMAIN,
            "pdn1_",
            &[policy.as_str().as_bytes(), &payload],
        ));
        node_by_point.insert(point.key().clone(), key.clone());
        nodes.push(ProgramDependenceNode {
            key,
            point: point.key().clone(),
            control_region_point: region.key().clone(),
            data_flow_point: data_point.key().clone(),
            source: point.source().cloned(),
            reachable: region.reachable(),
            exit_reachable: region.exit_reachable(),
        });
    }
    nodes.sort_by(|left, right| left.key.cmp(&right.key));

    let control_facts = regions
        .points()
        .iter()
        .map(|point| {
            (
                point.point().clone(),
                ControlFact {
                    reachable: point.reachable(),
                    exit_reachable: point.exit_reachable(),
                    post_dominators: point.post_dominators().iter().cloned().collect(),
                    immediate_post_dominator: point.immediate_post_dominator().cloned(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let witnesses = flow
        .edges()
        .iter()
        .map(|edge| ControlWitness {
            edge: edge.key().clone(),
            from: edge.from().clone(),
            to: edge.to().clone(),
        })
        .collect::<Vec<_>>();
    let (control_dependencies, control_gaps) =
        derive_control_dependencies(&control_facts, &witnesses);

    let mut edges = Vec::new();
    for ((from, to), inducing_edges) in control_dependencies {
        let kind = ProgramDependenceEdgeKind::Control {
            inducing_edges: inducing_edges.into_iter().collect(),
        };
        edges.push(make_edge(
            policy,
            node_by_point[&from].clone(),
            node_by_point[&to].clone(),
            kind,
        )?);
    }
    let definitions = data
        .definitions()
        .iter()
        .map(|definition| {
            (
                definition.key().clone(),
                FlowDefinitionFact {
                    point: definition.point().clone(),
                    symbol: definition.symbol().clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let accesses = data
        .accesses()
        .iter()
        .map(|access| FlowAccessFact {
            key: access.key().clone(),
            point: access.point().clone(),
            symbol: access.symbol().cloned(),
            reaching_definitions: access.reaching_definitions().to_vec(),
        })
        .collect::<Vec<_>>();
    let (flow_dependencies, unresolved_accesses) =
        derive_flow_dependencies(&definitions, &accesses)?;
    let mut gaps = Vec::new();
    for dependency in flow_dependencies {
        edges.push(make_edge(
            policy,
            node_by_point[&dependency.from].clone(),
            node_by_point[&dependency.to].clone(),
            ProgramDependenceEdgeKind::Flow {
                symbol: dependency.symbol,
                definition: dependency.definition,
                access: dependency.access,
            },
        )?);
    }
    for access in unresolved_accesses {
        gaps.push(make_gap(
            policy,
            ProgramDependenceGapKind::UnresolvedAccess { access },
        )?);
    }
    for witness in control_gaps {
        gaps.push(make_gap(
            policy,
            ProgramDependenceGapKind::ControlPostDominanceUnavailable {
                edge: witness.edge,
                from: witness.from,
                to: witness.to,
            },
        )?);
    }
    edges.sort_by(|left, right| left.key.cmp(&right.key));
    gaps.sort_by(|left, right| left.key.cmp(&right.key));

    let mut reasons = Vec::new();
    reasons.extend(
        regions
            .coverage()
            .reasons()
            .iter()
            .map(|reason| format!("control regions: {reason}")),
    );
    reasons.extend(
        non_structured
            .coverage()
            .reasons()
            .iter()
            .map(|reason| format!("non-structured control: {reason}")),
    );
    reasons.extend(
        data.coverage()
            .reasons()
            .iter()
            .map(|reason| format!("dataflow: {reason}")),
    );
    let local_pdg = flow
        .adapter()
        .capabilities()
        .declaration(AdapterCapability::LocalPdg);
    if local_pdg.support() != CapabilitySupport::Provided {
        reasons.push(format!(
            "adapter LocalPdg capability is {}",
            local_pdg.support().as_str()
        ));
    }
    for gap in &gaps {
        reasons.push(gap_reason(&gap.kind));
    }
    reasons.sort();
    reasons.dedup();
    let status = if reasons.is_empty()
        && regions.coverage().status() == FactCoverage::Complete
        && non_structured.coverage().status() == FactCoverage::Complete
        && data.coverage().status() == FactCoverage::Complete
        && local_pdg.support() == CapabilitySupport::Provided
    {
        FactCoverage::Complete
    } else if regions.coverage().status() == FactCoverage::Failed
        || non_structured.coverage().status() == FactCoverage::Failed
        || data.coverage().status() == FactCoverage::Failed
    {
        FactCoverage::Failed
    } else if local_pdg.support() == CapabilitySupport::Unsupported {
        FactCoverage::Unsupported
    } else {
        FactCoverage::Partial
    };
    let coverage = ProgramDependenceCoverageEvidence {
        status,
        local_pdg_support: local_pdg.support(),
        local_pdg_authority: local_pdg.authority(),
        reasons,
    };
    coverage.validate()?;

    let mut control_edges = flow
        .edges()
        .iter()
        .map(|edge| edge.key().clone())
        .collect::<Vec<_>>();
    control_edges.sort();
    let mut symbols = data
        .symbols()
        .iter()
        .map(|symbol| symbol.key().clone())
        .collect::<Vec<_>>();
    symbols.sort();
    let mut definition_keys = data
        .definitions()
        .iter()
        .map(|definition| definition.key().clone())
        .collect::<Vec<_>>();
    definition_keys.sort();
    let mut accesses = data
        .accesses()
        .iter()
        .map(|access| access.key().clone())
        .collect::<Vec<_>>();
    accesses.sort();
    let mut non_structured_facts = non_structured
        .facts()
        .iter()
        .map(|fact| fact.key().clone())
        .collect::<Vec<_>>();
    non_structured_facts.sort();
    let mut graph = ProgramDependenceGraph {
        key: ProgramDependenceGraphKey(String::new()),
        control_flow_graph: flow.key().clone(),
        control_region_graph: regions.key().clone(),
        non_structured_control_graph: non_structured.key().clone(),
        data_flow_graph: data.key().clone(),
        owner: flow.owner().clone(),
        coverage,
        control_edges,
        symbols,
        definitions: definition_keys,
        accesses,
        non_structured_facts,
        nodes,
        edges,
        gaps,
    };
    let payload = graph_payload(&graph)?;
    graph.key = ProgramDependenceGraphKey(derive_id(
        GRAPH_DOMAIN,
        "pdg1_",
        &[policy.as_str().as_bytes(), &payload],
    ));
    validate_graph(policy, &graph)?;
    Ok(graph)
}

fn make_edge(
    policy: &ProgramDependencePolicyId,
    from: ProgramDependenceNodeKey,
    to: ProgramDependenceNodeKey,
    kind: ProgramDependenceEdgeKind,
) -> Result<ProgramDependenceEdge, ProgramDependenceBuildError> {
    let payload = serde_json::to_vec(&(&from, &to, &kind))
        .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
    Ok(ProgramDependenceEdge {
        key: ProgramDependenceEdgeKey(derive_id(
            EDGE_DOMAIN,
            "pde1_",
            &[policy.as_str().as_bytes(), &payload],
        )),
        from,
        to,
        kind,
    })
}

fn make_gap(
    policy: &ProgramDependencePolicyId,
    kind: ProgramDependenceGapKind,
) -> Result<ProgramDependenceGap, ProgramDependenceBuildError> {
    let payload = serde_json::to_vec(&kind)
        .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
    Ok(ProgramDependenceGap {
        key: ProgramDependenceGapKey(derive_id(
            GAP_DOMAIN,
            "pdx1_",
            &[policy.as_str().as_bytes(), &payload],
        )),
        kind,
    })
}

fn validate_graph(
    policy: &ProgramDependencePolicyId,
    graph: &ProgramDependenceGraph,
) -> Result<(), ProgramDependenceBuildError> {
    graph.coverage.validate()?;
    if graph.coverage.status == FactCoverage::Complete && !graph.gaps.is_empty() {
        return Err(ProgramDependenceBuildError::Invalid(
            "Complete program-dependence coverage cannot carry typed gaps".into(),
        ));
    }
    validate_canonical("source control edges", &graph.control_edges)?;
    validate_canonical("source dataflow symbols", &graph.symbols)?;
    validate_canonical("source dataflow definitions", &graph.definitions)?;
    validate_canonical("source dataflow accesses", &graph.accesses)?;
    validate_canonical(
        "source non-structured control facts",
        &graph.non_structured_facts,
    )?;
    validate_sorted_by("program-dependence nodes", &graph.nodes, |node| {
        node.key.as_str()
    })?;
    validate_sorted_by("program-dependence edges", &graph.edges, |edge| {
        edge.key.as_str()
    })?;
    validate_sorted_by("program-dependence gaps", &graph.gaps, |gap| {
        gap.key.as_str()
    })?;
    let nodes = graph
        .nodes
        .iter()
        .map(|node| (&node.key, node))
        .collect::<BTreeMap<_, _>>();
    if nodes.len() != graph.nodes.len()
        || graph
            .nodes
            .iter()
            .map(|node| &node.point)
            .collect::<BTreeSet<_>>()
            .len()
            != graph.nodes.len()
    {
        return Err(ProgramDependenceBuildError::Invalid(
            "program-dependence nodes repeat a key or CFG point".into(),
        ));
    }
    for node in &graph.nodes {
        let payload = serde_json::to_vec(&(
            &node.point,
            &node.control_region_point,
            &node.data_flow_point,
            &node.source,
            node.reachable,
            node.exit_reachable,
        ))
        .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
        let expected = ProgramDependenceNodeKey(derive_id(
            NODE_DOMAIN,
            "pdn1_",
            &[policy.as_str().as_bytes(), &payload],
        ));
        if node.key != expected {
            return Err(ProgramDependenceBuildError::Invalid(
                "program-dependence node key does not bind its payload".into(),
            ));
        }
    }
    for edge in &graph.edges {
        let from = nodes.get(&edge.from).ok_or_else(|| {
            ProgramDependenceBuildError::Invalid("dependence edge has a missing origin".into())
        })?;
        let to = nodes.get(&edge.to).ok_or_else(|| {
            ProgramDependenceBuildError::Invalid("dependence edge has a missing target".into())
        })?;
        if !from.reachable || !to.reachable {
            return Err(ProgramDependenceBuildError::Invalid(
                "dependence edge references an unreachable point".into(),
            ));
        }
        match &edge.kind {
            ProgramDependenceEdgeKind::Control { inducing_edges } => {
                if !from.exit_reachable || !to.exit_reachable {
                    return Err(ProgramDependenceBuildError::Invalid(
                        "control dependence requires exit-reachable post-dominance evidence".into(),
                    ));
                }
                validate_canonical("inducing control edges", inducing_edges)?;
                if inducing_edges.is_empty()
                    || inducing_edges
                        .iter()
                        .any(|key| graph.control_edges.binary_search(key).is_err())
                {
                    return Err(ProgramDependenceBuildError::Invalid(
                        "control dependence has missing inducing CFG evidence".into(),
                    ));
                }
            }
            ProgramDependenceEdgeKind::Flow {
                symbol,
                definition,
                access,
            } => {
                if graph.symbols.binary_search(symbol).is_err()
                    || graph.definitions.binary_search(definition).is_err()
                    || graph.accesses.binary_search(access).is_err()
                {
                    return Err(ProgramDependenceBuildError::Invalid(
                        "flow dependence has missing dataflow evidence".into(),
                    ));
                }
            }
        }
        let payload = serde_json::to_vec(&(&edge.from, &edge.to, &edge.kind))
            .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
        let expected = ProgramDependenceEdgeKey(derive_id(
            EDGE_DOMAIN,
            "pde1_",
            &[policy.as_str().as_bytes(), &payload],
        ));
        if edge.key != expected {
            return Err(ProgramDependenceBuildError::Invalid(
                "program-dependence edge key does not bind its payload".into(),
            ));
        }
    }
    for gap in &graph.gaps {
        match &gap.kind {
            ProgramDependenceGapKind::UnresolvedAccess { access } => {
                if graph.accesses.binary_search(access).is_err() {
                    return Err(ProgramDependenceBuildError::Invalid(
                        "unresolved-access gap cites missing dataflow evidence".into(),
                    ));
                }
            }
            ProgramDependenceGapKind::ControlPostDominanceUnavailable { edge, from, to } => {
                let from_node = graph.nodes.iter().find(|node| &node.point == from);
                let to_node = graph.nodes.iter().find(|node| &node.point == to);
                if graph.control_edges.binary_search(edge).is_err()
                    || from_node.is_none_or(|node| !node.reachable)
                    || to_node.is_none_or(|node| !node.reachable)
                {
                    return Err(ProgramDependenceBuildError::Invalid(
                        "control gap cites missing CFG evidence".into(),
                    ));
                }
            }
        }
        let payload = serde_json::to_vec(&gap.kind)
            .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))?;
        let expected = ProgramDependenceGapKey(derive_id(
            GAP_DOMAIN,
            "pdx1_",
            &[policy.as_str().as_bytes(), &payload],
        ));
        if gap.key != expected {
            return Err(ProgramDependenceBuildError::Invalid(
                "program-dependence gap key does not bind its payload".into(),
            ));
        }
        if graph
            .coverage
            .reasons
            .binary_search(&gap_reason(&gap.kind))
            .is_err()
        {
            return Err(ProgramDependenceBuildError::Invalid(
                "typed program-dependence gap is missing its coverage reason".into(),
            ));
        }
    }
    let payload = graph_payload(graph)?;
    let expected = ProgramDependenceGraphKey(derive_id(
        GRAPH_DOMAIN,
        "pdg1_",
        &[policy.as_str().as_bytes(), &payload],
    ));
    if graph.key != expected {
        return Err(ProgramDependenceBuildError::Invalid(
            "program-dependence graph key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn graph_payload(graph: &ProgramDependenceGraph) -> Result<Vec<u8>, ProgramDependenceBuildError> {
    serde_json::to_vec(&(
        &graph.control_flow_graph,
        &graph.control_region_graph,
        &graph.non_structured_control_graph,
        &graph.data_flow_graph,
        &graph.owner,
        &graph.coverage,
        &graph.control_edges,
        &graph.symbols,
        &graph.definitions,
        &graph.accesses,
        &graph.non_structured_facts,
        &graph.nodes,
        &graph.edges,
        &graph.gaps,
    ))
    .map_err(|error| ProgramDependenceBuildError::Identity(error.to_string()))
}

fn validate_sorted_by<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), ProgramDependenceBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        Err(ProgramDependenceBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )))
    } else {
        Ok(())
    }
}

fn validate_canonical<T: Ord>(
    label: &str,
    values: &[T],
) -> Result<(), ProgramDependenceBuildError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(ProgramDependenceBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), ProgramDependenceBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(ProgramDependenceBuildError::Invalid(format!(
            "identity must start with {prefix}"
        )));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProgramDependenceBuildError::Invalid(
            "identity must contain a canonical 32-byte hexadecimal digest".into(),
        ));
    }
    Ok(())
}

fn validate_text(value: &str) -> Result<(), ProgramDependenceBuildError> {
    if value.trim().is_empty() || value.trim() != value {
        Err(ProgramDependenceBuildError::Invalid(
            "program-dependence reason must be canonical nonempty text".into(),
        ))
    } else {
        Ok(())
    }
}

fn gap_reason(kind: &ProgramDependenceGapKind) -> String {
    match kind {
        ProgramDependenceGapKind::UnresolvedAccess { access } => {
            format!("unresolved dataflow access {}", access.as_str())
        }
        ProgramDependenceGapKind::ControlPostDominanceUnavailable { edge, .. } => format!(
            "control edge {} lacks a complete post-dominator chain",
            edge.as_str()
        ),
    }
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
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldM4Corpus {
        schema: String,
        oracle: GoldOracle,
        cfg: GoldCfg,
        pst: GoldPst,
        pdg: GoldPdg,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldOracle {
        hand_labelled: String,
        compiler_graph_status: String,
        compiler_graph_reason: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldCfg {
        points: Vec<GoldCfgPoint>,
        edges: Vec<GoldCfgEdge>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldCfgPoint {
        label: String,
        kind: crate::ControlPointKind,
        source_kind: Option<String>,
        ordinal: u32,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldCfgEdge {
        label: String,
        from: String,
        to: String,
        kind: crate::ControlEdgeKind,
        source_kind: String,
        predicate_kind: Option<String>,
        precision: crate::ControlEdgePrecision,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldPst {
        points: Vec<GoldRegionPoint>,
        regions: Vec<GoldRegion>,
        residuals: Vec<GoldResidual>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldRegionPoint {
        point: String,
        reachable: bool,
        exit_reachable: bool,
        dominators: Vec<String>,
        immediate_dominator: Option<String>,
        dominator_depth: Option<u32>,
        post_dominators: Vec<String>,
        immediate_post_dominator: Option<String>,
        post_dominator_depth: Option<u32>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldRegion {
        label: String,
        kind: crate::StructuredControlRegionKind,
        entry: String,
        exit: String,
        points: Vec<String>,
        parent: Option<String>,
        children: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldResidual {
        kind: crate::StructuredControlRegionKind,
        entry: String,
        exit: String,
        points: Vec<String>,
        reason: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldPdg {
        control_edges: Vec<GoldPdgControlEdge>,
        flow_edges: Vec<GoldPdgFlowEdge>,
        unresolved_accesses: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldPdgControlEdge {
        from: String,
        to: String,
        inducing_cfg_edges: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldPdgFlowEdge {
        from: String,
        to: String,
        symbol: String,
        definition: String,
        access: String,
    }

    struct NormalizedCfg {
        gold: GoldCfg,
        point_labels: BTreeMap<ControlPointKey, String>,
        edge_labels: BTreeMap<ControlEdgeKey, String>,
    }

    fn normalize_cfg(graph: &crate::ControlFlowGraph) -> NormalizedCfg {
        let mut points = graph.points().iter().collect::<Vec<_>>();
        points.sort_by_key(|point| {
            (
                serde_json::to_string(point.kind()).unwrap(),
                point
                    .source()
                    .map(|source| source.raw_grammar_kind().to_string()),
                point.ordinal(),
            )
        });
        let point_labels = points
            .iter()
            .enumerate()
            .map(|(index, point)| (point.key().clone(), format!("p{index:02}")))
            .collect::<BTreeMap<_, _>>();
        let points = points
            .into_iter()
            .map(|point| GoldCfgPoint {
                label: point_labels[point.key()].clone(),
                kind: point.kind().clone(),
                source_kind: point
                    .source()
                    .map(|source| source.raw_grammar_kind().to_string()),
                ordinal: point.ordinal(),
            })
            .collect::<Vec<_>>();
        let mut edges = graph.edges().iter().collect::<Vec<_>>();
        edges.sort_by_key(|edge| {
            (
                point_labels[edge.from()].clone(),
                point_labels[edge.to()].clone(),
                serde_json::to_string(edge.kind()).unwrap(),
                edge.source().raw_grammar_kind().to_string(),
                edge.predicate()
                    .map(|node| node.raw_grammar_kind().to_string()),
            )
        });
        let edge_labels = edges
            .iter()
            .enumerate()
            .map(|(index, edge)| (edge.key().clone(), format!("e{index:02}")))
            .collect::<BTreeMap<_, _>>();
        let edges = edges
            .into_iter()
            .map(|edge| GoldCfgEdge {
                label: edge_labels[edge.key()].clone(),
                from: point_labels[edge.from()].clone(),
                to: point_labels[edge.to()].clone(),
                kind: edge.kind().clone(),
                source_kind: edge.source().raw_grammar_kind().to_string(),
                predicate_kind: edge
                    .predicate()
                    .map(|node| node.raw_grammar_kind().to_string()),
                precision: edge.precision().clone(),
            })
            .collect();
        NormalizedCfg {
            gold: GoldCfg { points, edges },
            point_labels,
            edge_labels,
        }
    }

    fn sorted_labels<'a>(
        keys: impl IntoIterator<Item = &'a ControlPointKey>,
        labels: &BTreeMap<ControlPointKey, String>,
    ) -> Vec<String> {
        let mut values = keys
            .into_iter()
            .map(|key| labels[key].clone())
            .collect::<Vec<_>>();
        values.sort();
        values
    }

    fn normalize_pst(
        graph: &crate::ControlRegionGraph,
        point_labels: &BTreeMap<ControlPointKey, String>,
    ) -> GoldPst {
        let mut points = graph
            .points()
            .iter()
            .map(|point| GoldRegionPoint {
                point: point_labels[point.point()].clone(),
                reachable: point.reachable(),
                exit_reachable: point.exit_reachable(),
                dominators: sorted_labels(point.dominators(), point_labels),
                immediate_dominator: point
                    .immediate_dominator()
                    .map(|key| point_labels[key].clone()),
                dominator_depth: point.dominator_depth(),
                post_dominators: sorted_labels(point.post_dominators(), point_labels),
                immediate_post_dominator: point
                    .immediate_post_dominator()
                    .map(|key| point_labels[key].clone()),
                post_dominator_depth: point.post_dominator_depth(),
            })
            .collect::<Vec<_>>();
        points.sort_by(|left, right| left.point.cmp(&right.point));

        let mut source_regions = graph.regions().iter().collect::<Vec<_>>();
        source_regions.sort_by_key(|region| {
            (
                region.kind(),
                point_labels[region.entry()].clone(),
                point_labels[region.exit()].clone(),
            )
        });
        let region_labels = source_regions
            .iter()
            .enumerate()
            .map(|(index, region)| (region.key().clone(), format!("r{index:02}")))
            .collect::<BTreeMap<_, _>>();
        let regions = source_regions
            .into_iter()
            .map(|region| GoldRegion {
                label: region_labels[region.key()].clone(),
                kind: region.kind(),
                entry: point_labels[region.entry()].clone(),
                exit: point_labels[region.exit()].clone(),
                points: sorted_labels(region.points(), point_labels),
                parent: region.parent().map(|key| region_labels[key].clone()),
                children: {
                    let mut children = region
                        .children()
                        .iter()
                        .map(|key| region_labels[key].clone())
                        .collect::<Vec<_>>();
                    children.sort();
                    children
                },
            })
            .collect();
        let mut residuals = graph
            .residuals()
            .iter()
            .map(|residual| GoldResidual {
                kind: residual.kind(),
                entry: point_labels[residual.entry()].clone(),
                exit: point_labels[residual.exit()].clone(),
                points: sorted_labels(residual.points(), point_labels),
                reason: residual.reason().into(),
            })
            .collect::<Vec<_>>();
        residuals
            .sort_by_key(|residual| (residual.kind, residual.entry.clone(), residual.exit.clone()));
        GoldPst {
            points,
            regions,
            residuals,
        }
    }

    fn actual_m4_gold() -> GoldM4Corpus {
        let flow = Arc::new(crate::control_flow::tests::complete_projection());
        let flow_graph = &flow.document().graphs()[0];
        let normalized = normalize_cfg(flow_graph);
        let regions = crate::derive_control_regions(
            Arc::clone(&flow),
            crate::ControlRegionPolicyId::from_parts(&[b"m4.9-gold-regions/1"]).unwrap(),
        )
        .unwrap();
        let region_graph = &regions.document().graphs()[0];
        let pst = normalize_pst(region_graph, &normalized.point_labels);

        let control_facts = region_graph
            .points()
            .iter()
            .map(|point| {
                (
                    point.point().clone(),
                    ControlFact {
                        reachable: point.reachable(),
                        exit_reachable: point.exit_reachable(),
                        post_dominators: point.post_dominators().iter().cloned().collect(),
                        immediate_post_dominator: point.immediate_post_dominator().cloned(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let witnesses = flow_graph
            .edges()
            .iter()
            .map(|edge| ControlWitness {
                edge: edge.key().clone(),
                from: edge.from().clone(),
                to: edge.to().clone(),
            })
            .collect::<Vec<_>>();
        let (control_dependencies, control_gaps) =
            derive_control_dependencies(&control_facts, &witnesses);
        assert!(control_gaps.is_empty());
        let mut control_edges = control_dependencies
            .into_iter()
            .map(|((from, to), inducing)| GoldPdgControlEdge {
                from: normalized.point_labels[&from].clone(),
                to: normalized.point_labels[&to].clone(),
                inducing_cfg_edges: {
                    let mut labels = inducing
                        .iter()
                        .map(|edge| normalized.edge_labels[edge].clone())
                        .collect::<Vec<_>>();
                    labels.sort();
                    labels
                },
            })
            .collect::<Vec<_>>();
        control_edges.sort_by(|left, right| {
            (&left.from, &left.to, &left.inducing_cfg_edges).cmp(&(
                &right.from,
                &right.to,
                &right.inducing_cfg_edges,
            ))
        });

        let value = symbol(0);
        let left = definition(0);
        let right = definition(1);
        let resolved = access(0);
        let unresolved = access(1);
        let definitions = BTreeMap::from([
            (
                left.clone(),
                FlowDefinitionFact {
                    point: point(0),
                    symbol: value.clone(),
                },
            ),
            (
                right.clone(),
                FlowDefinitionFact {
                    point: point(1),
                    symbol: value.clone(),
                },
            ),
        ]);
        let accesses = vec![
            FlowAccessFact {
                key: resolved.clone(),
                point: point(2),
                symbol: Some(value),
                reaching_definitions: vec![left, right],
            },
            FlowAccessFact {
                key: unresolved,
                point: point(2),
                symbol: None,
                reaching_definitions: vec![],
            },
        ];
        let (flow_dependencies, unresolved_accesses) =
            derive_flow_dependencies(&definitions, &accesses).unwrap();
        let flow_edges = flow_dependencies
            .iter()
            .map(|dependency| GoldPdgFlowEdge {
                from: if dependency.from == point(0) {
                    "q0".into()
                } else {
                    "q1".into()
                },
                to: "q2".into(),
                symbol: "s0".into(),
                definition: if dependency.definition == definition(0) {
                    "d0".into()
                } else {
                    "d1".into()
                },
                access: "a0".into(),
            })
            .collect();
        let unresolved_accesses = unresolved_accesses
            .iter()
            .map(|key| {
                if key == &access(1) {
                    "a1".into()
                } else {
                    panic!("unexpected unresolved gold access")
                }
            })
            .collect();
        GoldM4Corpus {
            schema: "deslop.m4-graph-gold/1".into(),
            oracle: GoldOracle {
                hand_labelled: "m4.9-complete-advanced-cfg-plus-multidef-flow/1".into(),
                compiler_graph_status: "unavailable".into(),
                compiler_graph_reason: "no retained compiler-authoritative CFG/PST/PDG artifact with version, configuration, and dependency identity".into(),
            },
            cfg: normalized.gold,
            pst,
            pdg: GoldPdg {
                control_edges,
                flow_edges,
                unresolved_accesses,
            },
        }
    }

    fn frozen_m4_gold() -> GoldM4Corpus {
        parse_m4_gold(include_str!("../../../tests/fixtures/m4_graph_gold.json")).unwrap()
    }

    fn parse_m4_gold(input: &str) -> Result<GoldM4Corpus, String> {
        let gold =
            serde_json::from_str::<GoldM4Corpus>(input).map_err(|error| error.to_string())?;
        if gold.schema != "deslop.m4-graph-gold/1" {
            return Err("unsupported M4 graph gold schema".into());
        }
        if gold.oracle.hand_labelled.trim().is_empty()
            || gold.oracle.compiler_graph_status != "unavailable"
            || gold.oracle.compiler_graph_reason.trim().is_empty()
        {
            return Err("M4 graph gold oracle provenance is incomplete".into());
        }
        let points = gold
            .cfg
            .points
            .iter()
            .map(|point| point.label.as_str())
            .collect::<BTreeSet<_>>();
        let edges = gold
            .cfg
            .edges
            .iter()
            .map(|edge| edge.label.as_str())
            .collect::<BTreeSet<_>>();
        if points.len() != gold.cfg.points.len()
            || edges.len() != gold.cfg.edges.len()
            || gold.cfg.edges.iter().any(|edge| {
                !points.contains(edge.from.as_str()) || !points.contains(edge.to.as_str())
            })
            || gold.pst.points.iter().any(|point| {
                !points.contains(point.point.as_str())
                    || point
                        .dominators
                        .iter()
                        .chain(&point.post_dominators)
                        .any(|label| !points.contains(label.as_str()))
            })
            || gold.pdg.control_edges.iter().any(|edge| {
                !points.contains(edge.from.as_str())
                    || !points.contains(edge.to.as_str())
                    || edge
                        .inducing_cfg_edges
                        .iter()
                        .any(|label| !edges.contains(label.as_str()))
            })
        {
            return Err("M4 graph gold contains duplicate or dangling labels".into());
        }
        Ok(gold)
    }

    #[test]
    fn m4_9_cfg_pst_and_pdg_match_frozen_hand_gold_exactly() {
        let expected = frozen_m4_gold();
        let actual = actual_m4_gold();
        if expected != actual {
            panic!(
                "M4.9 gold candidate:\n{}",
                serde_json::to_string_pretty(&actual).unwrap()
            );
        }
        assert_eq!(
            [
                actual.cfg.points.len(),
                actual.cfg.edges.len(),
                actual.pst.points.len(),
                actual.pst.regions.len(),
                actual.pst.residuals.len(),
                actual.pdg.control_edges.len(),
                actual.pdg.flow_edges.len(),
                actual.pdg.unresolved_accesses.len(),
            ],
            [11, 14, 11, 2, 0, 9, 2, 1]
        );
        assert_eq!(
            actual.cfg.points.len()
                + actual.cfg.edges.len()
                + actual.pst.points.len()
                + actual.pst.regions.len()
                + actual.pst.residuals.len()
                + actual.pdg.control_edges.len()
                + actual.pdg.flow_edges.len()
                + actual.pdg.unresolved_accesses.len(),
            50
        );
    }

    #[test]
    fn m4_9_gold_schema_rejects_unknown_fields_wrong_versions_and_dangling_labels() {
        let source = include_str!("../../../tests/fixtures/m4_graph_gold.json");
        let mut unknown = serde_json::from_str::<serde_json::Value>(source).unwrap();
        unknown["untrusted"] = true.into();
        assert!(parse_m4_gold(&serde_json::to_string(&unknown).unwrap()).is_err());
        let mut wrong_schema = serde_json::from_str::<serde_json::Value>(source).unwrap();
        wrong_schema["schema"] = "deslop.m4-graph-gold/999".into();
        assert!(parse_m4_gold(&serde_json::to_string(&wrong_schema).unwrap()).is_err());
        let mut dangling = serde_json::from_str::<serde_json::Value>(source).unwrap();
        dangling["cfg"]["edges"][0]["to"] = "p99".into();
        assert!(parse_m4_gold(&serde_json::to_string(&dangling).unwrap()).is_err());
    }

    #[test]
    fn m4_9_semantic_mutation_cannot_pass_exact_gold_comparison() {
        let actual = actual_m4_gold();
        let mut changed = actual.clone();
        changed.cfg.edges[0].kind = crate::ControlEdgeKind::Normal;
        assert_ne!(changed, actual);
        let mut changed = actual.clone();
        changed.pst.points[0].post_dominators.clear();
        assert_ne!(changed, actual);
        let mut changed = actual.clone();
        changed.pdg.flow_edges[0].from = "q9".into();
        assert_ne!(changed, actual);
    }

    #[test]
    fn m4_9_production_adapters_have_no_compiler_graph_oracle_to_compare() {
        let registry = deslop_lang::Registry::default();
        for language in [
            deslop_core::Lang::Clojure,
            deslop_core::Lang::Julia,
            deslop_core::Lang::Python,
            deslop_core::Lang::JavaScript,
            deslop_core::Lang::TypeScript,
            deslop_core::Lang::Rust,
        ] {
            let declaration = registry
                .pack_for_lang(language)
                .capability_manifest()
                .declaration(crate::AdapterCapability::CompilerTypeEvidence)
                .clone();
            assert_ne!(declaration.support(), crate::CapabilitySupport::Provided);
            assert_eq!(declaration.authority(), None);
        }
        let gold = frozen_m4_gold();
        assert_eq!(gold.oracle.compiler_graph_status, "unavailable");
        assert_eq!(
            gold.oracle.compiler_graph_reason,
            "no retained compiler-authoritative CFG/PST/PDG artifact with version, configuration, and dependency identity"
        );
    }

    fn point(index: usize) -> ControlPointKey {
        serde_json::from_str(&format!("\"cpt1_{:064x}\"", index + 1)).unwrap()
    }

    fn edge(index: usize) -> ControlEdgeKey {
        serde_json::from_str(&format!("\"ced1_{:064x}\"", index + 1)).unwrap()
    }

    fn symbol(index: usize) -> DataFlowSymbolKey {
        serde_json::from_str(&format!("\"dfs1_{:064x}\"", index + 1)).unwrap()
    }

    fn definition(index: usize) -> DataFlowDefinitionKey {
        serde_json::from_str(&format!("\"dfd1_{:064x}\"", index + 1)).unwrap()
    }

    fn access(index: usize) -> DataFlowAccessKey {
        serde_json::from_str(&format!("\"dfa1_{:064x}\"", index + 1)).unwrap()
    }

    fn fact(
        reachable: bool,
        exit_reachable: bool,
        post_dominators: &[usize],
        immediate_post_dominator: Option<usize>,
    ) -> ControlFact {
        ControlFact {
            reachable,
            exit_reachable,
            post_dominators: post_dominators.iter().copied().map(point).collect(),
            immediate_post_dominator: immediate_post_dominator.map(point),
        }
    }

    #[test]
    fn m4_6_diamond_has_only_branch_control_dependence() {
        let facts = BTreeMap::from([
            (point(0), fact(true, true, &[0, 3, 4], Some(3))),
            (point(1), fact(true, true, &[1, 3, 4], Some(3))),
            (point(2), fact(true, true, &[2, 3, 4], Some(3))),
            (point(3), fact(true, true, &[3, 4], Some(4))),
            (point(4), fact(true, true, &[4], None)),
        ]);
        let witnesses = vec![
            ControlWitness {
                edge: edge(0),
                from: point(0),
                to: point(1),
            },
            ControlWitness {
                edge: edge(1),
                from: point(0),
                to: point(2),
            },
            ControlWitness {
                edge: edge(2),
                from: point(1),
                to: point(3),
            },
            ControlWitness {
                edge: edge(3),
                from: point(2),
                to: point(3),
            },
            ControlWitness {
                edge: edge(4),
                from: point(3),
                to: point(4),
            },
        ];
        let (dependencies, gaps) = derive_control_dependencies(&facts, &witnesses);
        assert!(gaps.is_empty());
        assert_eq!(
            dependencies,
            BTreeMap::from([
                ((point(0), point(1)), BTreeSet::from([edge(0)])),
                ((point(0), point(2)), BTreeSet::from([edge(1)])),
            ])
        );
    }

    #[test]
    fn m4_6_nonterminating_target_is_an_explicit_control_gap() {
        let facts = BTreeMap::from([
            (point(0), fact(true, true, &[0, 2], Some(2))),
            (point(1), fact(true, false, &[], None)),
            (point(2), fact(true, true, &[2], None)),
        ]);
        let witness = ControlWitness {
            edge: edge(0),
            from: point(0),
            to: point(1),
        };
        let (dependencies, gaps) =
            derive_control_dependencies(&facts, std::slice::from_ref(&witness));
        assert!(dependencies.is_empty());
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].edge, witness.edge);
    }

    #[test]
    fn m4_6_nested_branches_walk_each_exact_post_dominator_chain() {
        let facts = BTreeMap::from([
            (point(0), fact(true, true, &[0, 5, 6], Some(5))),
            (point(1), fact(true, true, &[1, 4, 5, 6], Some(4))),
            (point(2), fact(true, true, &[2, 4, 5, 6], Some(4))),
            (point(3), fact(true, true, &[3, 4, 5, 6], Some(4))),
            (point(4), fact(true, true, &[4, 5, 6], Some(5))),
            (point(5), fact(true, true, &[5, 6], Some(6))),
            (point(6), fact(true, true, &[6], None)),
        ]);
        let witnesses = vec![
            ControlWitness {
                edge: edge(0),
                from: point(0),
                to: point(1),
            },
            ControlWitness {
                edge: edge(1),
                from: point(0),
                to: point(5),
            },
            ControlWitness {
                edge: edge(2),
                from: point(1),
                to: point(2),
            },
            ControlWitness {
                edge: edge(3),
                from: point(1),
                to: point(3),
            },
            ControlWitness {
                edge: edge(4),
                from: point(2),
                to: point(4),
            },
            ControlWitness {
                edge: edge(5),
                from: point(3),
                to: point(4),
            },
            ControlWitness {
                edge: edge(6),
                from: point(4),
                to: point(5),
            },
            ControlWitness {
                edge: edge(7),
                from: point(5),
                to: point(6),
            },
        ];
        let (dependencies, gaps) = derive_control_dependencies(&facts, &witnesses);
        assert!(gaps.is_empty());
        assert_eq!(
            dependencies,
            BTreeMap::from([
                ((point(0), point(1)), BTreeSet::from([edge(0)])),
                ((point(0), point(4)), BTreeSet::from([edge(0)])),
                ((point(1), point(2)), BTreeSet::from([edge(2)])),
                ((point(1), point(3)), BTreeSet::from([edge(3)])),
            ])
        );
    }

    #[test]
    fn m4_6_loop_header_has_explicit_self_control_dependence() {
        let facts = BTreeMap::from([
            (point(0), fact(true, true, &[0, 1, 3, 4], Some(1))),
            (point(1), fact(true, true, &[1, 3, 4], Some(3))),
            (point(2), fact(true, true, &[1, 2, 3, 4], Some(1))),
            (point(3), fact(true, true, &[3, 4], Some(4))),
            (point(4), fact(true, true, &[4], None)),
        ]);
        let witnesses = vec![
            ControlWitness {
                edge: edge(0),
                from: point(0),
                to: point(1),
            },
            ControlWitness {
                edge: edge(1),
                from: point(1),
                to: point(2),
            },
            ControlWitness {
                edge: edge(2),
                from: point(1),
                to: point(3),
            },
            ControlWitness {
                edge: edge(3),
                from: point(2),
                to: point(1),
            },
            ControlWitness {
                edge: edge(4),
                from: point(3),
                to: point(4),
            },
        ];
        let (dependencies, gaps) = derive_control_dependencies(&facts, &witnesses);
        assert!(gaps.is_empty());
        assert_eq!(
            dependencies,
            BTreeMap::from([
                ((point(1), point(1)), BTreeSet::from([edge(1)])),
                ((point(1), point(2)), BTreeSet::from([edge(1)])),
            ])
        );
    }

    #[test]
    fn m4_6_multi_definition_flow_and_unresolved_access_are_exact() {
        let value = symbol(0);
        let left = definition(0);
        let right = definition(1);
        let definitions = BTreeMap::from([
            (
                left.clone(),
                FlowDefinitionFact {
                    point: point(0),
                    symbol: value.clone(),
                },
            ),
            (
                right.clone(),
                FlowDefinitionFact {
                    point: point(1),
                    symbol: value.clone(),
                },
            ),
        ]);
        let resolved = access(0);
        let unresolved = access(1);
        let accesses = vec![
            FlowAccessFact {
                key: resolved.clone(),
                point: point(2),
                symbol: Some(value.clone()),
                reaching_definitions: vec![left.clone(), right.clone()],
            },
            FlowAccessFact {
                key: unresolved.clone(),
                point: point(2),
                symbol: None,
                reaching_definitions: vec![],
            },
        ];
        let (dependencies, gaps) = derive_flow_dependencies(&definitions, &accesses).unwrap();
        assert_eq!(
            dependencies,
            vec![
                FlowDependency {
                    from: point(0),
                    to: point(2),
                    symbol: value.clone(),
                    definition: left,
                    access: resolved.clone(),
                },
                FlowDependency {
                    from: point(1),
                    to: point(2),
                    symbol: value,
                    definition: right,
                    access: resolved,
                },
            ]
        );
        assert_eq!(gaps, vec![unresolved]);
    }

    #[test]
    fn m4_6_unreachable_edges_emit_neither_dependence_nor_gap() {
        let facts = BTreeMap::from([
            (point(0), fact(false, false, &[], None)),
            (point(1), fact(false, false, &[], None)),
        ]);
        let witnesses = [ControlWitness {
            edge: edge(0),
            from: point(0),
            to: point(1),
        }];
        let (dependencies, gaps) = derive_control_dependencies(&facts, &witnesses);
        assert!(dependencies.is_empty());
        assert!(gaps.is_empty());
    }
}
