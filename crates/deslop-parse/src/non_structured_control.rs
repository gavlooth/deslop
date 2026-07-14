use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ControlFlowGraph, ControlFlowGraphKey, ControlFlowPolicyId, ControlPointKey,
    ControlRegionGraph, ControlRegionGraphKey, ControlRegionPolicyId, ControlRegionProjection,
    ControlRegionResidual, ControlRegionResidualKey, FactCoverage, NodeKey, ProjectionId,
};

pub const NON_STRUCTURED_CONTROL_SCHEMA: &str = "deslop.non-structured-control-regions/1";
pub const NON_STRUCTURED_CONTROL_POLICY_SCHEMA: &str =
    "deslop.non-structured-control-region-policy/1";

const POLICY_ID_DOMAIN: &str = "deslop non-structured control-region policy v1";
const GRAPH_KEY_DOMAIN: &str = "deslop non-structured control-region graph key v1";
const FACT_KEY_DOMAIN: &str = "deslop non-structured control-region fact key v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NonStructuredControlPolicyId(String);

impl NonStructuredControlPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, NonStructuredControlBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(NonStructuredControlBuildError::Invalid(
                "non-structured control policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_parts_id(POLICY_ID_DOMAIN, "nsp1_", parts)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for NonStructuredControlPolicyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "nsp1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

macro_rules! digest_key {
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
                validate_digest_id(&value, $prefix).map_err(D::Error::custom)?;
                Ok(Self(value))
            }
        }
    };
}

digest_key!(NonStructuredControlGraphKey, "nsg1_");
digest_key!(NonStructuredControlFactKey, "nsf1_");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NonStructuredControlClassification {
    IrreducibleMultiEntryCycle,
    NonTerminatingCycle,
    UnknownIncompleteControlFlow,
    InvalidCandidateBoundary,
    IncomingBoundaryBypass,
    OutgoingBoundaryBypass,
    CrossingCandidates,
    MissingStructuredRoot,
}

impl NonStructuredControlClassification {
    fn is_scc(self) -> bool {
        matches!(
            self,
            Self::IrreducibleMultiEntryCycle | Self::NonTerminatingCycle
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "source", content = "key", rename_all = "kebab-case")]
pub enum NonStructuredControlFactSource {
    StronglyConnectedComponent,
    ControlRegionResidual(ControlRegionResidualKey),
    ControlFlowCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonStructuredControlFact {
    key: NonStructuredControlFactKey,
    classification: NonStructuredControlClassification,
    source: NonStructuredControlFactSource,
    points: Vec<ControlPointKey>,
    entry_points: Vec<ControlPointKey>,
    exit_points: Vec<ControlPointKey>,
}

impl NonStructuredControlFact {
    pub fn key(&self) -> &NonStructuredControlFactKey {
        &self.key
    }

    pub fn classification(&self) -> NonStructuredControlClassification {
        self.classification
    }

    pub fn source(&self) -> &NonStructuredControlFactSource {
        &self.source
    }

    pub fn points(&self) -> &[ControlPointKey] {
        &self.points
    }

    pub fn entry_points(&self) -> &[ControlPointKey] {
        &self.entry_points
    }

    pub fn exit_points(&self) -> &[ControlPointKey] {
        &self.exit_points
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonStructuredControlCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

impl NonStructuredControlCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    fn validate(&self) -> Result<(), NonStructuredControlBuildError> {
        validate_canonical_strings("non-structured control coverage reasons", &self.reasons)?;
        match (self.status, self.reasons.is_empty()) {
            (FactCoverage::Complete, true) => Ok(()),
            (FactCoverage::Complete, false) => Err(NonStructuredControlBuildError::Invalid(
                "complete non-structured control coverage cannot carry uncertainty reasons".into(),
            )),
            (_, false) => Ok(()),
            (_, true) => Err(NonStructuredControlBuildError::Invalid(
                "incomplete non-structured control coverage requires an exact reason".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonStructuredControlGraph {
    key: NonStructuredControlGraphKey,
    control_region_graph: ControlRegionGraphKey,
    control_flow_graph: ControlFlowGraphKey,
    owner: NodeKey,
    coverage: NonStructuredControlCoverageEvidence,
    facts: Vec<NonStructuredControlFact>,
}

impl NonStructuredControlGraph {
    pub fn key(&self) -> &NonStructuredControlGraphKey {
        &self.key
    }

    pub fn control_region_graph(&self) -> &ControlRegionGraphKey {
        &self.control_region_graph
    }

    pub fn control_flow_graph(&self) -> &ControlFlowGraphKey {
        &self.control_flow_graph
    }

    pub fn owner(&self) -> &NodeKey {
        &self.owner
    }

    pub fn coverage(&self) -> &NonStructuredControlCoverageEvidence {
        &self.coverage
    }

    pub fn facts(&self) -> &[NonStructuredControlFact] {
        &self.facts
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NonStructuredControlDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_region_projection_id: ProjectionId,
    control_region_policy: ControlRegionPolicyId,
    control_flow_projection_id: ProjectionId,
    control_flow_policy: ControlFlowPolicyId,
    policy: NonStructuredControlPolicyId,
    graphs: Vec<NonStructuredControlGraph>,
}

impl NonStructuredControlDocument {
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

    pub fn control_region_policy(&self) -> &ControlRegionPolicyId {
        &self.control_region_policy
    }

    pub fn control_flow_projection_id(&self) -> &ProjectionId {
        &self.control_flow_projection_id
    }

    pub fn control_flow_policy(&self) -> &ControlFlowPolicyId {
        &self.control_flow_policy
    }

    pub fn policy(&self) -> &NonStructuredControlPolicyId {
        &self.policy
    }

    pub fn graphs(&self) -> &[NonStructuredControlGraph] {
        &self.graphs
    }

    fn validate(&self) -> Result<(), NonStructuredControlBuildError> {
        if self.schema != NON_STRUCTURED_CONTROL_SCHEMA {
            return Err(NonStructuredControlBuildError::Invalid(format!(
                "unsupported non-structured control schema {}",
                self.schema
            )));
        }
        validate_digest_id(self.projection_id.as_str(), "pj1_")?;
        validate_digest_id(&self.analysis_id, "pa1_")?;
        validate_digest_id(self.control_region_projection_id.as_str(), "pj1_")?;
        validate_digest_id(self.control_flow_projection_id.as_str(), "pj1_")?;
        if self.graphs.is_empty() {
            return Err(NonStructuredControlBuildError::Invalid(
                "non-structured control document cannot be empty".into(),
            ));
        }
        validate_sorted_unique_by_key("non-structured control graphs", &self.graphs, |graph| {
            graph.key.as_str()
        })?;
        let mut source_graphs = BTreeSet::new();
        for graph in &self.graphs {
            if !source_graphs.insert(graph.control_region_graph.clone()) {
                return Err(NonStructuredControlBuildError::Invalid(
                    "non-structured control document repeats a source graph".into(),
                ));
            }
            validate_graph(&self.policy, graph)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NonStructuredControlDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_region_projection_id: ProjectionId,
    control_region_policy: ControlRegionPolicyId,
    control_flow_projection_id: ProjectionId,
    control_flow_policy: ControlFlowPolicyId,
    policy: NonStructuredControlPolicyId,
    graphs: Vec<NonStructuredControlGraph>,
}

impl<'de> Deserialize<'de> for NonStructuredControlDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NonStructuredControlDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            control_region_projection_id: wire.control_region_projection_id,
            control_region_policy: wire.control_region_policy,
            control_flow_projection_id: wire.control_flow_projection_id,
            control_flow_policy: wire.control_flow_policy,
            policy: wire.policy,
            graphs: wire.graphs,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct NonStructuredControlProjection {
    id: ProjectionId,
    control_regions: Arc<ControlRegionProjection>,
    policy: NonStructuredControlPolicyId,
    document: NonStructuredControlDocument,
}

impl NonStructuredControlProjection {
    pub fn schema(&self) -> &'static str {
        NON_STRUCTURED_CONTROL_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn control_regions(&self) -> &Arc<ControlRegionProjection> {
        &self.control_regions
    }

    pub fn policy(&self) -> &NonStructuredControlPolicyId {
        &self.policy
    }

    pub fn document(&self) -> &NonStructuredControlDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonStructuredControlBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for NonStructuredControlBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => {
                write!(
                    formatter,
                    "invalid non-structured control evidence: {detail}"
                )
            }
            Self::Identity(detail) => {
                write!(formatter, "non-structured control identity error: {detail}")
            }
        }
    }
}

impl std::error::Error for NonStructuredControlBuildError {}

#[derive(Debug)]
struct IndexedGraph {
    keys: Vec<ControlPointKey>,
    successors: Vec<BTreeSet<usize>>,
    predecessors: Vec<BTreeSet<usize>>,
    entry: usize,
}

impl IndexedGraph {
    fn new(graph: &ControlFlowGraph) -> Result<Self, NonStructuredControlBuildError> {
        let keys = graph
            .points()
            .iter()
            .map(|point| point.key().clone())
            .collect::<Vec<_>>();
        let index = keys
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect::<BTreeMap<_, _>>();
        if index.len() != keys.len() {
            return Err(NonStructuredControlBuildError::Invalid(
                "source graph repeats a point key".into(),
            ));
        }
        let entry = *index.get(graph.entry()).ok_or_else(|| {
            NonStructuredControlBuildError::Invalid("source entry point is missing".into())
        })?;
        let mut successors = vec![BTreeSet::new(); keys.len()];
        let mut predecessors = vec![BTreeSet::new(); keys.len()];
        for edge in graph.edges() {
            let from = *index.get(edge.from()).ok_or_else(|| {
                NonStructuredControlBuildError::Invalid("source edge has a dangling origin".into())
            })?;
            let to = *index.get(edge.to()).ok_or_else(|| {
                NonStructuredControlBuildError::Invalid("source edge has a dangling target".into())
            })?;
            successors[from].insert(to);
            predecessors[to].insert(from);
        }
        Ok(Self {
            keys,
            successors,
            predecessors,
            entry,
        })
    }
}

pub fn derive_non_structured_control_regions(
    control_regions: Arc<ControlRegionProjection>,
    policy: NonStructuredControlPolicyId,
) -> Result<NonStructuredControlProjection, NonStructuredControlBuildError> {
    let control_flow = control_regions.control_flow();
    let mut graphs = Vec::with_capacity(control_regions.document().graphs().len());
    for region_graph in control_regions.document().graphs() {
        let flow_graph = control_flow
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == region_graph.control_flow_graph())
            .ok_or_else(|| {
                NonStructuredControlBuildError::Invalid(
                    "control-region graph references a missing control-flow graph".into(),
                )
            })?;
        graphs.push(derive_graph(region_graph, flow_graph, &policy)?);
    }
    graphs.sort_by(|left, right| left.key.cmp(&right.key));

    let payload = serde_json::to_vec(&(
        control_regions.id(),
        control_regions.policy(),
        control_flow.id(),
        control_flow.policy(),
        &policy,
        &graphs,
    ))
    .map_err(|error| NonStructuredControlBuildError::Identity(error.to_string()))?;
    let id = control_flow
        .analysis()
        .derive_projection_id(
            NON_STRUCTURED_CONTROL_SCHEMA,
            &payload,
            control_regions.id().as_str().as_bytes(),
        )
        .map_err(|error| NonStructuredControlBuildError::Identity(error.to_string()))?;
    let document = NonStructuredControlDocument {
        schema: NON_STRUCTURED_CONTROL_SCHEMA.into(),
        projection_id: id.clone(),
        analysis_id: control_flow.analysis().id().as_str().to_string(),
        control_region_projection_id: control_regions.id().clone(),
        control_region_policy: control_regions.policy().clone(),
        control_flow_projection_id: control_flow.id().clone(),
        control_flow_policy: control_flow.policy().clone(),
        policy: policy.clone(),
        graphs,
    };
    document.validate()?;
    Ok(NonStructuredControlProjection {
        id,
        control_regions,
        policy,
        document,
    })
}

fn derive_graph(
    region_graph: &ControlRegionGraph,
    flow_graph: &ControlFlowGraph,
    policy: &NonStructuredControlPolicyId,
) -> Result<NonStructuredControlGraph, NonStructuredControlBuildError> {
    if region_graph.control_flow_graph() != flow_graph.key()
        || region_graph.owner() != flow_graph.owner()
    {
        return Err(NonStructuredControlBuildError::Invalid(
            "control-region graph does not match its source control-flow graph".into(),
        ));
    }
    let indexed = IndexedGraph::new(flow_graph)?;
    let reachable = reachability(indexed.entry, &indexed.successors);
    let exit_reachable = region_graph
        .points()
        .iter()
        .map(|fact| (fact.point().clone(), fact.exit_reachable()))
        .collect::<BTreeMap<_, _>>();
    let mut facts = Vec::new();

    for component in strongly_connected_components(&indexed, &reachable) {
        let cyclic = component.len() > 1
            || component
                .iter()
                .any(|point| indexed.successors[*point].contains(point));
        if !cyclic {
            continue;
        }
        let entry_points = component
            .iter()
            .copied()
            .filter(|point| {
                indexed.predecessors[*point]
                    .iter()
                    .any(|source| !component.contains(source))
            })
            .collect::<BTreeSet<_>>();
        let exit_points = component
            .iter()
            .copied()
            .filter(|point| {
                indexed.successors[*point]
                    .iter()
                    .any(|target| !component.contains(target))
            })
            .collect::<BTreeSet<_>>();
        let points = relation_keys(&indexed.keys, &component);
        let entries = relation_keys(&indexed.keys, &entry_points);
        let exits = relation_keys(&indexed.keys, &exit_points);
        if entries.len() >= 2 {
            facts.push(build_fact(
                policy,
                region_graph.key(),
                NonStructuredControlClassification::IrreducibleMultiEntryCycle,
                NonStructuredControlFactSource::StronglyConnectedComponent,
                points.clone(),
                entries.clone(),
                exits.clone(),
            )?);
        }
        if component.iter().all(|point| {
            !exit_reachable
                .get(&indexed.keys[*point])
                .copied()
                .unwrap_or(false)
        }) {
            facts.push(build_fact(
                policy,
                region_graph.key(),
                NonStructuredControlClassification::NonTerminatingCycle,
                NonStructuredControlFactSource::StronglyConnectedComponent,
                points,
                entries,
                exits,
            )?);
        }
    }

    for residual in region_graph.residuals() {
        facts.push(fact_from_residual(policy, region_graph.key(), residual)?);
    }
    if flow_graph.coverage().status() != FactCoverage::Complete {
        facts.push(build_fact(
            policy,
            region_graph.key(),
            NonStructuredControlClassification::UnknownIncompleteControlFlow,
            NonStructuredControlFactSource::ControlFlowCoverage,
            flow_graph
                .points()
                .iter()
                .map(|point| point.key().clone())
                .collect(),
            vec![flow_graph.entry().clone()],
            vec![flow_graph.exit().clone()],
        )?);
    }
    facts.sort_by(|left, right| left.key.cmp(&right.key));
    let coverage = NonStructuredControlCoverageEvidence {
        status: region_graph.coverage().status(),
        reasons: region_graph.coverage().reasons().to_vec(),
    };
    let mut graph = NonStructuredControlGraph {
        key: NonStructuredControlGraphKey(String::new()),
        control_region_graph: region_graph.key().clone(),
        control_flow_graph: flow_graph.key().clone(),
        owner: flow_graph.owner().clone(),
        coverage,
        facts,
    };
    graph.key = derive_graph_key(policy, &graph)?;
    validate_graph(policy, &graph)?;
    validate_source_closure(&graph, region_graph, flow_graph)?;
    Ok(graph)
}

fn fact_from_residual(
    policy: &NonStructuredControlPolicyId,
    graph: &ControlRegionGraphKey,
    residual: &ControlRegionResidual,
) -> Result<NonStructuredControlFact, NonStructuredControlBuildError> {
    let classification = match residual.reason() {
        "candidate lacks distinct entry, exit, or interior" => {
            NonStructuredControlClassification::InvalidCandidateBoundary
        }
        "candidate has an incoming edge that bypasses its entry" => {
            NonStructuredControlClassification::IncomingBoundaryBypass
        }
        "candidate has an outgoing edge that bypasses its exit" => {
            NonStructuredControlClassification::OutgoingBoundaryBypass
        }
        "candidate overlaps another region without containment" => {
            NonStructuredControlClassification::CrossingCandidates
        }
        "terminating core has no valid root region" => {
            NonStructuredControlClassification::MissingStructuredRoot
        }
        reason => {
            return Err(NonStructuredControlBuildError::Invalid(format!(
                "unclassified control-region residual reason {reason:?}"
            )));
        }
    };
    build_fact(
        policy,
        graph,
        classification,
        NonStructuredControlFactSource::ControlRegionResidual(residual.key().clone()),
        residual.points().to_vec(),
        vec![residual.entry().clone()],
        vec![residual.exit().clone()],
    )
}

fn build_fact(
    policy: &NonStructuredControlPolicyId,
    graph: &ControlRegionGraphKey,
    classification: NonStructuredControlClassification,
    source: NonStructuredControlFactSource,
    mut points: Vec<ControlPointKey>,
    mut entry_points: Vec<ControlPointKey>,
    mut exit_points: Vec<ControlPointKey>,
) -> Result<NonStructuredControlFact, NonStructuredControlBuildError> {
    points.sort();
    points.dedup();
    entry_points.sort();
    entry_points.dedup();
    exit_points.sort();
    exit_points.dedup();
    let mut fact = NonStructuredControlFact {
        key: NonStructuredControlFactKey(String::new()),
        classification,
        source,
        points,
        entry_points,
        exit_points,
    };
    fact.key = derive_fact_key(policy, graph, &fact)?;
    validate_fact(policy, graph, &fact)?;
    Ok(fact)
}

fn reachability(root: usize, adjacency: &[BTreeSet<usize>]) -> BTreeSet<usize> {
    let mut reached = BTreeSet::new();
    let mut queue = VecDeque::from([root]);
    while let Some(point) = queue.pop_front() {
        if !reached.insert(point) {
            continue;
        }
        queue.extend(adjacency[point].iter().copied());
    }
    reached
}

fn strongly_connected_components(
    indexed: &IndexedGraph,
    domain: &BTreeSet<usize>,
) -> Vec<BTreeSet<usize>> {
    let mut visited = BTreeSet::new();
    let mut order = Vec::with_capacity(domain.len());
    for start in domain {
        if !visited.insert(*start) {
            continue;
        }
        let mut stack = vec![(
            *start,
            indexed.successors[*start]
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            0,
        )];
        while let Some((point, targets, next)) = stack.last_mut() {
            if *next < targets.len() {
                let target = targets[*next];
                *next += 1;
                if domain.contains(&target) && visited.insert(target) {
                    stack.push((
                        target,
                        indexed.successors[target].iter().copied().collect(),
                        0,
                    ));
                }
            } else {
                order.push(*point);
                stack.pop();
            }
        }
    }

    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    for start in order.into_iter().rev() {
        if !assigned.insert(start) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(point) = stack.pop() {
            component.insert(point);
            for source in indexed.predecessors[point].iter().rev() {
                if domain.contains(source) && assigned.insert(*source) {
                    stack.push(*source);
                }
            }
        }
        components.push(component);
    }
    components.sort_by(|left, right| left.iter().next().cmp(&right.iter().next()));
    components
}

fn relation_keys(keys: &[ControlPointKey], relation: &BTreeSet<usize>) -> Vec<ControlPointKey> {
    let mut values = relation
        .iter()
        .map(|point| keys[*point].clone())
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn validate_graph(
    policy: &NonStructuredControlPolicyId,
    graph: &NonStructuredControlGraph,
) -> Result<(), NonStructuredControlBuildError> {
    graph.coverage.validate()?;
    validate_sorted_unique_by_key("non-structured control facts", &graph.facts, |fact| {
        fact.key.as_str()
    })?;
    for fact in &graph.facts {
        validate_fact(policy, &graph.control_region_graph, fact)?;
    }
    if derive_graph_key(policy, graph)? != graph.key {
        return Err(NonStructuredControlBuildError::Invalid(
            "non-structured control graph key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn validate_fact(
    policy: &NonStructuredControlPolicyId,
    graph: &ControlRegionGraphKey,
    fact: &NonStructuredControlFact,
) -> Result<(), NonStructuredControlBuildError> {
    validate_canonical_distinct_keys("non-structured fact points", &fact.points)?;
    validate_canonical_distinct_keys("non-structured fact entry points", &fact.entry_points)?;
    validate_canonical_distinct_keys("non-structured fact exit points", &fact.exit_points)?;
    if fact.points.is_empty()
        || fact
            .entry_points
            .iter()
            .chain(fact.exit_points.iter())
            .any(|point| fact.points.binary_search(point).is_err())
    {
        return Err(NonStructuredControlBuildError::Invalid(
            "non-structured fact has invalid point closure".into(),
        ));
    }
    match (&fact.source, fact.classification) {
        (NonStructuredControlFactSource::StronglyConnectedComponent, classification)
            if classification.is_scc() => {}
        (NonStructuredControlFactSource::ControlRegionResidual(_), classification)
            if !classification.is_scc()
                && classification
                    != NonStructuredControlClassification::UnknownIncompleteControlFlow => {}
        (
            NonStructuredControlFactSource::ControlFlowCoverage,
            NonStructuredControlClassification::UnknownIncompleteControlFlow,
        ) => {}
        _ => {
            return Err(NonStructuredControlBuildError::Invalid(
                "non-structured fact classification disagrees with its source".into(),
            ));
        }
    }
    if fact.classification == NonStructuredControlClassification::IrreducibleMultiEntryCycle
        && fact.entry_points.len() < 2
    {
        return Err(NonStructuredControlBuildError::Invalid(
            "irreducible cycle requires at least two distinct entry points".into(),
        ));
    }
    if fact.classification.is_scc() && fact.entry_points.is_empty() {
        return Err(NonStructuredControlBuildError::Invalid(
            "entry-reachable cyclic component requires an external entry point".into(),
        ));
    }
    if matches!(
        &fact.source,
        NonStructuredControlFactSource::ControlRegionResidual(_)
    ) && (fact.points.len() < 3
        || fact.entry_points.len() != 1
        || fact.exit_points.len() != 1
        || fact.entry_points[0] == fact.exit_points[0])
    {
        return Err(NonStructuredControlBuildError::Invalid(
            "residual-derived fact has invalid boundaries".into(),
        ));
    }
    if fact.classification == NonStructuredControlClassification::UnknownIncompleteControlFlow
        && (fact.entry_points.len() != 1
            || fact.exit_points.len() != 1
            || fact.entry_points[0] == fact.exit_points[0])
    {
        return Err(NonStructuredControlBuildError::Invalid(
            "unknown control-flow fact requires exact virtual boundaries".into(),
        ));
    }
    if derive_fact_key(policy, graph, fact)? != fact.key {
        return Err(NonStructuredControlBuildError::Invalid(
            "non-structured control fact key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn validate_source_closure(
    graph: &NonStructuredControlGraph,
    region_graph: &ControlRegionGraph,
    flow_graph: &ControlFlowGraph,
) -> Result<(), NonStructuredControlBuildError> {
    let points = flow_graph
        .points()
        .iter()
        .map(|point| point.key())
        .collect::<BTreeSet<_>>();
    let residuals = region_graph
        .residuals()
        .iter()
        .map(|residual| residual.key())
        .collect::<BTreeSet<_>>();
    let relation_facts = region_graph
        .points()
        .iter()
        .map(|fact| (fact.point(), fact))
        .collect::<BTreeMap<_, _>>();
    for fact in &graph.facts {
        if fact.points.iter().any(|point| !points.contains(point)) {
            return Err(NonStructuredControlBuildError::Invalid(
                "non-structured fact references another source graph".into(),
            ));
        }
        match &fact.source {
            NonStructuredControlFactSource::ControlRegionResidual(key)
                if !residuals.contains(key) =>
            {
                return Err(NonStructuredControlBuildError::Invalid(
                    "non-structured fact references a missing source residual".into(),
                ));
            }
            NonStructuredControlFactSource::StronglyConnectedComponent
                if fact.classification
                    == NonStructuredControlClassification::NonTerminatingCycle
                    && fact.points.iter().any(|point| {
                        relation_facts
                            .get(point)
                            .is_none_or(|relation| relation.exit_reachable())
                    }) =>
            {
                return Err(NonStructuredControlBuildError::Invalid(
                    "nonterminating fact contains an exit-reachable point".into(),
                ));
            }
            NonStructuredControlFactSource::ControlFlowCoverage
                if flow_graph.coverage().status() == FactCoverage::Complete =>
            {
                return Err(NonStructuredControlBuildError::Invalid(
                    "unknown control-flow fact has Complete source coverage".into(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn derive_graph_key(
    policy: &NonStructuredControlPolicyId,
    graph: &NonStructuredControlGraph,
) -> Result<NonStructuredControlGraphKey, NonStructuredControlBuildError> {
    let payload = serde_json::to_vec(&(
        &graph.control_region_graph,
        &graph.control_flow_graph,
        &graph.owner,
        &graph.coverage,
        &graph.facts,
    ))
    .map_err(|error| NonStructuredControlBuildError::Identity(error.to_string()))?;
    Ok(NonStructuredControlGraphKey(derive_parts_id(
        GRAPH_KEY_DOMAIN,
        "nsg1_",
        &[policy.as_str().as_bytes(), &payload],
    )))
}

fn derive_fact_key(
    policy: &NonStructuredControlPolicyId,
    graph: &ControlRegionGraphKey,
    fact: &NonStructuredControlFact,
) -> Result<NonStructuredControlFactKey, NonStructuredControlBuildError> {
    let payload = serde_json::to_vec(&(
        fact.classification,
        &fact.source,
        &fact.points,
        &fact.entry_points,
        &fact.exit_points,
    ))
    .map_err(|error| NonStructuredControlBuildError::Identity(error.to_string()))?;
    Ok(NonStructuredControlFactKey(derive_parts_id(
        FACT_KEY_DOMAIN,
        "nsf1_",
        &[
            policy.as_str().as_bytes(),
            graph.as_str().as_bytes(),
            &payload,
        ],
    )))
}

fn validate_sorted_unique_by_key<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), NonStructuredControlBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        return Err(NonStructuredControlBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )));
    }
    Ok(())
}

fn validate_canonical_distinct_keys<T: Ord>(
    label: &str,
    values: &[T],
) -> Result<(), NonStructuredControlBuildError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(NonStructuredControlBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )));
    }
    Ok(())
}

fn validate_canonical_strings(
    label: &str,
    values: &[String],
) -> Result<(), NonStructuredControlBuildError> {
    for value in values {
        if value.trim().is_empty() || value.trim() != value {
            return Err(NonStructuredControlBuildError::Invalid(format!(
                "{label} must contain nonempty canonical text"
            )));
        }
    }
    validate_canonical_distinct_keys(label, values)
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), NonStructuredControlBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(NonStructuredControlBuildError::Invalid(format!(
            "identity must start with {prefix}"
        )));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(NonStructuredControlBuildError::Invalid(
            "identity must contain a canonical 32-byte hexadecimal digest".into(),
        ));
    }
    Ok(())
}

fn derive_parts_id(domain: &str, prefix: &str, parts: &[&[u8]]) -> String {
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
    use deslop_lang::Registry;
    use serde_json::Value;

    use super::*;
    use crate::{
        ControlEdgeDraft, ControlEdgeKind, ControlEdgePrecision, ControlExitOutcome,
        ControlFlowBuilder, ControlFlowCoverageEvidence, ControlFlowGraphDraft,
        ControlFlowOwnerKind, ControlFlowPolicyId, ControlLoopKind, ControlPointDraft,
        ControlPointKind, ControlSyntheticPointKind, ProjectAnalysis, ProjectSnapshotBuilder,
        RepositoryId, derive_control_regions, lower_control_flow,
    };

    type JsonMutation = (&'static str, Box<dyn Fn(&mut Value)>);

    fn analysis(source: &str) -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("non-structured-control-test").unwrap(),
        )
        .unwrap()
        .with_registry(Registry::default())
        .with_overlay("flow.rs", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn owner(analysis: &ProjectAnalysis) -> crate::NodeId {
        analysis
            .node_ids()
            .find(|node| analysis.node(*node).unwrap().raw_kind() == "function_item")
            .unwrap()
    }

    fn point(
        kind: ControlPointKind,
        source: Option<crate::NodeId>,
        ordinal: u32,
    ) -> ControlPointDraft {
        ControlPointDraft {
            kind,
            source,
            ordinal,
        }
    }

    fn edge(
        from: usize,
        to: usize,
        kind: ControlEdgeKind,
        source: crate::NodeId,
    ) -> ControlEdgeDraft {
        ControlEdgeDraft {
            from,
            to,
            kind,
            source,
            predicate: None,
            precision: ControlEdgePrecision::Exact,
        }
    }

    fn custom_flow(
        analysis: Arc<ProjectAnalysis>,
        flow_policy: &[u8],
        points: Vec<ControlPointDraft>,
        edges: Vec<ControlEdgeDraft>,
    ) -> Arc<crate::ControlFlowProjection> {
        let owner = owner(&analysis);
        let mut builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[flow_policy]).unwrap(),
        );
        builder
            .add_graph(ControlFlowGraphDraft {
                owner,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points,
                edges,
            })
            .unwrap();
        Arc::new(builder.build().unwrap())
    }

    fn multi_entry_flow_with_exit(
        flow_policy: &[u8],
        terminating_exit: bool,
    ) -> Arc<crate::ControlFlowProjection> {
        let analysis = analysis("fn run() {}\n");
        let owner = owner(&analysis);
        let points = vec![
            point(ControlPointKind::Entry, None, 0),
            point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch),
                Some(owner),
                0,
            ),
            point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::NoOp),
                Some(owner),
                0,
            ),
            point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::NoOp),
                Some(owner),
                1,
            ),
            point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::LoopHeader),
                Some(owner),
                0,
            ),
            point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                Some(owner),
                0,
            ),
            point(ControlPointKind::Exit, None, 0),
        ];
        let mut edges = vec![
            edge(0, 1, ControlEdgeKind::Entry, owner),
            edge(1, 2, ControlEdgeKind::Normal, owner),
            edge(1, 3, ControlEdgeKind::Normal, owner),
            edge(2, 4, ControlEdgeKind::Normal, owner),
            edge(3, 4, ControlEdgeKind::Normal, owner),
            edge(4, 2, ControlEdgeKind::Loop(ControlLoopKind::Back), owner),
            edge(4, 3, ControlEdgeKind::Loop(ControlLoopKind::Body), owner),
            edge(
                5,
                6,
                ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                owner,
            ),
        ];
        if terminating_exit {
            edges.push(edge(
                4,
                5,
                ControlEdgeKind::Loop(ControlLoopKind::ConditionFalse),
                owner,
            ));
        }
        let mut builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[flow_policy]).unwrap(),
        );
        builder
            .add_graph(ControlFlowGraphDraft {
                owner,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points,
                edges,
            })
            .unwrap();
        Arc::new(builder.build().unwrap())
    }

    fn multi_entry_flow(flow_policy: &[u8]) -> Arc<crate::ControlFlowProjection> {
        multi_entry_flow_with_exit(flow_policy, true)
    }

    fn reducible_loop_flow() -> Arc<crate::ControlFlowProjection> {
        let analysis = analysis("fn run() {}\n");
        let owner = owner(&analysis);
        custom_flow(
            analysis,
            b"non-structured-reducible-flow/1",
            vec![
                point(ControlPointKind::Entry, None, 0),
                point(
                    ControlPointKind::Synthetic(ControlSyntheticPointKind::LoopHeader),
                    Some(owner),
                    0,
                ),
                point(
                    ControlPointKind::Synthetic(ControlSyntheticPointKind::NoOp),
                    Some(owner),
                    0,
                ),
                point(
                    ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                    Some(owner),
                    0,
                ),
                point(ControlPointKind::Exit, None, 0),
            ],
            vec![
                edge(0, 1, ControlEdgeKind::Entry, owner),
                edge(1, 2, ControlEdgeKind::Loop(ControlLoopKind::Body), owner),
                edge(2, 1, ControlEdgeKind::Loop(ControlLoopKind::Back), owner),
                edge(
                    1,
                    3,
                    ControlEdgeKind::Loop(ControlLoopKind::ConditionFalse),
                    owner,
                ),
                edge(
                    3,
                    4,
                    ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                    owner,
                ),
            ],
        )
    }

    fn region_projection(flow: Arc<crate::ControlFlowProjection>) -> Arc<ControlRegionProjection> {
        Arc::new(
            derive_control_regions(
                flow,
                ControlRegionPolicyId::from_parts(&[b"non-structured-region-policy/1"]).unwrap(),
            )
            .unwrap(),
        )
    }

    fn projection(
        regions: Arc<ControlRegionProjection>,
        policy: &[u8],
    ) -> NonStructuredControlProjection {
        derive_non_structured_control_regions(
            regions,
            NonStructuredControlPolicyId::from_parts(&[policy]).unwrap(),
        )
        .unwrap()
    }

    fn production_projection(source: &str) -> NonStructuredControlProjection {
        let analysis = analysis(source);
        let lowered = lower_control_flow(
            analysis,
            ControlFlowPolicyId::from_parts(&[b"non-structured-production-flow/1"]).unwrap(),
        )
        .unwrap();
        projection(
            region_projection(Arc::new(lowered.projection().unwrap().clone())),
            b"non-structured-policy/1",
        )
    }

    #[test]
    fn m4_4_multi_entry_scc_is_irreducible_but_never_a_structured_region() {
        let regions = region_projection(multi_entry_flow(b"non-structured-multi-entry-flow/1"));
        let projection = projection(Arc::clone(&regions), b"non-structured-policy/1");
        assert_eq!(projection.schema(), NON_STRUCTURED_CONTROL_SCHEMA);
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.coverage().reasons().is_empty());
        assert_eq!(graph.facts().len(), 1, "{:#?}", graph.facts());
        let fact = &graph.facts()[0];
        assert_eq!(
            fact.classification(),
            NonStructuredControlClassification::IrreducibleMultiEntryCycle
        );
        assert_eq!(
            fact.source(),
            &NonStructuredControlFactSource::StronglyConnectedComponent
        );
        assert_eq!(fact.points().len(), 3);
        assert_eq!(fact.entry_points().len(), 2);
        assert_eq!(fact.exit_points().len(), 1);
        assert!(
            regions.document().graphs()[0]
                .regions()
                .iter()
                .all(|region| region.points() != fact.points())
        );
        assert_eq!(
            projection.document().control_region_projection_id(),
            regions.id()
        );
        assert_eq!(
            projection.document().control_flow_projection_id(),
            regions.control_flow().id()
        );
    }

    #[test]
    fn m4_4_one_entry_reducible_loop_is_not_classified_irreducible() {
        let projection = projection(
            region_projection(reducible_loop_flow()),
            b"non-structured-policy/1",
        );
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.facts().is_empty(), "{:#?}", graph.facts());
    }

    #[test]
    fn m4_4_exit_unreachable_cycle_is_preserved_as_nonterminating() {
        let projection = production_projection("fn run() { loop {} }\n");
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        let facts = graph
            .facts()
            .iter()
            .filter(|fact| {
                fact.classification() == NonStructuredControlClassification::NonTerminatingCycle
            })
            .collect::<Vec<_>>();
        assert_eq!(facts.len(), 1, "{:#?}", graph.facts());
        assert!(!facts[0].points().is_empty());
        assert!(facts[0].exit_points().is_empty());
        let relations = projection.control_regions().document().graphs()[0]
            .points()
            .iter()
            .map(|fact| (fact.point(), fact.exit_reachable()))
            .collect::<BTreeMap<_, _>>();
        assert!(facts[0].points().iter().all(|point| !relations[point]));
    }

    #[test]
    fn m4_4_irreducibility_and_nontermination_are_independent_facts() {
        let projection = projection(
            region_projection(multi_entry_flow_with_exit(
                b"non-structured-multi-entry-nonterminating-flow/1",
                false,
            )),
            b"non-structured-policy/1",
        );
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert_eq!(graph.facts().len(), 2, "{:#?}", graph.facts());
        let classifications = graph
            .facts()
            .iter()
            .map(NonStructuredControlFact::classification)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            classifications,
            BTreeSet::from([
                NonStructuredControlClassification::IrreducibleMultiEntryCycle,
                NonStructuredControlClassification::NonTerminatingCycle,
            ])
        );
        assert_eq!(graph.facts()[0].points(), graph.facts()[1].points());
        assert!(
            graph
                .facts()
                .iter()
                .all(|fact| fact.entry_points().len() == 2)
        );
        assert!(
            graph
                .facts()
                .iter()
                .all(|fact| fact.exit_points().is_empty())
        );
    }

    #[test]
    fn m4_4_m4_3_residual_is_typed_and_source_bound() {
        let projection =
            production_projection("fn run(x: bool) { if x { loop {} } else { 1; } }\n");
        let graph = &projection.document().graphs()[0];
        let residual = projection.control_regions().document().graphs()[0]
            .residuals()
            .first()
            .unwrap();
        let fact = graph
            .facts()
            .iter()
            .find(|fact| {
                fact.classification() == NonStructuredControlClassification::OutgoingBoundaryBypass
            })
            .expect("typed outgoing-boundary residual");
        assert_eq!(
            fact.source(),
            &NonStructuredControlFactSource::ControlRegionResidual(residual.key().clone())
        );
        assert_eq!(fact.points(), residual.points());
        assert_eq!(fact.entry_points(), [residual.entry().clone()]);
        assert_eq!(fact.exit_points(), [residual.exit().clone()]);
    }

    #[test]
    fn m4_4_partial_source_flow_emits_explicit_unknown_fact() {
        let projection = production_projection("fn run() { println!(\"x\"); }\n");
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        let unknown = graph
            .facts()
            .iter()
            .find(|fact| {
                fact.classification()
                    == NonStructuredControlClassification::UnknownIncompleteControlFlow
            })
            .expect("explicit unknown control-flow fact");
        assert_eq!(
            unknown.source(),
            &NonStructuredControlFactSource::ControlFlowCoverage
        );
        assert_eq!(
            unknown.points().len(),
            projection
                .control_regions()
                .control_flow()
                .document()
                .graphs()[0]
                .points()
                .len()
        );
        assert_eq!(
            graph.coverage().reasons(),
            ["Rust macro expansion is unavailable"]
        );
    }

    #[test]
    fn m4_4_document_rejects_fact_boundary_key_and_unknown_field_corruption() {
        let projection = projection(
            region_projection(multi_entry_flow(b"non-structured-corruption-flow/1")),
            b"non-structured-policy/1",
        );
        let original = serde_json::to_value(projection.document()).unwrap();
        let mutations: Vec<JsonMutation> = vec![
            (
                "fact-key",
                Box::new(|value| {
                    value["graphs"][0]["facts"][0]["key"] =
                        format!("nsf1_{}", "0".repeat(64)).into()
                }),
            ),
            (
                "single-entry-irreducible",
                Box::new(|value| {
                    value["graphs"][0]["facts"][0]["entry_points"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }),
            ),
            (
                "noncanonical-points",
                Box::new(|value| {
                    value["graphs"][0]["facts"][0]["points"]
                        .as_array_mut()
                        .unwrap()
                        .swap(0, 1);
                }),
            ),
            (
                "graph-key",
                Box::new(|value| {
                    value["graphs"][0]["key"] = format!("nsg1_{}", "0".repeat(64)).into()
                }),
            ),
            (
                "unknown-field",
                Box::new(|value| value["unexpected"] = true.into()),
            ),
        ];
        for (label, mutate) in mutations {
            let mut value = original.clone();
            mutate(&mut value);
            assert!(
                serde_json::from_value::<NonStructuredControlDocument>(value).is_err(),
                "mutation {label} was accepted"
            );
        }
    }

    #[test]
    fn m4_4_projection_is_deterministic_and_policy_source_bound() {
        let first_regions = region_projection(multi_entry_flow(b"non-structured-identity-flow/1"));
        let first = projection(Arc::clone(&first_regions), b"non-structured-policy/1");
        let repeated = projection(first_regions, b"non-structured-policy/1");
        assert_eq!(first.id(), repeated.id());
        assert_eq!(
            serde_json::to_vec(first.document()).unwrap(),
            serde_json::to_vec(repeated.document()).unwrap()
        );

        let changed_policy = projection(
            region_projection(multi_entry_flow(b"non-structured-identity-flow/1")),
            b"non-structured-policy/2",
        );
        let changed_source = projection(
            region_projection(multi_entry_flow(b"non-structured-identity-flow/2")),
            b"non-structured-policy/1",
        );
        assert_ne!(first.id(), changed_policy.id());
        assert_ne!(
            first.document().graphs()[0].key(),
            changed_policy.document().graphs()[0].key()
        );
        assert_ne!(first.id(), changed_source.id());
        assert_ne!(
            first.document().graphs()[0].control_flow_graph(),
            changed_source.document().graphs()[0].control_flow_graph()
        );

        let bytes = serde_json::to_vec(first.document()).unwrap();
        let decoded: NonStructuredControlDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
        assert_eq!(
            decoded.analysis_id(),
            first
                .control_regions()
                .control_flow()
                .analysis()
                .id()
                .as_str()
        );
    }
}
