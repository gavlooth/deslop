use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, ControlFlowGraph,
    ControlFlowGraphKey, ControlPointKey, ControlRegionGraph, ControlRegionProjection,
    FactCoverage, NodeKey, ProjectionId, ReferenceRole, ResolutionEndpoint, ResolutionProjection,
    ResolutionResultKey, ResolutionStatus, ScopeFactData, ScopeFactKey, ScopeFactKind,
};

pub const DATA_FLOW_SCHEMA: &str = "deslop.data-flow/1";
pub const DATA_FLOW_POLICY_SCHEMA: &str = "deslop.data-flow-policy/1";

const POLICY_DOMAIN: &str = "deslop data-flow policy v1";
const GRAPH_DOMAIN: &str = "deslop data-flow graph v1";
const SYMBOL_DOMAIN: &str = "deslop data-flow symbol v1";
const DEFINITION_DOMAIN: &str = "deslop data-flow definition v1";
const ACCESS_DOMAIN: &str = "deslop data-flow access v1";
const BOUNDARY_DOMAIN: &str = "deslop data-flow boundary v1";
const EFFECT_DOMAIN: &str = "deslop data-flow effect v1";
const POINT_DOMAIN: &str = "deslop data-flow point v1";

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

digest_id!(DataFlowPolicyId, "dfp1_");
digest_id!(DataFlowGraphKey, "dfg1_");
digest_id!(DataFlowSymbolKey, "dfs1_");
digest_id!(DataFlowDefinitionKey, "dfd1_");
digest_id!(DataFlowAccessKey, "dfa1_");
digest_id!(DataFlowBoundaryKey, "dfb1_");
digest_id!(DataFlowEffectKey, "dfe1_");
digest_id!(DataFlowPointKey, "dfn1_");

impl DataFlowPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, DataFlowBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(DataFlowBuildError::Invalid(
                "data-flow policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_id(POLICY_DOMAIN, "dfp1_", parts)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataFlowAccessKind {
    Read,
    Write,
    ReadWrite,
    Call,
    Borrow,
    Capture,
}

impl DataFlowAccessKind {
    fn reads(self) -> bool {
        matches!(
            self,
            Self::Read | Self::ReadWrite | Self::Call | Self::Borrow | Self::Capture
        )
    }

    fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataFlowBoundaryKind {
    ParameterInput,
    ReturnOutput,
    MutationOutput,
    ExceptionalOutput,
    SuspensionOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataFlowEffectKind {
    ReadsMemory,
    WritesMemory,
    Allocates,
    Calls,
    Throws,
    Suspends,
    Returns,
    Terminates,
    Io,
    GlobalState,
    Captures,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowCoverageEvidence {
    status: FactCoverage,
    def_use_support: CapabilitySupport,
    def_use_authority: Option<CapabilityAuthority>,
    effects_support: CapabilitySupport,
    effects_authority: Option<CapabilityAuthority>,
    reasons: Vec<String>,
}

impl DataFlowCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }
    pub fn def_use_support(&self) -> CapabilitySupport {
        self.def_use_support
    }
    pub fn def_use_authority(&self) -> Option<CapabilityAuthority> {
        self.def_use_authority
    }
    pub fn effects_support(&self) -> CapabilitySupport {
        self.effects_support
    }
    pub fn effects_authority(&self) -> Option<CapabilityAuthority> {
        self.effects_authority
    }
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowSymbol {
    key: DataFlowSymbolKey,
    declaration: ScopeFactKey,
}

impl DataFlowSymbol {
    pub fn key(&self) -> &DataFlowSymbolKey {
        &self.key
    }
    pub fn declaration(&self) -> &ScopeFactKey {
        &self.declaration
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowDefinition {
    key: DataFlowDefinitionKey,
    point: ControlPointKey,
    symbol: DataFlowSymbolKey,
    source_fact: ScopeFactKey,
    ordinal: u32,
}

impl DataFlowDefinition {
    pub fn key(&self) -> &DataFlowDefinitionKey {
        &self.key
    }
    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }
    pub fn symbol(&self) -> &DataFlowSymbolKey {
        &self.symbol
    }
    pub fn source_fact(&self) -> &ScopeFactKey {
        &self.source_fact
    }
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowAccess {
    key: DataFlowAccessKey,
    point: ControlPointKey,
    reference: ScopeFactKey,
    resolution: ResolutionResultKey,
    kind: DataFlowAccessKind,
    ordinal: u32,
    symbol: Option<DataFlowSymbolKey>,
    reaching_definitions: Vec<DataFlowDefinitionKey>,
    uncertainty: Option<String>,
}

impl DataFlowAccess {
    pub fn key(&self) -> &DataFlowAccessKey {
        &self.key
    }
    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }
    pub fn reference(&self) -> &ScopeFactKey {
        &self.reference
    }
    pub fn resolution(&self) -> &ResolutionResultKey {
        &self.resolution
    }
    pub fn kind(&self) -> DataFlowAccessKind {
        self.kind
    }
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }
    pub fn symbol(&self) -> Option<&DataFlowSymbolKey> {
        self.symbol.as_ref()
    }
    pub fn reaching_definitions(&self) -> &[DataFlowDefinitionKey] {
        &self.reaching_definitions
    }
    pub fn uncertainty(&self) -> Option<&str> {
        self.uncertainty.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowBoundary {
    key: DataFlowBoundaryKey,
    point: ControlPointKey,
    kind: DataFlowBoundaryKind,
    symbol: Option<DataFlowSymbolKey>,
    source_fact: ScopeFactKey,
}

impl DataFlowBoundary {
    pub fn key(&self) -> &DataFlowBoundaryKey {
        &self.key
    }
    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }
    pub fn kind(&self) -> DataFlowBoundaryKind {
        self.kind
    }
    pub fn symbol(&self) -> Option<&DataFlowSymbolKey> {
        self.symbol.as_ref()
    }
    pub fn source_fact(&self) -> &ScopeFactKey {
        &self.source_fact
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowEffect {
    key: DataFlowEffectKey,
    point: ControlPointKey,
    effects: Vec<DataFlowEffectKind>,
    uncertainty: Option<String>,
}

impl DataFlowEffect {
    pub fn key(&self) -> &DataFlowEffectKey {
        &self.key
    }
    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }
    pub fn effects(&self) -> &[DataFlowEffectKind] {
        &self.effects
    }
    pub fn uncertainty(&self) -> Option<&str> {
        self.uncertainty.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowPointFacts {
    key: DataFlowPointKey,
    point: ControlPointKey,
    reachable: bool,
    reaching_in: Vec<DataFlowDefinitionKey>,
    reaching_out: Vec<DataFlowDefinitionKey>,
    live_in: Vec<DataFlowSymbolKey>,
    live_out: Vec<DataFlowSymbolKey>,
}

impl DataFlowPointFacts {
    pub fn key(&self) -> &DataFlowPointKey {
        &self.key
    }
    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }
    pub fn reachable(&self) -> bool {
        self.reachable
    }
    pub fn reaching_in(&self) -> &[DataFlowDefinitionKey] {
        &self.reaching_in
    }
    pub fn reaching_out(&self) -> &[DataFlowDefinitionKey] {
        &self.reaching_out
    }
    pub fn live_in(&self) -> &[DataFlowSymbolKey] {
        &self.live_in
    }
    pub fn live_out(&self) -> &[DataFlowSymbolKey] {
        &self.live_out
    }
}

#[derive(Debug, Clone)]
pub struct DataFlowDefinitionDraft {
    pub point: ControlPointKey,
    pub declaration: ScopeFactKey,
    pub source_fact: ScopeFactKey,
    pub ordinal: u32,
}

#[derive(Debug, Clone)]
pub struct DataFlowAccessDraft {
    pub point: ControlPointKey,
    pub reference: ScopeFactKey,
    pub kind: DataFlowAccessKind,
    pub ordinal: u32,
}

#[derive(Debug, Clone)]
pub struct DataFlowBoundaryDraft {
    pub point: ControlPointKey,
    pub kind: DataFlowBoundaryKind,
    pub declaration: Option<ScopeFactKey>,
    pub source_fact: ScopeFactKey,
}

#[derive(Debug, Clone)]
pub struct DataFlowEffectDraft {
    pub point: ControlPointKey,
    pub effects: Vec<DataFlowEffectKind>,
    pub uncertainty: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DataFlowGraphDraft {
    pub control_flow_graph: ControlFlowGraphKey,
    pub definitions: Vec<DataFlowDefinitionDraft>,
    pub accesses: Vec<DataFlowAccessDraft>,
    pub boundaries: Vec<DataFlowBoundaryDraft>,
    pub effects: Vec<DataFlowEffectDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowGraph {
    key: DataFlowGraphKey,
    control_flow_graph: ControlFlowGraphKey,
    control_region_graph: crate::ControlRegionGraphKey,
    owner: NodeKey,
    coverage: DataFlowCoverageEvidence,
    symbols: Vec<DataFlowSymbol>,
    definitions: Vec<DataFlowDefinition>,
    accesses: Vec<DataFlowAccess>,
    boundaries: Vec<DataFlowBoundary>,
    effects: Vec<DataFlowEffect>,
    points: Vec<DataFlowPointFacts>,
}

impl DataFlowGraph {
    pub fn key(&self) -> &DataFlowGraphKey {
        &self.key
    }
    pub fn control_flow_graph(&self) -> &ControlFlowGraphKey {
        &self.control_flow_graph
    }
    pub fn control_region_graph(&self) -> &crate::ControlRegionGraphKey {
        &self.control_region_graph
    }
    pub fn owner(&self) -> &NodeKey {
        &self.owner
    }
    pub fn coverage(&self) -> &DataFlowCoverageEvidence {
        &self.coverage
    }
    pub fn symbols(&self) -> &[DataFlowSymbol] {
        &self.symbols
    }
    pub fn definitions(&self) -> &[DataFlowDefinition] {
        &self.definitions
    }
    pub fn accesses(&self) -> &[DataFlowAccess] {
        &self.accesses
    }
    pub fn boundaries(&self) -> &[DataFlowBoundary] {
        &self.boundaries
    }
    pub fn effects(&self) -> &[DataFlowEffect] {
        &self.effects
    }
    pub fn points(&self) -> &[DataFlowPointFacts] {
        &self.points
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DataFlowDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_region_projection_id: ProjectionId,
    resolution_projection_id: ProjectionId,
    policy: DataFlowPolicyId,
    graphs: Vec<DataFlowGraph>,
}

impl DataFlowDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }
    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }
    pub fn control_region_projection_id(&self) -> &ProjectionId {
        &self.control_region_projection_id
    }
    pub fn resolution_projection_id(&self) -> &ProjectionId {
        &self.resolution_projection_id
    }
    pub fn policy(&self) -> &DataFlowPolicyId {
        &self.policy
    }
    pub fn graphs(&self) -> &[DataFlowGraph] {
        &self.graphs
    }

    fn validate(&self) -> Result<(), DataFlowBuildError> {
        if self.schema != DATA_FLOW_SCHEMA {
            return Err(DataFlowBuildError::Invalid(format!(
                "unsupported data-flow schema {}",
                self.schema
            )));
        }
        validate_digest(self.projection_id.as_str(), "pj1_")?;
        validate_digest(&self.analysis_id, "pa1_")?;
        validate_digest(self.control_region_projection_id.as_str(), "pj1_")?;
        validate_digest(self.resolution_projection_id.as_str(), "pj1_")?;
        if self.graphs.is_empty() {
            return Err(DataFlowBuildError::Invalid(
                "data-flow document cannot be empty".into(),
            ));
        }
        validate_sorted_by("data-flow graphs", &self.graphs, |graph| graph.key.as_str())?;
        let mut sources = BTreeSet::new();
        for graph in &self.graphs {
            if !sources.insert(graph.control_flow_graph.clone()) {
                return Err(DataFlowBuildError::Invalid(
                    "data-flow document repeats a source CFG".into(),
                ));
            }
            validate_graph(&self.policy, graph)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataFlowDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_region_projection_id: ProjectionId,
    resolution_projection_id: ProjectionId,
    policy: DataFlowPolicyId,
    graphs: Vec<DataFlowGraph>,
}

impl<'de> Deserialize<'de> for DataFlowDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = DataFlowDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            control_region_projection_id: wire.control_region_projection_id,
            resolution_projection_id: wire.resolution_projection_id,
            policy: wire.policy,
            graphs: wire.graphs,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct DataFlowProjection {
    id: ProjectionId,
    control_regions: Arc<ControlRegionProjection>,
    resolution: Arc<ResolutionProjection>,
    policy: DataFlowPolicyId,
    document: DataFlowDocument,
}

impl DataFlowProjection {
    pub fn id(&self) -> &ProjectionId {
        &self.id
    }
    pub fn control_regions(&self) -> &Arc<ControlRegionProjection> {
        &self.control_regions
    }
    pub fn resolution(&self) -> &Arc<ResolutionProjection> {
        &self.resolution
    }
    pub fn policy(&self) -> &DataFlowPolicyId {
        &self.policy
    }
    pub fn document(&self) -> &DataFlowDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataFlowBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for DataFlowBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid data-flow evidence: {detail}"),
            Self::Identity(detail) => write!(formatter, "data-flow identity error: {detail}"),
        }
    }
}

impl std::error::Error for DataFlowBuildError {}

#[derive(Debug)]
pub struct DataFlowBuilder {
    control_regions: Arc<ControlRegionProjection>,
    resolution: Arc<ResolutionProjection>,
    policy: DataFlowPolicyId,
    graphs: Vec<DataFlowGraph>,
}

impl DataFlowBuilder {
    pub fn new(
        control_regions: Arc<ControlRegionProjection>,
        resolution: Arc<ResolutionProjection>,
        policy: DataFlowPolicyId,
    ) -> Result<Self, DataFlowBuildError> {
        if control_regions.control_flow().analysis().id()
            != resolution.scope_graph().analysis().id()
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow sources belong to different analyses".into(),
            ));
        }
        Ok(Self {
            control_regions,
            resolution,
            policy,
            graphs: Vec::new(),
        })
    }

    pub fn add_graph(
        &mut self,
        draft: DataFlowGraphDraft,
    ) -> Result<DataFlowGraphKey, DataFlowBuildError> {
        let flow = self
            .control_regions
            .control_flow()
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == &draft.control_flow_graph)
            .ok_or_else(|| {
                DataFlowBuildError::Invalid("data-flow draft references a missing CFG".into())
            })?;
        let regions = self
            .control_regions
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.control_flow_graph() == flow.key())
            .ok_or_else(|| {
                DataFlowBuildError::Invalid(
                    "data-flow draft references a missing region graph".into(),
                )
            })?;
        if self
            .graphs
            .iter()
            .any(|graph| graph.control_flow_graph == draft.control_flow_graph)
        {
            return Err(DataFlowBuildError::Invalid(
                "duplicate data-flow graph".into(),
            ));
        }
        let graph = derive_graph(flow, regions, &self.resolution, &self.policy, draft)?;
        let key = graph.key.clone();
        self.graphs.push(graph);
        Ok(key)
    }

    pub fn build(mut self) -> Result<DataFlowProjection, DataFlowBuildError> {
        if self.graphs.len() != self.control_regions.document().graphs().len() {
            return Err(DataFlowBuildError::Invalid(
                "data-flow projection requires exactly one graph per source CFG".into(),
            ));
        }
        self.graphs.sort_by(|left, right| left.key.cmp(&right.key));
        let payload = serde_json::to_vec(&(
            self.control_regions.id(),
            self.resolution.id(),
            &self.policy,
            &self.graphs,
        ))
        .map_err(|error| DataFlowBuildError::Identity(error.to_string()))?;
        let analysis = self.control_regions.control_flow().analysis();
        let id = analysis
            .derive_projection_id(
                DATA_FLOW_SCHEMA,
                &payload,
                self.resolution.id().as_str().as_bytes(),
            )
            .map_err(|error| DataFlowBuildError::Identity(error.to_string()))?;
        let document = DataFlowDocument {
            schema: DATA_FLOW_SCHEMA.into(),
            projection_id: id.clone(),
            analysis_id: analysis.id().as_str().into(),
            control_region_projection_id: self.control_regions.id().clone(),
            resolution_projection_id: self.resolution.id().clone(),
            policy: self.policy.clone(),
            graphs: self.graphs,
        };
        document.validate()?;
        Ok(DataFlowProjection {
            id,
            control_regions: self.control_regions,
            resolution: self.resolution,
            policy: self.policy,
            document,
        })
    }
}

#[derive(Debug)]
struct IndexedFlow {
    keys: Vec<ControlPointKey>,
    index: BTreeMap<ControlPointKey, usize>,
    successors: Vec<BTreeSet<usize>>,
    predecessors: Vec<BTreeSet<usize>>,
    reachable: BTreeSet<usize>,
}

impl IndexedFlow {
    fn new(
        flow: &ControlFlowGraph,
        regions: &ControlRegionGraph,
    ) -> Result<Self, DataFlowBuildError> {
        let keys = flow
            .points()
            .iter()
            .map(|point| point.key().clone())
            .collect::<Vec<_>>();
        let index = keys
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, key)| (key, i))
            .collect::<BTreeMap<_, _>>();
        let mut successors = vec![BTreeSet::new(); keys.len()];
        let mut predecessors = vec![BTreeSet::new(); keys.len()];
        for edge in flow.edges() {
            let from = index[edge.from()];
            let to = index[edge.to()];
            successors[from].insert(to);
            predecessors[to].insert(from);
        }
        let reachable = regions
            .points()
            .iter()
            .filter(|fact| fact.reachable())
            .map(|fact| index[fact.point()])
            .collect();
        Ok(Self {
            keys,
            index,
            successors,
            predecessors,
            reachable,
        })
    }
}

fn derive_graph(
    flow: &ControlFlowGraph,
    regions: &ControlRegionGraph,
    resolution: &ResolutionProjection,
    policy: &DataFlowPolicyId,
    draft: DataFlowGraphDraft,
) -> Result<DataFlowGraph, DataFlowBuildError> {
    let indexed = IndexedFlow::new(flow, regions)?;
    let analysis = resolution.scope_graph().analysis();
    let owner = analysis.node_by_key(flow.owner()).map_err(|_| {
        DataFlowBuildError::Invalid("data-flow CFG owner is not retained by the analysis".into())
    })?;
    let scope_facts = resolution
        .scope_graph()
        .facts()
        .iter()
        .map(|fact| (fact.key().clone(), fact))
        .collect::<BTreeMap<_, _>>();
    let results = resolution
        .results()
        .iter()
        .map(|record| (record.wire().reference().clone(), record.wire()))
        .collect::<BTreeMap<_, _>>();
    let mut declarations = BTreeSet::new();
    for definition in &draft.definitions {
        declarations.insert(definition.declaration.clone());
    }
    for boundary in &draft.boundaries {
        if let Some(declaration) = &boundary.declaration {
            declarations.insert(declaration.clone());
        }
    }
    let mut resolved_accesses = Vec::new();
    let mut reasons = Vec::new();
    for access in &draft.accesses {
        require_point(&indexed, &access.point)?;
        let reference = scope_facts.get(&access.reference).ok_or_else(|| {
            DataFlowBuildError::Invalid("access references a missing scope fact".into())
        })?;
        require_owner_containment(analysis, owner.id(), reference)?;
        if reference.data().kind() != ScopeFactKind::Reference {
            return Err(DataFlowBuildError::Invalid(
                "access source is not a Reference fact".into(),
            ));
        }
        validate_access_role(access.kind, reference.data())?;
        let result = results
            .get(&access.reference)
            .ok_or_else(|| DataFlowBuildError::Invalid("access has no resolution result".into()))?;
        let resolved = resolved_declaration(result, &scope_facts);
        if let Some(declaration) = &resolved {
            declarations.insert(declaration.clone());
        }
        let uncertainty = resolved
            .is_none()
            .then(|| "access does not have one Complete Unique resolved declaration".to_string());
        if let Some(reason) = &uncertainty {
            reasons.push(reason.clone());
        }
        resolved_accesses.push((access, result.key().clone(), resolved, uncertainty));
    }
    let mut symbols = declarations
        .into_iter()
        .map(|declaration| {
            require_declaration(&scope_facts, &declaration)?;
            let key = DataFlowSymbolKey(derive_id(
                SYMBOL_DOMAIN,
                "dfs1_",
                &[
                    policy.as_str().as_bytes(),
                    flow.key().as_str().as_bytes(),
                    declaration.as_str().as_bytes(),
                ],
            ));
            Ok(DataFlowSymbol { key, declaration })
        })
        .collect::<Result<Vec<_>, DataFlowBuildError>>()?;
    symbols.sort_by(|left, right| left.key.cmp(&right.key));
    let symbol_by_declaration = symbols
        .iter()
        .map(|symbol| (symbol.declaration.clone(), symbol.key.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut definitions = draft
        .definitions
        .into_iter()
        .map(|definition| {
            require_point(&indexed, &definition.point)?;
            require_owner_containment(
                analysis,
                owner.id(),
                scope_facts.get(&definition.source_fact).ok_or_else(|| {
                    DataFlowBuildError::Invalid("definition source fact is missing".into())
                })?,
            )?;
            validate_definition_source(
                &scope_facts,
                &results,
                &definition.source_fact,
                &definition.declaration,
            )?;
            let symbol = symbol_by_declaration[&definition.declaration].clone();
            let payload = serde_json::to_vec(&(
                &definition.point,
                &symbol,
                &definition.source_fact,
                definition.ordinal,
            ))
            .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
            Ok(DataFlowDefinition {
                key: DataFlowDefinitionKey(derive_id(
                    DEFINITION_DOMAIN,
                    "dfd1_",
                    &[policy.as_str().as_bytes(), &payload],
                )),
                point: definition.point,
                symbol,
                source_fact: definition.source_fact,
                ordinal: definition.ordinal,
            })
        })
        .collect::<Result<Vec<_>, DataFlowBuildError>>()?;
    definitions.sort_by(|left, right| left.key.cmp(&right.key));
    validate_event_ordinals(&definitions, &resolved_accesses, &symbol_by_declaration)?;

    let definition_symbols = definitions
        .iter()
        .map(|definition| (definition.key.clone(), definition.symbol.clone()))
        .collect::<BTreeMap<_, _>>();
    let point_definitions = ordered_point_definitions(&indexed, &definitions);
    let (reaching_in, reaching_out) =
        reaching_definitions(&indexed, &point_definitions, &definition_symbols);
    let point_uses = point_uses(
        &indexed,
        &definitions,
        &resolved_accesses,
        &symbol_by_declaration,
    );
    let point_defs = point_definitions
        .iter()
        .map(|definitions| {
            definitions
                .iter()
                .map(|(_, key)| definition_symbols[key].clone())
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let (live_in, live_out) = liveness(&indexed, &point_uses, &point_defs);

    let mut accesses = Vec::new();
    for (draft, resolution_key, declaration, uncertainty) in resolved_accesses {
        let symbol = declaration
            .as_ref()
            .map(|declaration| symbol_by_declaration[declaration].clone());
        let point = indexed.index[&draft.point];
        let reaching = symbol
            .as_ref()
            .map(|symbol| {
                reaching_at_access(
                    point,
                    draft.ordinal,
                    symbol,
                    &reaching_in,
                    &point_definitions,
                    &definition_symbols,
                )
            })
            .unwrap_or_default();
        let payload = serde_json::to_vec(&(
            &draft.point,
            &draft.reference,
            &resolution_key,
            draft.kind,
            draft.ordinal,
            &symbol,
            &reaching,
            &uncertainty,
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        accesses.push(DataFlowAccess {
            key: DataFlowAccessKey(derive_id(
                ACCESS_DOMAIN,
                "dfa1_",
                &[policy.as_str().as_bytes(), &payload],
            )),
            point: draft.point.clone(),
            reference: draft.reference.clone(),
            resolution: resolution_key,
            kind: draft.kind,
            ordinal: draft.ordinal,
            symbol,
            reaching_definitions: reaching,
            uncertainty,
        });
    }
    accesses.sort_by(|left, right| left.key.cmp(&right.key));

    let mut boundaries = Vec::new();
    for boundary in draft.boundaries {
        require_point(&indexed, &boundary.point)?;
        if !scope_facts.contains_key(&boundary.source_fact) {
            return Err(DataFlowBuildError::Invalid(
                "boundary source fact is missing".into(),
            ));
        }
        let source_fact = scope_facts[&boundary.source_fact];
        require_owner_containment(analysis, owner.id(), source_fact)?;
        if let Some(declaration) = &boundary.declaration {
            require_declaration(&scope_facts, declaration)?;
            validate_definition_source(&scope_facts, &results, &boundary.source_fact, declaration)?;
        }
        let symbol = boundary
            .declaration
            .as_ref()
            .map(|declaration| symbol_by_declaration[declaration].clone());
        if boundary.kind == DataFlowBoundaryKind::ParameterInput {
            let parameter_symbol = symbol.as_ref().ok_or_else(|| {
                DataFlowBuildError::Invalid("parameter input boundary requires a symbol".into())
            })?;
            if boundary.point != *flow.entry() {
                return Err(DataFlowBuildError::Invalid(
                    "parameter input boundary must occur at the CFG entry".into(),
                ));
            }
            if !matches!(
                source_fact.data(),
                ScopeFactData::Binding {
                    form: crate::BindingForm::Parameter,
                    ..
                }
            ) {
                return Err(DataFlowBuildError::Invalid(
                    "parameter input boundary requires a Parameter binding source".into(),
                ));
            }
            if !definitions.iter().any(|definition| {
                definition.point == boundary.point
                    && &definition.symbol == parameter_symbol
                    && definition.source_fact == boundary.source_fact
            }) {
                return Err(DataFlowBuildError::Invalid(
                    "parameter input boundary requires its source-bound definition at CFG entry"
                        .into(),
                ));
            }
        }
        let payload = serde_json::to_vec(&(
            &boundary.point,
            boundary.kind,
            &symbol,
            &boundary.source_fact,
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        boundaries.push(DataFlowBoundary {
            key: DataFlowBoundaryKey(derive_id(
                BOUNDARY_DOMAIN,
                "dfb1_",
                &[policy.as_str().as_bytes(), &payload],
            )),
            point: boundary.point,
            kind: boundary.kind,
            symbol,
            source_fact: boundary.source_fact,
        });
    }
    boundaries.sort_by(|left, right| left.key.cmp(&right.key));

    let mut effects = Vec::new();
    for mut effect in draft.effects {
        require_point(&indexed, &effect.point)?;
        effect.effects.sort();
        effect.effects.dedup();
        if let Some(reason) = &effect.uncertainty {
            validate_text(reason)?;
            reasons.push(reason.clone());
        }
        let payload = serde_json::to_vec(&(&effect.point, &effect.effects, &effect.uncertainty))
            .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        effects.push(DataFlowEffect {
            key: DataFlowEffectKey(derive_id(
                EFFECT_DOMAIN,
                "dfe1_",
                &[policy.as_str().as_bytes(), &payload],
            )),
            point: effect.point,
            effects: effect.effects,
            uncertainty: effect.uncertainty,
        });
    }
    effects.sort_by(|left, right| left.key.cmp(&right.key));

    let mut points = Vec::new();
    for point in 0..indexed.keys.len() {
        let payload = serde_json::to_vec(&(
            &indexed.keys[point],
            indexed.reachable.contains(&point),
            &reaching_in[point],
            &reaching_out[point],
            &live_in[point],
            &live_out[point],
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        points.push(DataFlowPointFacts {
            key: DataFlowPointKey(derive_id(
                POINT_DOMAIN,
                "dfn1_",
                &[policy.as_str().as_bytes(), &payload],
            )),
            point: indexed.keys[point].clone(),
            reachable: indexed.reachable.contains(&point),
            reaching_in: reaching_in[point].iter().cloned().collect(),
            reaching_out: reaching_out[point].iter().cloned().collect(),
            live_in: live_in[point].iter().cloned().collect(),
            live_out: live_out[point].iter().cloned().collect(),
        });
    }
    points.sort_by(|left, right| left.point.cmp(&right.point));
    let used_source_facts = definitions
        .iter()
        .map(|definition| definition.source_fact())
        .chain(accesses.iter().map(|access| access.reference()))
        .chain(boundaries.iter().map(|boundary| boundary.source_fact()))
        .collect::<BTreeSet<_>>();
    for source in used_source_facts {
        if scope_facts[source].evidence().coverage.status != FactCoverage::Complete {
            reasons.push(format!("scope fact {} is not Complete", source.as_str()));
        }
    }
    reasons.extend(regions.coverage().reasons().iter().cloned());
    reasons.sort();
    reasons.dedup();
    let def_use = flow
        .adapter()
        .capabilities()
        .declaration(AdapterCapability::DefUse);
    let effect_capability = flow
        .adapter()
        .capabilities()
        .declaration(AdapterCapability::Effects);
    if effect_capability.support() == CapabilitySupport::Provided
        && indexed.reachable.iter().any(|point| {
            !effects
                .iter()
                .any(|effect| indexed.index[effect.point()] == *point)
        })
    {
        reasons.push("Provided Effects evidence omits a reachable control point".into());
    }
    if def_use.support() != CapabilitySupport::Provided {
        reasons.push(format!(
            "adapter DefUse capability is {}",
            def_use.support().as_str()
        ));
    }
    if effect_capability.support() != CapabilitySupport::Provided {
        reasons.push(format!(
            "adapter Effects capability is {}",
            effect_capability.support().as_str()
        ));
    }
    reasons.sort();
    reasons.dedup();
    let status = if reasons.is_empty() {
        FactCoverage::Complete
    } else if def_use.support() == CapabilitySupport::Unsupported
        && effect_capability.support() == CapabilitySupport::Unsupported
    {
        FactCoverage::Unsupported
    } else {
        FactCoverage::Partial
    };
    let coverage = DataFlowCoverageEvidence {
        status,
        def_use_support: def_use.support(),
        def_use_authority: def_use.authority(),
        effects_support: effect_capability.support(),
        effects_authority: effect_capability.authority(),
        reasons,
    };
    let mut graph = DataFlowGraph {
        key: DataFlowGraphKey(String::new()),
        control_flow_graph: flow.key().clone(),
        control_region_graph: regions.key().clone(),
        owner: flow.owner().clone(),
        coverage,
        symbols,
        definitions,
        accesses,
        boundaries,
        effects,
        points,
    };
    let payload = serde_json::to_vec(&(
        &graph.control_flow_graph,
        &graph.control_region_graph,
        &graph.owner,
        &graph.coverage,
        &graph.symbols,
        &graph.definitions,
        &graph.accesses,
        &graph.boundaries,
        &graph.effects,
        &graph.points,
    ))
    .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
    graph.key = DataFlowGraphKey(derive_id(
        GRAPH_DOMAIN,
        "dfg1_",
        &[policy.as_str().as_bytes(), &payload],
    ));
    validate_graph(policy, &graph)?;
    Ok(graph)
}

fn validate_graph(
    policy: &DataFlowPolicyId,
    graph: &DataFlowGraph,
) -> Result<(), DataFlowBuildError> {
    validate_coverage(&graph.coverage)?;
    validate_sorted_by("data-flow symbols", &graph.symbols, |symbol| {
        symbol.key.as_str()
    })?;
    validate_sorted_by("data-flow definitions", &graph.definitions, |definition| {
        definition.key.as_str()
    })?;
    validate_sorted_by("data-flow accesses", &graph.accesses, |access| {
        access.key.as_str()
    })?;
    validate_sorted_by("data-flow boundaries", &graph.boundaries, |boundary| {
        boundary.key.as_str()
    })?;
    validate_sorted_by("data-flow effects", &graph.effects, |effect| {
        effect.key.as_str()
    })?;
    validate_sorted_by("data-flow points", &graph.points, |point| {
        point.point.as_str()
    })?;

    let symbols = graph
        .symbols
        .iter()
        .map(|symbol| (&symbol.key, symbol))
        .collect::<BTreeMap<_, _>>();
    if graph
        .symbols
        .iter()
        .map(|symbol| &symbol.declaration)
        .collect::<BTreeSet<_>>()
        .len()
        != graph.symbols.len()
    {
        return Err(DataFlowBuildError::Invalid(
            "data-flow graph repeats a declaration symbol".into(),
        ));
    }
    let points = graph
        .points
        .iter()
        .map(|point| (&point.point, point))
        .collect::<BTreeMap<_, _>>();
    let definitions = graph
        .definitions
        .iter()
        .map(|definition| (&definition.key, definition))
        .collect::<BTreeMap<_, _>>();

    for symbol in &graph.symbols {
        let expected = DataFlowSymbolKey(derive_id(
            SYMBOL_DOMAIN,
            "dfs1_",
            &[
                policy.as_str().as_bytes(),
                graph.control_flow_graph.as_str().as_bytes(),
                symbol.declaration.as_str().as_bytes(),
            ],
        ));
        if symbol.key != expected {
            return Err(DataFlowBuildError::Invalid(
                "data-flow symbol key does not bind its payload".into(),
            ));
        }
    }
    for definition in &graph.definitions {
        if !points.contains_key(&definition.point) || !symbols.contains_key(&definition.symbol) {
            return Err(DataFlowBuildError::Invalid(
                "data-flow definition has dangling point or symbol".into(),
            ));
        }
        let payload = serde_json::to_vec(&(
            &definition.point,
            &definition.symbol,
            &definition.source_fact,
            definition.ordinal,
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        if definition.key
            != DataFlowDefinitionKey(derive_id(
                DEFINITION_DOMAIN,
                "dfd1_",
                &[policy.as_str().as_bytes(), &payload],
            ))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow definition key does not bind its payload".into(),
            ));
        }
    }
    for access in &graph.accesses {
        if !points.contains_key(&access.point) {
            return Err(DataFlowBuildError::Invalid(
                "data-flow access has a dangling point".into(),
            ));
        }
        match (&access.symbol, &access.uncertainty) {
            (Some(symbol), None) if symbols.contains_key(symbol) => {}
            (None, Some(reason)) => validate_text(reason)?,
            _ => {
                return Err(DataFlowBuildError::Invalid(
                    "data-flow access resolution evidence is inconsistent".into(),
                ));
            }
        }
        validate_canonical("access reaching definitions", &access.reaching_definitions)?;
        for key in &access.reaching_definitions {
            let definition = definitions.get(key).ok_or_else(|| {
                DataFlowBuildError::Invalid(
                    "access references a missing reaching definition".into(),
                )
            })?;
            if Some(&definition.symbol) != access.symbol.as_ref() {
                return Err(DataFlowBuildError::Invalid(
                    "access reaches a definition of another symbol".into(),
                ));
            }
        }
        let payload = serde_json::to_vec(&(
            &access.point,
            &access.reference,
            &access.resolution,
            access.kind,
            access.ordinal,
            &access.symbol,
            &access.reaching_definitions,
            &access.uncertainty,
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        if access.key
            != DataFlowAccessKey(derive_id(
                ACCESS_DOMAIN,
                "dfa1_",
                &[policy.as_str().as_bytes(), &payload],
            ))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow access key does not bind its payload".into(),
            ));
        }
    }
    for boundary in &graph.boundaries {
        if !points.contains_key(&boundary.point)
            || boundary
                .symbol
                .as_ref()
                .is_some_and(|symbol| !symbols.contains_key(symbol))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow boundary has a dangling point or symbol".into(),
            ));
        }
        if matches!(
            boundary.kind,
            DataFlowBoundaryKind::ParameterInput | DataFlowBoundaryKind::MutationOutput
        ) && boundary.symbol.is_none()
        {
            return Err(DataFlowBuildError::Invalid(
                "parameter/mutation boundary requires a symbol".into(),
            ));
        }
        let payload = serde_json::to_vec(&(
            &boundary.point,
            boundary.kind,
            &boundary.symbol,
            &boundary.source_fact,
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        if boundary.key
            != DataFlowBoundaryKey(derive_id(
                BOUNDARY_DOMAIN,
                "dfb1_",
                &[policy.as_str().as_bytes(), &payload],
            ))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow boundary key does not bind its payload".into(),
            ));
        }
    }
    let mut effect_points = BTreeSet::new();
    for effect in &graph.effects {
        if !points.contains_key(&effect.point) || !effect_points.insert(&effect.point) {
            return Err(DataFlowBuildError::Invalid(
                "data-flow effects have a dangling or duplicate point".into(),
            ));
        }
        validate_canonical("effect kinds", &effect.effects)?;
        if let Some(reason) = &effect.uncertainty {
            validate_text(reason)?;
        }
        let payload = serde_json::to_vec(&(&effect.point, &effect.effects, &effect.uncertainty))
            .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        if effect.key
            != DataFlowEffectKey(derive_id(
                EFFECT_DOMAIN,
                "dfe1_",
                &[policy.as_str().as_bytes(), &payload],
            ))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow effect key does not bind its payload".into(),
            ));
        }
    }
    for point in &graph.points {
        validate_canonical("reaching-in definitions", &point.reaching_in)?;
        validate_canonical("reaching-out definitions", &point.reaching_out)?;
        validate_canonical("live-in symbols", &point.live_in)?;
        validate_canonical("live-out symbols", &point.live_out)?;
        if point
            .reaching_in
            .iter()
            .chain(point.reaching_out.iter())
            .any(|key| !definitions.contains_key(key))
            || point
                .live_in
                .iter()
                .chain(point.live_out.iter())
                .any(|key| !symbols.contains_key(key))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow point relations contain dangling facts".into(),
            ));
        }
        if !point.reachable
            && (!point.reaching_in.is_empty()
                || !point.reaching_out.is_empty()
                || !point.live_in.is_empty()
                || !point.live_out.is_empty())
        {
            return Err(DataFlowBuildError::Invalid(
                "unreachable point carries execution relations".into(),
            ));
        }
        let payload = serde_json::to_vec(&(
            &point.point,
            point.reachable,
            &point.reaching_in,
            &point.reaching_out,
            &point.live_in,
            &point.live_out,
        ))
        .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
        if point.key
            != DataFlowPointKey(derive_id(
                POINT_DOMAIN,
                "dfn1_",
                &[policy.as_str().as_bytes(), &payload],
            ))
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow point key does not bind its payload".into(),
            ));
        }
    }
    let payload = serde_json::to_vec(&(
        &graph.control_flow_graph,
        &graph.control_region_graph,
        &graph.owner,
        &graph.coverage,
        &graph.symbols,
        &graph.definitions,
        &graph.accesses,
        &graph.boundaries,
        &graph.effects,
        &graph.points,
    ))
    .map_err(|e| DataFlowBuildError::Identity(e.to_string()))?;
    if graph.key
        != DataFlowGraphKey(derive_id(
            GRAPH_DOMAIN,
            "dfg1_",
            &[policy.as_str().as_bytes(), &payload],
        ))
    {
        return Err(DataFlowBuildError::Invalid(
            "data-flow graph key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn validate_coverage(coverage: &DataFlowCoverageEvidence) -> Result<(), DataFlowBuildError> {
    validate_canonical("data-flow coverage reasons", &coverage.reasons)?;
    for (label, support, authority) in [
        (
            "DefUse",
            coverage.def_use_support,
            coverage.def_use_authority,
        ),
        (
            "Effects",
            coverage.effects_support,
            coverage.effects_authority,
        ),
    ] {
        if (support == CapabilitySupport::Provided) != authority.is_some() {
            return Err(DataFlowBuildError::Invalid(format!(
                "{label} support and authority disagree"
            )));
        }
    }
    match (coverage.status, coverage.reasons.is_empty()) {
        (FactCoverage::Complete, true)
            if coverage.def_use_support == CapabilitySupport::Provided
                && coverage.effects_support == CapabilitySupport::Provided =>
        {
            Ok(())
        }
        (FactCoverage::Complete, _) => Err(DataFlowBuildError::Invalid(
            "Complete data-flow coverage requires provided capabilities and no reasons".into(),
        )),
        (_, false) => Ok(()),
        (_, true) => Err(DataFlowBuildError::Invalid(
            "incomplete data-flow coverage requires an exact reason".into(),
        )),
    }
}

fn validate_sorted_by<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), DataFlowBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        Err(DataFlowBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )))
    } else {
        Ok(())
    }
}

fn validate_canonical<T: Ord>(label: &str, values: &[T]) -> Result<(), DataFlowBuildError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(DataFlowBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )))
    } else {
        Ok(())
    }
}

fn require_point(indexed: &IndexedFlow, point: &ControlPointKey) -> Result<(), DataFlowBuildError> {
    indexed
        .index
        .contains_key(point)
        .then_some(())
        .ok_or_else(|| {
            DataFlowBuildError::Invalid("data-flow fact references another CFG point".into())
        })
}

fn require_owner_containment(
    analysis: &crate::ProjectAnalysis,
    owner: crate::NodeId,
    fact: &crate::ScopeFactRecord,
) -> Result<(), DataFlowBuildError> {
    match analysis.node_contains(owner, fact.node()) {
        Ok(true) => Ok(()),
        Ok(false) => Err(DataFlowBuildError::Invalid(
            "data-flow source fact is outside the CFG owner".into(),
        )),
        Err(_) => Err(DataFlowBuildError::Invalid(
            "data-flow source fact is not retained by the CFG analysis".into(),
        )),
    }
}

fn require_declaration(
    facts: &BTreeMap<ScopeFactKey, &crate::ScopeFactRecord>,
    key: &ScopeFactKey,
) -> Result<(), DataFlowBuildError> {
    facts
        .get(key)
        .filter(|fact| fact.data().kind() == ScopeFactKind::Declaration)
        .map(|_| ())
        .ok_or_else(|| {
            DataFlowBuildError::Invalid(
                "data-flow symbol is not a retained Declaration fact".into(),
            )
        })
}

fn normalized_declaration(
    facts: &BTreeMap<ScopeFactKey, &crate::ScopeFactRecord>,
    key: &ScopeFactKey,
) -> Option<ScopeFactKey> {
    let fact = facts.get(key)?;
    match fact.data() {
        ScopeFactData::Declaration { .. } => Some(key.clone()),
        ScopeFactData::Definition { declaration, .. } => Some(declaration.clone()),
        ScopeFactData::Binding { target, .. } => match target {
            crate::BindingTarget::Declaration(key) => Some(key.clone()),
            crate::BindingTarget::Definition(key) => normalized_declaration(facts, key),
        },
        _ => None,
    }
}

fn resolved_declaration(
    result: &crate::ResolutionResult,
    facts: &BTreeMap<ScopeFactKey, &crate::ScopeFactRecord>,
) -> Option<ScopeFactKey> {
    if result.status() != ResolutionStatus::Unique
        || result.coverage().status() != FactCoverage::Complete
    {
        return None;
    }
    let preferred = result.preferred()?;
    if preferred.status() != ResolutionStatus::Unique || preferred.endpoints().len() != 1 {
        return None;
    }
    match &preferred.endpoints()[0] {
        ResolutionEndpoint::Declaration(key) | ResolutionEndpoint::Definition(key) => {
            normalized_declaration(facts, key)
        }
        _ => None,
    }
}

fn validate_definition_source(
    facts: &BTreeMap<ScopeFactKey, &crate::ScopeFactRecord>,
    results: &BTreeMap<ScopeFactKey, &crate::ResolutionResult>,
    source: &ScopeFactKey,
    declaration: &ScopeFactKey,
) -> Result<(), DataFlowBuildError> {
    let normalized = normalized_declaration(facts, source).or_else(|| {
        results
            .get(source)
            .and_then(|result| resolved_declaration(result, facts))
    });
    if normalized.as_ref() == Some(declaration) {
        Ok(())
    } else {
        Err(DataFlowBuildError::Invalid(
            "definition source does not normalize to its declared symbol".into(),
        ))
    }
}

fn validate_access_role(
    kind: DataFlowAccessKind,
    data: &ScopeFactData,
) -> Result<(), DataFlowBuildError> {
    let ScopeFactData::Reference { role, .. } = data else {
        return Err(DataFlowBuildError::Invalid(
            "access source is not a Reference fact".into(),
        ));
    };
    let valid = match kind {
        DataFlowAccessKind::Read => *role == ReferenceRole::Read,
        DataFlowAccessKind::Write | DataFlowAccessKind::ReadWrite => *role == ReferenceRole::Write,
        DataFlowAccessKind::Call => *role == ReferenceRole::Call,
        DataFlowAccessKind::Borrow | DataFlowAccessKind::Capture => {
            matches!(role, ReferenceRole::Read | ReferenceRole::Write)
        }
    };
    valid.then_some(()).ok_or_else(|| {
        DataFlowBuildError::Invalid(
            "data-flow access kind disagrees with its Reference role".into(),
        )
    })
}

fn validate_event_ordinals(
    definitions: &[DataFlowDefinition],
    accesses: &[(
        &DataFlowAccessDraft,
        ResolutionResultKey,
        Option<ScopeFactKey>,
        Option<String>,
    )],
    symbols: &BTreeMap<ScopeFactKey, DataFlowSymbolKey>,
) -> Result<(), DataFlowBuildError> {
    let mut definition_events = BTreeMap::new();
    for definition in definitions {
        if definition_events
            .insert((definition.point.clone(), definition.ordinal), definition)
            .is_some()
        {
            return Err(DataFlowBuildError::Invalid(
                "data-flow point repeats a definition ordinal".into(),
            ));
        }
    }
    let mut access_events = BTreeSet::new();
    for (access, _, declaration, _) in accesses {
        if !access_events.insert((access.point.clone(), access.ordinal)) {
            return Err(DataFlowBuildError::Invalid(
                "data-flow point repeats an access ordinal".into(),
            ));
        }
        let definition = definition_events.get(&(access.point.clone(), access.ordinal));
        match (access.kind.writes(), declaration, definition) {
            (true, Some(declaration), Some(definition))
                if definition.source_fact == access.reference
                    && definition.symbol == symbols[declaration] => {}
            (true, Some(_), _) => return Err(DataFlowBuildError::Invalid(
                "resolved write access requires a same-event definition from the same Reference fact".into(),
            )),
            (false, _, Some(_)) => return Err(DataFlowBuildError::Invalid(
                "read-only access shares an event ordinal with a definition".into(),
            )),
            _ => {}
        }
    }
    Ok(())
}

fn ordered_point_definitions(
    indexed: &IndexedFlow,
    definitions: &[DataFlowDefinition],
) -> Vec<Vec<(u32, DataFlowDefinitionKey)>> {
    let mut values = vec![Vec::new(); indexed.keys.len()];
    for definition in definitions {
        values[indexed.index[definition.point()]]
            .push((definition.ordinal, definition.key.clone()));
    }
    for definitions in &mut values {
        definitions.sort();
    }
    values
}

fn point_uses(
    indexed: &IndexedFlow,
    definitions: &[DataFlowDefinition],
    accesses: &[(
        &DataFlowAccessDraft,
        ResolutionResultKey,
        Option<ScopeFactKey>,
        Option<String>,
    )],
    symbols: &BTreeMap<ScopeFactKey, DataFlowSymbolKey>,
) -> Vec<BTreeSet<DataFlowSymbolKey>> {
    let first_definition = definitions
        .iter()
        .fold(BTreeMap::new(), |mut map, definition| {
            map.entry((definition.point.clone(), definition.symbol.clone()))
                .and_modify(|ordinal: &mut u32| *ordinal = (*ordinal).min(definition.ordinal))
                .or_insert(definition.ordinal);
            map
        });
    let mut uses = vec![BTreeSet::new(); indexed.keys.len()];
    for (access, _, declaration, _) in accesses {
        if access.kind.reads()
            && let Some(declaration) = declaration
        {
            let symbol = &symbols[declaration];
            let first = first_definition
                .get(&(access.point.clone(), symbol.clone()))
                .copied();
            if first.is_none_or(|ordinal| {
                access.ordinal < ordinal
                    || (access.kind == DataFlowAccessKind::ReadWrite && access.ordinal == ordinal)
            }) {
                uses[indexed.index[&access.point]].insert(symbol.clone());
            }
        }
    }
    uses
}

fn reaching_definitions(
    indexed: &IndexedFlow,
    point_definitions: &[Vec<(u32, DataFlowDefinitionKey)>],
    definition_symbols: &BTreeMap<DataFlowDefinitionKey, DataFlowSymbolKey>,
) -> (
    Vec<BTreeSet<DataFlowDefinitionKey>>,
    Vec<BTreeSet<DataFlowDefinitionKey>>,
) {
    let mut by_symbol = BTreeMap::<DataFlowSymbolKey, BTreeSet<DataFlowDefinitionKey>>::new();
    for (definition, symbol) in definition_symbols {
        by_symbol
            .entry(symbol.clone())
            .or_default()
            .insert(definition.clone());
    }
    let mut input = vec![BTreeSet::new(); indexed.keys.len()];
    let mut output = input.clone();
    loop {
        let mut changed = false;
        for point in &indexed.reachable {
            let next_in = indexed.predecessors[*point]
                .iter()
                .filter(|p| indexed.reachable.contains(p))
                .flat_map(|p| output[*p].iter().cloned())
                .collect::<BTreeSet<_>>();
            let mut next_out = next_in.clone();
            for (_, definition) in &point_definitions[*point] {
                let symbol = &definition_symbols[definition];
                for killed in &by_symbol[symbol] {
                    next_out.remove(killed);
                }
                next_out.insert(definition.clone());
            }
            if input[*point] != next_in {
                input[*point] = next_in;
                changed = true;
            }
            if output[*point] != next_out {
                output[*point] = next_out;
                changed = true;
            }
        }
        if !changed {
            return (input, output);
        }
    }
}

fn reaching_at_access(
    point: usize,
    ordinal: u32,
    symbol: &DataFlowSymbolKey,
    input: &[BTreeSet<DataFlowDefinitionKey>],
    point_definitions: &[Vec<(u32, DataFlowDefinitionKey)>],
    definition_symbols: &BTreeMap<DataFlowDefinitionKey, DataFlowSymbolKey>,
) -> Vec<DataFlowDefinitionKey> {
    let mut reaching = input[point]
        .iter()
        .filter(|definition| &definition_symbols[*definition] == symbol)
        .cloned()
        .collect::<BTreeSet<_>>();
    for (definition_ordinal, definition) in &point_definitions[point] {
        if *definition_ordinal >= ordinal {
            break;
        }
        if &definition_symbols[definition] == symbol {
            reaching.clear();
            reaching.insert(definition.clone());
        }
    }
    reaching.into_iter().collect()
}

fn liveness(
    indexed: &IndexedFlow,
    uses: &[BTreeSet<DataFlowSymbolKey>],
    definitions: &[BTreeSet<DataFlowSymbolKey>],
) -> (
    Vec<BTreeSet<DataFlowSymbolKey>>,
    Vec<BTreeSet<DataFlowSymbolKey>>,
) {
    let mut input = vec![BTreeSet::new(); indexed.keys.len()];
    let mut output = input.clone();
    loop {
        let mut changed = false;
        for point in indexed.reachable.iter().rev() {
            let next_out = indexed.successors[*point]
                .iter()
                .filter(|s| indexed.reachable.contains(s))
                .flat_map(|s| input[*s].iter().cloned())
                .collect::<BTreeSet<_>>();
            let mut next_in = next_out
                .difference(&definitions[*point])
                .cloned()
                .collect::<BTreeSet<_>>();
            next_in.extend(uses[*point].iter().cloned());
            if output[*point] != next_out {
                output[*point] = next_out;
                changed = true;
            }
            if input[*point] != next_in {
                input[*point] = next_in;
                changed = true;
            }
        }
        if !changed {
            return (input, output);
        }
    }
}

fn validate_text(value: &str) -> Result<(), DataFlowBuildError> {
    if value.trim().is_empty() || value.trim() != value {
        Err(DataFlowBuildError::Invalid(
            "data-flow uncertainty must be canonical nonempty text".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), DataFlowBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(DataFlowBuildError::Invalid(format!(
            "identity must start with {prefix}"
        )));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(DataFlowBuildError::Invalid(
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
pub(crate) mod tests {
    use std::path::Path;

    use deslop_core::Lang;
    use deslop_lang::{
        CapabilityDeclaration, DuplicateDefinitionRule, ExtractionFactKind, GrammarDescriptor,
        ImportTraversalRule, LangPack, LanguageAdapterCapabilityManifest,
        LanguageResolutionRulePack, PrecedenceDimension, PrecedenceDirection, RUST_PACK,
        RegionSpan, ResolutionInstruction, ResolutionRuleSection, ResolutionRuleSectionKind,
        ResolutionSyntaxSelector, RuleNamespace,
    };

    use super::*;
    use crate::{
        BindingDraft, BindingForm, BindingTargetDraft, BuildContextId, ControlBranchKind,
        ControlEdgeDraft, ControlEdgeKind, ControlEdgePrecision, ControlExitOutcome,
        ControlFlowBuilder, ControlFlowCoverageEvidence, ControlFlowGraphDraft,
        ControlFlowOwnerKind, ControlFlowPolicyId, ControlLoopKind, ControlPointDraft,
        ControlPointKind, ControlSyntheticPointKind, FactCoverageEvidence, Mutability,
        NameNamespace, NamespacePolicy, ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft,
        RepositoryId, ResolutionPolicyId, ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder,
        ScopeKind, VisibilityDraft, VisibilityKind, derive_control_regions,
    };

    pub(crate) struct DataFlowTestPack;

    pub(crate) static DATA_FLOW_TEST_PACK: DataFlowTestPack = DataFlowTestPack;

    impl LangPack for DataFlowTestPack {
        fn name(&self) -> &'static str {
            "data-flow-test-rust"
        }

        fn adapter_schema(&self) -> &'static str {
            RUST_PACK.adapter_schema()
        }

        fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
            let mut manifest = RUST_PACK.capability_manifest();
            for capability in [
                AdapterCapability::LexicalScopes,
                AdapterCapability::NameResolution,
                AdapterCapability::DefUse,
                AdapterCapability::Effects,
                AdapterCapability::LocalPdg,
                AdapterCapability::CallGraph,
                AdapterCapability::Sdg,
            ] {
                manifest = manifest
                    .with_declaration(CapabilityDeclaration::provided(
                        capability,
                        CapabilityAuthority::Adapter,
                    ))
                    .unwrap();
            }
            manifest
        }

        fn query_pack(&self) -> deslop_lang::LanguageQueryPack {
            RUST_PACK.query_pack()
        }
        fn lexical_policy(&self) -> deslop_lang::LanguageLexicalPolicy {
            RUST_PACK.lexical_policy()
        }
        fn construct_policy(&self) -> deslop_lang::LanguageConstructPolicy {
            RUST_PACK.construct_policy()
        }
        fn control_flow_rule_pack(&self) -> deslop_lang::LanguageControlFlowRulePack {
            RUST_PACK.control_flow_rule_pack()
        }

        fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
            let source = RUST_PACK.resolution_rule_pack();
            let mut sections = source.sections().to_vec();
            let extraction = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::Extraction)
                .unwrap();
            sections[extraction] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::Extraction,
                [
                    ExtractionFactKind::Declaration,
                    ExtractionFactKind::Definition,
                    ExtractionFactKind::Binding,
                    ExtractionFactKind::Reference,
                ]
                .into_iter()
                .map(|fact_kind| ResolutionInstruction::ExtractFact {
                    selector: ResolutionSyntaxSelector::new("identifier", None, None).unwrap(),
                    name_field: None,
                    namespace: matches!(
                        fact_kind,
                        ExtractionFactKind::Declaration | ExtractionFactKind::Reference
                    )
                    .then_some(RuleNamespace::Value),
                    fact_kind,
                })
                .collect(),
            )
            .unwrap();
            let imports = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::ImportsExports)
                .unwrap();
            sections[imports] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::ImportsExports,
                [
                    ImportTraversalRule::Explicit,
                    ImportTraversalRule::Selective,
                    ImportTraversalRule::Alias,
                    ImportTraversalRule::Glob,
                    ImportTraversalRule::Prelude,
                    ImportTraversalRule::Export,
                    ImportTraversalRule::ReExport,
                ]
                .into_iter()
                .map(|rule| ResolutionInstruction::ImportTraversal { rule })
                .collect(),
            )
            .unwrap();
            let duplicates = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::ShadowingDuplicates)
                .unwrap();
            sections[duplicates] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::ShadowingDuplicates,
                sections[duplicates]
                    .instructions()
                    .iter()
                    .cloned()
                    .map(|instruction| match instruction {
                        ResolutionInstruction::DuplicateDefinitions { namespace, .. } => {
                            ResolutionInstruction::DuplicateDefinitions {
                                namespace,
                                rule: DuplicateDefinitionRule::Ambiguous,
                            }
                        }
                        other => other,
                    })
                    .collect(),
            )
            .unwrap();
            let precedence = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::Precedence)
                .unwrap();
            sections[precedence] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::Precedence,
                vec![ResolutionInstruction::Precedence {
                    terms: vec![
                        deslop_lang::PrecedenceTerm::new(
                            PrecedenceDimension::RuleStep,
                            PrecedenceDirection::LowerFirst,
                        ),
                        deslop_lang::PrecedenceTerm::new(
                            PrecedenceDimension::LexicalDistance,
                            PrecedenceDirection::LowerFirst,
                        ),
                        deslop_lang::PrecedenceTerm::new(
                            PrecedenceDimension::Namespace,
                            PrecedenceDirection::LowerFirst,
                        ),
                    ],
                }],
            )
            .unwrap();
            LanguageResolutionRulePack::new(
                self.adapter_schema(),
                source.dialects().to_vec(),
                sections,
            )
            .unwrap()
        }

        fn canonical_roles(
            &self,
            node: tree_sitter::Node<'_>,
            text: &str,
        ) -> deslop_lang::CanonicalRoleSet {
            RUST_PACK.canonical_roles(node, text)
        }
        fn lang(&self) -> Lang {
            Lang::Rust
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["dflowrs"]
        }
        fn grammar(&self) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar()
        }
        fn grammar_for_path(&self, path: &Path) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar_for_path(path)
        }
        fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
            RUST_PACK.grammar_descriptor_for_path(Path::new("fixture.rs"))
        }
        fn line_comments(&self) -> &'static [&'static str] {
            RUST_PACK.line_comments()
        }
        fn metrics_regions(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_regions()
        }
        fn metrics_branches(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_branches()
        }
        fn metrics_nesting(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_nesting()
        }
        fn metrics_flow_breaks(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_flow_breaks()
        }
        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            RUST_PACK.halstead_operator_tokens()
        }
        fn enclosing_region(&self, node: tree_sitter::Node<'_>, text: &str) -> Option<RegionSpan> {
            RUST_PACK.enclosing_region(node, text)
        }
    }

    fn point(index: usize) -> ControlPointKey {
        serde_json::from_str(&format!("\"cpt1_{:064x}\"", index + 1)).unwrap()
    }

    fn symbol(index: usize) -> DataFlowSymbolKey {
        DataFlowSymbolKey(format!("dfs1_{:064x}", index + 1))
    }

    fn definition(index: usize) -> DataFlowDefinitionKey {
        DataFlowDefinitionKey(format!("dfd1_{:064x}", index + 1))
    }

    fn indexed(successors: &[&[usize]], reachable: &[usize]) -> IndexedFlow {
        let keys = (0..successors.len()).map(point).collect::<Vec<_>>();
        let index = keys
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect();
        let successors = successors
            .iter()
            .map(|targets| targets.iter().copied().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();
        let mut predecessors = vec![BTreeSet::new(); successors.len()];
        for (from, targets) in successors.iter().enumerate() {
            for to in targets {
                predecessors[*to].insert(from);
            }
        }
        IndexedFlow {
            keys,
            index,
            successors,
            predecessors,
            reachable: reachable.iter().copied().collect(),
        }
    }

    #[test]
    fn m4_8_capture_borrow_and_mutation_access_modes_are_conservative() {
        for kind in [DataFlowAccessKind::Capture, DataFlowAccessKind::Borrow] {
            assert!(kind.reads());
            assert!(!kind.writes());
        }
        assert!(DataFlowAccessKind::ReadWrite.reads());
        assert!(DataFlowAccessKind::ReadWrite.writes());
        assert!(!DataFlowAccessKind::Write.reads());
        assert!(DataFlowAccessKind::Write.writes());
    }

    #[test]
    fn m4_8_advanced_output_and_effect_catalogs_round_trip_without_collapse() {
        let boundaries = [
            DataFlowBoundaryKind::MutationOutput,
            DataFlowBoundaryKind::ExceptionalOutput,
            DataFlowBoundaryKind::SuspensionOutput,
        ];
        let effects = [
            DataFlowEffectKind::ReadsMemory,
            DataFlowEffectKind::WritesMemory,
            DataFlowEffectKind::Throws,
            DataFlowEffectKind::Suspends,
            DataFlowEffectKind::Captures,
            DataFlowEffectKind::GlobalState,
        ];
        let boundary_wire = serde_json::to_vec(&boundaries).unwrap();
        let effect_wire = serde_json::to_vec(&effects).unwrap();
        assert_eq!(
            serde_json::from_slice::<[DataFlowBoundaryKind; 3]>(&boundary_wire).unwrap(),
            boundaries
        );
        assert_eq!(
            serde_json::from_slice::<[DataFlowEffectKind; 6]>(&effect_wire).unwrap(),
            effects
        );
        assert_eq!(boundaries.into_iter().collect::<BTreeSet<_>>().len(), 3);
        assert_eq!(effects.into_iter().collect::<BTreeSet<_>>().len(), 6);
    }

    #[test]
    fn m4_5_linear_reaching_definitions_kill_prior_symbol_definition() {
        let graph = indexed(&[&[1], &[2], &[]], &[0, 1, 2]);
        let a = symbol(0);
        let d0 = definition(0);
        let d1 = definition(1);
        let point_definitions = vec![vec![(0, d0.clone())], vec![], vec![(0, d1.clone())]];
        let definition_symbols = BTreeMap::from([(d0.clone(), a.clone()), (d1.clone(), a)]);
        let (input, output) = reaching_definitions(&graph, &point_definitions, &definition_symbols);
        assert!(input[0].is_empty());
        assert_eq!(output[0], BTreeSet::from([d0.clone()]));
        assert_eq!(input[1], BTreeSet::from([d0.clone()]));
        assert_eq!(input[2], BTreeSet::from([d0]));
        assert_eq!(output[2], BTreeSet::from([d1]));
    }

    #[test]
    fn m4_5_branch_join_retains_both_reaching_definitions() {
        let graph = indexed(&[&[1, 2], &[3], &[3], &[]], &[0, 1, 2, 3]);
        let a = symbol(0);
        let left = definition(0);
        let right = definition(1);
        let point_definitions = vec![
            vec![],
            vec![(0, left.clone())],
            vec![(0, right.clone())],
            vec![],
        ];
        let definition_symbols = BTreeMap::from([(left.clone(), a.clone()), (right.clone(), a)]);
        let (input, output) = reaching_definitions(&graph, &point_definitions, &definition_symbols);
        assert_eq!(input[3], BTreeSet::from([left.clone(), right.clone()]));
        assert_eq!(output[3], BTreeSet::from([left, right]));
    }

    #[test]
    fn m4_5_loop_liveness_converges_without_forcing_virtual_exit() {
        let graph = indexed(&[&[1], &[2, 3], &[1], &[]], &[0, 1, 2, 3]);
        let a = symbol(0);
        let uses = vec![
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::from([a.clone()]),
            BTreeSet::new(),
        ];
        let definitions = vec![BTreeSet::new(); 4];
        let (input, output) = liveness(&graph, &uses, &definitions);
        assert_eq!(input[0], BTreeSet::from([a.clone()]));
        assert_eq!(input[1], BTreeSet::from([a.clone()]));
        assert_eq!(input[2], BTreeSet::from([a.clone()]));
        assert_eq!(output[2], BTreeSet::from([a]));
        assert!(input[3].is_empty());
    }

    #[test]
    fn m4_5_unreachable_points_have_no_execution_relations() {
        let graph = indexed(&[&[1], &[], &[2]], &[0, 1]);
        let a = symbol(0);
        let dead = definition(0);
        let point_definitions = vec![vec![], vec![], vec![(0, dead.clone())]];
        let definition_symbols = BTreeMap::from([(dead, a.clone())]);
        let (reaching_in, reaching_out) =
            reaching_definitions(&graph, &point_definitions, &definition_symbols);
        let uses = vec![BTreeSet::new(), BTreeSet::new(), BTreeSet::from([a])];
        let definitions = vec![BTreeSet::new(); 3];
        let (live_in, live_out) = liveness(&graph, &uses, &definitions);
        assert!(reaching_in[2].is_empty());
        assert!(reaching_out[2].is_empty());
        assert!(live_in[2].is_empty());
        assert!(live_out[2].is_empty());
    }

    #[test]
    fn m4_5_same_point_access_observes_only_prior_definitions() {
        let a = symbol(0);
        let incoming = definition(0);
        let local = definition(1);
        let reaching_in = vec![BTreeSet::from([incoming.clone()])];
        let point_definitions = vec![vec![(5, local.clone())]];
        let definition_symbols =
            BTreeMap::from([(incoming.clone(), a.clone()), (local.clone(), a.clone())]);
        assert_eq!(
            reaching_at_access(
                0,
                5,
                &a,
                &reaching_in,
                &point_definitions,
                &definition_symbols,
            ),
            [incoming]
        );
        assert_eq!(
            reaching_at_access(
                0,
                6,
                &a,
                &reaching_in,
                &point_definitions,
                &definition_symbols,
            ),
            [local]
        );
    }

    #[test]
    fn m4_5_shadowed_resolved_symbols_do_not_cross_kill() {
        let graph = indexed(&[&[1], &[2], &[]], &[0, 1, 2]);
        let outer = symbol(0);
        let inner = symbol(1);
        let outer_definition = definition(0);
        let inner_definition = definition(1);
        let point_definitions = vec![
            vec![(0, outer_definition.clone())],
            vec![(0, inner_definition.clone())],
            vec![],
        ];
        let definition_symbols = BTreeMap::from([
            (outer_definition.clone(), outer),
            (inner_definition.clone(), inner.clone()),
        ]);
        let (input, _) = reaching_definitions(&graph, &point_definitions, &definition_symbols);
        assert_eq!(
            input[2],
            BTreeSet::from([outer_definition, inner_definition.clone()])
        );
        assert_eq!(
            reaching_at_access(
                2,
                0,
                &inner,
                &input,
                &point_definitions,
                &definition_symbols,
            ),
            [inner_definition]
        );
    }

    fn integration_analysis() -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let mut registry = deslop_lang::Registry::default();
        registry.register(&DATA_FLOW_TEST_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("data-flow-integration-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay(
            "flow.dflowrs",
            b"fn run(x: i32) -> i32 { let mut y = x; if x > 0 { y += 1; } y }\n".to_vec(),
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

    fn scope_visibility(scope: crate::ScopeFactId) -> VisibilityDraft {
        VisibilityDraft {
            kind: VisibilityKind::Scope,
            boundary: Some(scope),
            adapter_rule: None,
        }
    }

    #[test]
    fn m4_5_m4_6_m4_8_ambiguous_capture_remains_unknown_partial_and_a_pdg_gap() {
        let analysis = integration_analysis();
        let root_node = nodes_by_kind(&analysis, "source_file")[0];
        let function = nodes_by_kind(&analysis, "function_item")[0];
        let reference_node = *nodes_by_text(&analysis, "y").last().unwrap();
        let declaration_nodes = [
            nodes_by_text(&analysis, "x")[0],
            nodes_by_text(&analysis, "y")[0],
        ];
        let complete = FactCoverageEvidence::complete();
        let namespaces = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let mut scope_builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"data-flow-ambiguous-target"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"data-flow-ambiguous-scope/1"]).unwrap(),
        )
        .unwrap();
        let file_scope = scope_builder
            .add_scope(
                root_node,
                roles(&analysis, root_node),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let callable_scope = scope_builder
            .add_scope(
                function,
                roles(&analysis, function),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();
        for node in declaration_nodes {
            let declaration = scope_builder
                .add_declaration(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    crate::DeclarationDraft {
                        original_name: "collision".into(),
                        lookup_key: "collision".into(),
                        namespace: NameNamespace::Value,
                        scope: callable_scope,
                        visibility: scope_visibility(callable_scope),
                        modifiers: vec![],
                    },
                )
                .unwrap();
            scope_builder
                .add_binding(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(declaration),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AtDeclaration,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
        }
        let reference = scope_builder
            .add_reference(
                reference_node,
                roles(&analysis, reference_node),
                complete,
                ReferenceDraft {
                    original_spelling: "collision".into(),
                    segments: vec!["collision".into()],
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        let scope_graph = Arc::new(scope_builder.build().unwrap());
        let reference_key = scope_graph.fact(reference).unwrap().key().clone();
        let resolution = Arc::new(
            ResolutionProjection::build(
                scope_graph,
                ResolutionPolicyId::from_parts(&[b"data-flow-ambiguous-resolution/1"]).unwrap(),
            )
            .unwrap(),
        );
        assert_eq!(resolution.results().len(), 1);
        assert_eq!(
            resolution.results()[0].wire().status(),
            ResolutionStatus::Ambiguous
        );

        let mut flow_builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"data-flow-ambiguous-cfg/1"]).unwrap(),
        );
        flow_builder
            .add_graph(ControlFlowGraphDraft {
                owner: function,
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
                        source: Some(reference_node),
                        ordinal: 0,
                    },
                    ControlPointDraft {
                        kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                        source: Some(function),
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
                        source: function,
                        predicate: None,
                        precision: ControlEdgePrecision::Exact,
                    },
                    ControlEdgeDraft {
                        from: 1,
                        to: 1,
                        kind: ControlEdgeKind::Loop(ControlLoopKind::Back),
                        source: function,
                        predicate: None,
                        precision: ControlEdgePrecision::Exact,
                    },
                    ControlEdgeDraft {
                        from: 2,
                        to: 3,
                        kind: ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                        source: function,
                        predicate: None,
                        precision: ControlEdgePrecision::Exact,
                    },
                ],
            })
            .unwrap();
        let flow = Arc::new(flow_builder.build().unwrap());
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                crate::ControlRegionPolicyId::from_parts(&[b"data-flow-ambiguous-regions/1"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let graph = &flow.document().graphs()[0];
        let access_point = graph
            .points()
            .iter()
            .find(|point| point.source() == Some(analysis.node_key(reference_node).unwrap()))
            .unwrap()
            .key()
            .clone();
        let effects = graph
            .points()
            .iter()
            .map(|point| DataFlowEffectDraft {
                point: point.key().clone(),
                effects: vec![],
                uncertainty: None,
            })
            .collect();
        let mut builder = DataFlowBuilder::new(
            regions,
            resolution,
            DataFlowPolicyId::from_parts(&[b"data-flow-ambiguous/1"]).unwrap(),
        )
        .unwrap();
        builder
            .add_graph(DataFlowGraphDraft {
                control_flow_graph: graph.key().clone(),
                definitions: vec![],
                accesses: vec![DataFlowAccessDraft {
                    point: access_point,
                    reference: reference_key,
                    kind: DataFlowAccessKind::Capture,
                    ordinal: 0,
                }],
                boundaries: vec![],
                effects,
            })
            .unwrap();
        let projection = builder.build().unwrap();
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert_eq!(graph.accesses().len(), 1);
        assert_eq!(graph.accesses()[0].kind(), DataFlowAccessKind::Capture);
        assert!(graph.accesses()[0].symbol().is_none());
        assert!(graph.accesses()[0].reaching_definitions().is_empty());
        assert!(graph.accesses()[0].uncertainty().is_some());

        let projection = Arc::new(projection);
        let non_structured = Arc::new(
            crate::derive_non_structured_control_regions(
                Arc::clone(projection.control_regions()),
                crate::NonStructuredControlPolicyId::from_parts(&[
                    b"data-flow-ambiguous-non-structured/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        assert_eq!(non_structured.document().graphs()[0].facts().len(), 1);
        assert_eq!(
            non_structured.document().graphs()[0].facts()[0].classification(),
            crate::NonStructuredControlClassification::NonTerminatingCycle
        );
        let pdg = crate::derive_program_dependence(
            projection,
            Arc::clone(&non_structured),
            crate::ProgramDependencePolicyId::from_parts(&[b"data-flow-ambiguous-pdg/1"]).unwrap(),
        )
        .unwrap();
        let graph = &pdg.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert!(
            graph
                .edges()
                .iter()
                .all(|edge| !matches!(edge.kind(), crate::ProgramDependenceEdgeKind::Flow { .. }))
        );
        assert_eq!(graph.non_structured_facts().len(), 1);
        assert_eq!(
            graph
                .gaps()
                .iter()
                .filter(|gap| matches!(
                    gap.kind(),
                    crate::ProgramDependenceGapKind::UnresolvedAccess { .. }
                ))
                .count(),
            1
        );
        assert_eq!(
            graph
                .gaps()
                .iter()
                .filter(|gap| matches!(
                    gap.kind(),
                    crate::ProgramDependenceGapKind::ControlPostDominanceUnavailable { .. }
                ))
                .count(),
            2
        );
    }

    #[test]
    fn m4_5_m4_6_end_to_end_projection_joins_resolution_cfg_boundaries_effects_and_pdg() {
        let analysis = integration_analysis();
        let root_node = nodes_by_kind(&analysis, "source_file")[0];
        let function = nodes_by_kind(&analysis, "function_item")[0];
        let block = nodes_by_kind(&analysis, "block")[0];
        let let_node = nodes_by_kind(&analysis, "let_declaration")[0];
        let if_node = nodes_by_kind(&analysis, "if_expression")[0];
        let assignment = nodes_by_kind(&analysis, "expression_statement")[0];
        let xs = nodes_by_text(&analysis, "x");
        let ys = nodes_by_text(&analysis, "y");
        assert_eq!(xs.len(), 3);
        assert_eq!(ys.len(), 3);
        let complete = FactCoverageEvidence::complete();
        let namespaces = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let mut scope_builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"data-flow-integration-target"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"data-flow-integration-scope/1"]).unwrap(),
        )
        .unwrap();
        let file_scope = scope_builder
            .add_scope(
                root_node,
                roles(&analysis, root_node),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let callable_scope = scope_builder
            .add_scope(
                function,
                roles(&analysis, function),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();
        let function_declaration = scope_builder
            .add_declaration(
                function,
                roles(&analysis, function),
                complete.clone(),
                crate::DeclarationDraft {
                    original_name: "run".into(),
                    lookup_key: "run".into(),
                    namespace: NameNamespace::Value,
                    scope: file_scope,
                    visibility: scope_visibility(file_scope),
                    modifiers: vec![],
                },
            )
            .unwrap();
        let x_declaration = scope_builder
            .add_declaration(
                xs[0],
                roles(&analysis, xs[0]),
                complete.clone(),
                crate::DeclarationDraft {
                    original_name: "x".into(),
                    lookup_key: "x".into(),
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    visibility: scope_visibility(callable_scope),
                    modifiers: vec![],
                },
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
                crate::DeclarationDraft {
                    original_name: "y".into(),
                    lookup_key: "y".into(),
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    visibility: scope_visibility(callable_scope),
                    modifiers: vec![],
                },
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
                    mutability: Mutability::Mutable,
                },
            )
            .unwrap();
        let x_reference = scope_builder
            .add_reference(
                xs[1],
                roles(&analysis, xs[1]),
                complete.clone(),
                ReferenceDraft {
                    original_spelling: "x".into(),
                    segments: vec!["x".into()],
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        let x_condition = scope_builder
            .add_reference(
                xs[2],
                roles(&analysis, xs[2]),
                complete.clone(),
                ReferenceDraft {
                    original_spelling: "x".into(),
                    segments: vec!["x".into()],
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        let y_write = scope_builder
            .add_reference(
                ys[1],
                roles(&analysis, ys[1]),
                complete.clone(),
                ReferenceDraft {
                    original_spelling: "y".into(),
                    segments: vec!["y".into()],
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    role: ReferenceRole::Write,
                },
            )
            .unwrap();
        let y_read = scope_builder
            .add_reference(
                ys[2],
                roles(&analysis, ys[2]),
                complete,
                ReferenceDraft {
                    original_spelling: "y".into(),
                    segments: vec!["y".into()],
                    namespace: NameNamespace::Value,
                    scope: callable_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        let scope_graph = Arc::new(scope_builder.build().unwrap());
        let key = |id| scope_graph.fact(id).unwrap().key().clone();
        let file_scope_key = key(file_scope);
        let function_key = key(function_declaration);
        let x_declaration_key = key(x_declaration);
        let x_binding_key = key(x_binding);
        let y_declaration_key = key(y_declaration);
        let y_binding_key = key(y_binding);
        let x_reference_key = key(x_reference);
        let x_condition_key = key(x_condition);
        let y_write_key = key(y_write);
        let y_read_key = key(y_read);
        let resolution = Arc::new(
            crate::ResolutionProjection::build(
                Arc::clone(&scope_graph),
                ResolutionPolicyId::from_parts(&[b"data-flow-resolution/1"]).unwrap(),
            )
            .unwrap(),
        );
        assert!(
            resolution
                .results()
                .iter()
                .all(|result| result.wire().status() == ResolutionStatus::Unique)
        );

        let owner = function;
        let points = vec![
            ControlPointDraft {
                kind: ControlPointKind::Entry,
                source: None,
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(let_node),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(if_node),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(assignment),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(ys[2]),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                source: Some(block),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Exit,
                source: None,
                ordinal: 0,
            },
        ];
        let edge = |from, to, kind| ControlEdgeDraft {
            from,
            to,
            kind,
            source: owner,
            predicate: None,
            precision: ControlEdgePrecision::Exact,
        };
        let edges = vec![
            edge(0, 1, ControlEdgeKind::Entry),
            edge(1, 2, ControlEdgeKind::Normal),
            ControlEdgeDraft {
                from: 2,
                to: 3,
                kind: ControlEdgeKind::Branch(ControlBranchKind::True),
                source: if_node,
                predicate: Some(if_node),
                precision: ControlEdgePrecision::Exact,
            },
            ControlEdgeDraft {
                from: 2,
                to: 4,
                kind: ControlEdgeKind::Branch(ControlBranchKind::False),
                source: if_node,
                predicate: Some(if_node),
                precision: ControlEdgePrecision::Exact,
            },
            edge(3, 4, ControlEdgeKind::Normal),
            edge(4, 5, ControlEdgeKind::Normal),
            edge(5, 6, ControlEdgeKind::Exit(ControlExitOutcome::Normal)),
        ];
        let mut flow_builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"data-flow-cfg/1"]).unwrap(),
        );
        flow_builder
            .add_graph(ControlFlowGraphDraft {
                owner,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points,
                edges,
            })
            .unwrap();
        let flow = Arc::new(flow_builder.build().unwrap());
        let flow_graph = &flow.document().graphs()[0];
        let point_for_source = |node| {
            flow_graph
                .points()
                .iter()
                .find(|point| point.source() == Some(analysis.node_key(node).unwrap()))
                .unwrap()
                .key()
                .clone()
        };
        let let_point = point_for_source(let_node);
        let condition_point = point_for_source(if_node);
        let write_point = point_for_source(assignment);
        let read_point = point_for_source(ys[2]);
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                crate::ControlRegionPolicyId::from_parts(&[b"data-flow-regions/1"]).unwrap(),
            )
            .unwrap(),
        );
        let all_points = regions.control_flow().document().graphs()[0]
            .points()
            .iter()
            .map(|point| point.key().clone())
            .collect::<Vec<_>>();
        let mut effects = all_points
            .iter()
            .cloned()
            .map(|point| DataFlowEffectDraft {
                point,
                effects: vec![],
                uncertainty: None,
            })
            .collect::<Vec<_>>();
        effects
            .iter_mut()
            .find(|effect| effect.point == *regions.control_flow().document().graphs()[0].exit())
            .unwrap()
            .effects
            .push(DataFlowEffectKind::Returns);
        let policy = DataFlowPolicyId::from_parts(&[b"data-flow-integration/1"]).unwrap();
        let draft = DataFlowGraphDraft {
            control_flow_graph: flow_graph.key().clone(),
            definitions: vec![
                DataFlowDefinitionDraft {
                    point: flow_graph.entry().clone(),
                    declaration: x_declaration_key.clone(),
                    source_fact: x_binding_key.clone(),
                    ordinal: 0,
                },
                DataFlowDefinitionDraft {
                    point: let_point.clone(),
                    declaration: y_declaration_key.clone(),
                    source_fact: y_binding_key,
                    ordinal: 1,
                },
                DataFlowDefinitionDraft {
                    point: write_point.clone(),
                    declaration: y_declaration_key.clone(),
                    source_fact: y_write_key.clone(),
                    ordinal: 0,
                },
            ],
            accesses: vec![
                DataFlowAccessDraft {
                    point: let_point.clone(),
                    reference: x_reference_key,
                    kind: DataFlowAccessKind::Read,
                    ordinal: 0,
                },
                DataFlowAccessDraft {
                    point: condition_point,
                    reference: x_condition_key,
                    kind: DataFlowAccessKind::Read,
                    ordinal: 0,
                },
                DataFlowAccessDraft {
                    point: write_point,
                    reference: y_write_key.clone(),
                    kind: DataFlowAccessKind::ReadWrite,
                    ordinal: 0,
                },
                DataFlowAccessDraft {
                    point: read_point,
                    reference: y_read_key.clone(),
                    kind: DataFlowAccessKind::Read,
                    ordinal: 0,
                },
            ],
            boundaries: vec![
                DataFlowBoundaryDraft {
                    point: flow_graph.entry().clone(),
                    kind: DataFlowBoundaryKind::ParameterInput,
                    declaration: Some(x_declaration_key),
                    source_fact: x_binding_key,
                },
                DataFlowBoundaryDraft {
                    point: flow_graph.exit().clone(),
                    kind: DataFlowBoundaryKind::MutationOutput,
                    declaration: Some(y_declaration_key.clone()),
                    source_fact: y_write_key,
                },
                DataFlowBoundaryDraft {
                    point: flow_graph.exit().clone(),
                    kind: DataFlowBoundaryKind::ReturnOutput,
                    declaration: Some(y_declaration_key),
                    source_fact: y_read_key.clone(),
                },
                DataFlowBoundaryDraft {
                    point: flow_graph.exit().clone(),
                    kind: DataFlowBoundaryKind::ReturnOutput,
                    declaration: None,
                    source_fact: function_key,
                },
            ],
            effects,
        };
        let mut builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            policy.clone(),
        )
        .unwrap();
        builder.add_graph(draft.clone()).unwrap();
        let projection = builder.build().unwrap();
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert_eq!(graph.symbols().len(), 2);
        assert_eq!(graph.definitions().len(), 3);
        assert_eq!(graph.accesses().len(), 4);
        assert_eq!(graph.boundaries().len(), 4);
        assert_eq!(graph.effects().len(), all_points.len());
        let tail = graph
            .accesses()
            .iter()
            .find(|access| access.reference() == &y_read_key)
            .unwrap();
        assert_eq!(tail.reaching_definitions().len(), 2);
        assert_eq!(
            graph
                .boundaries()
                .iter()
                .filter(|boundary| boundary.kind() == DataFlowBoundaryKind::ParameterInput)
                .count(),
            1
        );

        let non_structured = Arc::new(
            crate::derive_non_structured_control_regions(
                Arc::clone(&regions),
                crate::NonStructuredControlPolicyId::from_parts(&[
                    b"data-flow-integration-non-structured/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        let pdg = crate::derive_program_dependence(
            Arc::new(projection.clone()),
            Arc::clone(&non_structured),
            crate::ProgramDependencePolicyId::from_parts(&[b"data-flow-integration-pdg/1"])
                .unwrap(),
        )
        .unwrap();
        let pdg_graph = &pdg.document().graphs()[0];
        assert_eq!(pdg_graph.coverage().status(), FactCoverage::Complete);
        assert_eq!(pdg_graph.nodes().len(), all_points.len());
        assert!(pdg_graph.gaps().is_empty());
        assert!(pdg_graph.non_structured_facts().is_empty());
        assert_eq!(
            pdg_graph
                .edges()
                .iter()
                .filter(|edge| matches!(edge.kind(), crate::ProgramDependenceEdgeKind::Flow { .. }))
                .count(),
            5
        );
        assert_eq!(
            pdg_graph
                .edges()
                .iter()
                .filter(|edge| matches!(
                    edge.kind(),
                    crate::ProgramDependenceEdgeKind::Control { .. }
                ))
                .count(),
            1
        );
        let controller = pdg_graph
            .nodes()
            .iter()
            .find(|node| node.source() == Some(analysis.node_key(if_node).unwrap()))
            .unwrap();
        let controlled = pdg_graph
            .nodes()
            .iter()
            .find(|node| node.source() == Some(analysis.node_key(assignment).unwrap()))
            .unwrap();
        let control_edge = pdg_graph
            .edges()
            .iter()
            .find(|edge| {
                matches!(
                    edge.kind(),
                    crate::ProgramDependenceEdgeKind::Control { .. }
                )
            })
            .unwrap();
        assert_eq!(control_edge.from(), controller.key());
        assert_eq!(control_edge.to(), controlled.key());
        let crate::ProgramDependenceEdgeKind::Control { inducing_edges } = control_edge.kind()
        else {
            unreachable!()
        };
        assert_eq!(inducing_edges.len(), 1);
        let tail_node = pdg_graph
            .nodes()
            .iter()
            .find(|node| node.source() == Some(analysis.node_key(ys[2]).unwrap()))
            .unwrap();
        assert_eq!(
            pdg_graph
                .edges()
                .iter()
                .filter(|edge| {
                    edge.to() == tail_node.key()
                        && matches!(edge.kind(), crate::ProgramDependenceEdgeKind::Flow { .. })
                })
                .count(),
            2
        );
        let pdg_bytes = serde_json::to_vec(pdg.document()).unwrap();
        let decoded_pdg: crate::ProgramDependenceDocument =
            serde_json::from_slice(&pdg_bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded_pdg).unwrap(), pdg_bytes);
        let repeated_pdg = crate::derive_program_dependence(
            Arc::new(projection.clone()),
            Arc::clone(&non_structured),
            crate::ProgramDependencePolicyId::from_parts(&[b"data-flow-integration-pdg/1"])
                .unwrap(),
        )
        .unwrap();
        assert_eq!(repeated_pdg.id(), pdg.id());
        assert_eq!(
            serde_json::to_vec(repeated_pdg.document()).unwrap(),
            pdg_bytes
        );
        let changed_pdg = crate::derive_program_dependence(
            Arc::new(projection.clone()),
            Arc::clone(&non_structured),
            crate::ProgramDependencePolicyId::from_parts(&[b"data-flow-integration-pdg/2"])
                .unwrap(),
        )
        .unwrap();
        assert_ne!(changed_pdg.id(), pdg.id());
        let mut corrupted_pdg: serde_json::Value = serde_json::from_slice(&pdg_bytes).unwrap();
        corrupted_pdg["graphs"][0]["nodes"][0]["reachable"] = false.into();
        assert!(serde_json::from_value::<crate::ProgramDependenceDocument>(corrupted_pdg).is_err());
        let mut unknown_pdg_field: serde_json::Value = serde_json::from_slice(&pdg_bytes).unwrap();
        unknown_pdg_field
            .as_object_mut()
            .unwrap()
            .insert("untrusted".into(), true.into());
        assert!(
            serde_json::from_value::<crate::ProgramDependenceDocument>(unknown_pdg_field).is_err()
        );
        let mut wrong_pdg_schema: serde_json::Value = serde_json::from_slice(&pdg_bytes).unwrap();
        wrong_pdg_schema["schema"] = "deslop.program-dependence/999".into();
        assert!(
            serde_json::from_value::<crate::ProgramDependenceDocument>(wrong_pdg_schema).is_err()
        );

        let mut missing_effect = draft.clone();
        missing_effect
            .effects
            .retain(|effect| effect.point != *flow_graph.entry());
        let mut partial_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"data-flow-missing-effect/1"]).unwrap(),
        )
        .unwrap();
        partial_builder.add_graph(missing_effect).unwrap();
        let partial = partial_builder.build().unwrap();
        assert_eq!(
            partial.document().graphs()[0].coverage().status(),
            FactCoverage::Partial
        );
        assert!(partial.document().graphs()[0]
            .coverage()
            .reasons()
            .iter()
            .any(|reason| reason == "Provided Effects evidence omits a reachable control point"));

        let mut missing_parameter_definition = draft.clone();
        missing_parameter_definition
            .definitions
            .retain(|definition| definition.point != *flow_graph.entry());
        let mut invalid_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"data-flow-invalid-parameter-definition/1"]).unwrap(),
        )
        .unwrap();
        assert!(
            invalid_builder
                .add_graph(missing_parameter_definition)
                .is_err()
        );

        let mut misplaced_parameter = draft.clone();
        misplaced_parameter
            .boundaries
            .iter_mut()
            .find(|boundary| boundary.kind == DataFlowBoundaryKind::ParameterInput)
            .unwrap()
            .point = flow_graph.exit().clone();
        let mut invalid_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"data-flow-invalid-parameter-point/1"]).unwrap(),
        )
        .unwrap();
        assert!(invalid_builder.add_graph(misplaced_parameter).is_err());

        let parameter_source = draft
            .boundaries
            .iter()
            .find(|boundary| boundary.kind == DataFlowBoundaryKind::ParameterInput)
            .unwrap()
            .source_fact
            .clone();
        let mut mismatched_output = draft.clone();
        mismatched_output
            .boundaries
            .iter_mut()
            .find(|boundary| {
                boundary.kind == DataFlowBoundaryKind::ReturnOutput
                    && boundary.declaration.is_some()
            })
            .unwrap()
            .source_fact = parameter_source;
        let mut invalid_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"data-flow-invalid-output-source/1"]).unwrap(),
        )
        .unwrap();
        assert!(invalid_builder.add_graph(mismatched_output).is_err());

        let mut foreign_owner_source = draft.clone();
        foreign_owner_source.boundaries.push(DataFlowBoundaryDraft {
            point: flow_graph.exit().clone(),
            kind: DataFlowBoundaryKind::ReturnOutput,
            declaration: None,
            source_fact: file_scope_key,
        });
        let mut invalid_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            Arc::clone(&resolution),
            DataFlowPolicyId::from_parts(&[b"data-flow-invalid-owner-source/1"]).unwrap(),
        )
        .unwrap();
        assert!(invalid_builder.add_graph(foreign_owner_source).is_err());

        let bytes = serde_json::to_vec(projection.document()).unwrap();
        let decoded: DataFlowDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);

        let mut repeated =
            DataFlowBuilder::new(Arc::clone(&regions), Arc::clone(&resolution), policy).unwrap();
        repeated.add_graph(draft.clone()).unwrap();
        let repeated = repeated.build().unwrap();
        assert_eq!(repeated.id(), projection.id());
        assert_eq!(
            serde_json::to_vec(repeated.document()).unwrap(),
            serde_json::to_vec(projection.document()).unwrap()
        );

        let mut changed = DataFlowBuilder::new(
            regions,
            resolution,
            DataFlowPolicyId::from_parts(&[b"data-flow-integration/2"]).unwrap(),
        )
        .unwrap();
        changed.add_graph(draft).unwrap();
        let changed = changed.build().unwrap();
        assert_ne!(changed.id(), projection.id());
        assert_ne!(
            changed.document().graphs()[0].key(),
            projection.document().graphs()[0].key()
        );

        let mut unknown_field: serde_json::Value =
            serde_json::from_slice(&bytes).expect("document is JSON");
        unknown_field
            .as_object_mut()
            .unwrap()
            .insert("untrusted".into(), true.into());
        assert!(serde_json::from_value::<DataFlowDocument>(unknown_field).is_err());

        let mut corrupted_payload: serde_json::Value =
            serde_json::from_slice(&bytes).expect("document is JSON");
        corrupted_payload["graphs"][0]["definitions"][0]["ordinal"] = 99.into();
        assert!(serde_json::from_value::<DataFlowDocument>(corrupted_payload).is_err());

        let mut wrong_schema: serde_json::Value =
            serde_json::from_slice(&bytes).expect("document is JSON");
        wrong_schema["schema"] = "deslop.data-flow/999".into();
        assert!(serde_json::from_value::<DataFlowDocument>(wrong_schema).is_err());
    }
}
