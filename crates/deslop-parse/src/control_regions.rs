use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ControlFlowGraph, ControlFlowGraphKey, ControlFlowPolicyId, ControlFlowProjection,
    ControlPointKey, ControlPointKind, ControlSyntheticPointKind, FactCoverage, NodeKey,
    ProjectionId,
};

pub const CONTROL_REGION_SCHEMA: &str = "deslop.control-regions/1";
pub const CONTROL_REGION_POLICY_SCHEMA: &str = "deslop.control-region-policy/1";

const POLICY_ID_DOMAIN: &str = "deslop control-region policy v1";
const GRAPH_KEY_DOMAIN: &str = "deslop control-region graph key v1";
const POINT_KEY_DOMAIN: &str = "deslop control-region point key v1";
const REGION_KEY_DOMAIN: &str = "deslop control-region key v1";
const RESIDUAL_KEY_DOMAIN: &str = "deslop control-region residual key v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ControlRegionPolicyId(String);

impl ControlRegionPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ControlRegionBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(ControlRegionBuildError::Invalid(
                "control-region policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_parts_id(POLICY_ID_DOMAIN, "crp1_", parts)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ControlRegionPolicyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "crp1_").map_err(D::Error::custom)?;
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

digest_key!(ControlRegionGraphKey, "crg1_");
digest_key!(ControlRegionPointKey, "crn1_");
digest_key!(ControlRegionKey, "cre1_");
digest_key!(ControlRegionResidualKey, "crx1_");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRegionCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

impl ControlRegionCoverageEvidence {
    pub fn complete() -> Self {
        Self {
            status: FactCoverage::Complete,
            reasons: Vec::new(),
        }
    }

    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    fn from_source(
        status: FactCoverage,
        source_reasons: &[String],
        additional: impl IntoIterator<Item = String>,
    ) -> Result<Self, ControlRegionBuildError> {
        let mut reasons = source_reasons.to_vec();
        reasons.extend(additional);
        reasons.sort();
        reasons.dedup();
        let status = if reasons.is_empty() {
            status
        } else {
            match status {
                FactCoverage::Complete => FactCoverage::Partial,
                other => other,
            }
        };
        let evidence = Self { status, reasons };
        evidence.validate()?;
        Ok(evidence)
    }

    fn validate(&self) -> Result<(), ControlRegionBuildError> {
        validate_canonical_strings("control-region coverage reasons", &self.reasons)?;
        match (self.status, self.reasons.is_empty()) {
            (FactCoverage::Complete, true) => Ok(()),
            (FactCoverage::Complete, false) => Err(ControlRegionBuildError::Invalid(
                "complete control-region coverage cannot carry uncertainty reasons".into(),
            )),
            (_, false) => Ok(()),
            (_, true) => Err(ControlRegionBuildError::Invalid(
                "incomplete control-region coverage requires an exact reason".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlPointRelations {
    key: ControlRegionPointKey,
    point: ControlPointKey,
    reachable: bool,
    exit_reachable: bool,
    dominators: Vec<ControlPointKey>,
    immediate_dominator: Option<ControlPointKey>,
    dominator_depth: Option<u32>,
    post_dominators: Vec<ControlPointKey>,
    immediate_post_dominator: Option<ControlPointKey>,
    post_dominator_depth: Option<u32>,
}

impl ControlPointRelations {
    pub fn key(&self) -> &ControlRegionPointKey {
        &self.key
    }

    pub fn point(&self) -> &ControlPointKey {
        &self.point
    }

    pub fn reachable(&self) -> bool {
        self.reachable
    }

    pub fn exit_reachable(&self) -> bool {
        self.exit_reachable
    }

    pub fn dominators(&self) -> &[ControlPointKey] {
        &self.dominators
    }

    pub fn immediate_dominator(&self) -> Option<&ControlPointKey> {
        self.immediate_dominator.as_ref()
    }

    pub fn dominator_depth(&self) -> Option<u32> {
        self.dominator_depth
    }

    pub fn post_dominators(&self) -> &[ControlPointKey] {
        &self.post_dominators
    }

    pub fn immediate_post_dominator(&self) -> Option<&ControlPointKey> {
        self.immediate_post_dominator.as_ref()
    }

    pub fn post_dominator_depth(&self) -> Option<u32> {
        self.post_dominator_depth
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructuredControlRegionKind {
    Root,
    Branch,
    Loop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredControlRegion {
    key: ControlRegionKey,
    kind: StructuredControlRegionKind,
    entry: ControlPointKey,
    exit: ControlPointKey,
    points: Vec<ControlPointKey>,
    parent: Option<ControlRegionKey>,
    children: Vec<ControlRegionKey>,
}

impl StructuredControlRegion {
    pub fn key(&self) -> &ControlRegionKey {
        &self.key
    }

    pub fn kind(&self) -> StructuredControlRegionKind {
        self.kind
    }

    pub fn entry(&self) -> &ControlPointKey {
        &self.entry
    }

    pub fn exit(&self) -> &ControlPointKey {
        &self.exit
    }

    pub fn points(&self) -> &[ControlPointKey] {
        &self.points
    }

    pub fn parent(&self) -> Option<&ControlRegionKey> {
        self.parent.as_ref()
    }

    pub fn children(&self) -> &[ControlRegionKey] {
        &self.children
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRegionResidual {
    key: ControlRegionResidualKey,
    kind: StructuredControlRegionKind,
    entry: ControlPointKey,
    exit: ControlPointKey,
    points: Vec<ControlPointKey>,
    reason: String,
}

impl ControlRegionResidual {
    pub fn key(&self) -> &ControlRegionResidualKey {
        &self.key
    }

    pub fn kind(&self) -> StructuredControlRegionKind {
        self.kind
    }

    pub fn entry(&self) -> &ControlPointKey {
        &self.entry
    }

    pub fn exit(&self) -> &ControlPointKey {
        &self.exit
    }

    pub fn points(&self) -> &[ControlPointKey] {
        &self.points
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRegionGraph {
    key: ControlRegionGraphKey,
    control_flow_graph: ControlFlowGraphKey,
    owner: NodeKey,
    entry: ControlPointKey,
    exit: ControlPointKey,
    coverage: ControlRegionCoverageEvidence,
    points: Vec<ControlPointRelations>,
    regions: Vec<StructuredControlRegion>,
    residuals: Vec<ControlRegionResidual>,
}

impl ControlRegionGraph {
    pub fn key(&self) -> &ControlRegionGraphKey {
        &self.key
    }

    pub fn control_flow_graph(&self) -> &ControlFlowGraphKey {
        &self.control_flow_graph
    }

    pub fn owner(&self) -> &NodeKey {
        &self.owner
    }

    pub fn entry(&self) -> &ControlPointKey {
        &self.entry
    }

    pub fn exit(&self) -> &ControlPointKey {
        &self.exit
    }

    pub fn coverage(&self) -> &ControlRegionCoverageEvidence {
        &self.coverage
    }

    pub fn points(&self) -> &[ControlPointRelations] {
        &self.points
    }

    pub fn regions(&self) -> &[StructuredControlRegion] {
        &self.regions
    }

    pub fn residuals(&self) -> &[ControlRegionResidual] {
        &self.residuals
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRegionDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_flow_projection_id: ProjectionId,
    control_flow_policy: ControlFlowPolicyId,
    policy: ControlRegionPolicyId,
    graphs: Vec<ControlRegionGraph>,
}

impl ControlRegionDocument {
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

    pub fn policy(&self) -> &ControlRegionPolicyId {
        &self.policy
    }

    pub fn graphs(&self) -> &[ControlRegionGraph] {
        &self.graphs
    }

    fn validate(&self) -> Result<(), ControlRegionBuildError> {
        if self.schema != CONTROL_REGION_SCHEMA {
            return Err(ControlRegionBuildError::Invalid(format!(
                "unsupported control-region schema {}",
                self.schema
            )));
        }
        validate_digest_id(self.projection_id.as_str(), "pj1_")?;
        validate_digest_id(&self.analysis_id, "pa1_")?;
        validate_digest_id(self.control_flow_projection_id.as_str(), "pj1_")?;
        if self.graphs.is_empty() {
            return Err(ControlRegionBuildError::Invalid(
                "control-region document cannot be empty".into(),
            ));
        }
        validate_sorted_unique_by_key("control-region graphs", &self.graphs, |graph| {
            graph.key.as_str()
        })?;
        let mut source_graphs = BTreeSet::new();
        for graph in &self.graphs {
            if !source_graphs.insert(graph.control_flow_graph.clone()) {
                return Err(ControlRegionBuildError::Invalid(
                    "control-region document repeats a source graph".into(),
                ));
            }
            validate_region_graph(&self.policy, graph)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlRegionDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    control_flow_projection_id: ProjectionId,
    control_flow_policy: ControlFlowPolicyId,
    policy: ControlRegionPolicyId,
    graphs: Vec<ControlRegionGraph>,
}

impl<'de> Deserialize<'de> for ControlRegionDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ControlRegionDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
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
pub struct ControlRegionProjection {
    id: ProjectionId,
    control_flow: Arc<ControlFlowProjection>,
    policy: ControlRegionPolicyId,
    document: ControlRegionDocument,
}

impl ControlRegionProjection {
    pub fn schema(&self) -> &'static str {
        CONTROL_REGION_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn control_flow(&self) -> &Arc<ControlFlowProjection> {
        &self.control_flow
    }

    pub fn policy(&self) -> &ControlRegionPolicyId {
        &self.policy
    }

    pub fn document(&self) -> &ControlRegionDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRegionBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for ControlRegionBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid control-region evidence: {detail}"),
            Self::Identity(detail) => write!(formatter, "control-region identity error: {detail}"),
        }
    }
}

impl std::error::Error for ControlRegionBuildError {}

#[derive(Debug)]
struct IndexedGraph {
    keys: Vec<ControlPointKey>,
    kinds: Vec<ControlPointKind>,
    index: BTreeMap<ControlPointKey, usize>,
    successors: Vec<BTreeSet<usize>>,
    predecessors: Vec<BTreeSet<usize>>,
    entry: usize,
    exit: usize,
}

impl IndexedGraph {
    fn new(graph: &ControlFlowGraph) -> Result<Self, ControlRegionBuildError> {
        let keys = graph
            .points()
            .iter()
            .map(|point| point.key().clone())
            .collect::<Vec<_>>();
        let kinds = graph
            .points()
            .iter()
            .map(|point| point.kind().clone())
            .collect::<Vec<_>>();
        let index = keys
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect::<BTreeMap<_, _>>();
        if index.len() != keys.len() {
            return Err(ControlRegionBuildError::Invalid(
                "source graph repeats a point key".into(),
            ));
        }
        let entry = *index.get(graph.entry()).ok_or_else(|| {
            ControlRegionBuildError::Invalid("source entry point is missing".into())
        })?;
        let exit = *index.get(graph.exit()).ok_or_else(|| {
            ControlRegionBuildError::Invalid("source exit point is missing".into())
        })?;
        let mut successors = vec![BTreeSet::new(); keys.len()];
        let mut predecessors = vec![BTreeSet::new(); keys.len()];
        for edge in graph.edges() {
            let from = *index.get(edge.from()).ok_or_else(|| {
                ControlRegionBuildError::Invalid("source edge has a dangling origin".into())
            })?;
            let to = *index.get(edge.to()).ok_or_else(|| {
                ControlRegionBuildError::Invalid("source edge has a dangling target".into())
            })?;
            successors[from].insert(to);
            predecessors[to].insert(from);
        }
        Ok(Self {
            keys,
            kinds,
            index,
            successors,
            predecessors,
            entry,
            exit,
        })
    }
}

#[derive(Debug, Clone)]
struct RegionCandidate {
    kind: StructuredControlRegionKind,
    entry: usize,
    exit: usize,
    points: BTreeSet<usize>,
}

pub fn derive_control_regions(
    control_flow: Arc<ControlFlowProjection>,
    policy: ControlRegionPolicyId,
) -> Result<ControlRegionProjection, ControlRegionBuildError> {
    let mut graphs = control_flow
        .document()
        .graphs()
        .iter()
        .map(|graph| derive_region_graph(graph, &policy))
        .collect::<Result<Vec<_>, _>>()?;
    graphs.sort_by(|left, right| left.key.cmp(&right.key));
    let payload = serde_json::to_vec(&(control_flow.id(), control_flow.policy(), &policy, &graphs))
        .map_err(|error| ControlRegionBuildError::Identity(error.to_string()))?;
    let id = control_flow
        .analysis()
        .derive_projection_id(
            CONTROL_REGION_SCHEMA,
            &payload,
            control_flow.id().as_str().as_bytes(),
        )
        .map_err(|error| ControlRegionBuildError::Identity(error.to_string()))?;
    let document = ControlRegionDocument {
        schema: CONTROL_REGION_SCHEMA.into(),
        projection_id: id.clone(),
        analysis_id: control_flow.analysis().id().as_str().to_string(),
        control_flow_projection_id: control_flow.id().clone(),
        control_flow_policy: control_flow.policy().clone(),
        policy: policy.clone(),
        graphs,
    };
    document.validate()?;
    Ok(ControlRegionProjection {
        id,
        control_flow,
        policy,
        document,
    })
}

fn derive_region_graph(
    graph: &ControlFlowGraph,
    policy: &ControlRegionPolicyId,
) -> Result<ControlRegionGraph, ControlRegionBuildError> {
    let indexed = IndexedGraph::new(graph)?;
    let reachable = reachability(indexed.entry, &indexed.successors);
    let exit_reachable = reachability(indexed.exit, &indexed.predecessors);
    let dominators = fixed_point_relations(
        indexed.entry,
        &reachable,
        &indexed.predecessors,
        indexed.keys.len(),
    );
    let post_dominators = fixed_point_relations(
        indexed.exit,
        &exit_reachable,
        &indexed.successors,
        indexed.keys.len(),
    );
    let immediate_dominators = immediate_relations(&dominators);
    let immediate_post_dominators = immediate_relations(&post_dominators);

    let mut points = Vec::with_capacity(indexed.keys.len());
    for point in 0..indexed.keys.len() {
        let mut fact = ControlPointRelations {
            key: ControlRegionPointKey(String::new()),
            point: indexed.keys[point].clone(),
            reachable: reachable.contains(&point),
            exit_reachable: exit_reachable.contains(&point),
            dominators: relation_keys(&indexed.keys, &dominators[point]),
            immediate_dominator: immediate_dominators[point]
                .map(|parent| indexed.keys[parent].clone()),
            dominator_depth: reachable
                .contains(&point)
                .then(|| (dominators[point].len() - 1) as u32),
            post_dominators: relation_keys(&indexed.keys, &post_dominators[point]),
            immediate_post_dominator: immediate_post_dominators[point]
                .map(|parent| indexed.keys[parent].clone()),
            post_dominator_depth: exit_reachable
                .contains(&point)
                .then(|| (post_dominators[point].len() - 1) as u32),
        };
        fact.key = derive_point_key(policy, graph.key(), &fact)?;
        points.push(fact);
    }
    points.sort_by(|left, right| left.point.cmp(&right.point));

    let core = reachable
        .intersection(&exit_reachable)
        .copied()
        .collect::<BTreeSet<_>>();
    let (mut regions, mut residuals) = derive_regions(
        graph,
        policy,
        &indexed,
        &core,
        &dominators,
        &post_dominators,
        &immediate_post_dominators,
    )?;
    regions.sort_by(|left, right| left.key.cmp(&right.key));
    residuals.sort_by(|left, right| left.key.cmp(&right.key));

    let mut additional_reasons = Vec::new();
    if reachable
        .iter()
        .any(|point| !exit_reachable.contains(point))
    {
        additional_reasons.push("entry-reachable points cannot reach the virtual exit".to_string());
    }
    if !residuals.is_empty() {
        additional_reasons.push("non-laminar or invalid SESE candidates remain".to_string());
    }
    if !regions
        .iter()
        .any(|region| region.kind == StructuredControlRegionKind::Root)
    {
        additional_reasons.push("terminating core has no valid structured root".to_string());
    }
    let coverage = ControlRegionCoverageEvidence::from_source(
        graph.coverage().status(),
        graph.coverage().reasons(),
        additional_reasons,
    )?;
    let mut derived = ControlRegionGraph {
        key: ControlRegionGraphKey(String::new()),
        control_flow_graph: graph.key().clone(),
        owner: graph.owner().clone(),
        entry: graph.entry().clone(),
        exit: graph.exit().clone(),
        coverage,
        points,
        regions,
        residuals,
    };
    derived.key = derive_graph_key(policy, &derived)?;
    validate_region_graph(policy, &derived)?;
    Ok(derived)
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

fn fixed_point_relations(
    root: usize,
    domain: &BTreeSet<usize>,
    incoming: &[BTreeSet<usize>],
    point_count: usize,
) -> Vec<BTreeSet<usize>> {
    let mut relations = vec![BTreeSet::new(); point_count];
    for point in domain {
        relations[*point] = if *point == root {
            BTreeSet::from([root])
        } else {
            domain.clone()
        };
    }
    loop {
        let mut changed = false;
        for point in domain {
            if *point == root {
                continue;
            }
            let mut sources = incoming[*point]
                .iter()
                .copied()
                .filter(|source| domain.contains(source));
            let mut next = sources
                .next()
                .map(|source| relations[source].clone())
                .unwrap_or_default();
            for source in sources {
                next = next.intersection(&relations[source]).copied().collect();
            }
            next.insert(*point);
            if next != relations[*point] {
                relations[*point] = next;
                changed = true;
            }
        }
        if !changed {
            return relations;
        }
    }
}

fn immediate_relations(relations: &[BTreeSet<usize>]) -> Vec<Option<usize>> {
    relations
        .iter()
        .enumerate()
        .map(|(point, relation)| {
            relation
                .iter()
                .copied()
                .filter(|candidate| *candidate != point)
                .find(|candidate| {
                    relation.iter().copied().all(|other| {
                        other == point
                            || other == *candidate
                            || relations[*candidate].contains(&other)
                    })
                })
        })
        .collect()
}

fn relation_keys(keys: &[ControlPointKey], relation: &BTreeSet<usize>) -> Vec<ControlPointKey> {
    let mut values = relation
        .iter()
        .map(|point| keys[*point].clone())
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn derive_regions(
    graph: &ControlFlowGraph,
    policy: &ControlRegionPolicyId,
    indexed: &IndexedGraph,
    core: &BTreeSet<usize>,
    dominators: &[BTreeSet<usize>],
    post_dominators: &[BTreeSet<usize>],
    immediate_post_dominators: &[Option<usize>],
) -> Result<(Vec<StructuredControlRegion>, Vec<ControlRegionResidual>), ControlRegionBuildError> {
    let mut candidates = Vec::new();
    if core.contains(&indexed.entry) && core.contains(&indexed.exit) && core.len() >= 3 {
        candidates.push(RegionCandidate {
            kind: StructuredControlRegionKind::Root,
            entry: indexed.entry,
            exit: indexed.exit,
            points: core.clone(),
        });
    }
    for entry in core {
        let kind = match indexed.kinds[*entry] {
            ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch) => {
                StructuredControlRegionKind::Branch
            }
            ControlPointKind::Synthetic(ControlSyntheticPointKind::LoopHeader) => {
                StructuredControlRegionKind::Loop
            }
            _ => continue,
        };
        let Some(exit) = immediate_post_dominators[*entry] else {
            continue;
        };
        if exit == *entry || !core.contains(&exit) {
            continue;
        }
        let points = core
            .iter()
            .copied()
            .filter(|point| {
                dominators[*point].contains(entry) && post_dominators[*point].contains(&exit)
            })
            .collect::<BTreeSet<_>>();
        if points.len() >= 3 {
            candidates.push(RegionCandidate {
                kind,
                entry: *entry,
                exit,
                points,
            });
        }
    }

    candidates.sort_by(|left, right| {
        (left.entry, left.exit, left.kind, &left.points).cmp(&(
            right.entry,
            right.exit,
            right.kind,
            &right.points,
        ))
    });
    candidates.dedup_by(|left, right| {
        left.entry == right.entry
            && left.exit == right.exit
            && left.kind == right.kind
            && left.points == right.points
    });

    let mut residuals = Vec::new();
    let mut valid = Vec::new();
    for candidate in candidates {
        if let Some(reason) = region_boundary_error(graph, indexed, &candidate) {
            residuals.push(build_residual(
                policy,
                graph.key(),
                indexed,
                candidate,
                reason,
            )?);
        } else {
            valid.push(candidate);
        }
    }

    let mut overlaps = BTreeSet::new();
    for left in 0..valid.len() {
        for right in left + 1..valid.len() {
            let equal = valid[left].points == valid[right].points;
            let intersects = valid[left]
                .points
                .intersection(&valid[right].points)
                .next()
                .is_some();
            let nested = valid[left].points.is_subset(&valid[right].points)
                || valid[right].points.is_subset(&valid[left].points);
            if equal || (intersects && !nested) {
                overlaps.insert(left);
                overlaps.insert(right);
            }
        }
    }
    let mut structured_candidates = Vec::new();
    for (index, candidate) in valid.into_iter().enumerate() {
        if overlaps.contains(&index) {
            residuals.push(build_residual(
                policy,
                graph.key(),
                indexed,
                candidate,
                "candidate overlaps another region without containment".into(),
            )?);
        } else {
            structured_candidates.push(candidate);
        }
    }
    if !structured_candidates
        .iter()
        .any(|candidate| candidate.kind == StructuredControlRegionKind::Root)
    {
        for candidate in structured_candidates.drain(..) {
            residuals.push(build_residual(
                policy,
                graph.key(),
                indexed,
                candidate,
                "terminating core has no valid root region".into(),
            )?);
        }
    }

    let mut regions = structured_candidates
        .iter()
        .map(|candidate| build_region(policy, graph.key(), indexed, candidate))
        .collect::<Result<Vec<_>, _>>()?;
    for child in 0..regions.len() {
        let mut parents = (0..regions.len())
            .filter(|parent| {
                *parent != child
                    && regions[child].points.len() < regions[*parent].points.len()
                    && sorted_subset(&regions[child].points, &regions[*parent].points)
            })
            .collect::<Vec<_>>();
        parents.sort_by_key(|parent| regions[*parent].points.len());
        if let Some(parent) = parents.first().copied() {
            regions[child].parent = Some(regions[parent].key.clone());
        }
    }
    let parents = regions
        .iter()
        .map(|region| region.parent.clone())
        .collect::<Vec<_>>();
    for (child, parent) in parents.into_iter().enumerate() {
        if let Some(parent) = parent {
            let parent_index = regions
                .iter()
                .position(|region| region.key == parent)
                .ok_or_else(|| {
                    ControlRegionBuildError::Invalid("derived region parent is missing".into())
                })?;
            let child_key = regions[child].key.clone();
            regions[parent_index].children.push(child_key);
        }
    }
    for region in &mut regions {
        region.children.sort();
    }
    Ok((regions, residuals))
}

fn region_boundary_error(
    graph: &ControlFlowGraph,
    indexed: &IndexedGraph,
    candidate: &RegionCandidate,
) -> Option<String> {
    if candidate.entry == candidate.exit
        || candidate.points.len() < 3
        || !candidate.points.contains(&candidate.entry)
        || !candidate.points.contains(&candidate.exit)
    {
        return Some("candidate lacks distinct entry, exit, or interior".into());
    }
    for edge in graph.edges() {
        let from = indexed.index[edge.from()];
        let to = indexed.index[edge.to()];
        if candidate.points.contains(&to)
            && to != candidate.entry
            && !candidate.points.contains(&from)
        {
            return Some("candidate has an incoming edge that bypasses its entry".into());
        }
        if candidate.points.contains(&from)
            && from != candidate.exit
            && !candidate.points.contains(&to)
        {
            return Some("candidate has an outgoing edge that bypasses its exit".into());
        }
    }
    None
}

fn build_region(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    indexed: &IndexedGraph,
    candidate: &RegionCandidate,
) -> Result<StructuredControlRegion, ControlRegionBuildError> {
    let mut region = StructuredControlRegion {
        key: ControlRegionKey(String::new()),
        kind: candidate.kind,
        entry: indexed.keys[candidate.entry].clone(),
        exit: indexed.keys[candidate.exit].clone(),
        points: relation_keys(&indexed.keys, &candidate.points),
        parent: None,
        children: Vec::new(),
    };
    region.key = derive_region_key(policy, graph, &region)?;
    Ok(region)
}

fn build_residual(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    indexed: &IndexedGraph,
    candidate: RegionCandidate,
    reason: String,
) -> Result<ControlRegionResidual, ControlRegionBuildError> {
    let mut residual = ControlRegionResidual {
        key: ControlRegionResidualKey(String::new()),
        kind: candidate.kind,
        entry: indexed.keys[candidate.entry].clone(),
        exit: indexed.keys[candidate.exit].clone(),
        points: relation_keys(&indexed.keys, &candidate.points),
        reason,
    };
    residual.key = derive_residual_key(policy, graph, &residual)?;
    Ok(residual)
}

fn validate_region_graph(
    policy: &ControlRegionPolicyId,
    graph: &ControlRegionGraph,
) -> Result<(), ControlRegionBuildError> {
    graph.coverage.validate()?;
    if graph.points.is_empty() {
        return Err(ControlRegionBuildError::Invalid(
            "control-region graph has no point facts".into(),
        ));
    }
    validate_sorted_unique_by_key("control-region point facts", &graph.points, |point| {
        point.point.as_str()
    })?;
    validate_sorted_unique_by_key("structured control regions", &graph.regions, |region| {
        region.key.as_str()
    })?;
    validate_sorted_unique_by_key("control-region residuals", &graph.residuals, |residual| {
        residual.key.as_str()
    })?;
    let facts = graph
        .points
        .iter()
        .map(|fact| (fact.point.clone(), fact))
        .collect::<BTreeMap<_, _>>();
    if !facts.contains_key(&graph.entry) || !facts.contains_key(&graph.exit) {
        return Err(ControlRegionBuildError::Invalid(
            "control-region graph omits a virtual boundary fact".into(),
        ));
    }
    for fact in &graph.points {
        validate_point_relations(policy, &graph.control_flow_graph, fact, &facts)?;
    }
    let entry = facts[&graph.entry];
    if !entry.reachable
        || entry.dominators != [graph.entry.clone()]
        || entry.immediate_dominator.is_some()
        || entry.dominator_depth != Some(0)
    {
        return Err(ControlRegionBuildError::Invalid(
            "entry dominance root is inconsistent".into(),
        ));
    }
    let exit = facts[&graph.exit];
    if !exit.exit_reachable
        || exit.post_dominators != [graph.exit.clone()]
        || exit.immediate_post_dominator.is_some()
        || exit.post_dominator_depth != Some(0)
    {
        return Err(ControlRegionBuildError::Invalid(
            "exit post-dominance root is inconsistent".into(),
        ));
    }

    let regions = graph
        .regions
        .iter()
        .map(|region| (region.key.clone(), region))
        .collect::<BTreeMap<_, _>>();
    if !graph.regions.is_empty()
        && graph
            .regions
            .iter()
            .filter(|region| region.kind == StructuredControlRegionKind::Root)
            .count()
            != 1
    {
        return Err(ControlRegionBuildError::Invalid(
            "structured region hierarchy requires exactly one root".into(),
        ));
    }
    for region in &graph.regions {
        validate_region(policy, &graph.control_flow_graph, region, &facts, &regions)?;
    }
    for left in 0..graph.regions.len() {
        for right in left + 1..graph.regions.len() {
            let left_points = &graph.regions[left].points;
            let right_points = &graph.regions[right].points;
            let intersects = sorted_intersects(left_points, right_points);
            if intersects
                && (left_points == right_points
                    || (!sorted_subset(left_points, right_points)
                        && !sorted_subset(right_points, left_points)))
            {
                return Err(ControlRegionBuildError::Invalid(
                    "structured regions overlap without containment".into(),
                ));
            }
        }
    }
    for residual in &graph.residuals {
        validate_canonical_distinct_keys("residual points", &residual.points)?;
        if residual.points.len() < 3
            || !residual.points.contains(&residual.entry)
            || !residual.points.contains(&residual.exit)
            || residual.entry == residual.exit
            || residual
                .points
                .iter()
                .any(|point| !facts.contains_key(point))
        {
            return Err(ControlRegionBuildError::Invalid(
                "control-region residual has invalid point closure".into(),
            ));
        }
        validate_text("control-region residual reason", &residual.reason)?;
        if derive_residual_key(policy, &graph.control_flow_graph, residual)? != residual.key {
            return Err(ControlRegionBuildError::Invalid(
                "control-region residual key does not bind its payload".into(),
            ));
        }
    }
    if derive_graph_key(policy, graph)? != graph.key {
        return Err(ControlRegionBuildError::Invalid(
            "control-region graph key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn validate_point_relations(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    fact: &ControlPointRelations,
    facts: &BTreeMap<ControlPointKey, &ControlPointRelations>,
) -> Result<(), ControlRegionBuildError> {
    validate_canonical_distinct_keys("dominators", &fact.dominators)?;
    validate_canonical_distinct_keys("post-dominators", &fact.post_dominators)?;
    if fact
        .dominators
        .iter()
        .chain(fact.post_dominators.iter())
        .any(|point| !facts.contains_key(point))
    {
        return Err(ControlRegionBuildError::Invalid(
            "dominance relation references another graph".into(),
        ));
    }
    match fact.reachable {
        true => {
            if !fact.dominators.contains(&fact.point)
                || fact.dominator_depth != Some((fact.dominators.len() - 1) as u32)
            {
                return Err(ControlRegionBuildError::Invalid(
                    "reachable point has incomplete dominance evidence".into(),
                ));
            }
            let expected = immediate_key(&fact.point, &fact.dominators, facts, false)?;
            if fact.immediate_dominator != expected {
                return Err(ControlRegionBuildError::Invalid(
                    "immediate dominator disagrees with the full relation".into(),
                ));
            }
        }
        false => {
            if !fact.dominators.is_empty()
                || fact.immediate_dominator.is_some()
                || fact.dominator_depth.is_some()
            {
                return Err(ControlRegionBuildError::Invalid(
                    "unreachable point carries dominance evidence".into(),
                ));
            }
        }
    }
    match fact.exit_reachable {
        true => {
            if !fact.post_dominators.contains(&fact.point)
                || fact.post_dominator_depth != Some((fact.post_dominators.len() - 1) as u32)
            {
                return Err(ControlRegionBuildError::Invalid(
                    "exit-reachable point has incomplete post-dominance evidence".into(),
                ));
            }
            let expected = immediate_key(&fact.point, &fact.post_dominators, facts, true)?;
            if fact.immediate_post_dominator != expected {
                return Err(ControlRegionBuildError::Invalid(
                    "immediate post-dominator disagrees with the full relation".into(),
                ));
            }
        }
        false => {
            if !fact.post_dominators.is_empty()
                || fact.immediate_post_dominator.is_some()
                || fact.post_dominator_depth.is_some()
            {
                return Err(ControlRegionBuildError::Invalid(
                    "exit-unreachable point carries post-dominance evidence".into(),
                ));
            }
        }
    }
    if derive_point_key(policy, graph, fact)? != fact.key {
        return Err(ControlRegionBuildError::Invalid(
            "control-region point key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn immediate_key(
    point: &ControlPointKey,
    relation: &[ControlPointKey],
    facts: &BTreeMap<ControlPointKey, &ControlPointRelations>,
    post: bool,
) -> Result<Option<ControlPointKey>, ControlRegionBuildError> {
    let strict = relation
        .iter()
        .filter(|candidate| *candidate != point)
        .collect::<Vec<_>>();
    let candidates = strict
        .iter()
        .copied()
        .filter(|candidate| {
            strict.iter().copied().all(|other| {
                other == *candidate
                    || if post {
                        facts[*candidate].post_dominators.contains(other)
                    } else {
                        facts[*candidate].dominators.contains(other)
                    }
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] if strict.is_empty() => Ok(None),
        [candidate] => Ok(Some(candidate.clone())),
        _ => Err(ControlRegionBuildError::Invalid(
            "dominance relation has no unique immediate parent".into(),
        )),
    }
}

fn validate_region(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    region: &StructuredControlRegion,
    facts: &BTreeMap<ControlPointKey, &ControlPointRelations>,
    regions: &BTreeMap<ControlRegionKey, &StructuredControlRegion>,
) -> Result<(), ControlRegionBuildError> {
    validate_canonical_distinct_keys("structured region points", &region.points)?;
    validate_canonical_distinct_keys("structured region children", &region.children)?;
    if region.entry == region.exit
        || region.points.len() < 3
        || !region.points.contains(&region.entry)
        || !region.points.contains(&region.exit)
        || region.points.iter().any(|point| !facts.contains_key(point))
    {
        return Err(ControlRegionBuildError::Invalid(
            "structured region has invalid point closure".into(),
        ));
    }
    for point in &region.points {
        let fact = facts[point];
        if !fact.reachable
            || !fact.exit_reachable
            || !fact.dominators.contains(&region.entry)
            || !fact.post_dominators.contains(&region.exit)
        {
            return Err(ControlRegionBuildError::Invalid(
                "structured region is outside its dominance boundaries".into(),
            ));
        }
    }
    if let Some(parent) = &region.parent {
        let parent = regions.get(parent).ok_or_else(|| {
            ControlRegionBuildError::Invalid("structured region parent is missing".into())
        })?;
        if !sorted_subset(&region.points, &parent.points) || region.points == parent.points {
            return Err(ControlRegionBuildError::Invalid(
                "structured region parent does not strictly contain its child".into(),
            ));
        }
        if !parent.children.contains(&region.key) {
            return Err(ControlRegionBuildError::Invalid(
                "structured region parent/child links are not reciprocal".into(),
            ));
        }
        if regions.values().any(|candidate| {
            candidate.key != region.key
                && candidate.key != parent.key
                && region.points.len() < candidate.points.len()
                && candidate.points.len() < parent.points.len()
                && sorted_subset(&region.points, &candidate.points)
        }) {
            return Err(ControlRegionBuildError::Invalid(
                "structured region parent is not the smallest strict container".into(),
            ));
        }
    } else if region.kind != StructuredControlRegionKind::Root {
        return Err(ControlRegionBuildError::Invalid(
            "non-root structured region has no parent".into(),
        ));
    } else if regions
        .values()
        .any(|candidate| candidate.key != region.key && candidate.parent.is_none())
    {
        return Err(ControlRegionBuildError::Invalid(
            "structured region hierarchy has more than one root".into(),
        ));
    }
    for child in &region.children {
        if regions
            .get(child)
            .is_none_or(|child| child.parent.as_ref() != Some(&region.key))
        {
            return Err(ControlRegionBuildError::Invalid(
                "structured region child/parent links are not reciprocal".into(),
            ));
        }
    }
    if derive_region_key(policy, graph, region)? != region.key {
        return Err(ControlRegionBuildError::Invalid(
            "structured region key does not bind its payload".into(),
        ));
    }
    Ok(())
}

fn derive_graph_key(
    policy: &ControlRegionPolicyId,
    graph: &ControlRegionGraph,
) -> Result<ControlRegionGraphKey, ControlRegionBuildError> {
    let payload = serde_json::to_vec(&(
        &graph.control_flow_graph,
        &graph.owner,
        &graph.entry,
        &graph.exit,
        &graph.coverage,
        &graph.points,
        &graph.regions,
        &graph.residuals,
    ))
    .map_err(|error| ControlRegionBuildError::Identity(error.to_string()))?;
    Ok(ControlRegionGraphKey(derive_parts_id(
        GRAPH_KEY_DOMAIN,
        "crg1_",
        &[policy.as_str().as_bytes(), &payload],
    )))
}

fn derive_point_key(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    fact: &ControlPointRelations,
) -> Result<ControlRegionPointKey, ControlRegionBuildError> {
    let payload = serde_json::to_vec(&(
        &fact.point,
        fact.reachable,
        fact.exit_reachable,
        &fact.dominators,
        &fact.immediate_dominator,
        fact.dominator_depth,
        &fact.post_dominators,
        &fact.immediate_post_dominator,
        fact.post_dominator_depth,
    ))
    .map_err(|error| ControlRegionBuildError::Identity(error.to_string()))?;
    Ok(ControlRegionPointKey(derive_parts_id(
        POINT_KEY_DOMAIN,
        "crn1_",
        &[
            policy.as_str().as_bytes(),
            graph.as_str().as_bytes(),
            &payload,
        ],
    )))
}

fn derive_region_key(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    region: &StructuredControlRegion,
) -> Result<ControlRegionKey, ControlRegionBuildError> {
    let payload = serde_json::to_vec(&(region.kind, &region.entry, &region.exit, &region.points))
        .map_err(|error| ControlRegionBuildError::Identity(error.to_string()))?;
    Ok(ControlRegionKey(derive_parts_id(
        REGION_KEY_DOMAIN,
        "cre1_",
        &[
            policy.as_str().as_bytes(),
            graph.as_str().as_bytes(),
            &payload,
        ],
    )))
}

fn derive_residual_key(
    policy: &ControlRegionPolicyId,
    graph: &ControlFlowGraphKey,
    residual: &ControlRegionResidual,
) -> Result<ControlRegionResidualKey, ControlRegionBuildError> {
    let payload = serde_json::to_vec(&(
        residual.kind,
        &residual.entry,
        &residual.exit,
        &residual.points,
        &residual.reason,
    ))
    .map_err(|error| ControlRegionBuildError::Identity(error.to_string()))?;
    Ok(ControlRegionResidualKey(derive_parts_id(
        RESIDUAL_KEY_DOMAIN,
        "crx1_",
        &[
            policy.as_str().as_bytes(),
            graph.as_str().as_bytes(),
            &payload,
        ],
    )))
}

fn sorted_subset<T: Ord>(left: &[T], right: &[T]) -> bool {
    left.iter().all(|value| right.binary_search(value).is_ok())
}

fn sorted_intersects<T: Ord>(left: &[T], right: &[T]) -> bool {
    left.iter().any(|value| right.binary_search(value).is_ok())
}

fn validate_sorted_unique_by_key<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), ControlRegionBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        return Err(ControlRegionBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )));
    }
    Ok(())
}

fn validate_canonical_distinct_keys<T: Ord>(
    label: &str,
    values: &[T],
) -> Result<(), ControlRegionBuildError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ControlRegionBuildError::Invalid(format!(
            "{label} are not canonical and distinct"
        )));
    }
    Ok(())
}

fn validate_canonical_strings(
    label: &str,
    values: &[String],
) -> Result<(), ControlRegionBuildError> {
    for value in values {
        validate_text(label, value)?;
    }
    validate_canonical_distinct_keys(label, values)
}

fn validate_text(label: &str, value: &str) -> Result<(), ControlRegionBuildError> {
    if value.trim().is_empty() || value.trim() != value {
        return Err(ControlRegionBuildError::Invalid(format!(
            "{label} must be nonempty canonical text"
        )));
    }
    Ok(())
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), ControlRegionBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(ControlRegionBuildError::Invalid(format!(
            "identity must start with {prefix}"
        )));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ControlRegionBuildError::Invalid(
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
    use std::path::Path;

    use deslop_lang::Registry;
    use serde_json::Value;

    use super::*;
    use crate::{
        ControlFlowPolicyId, ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId,
        lower_control_flow,
    };

    type JsonMutation = (&'static str, Box<dyn Fn(&mut Value)>);

    fn flow_with_policy(source: &str, policy: &[u8]) -> Arc<ControlFlowProjection> {
        let root = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("control-region-test").unwrap(),
        )
        .unwrap()
        .with_registry(Registry::default())
        .with_overlay("flow.rs", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let lowered = lower_control_flow(
            analysis,
            ControlFlowPolicyId::from_parts(&[policy]).unwrap(),
        )
        .unwrap();
        Arc::new(lowered.projection().unwrap().clone())
    }

    fn flow(source: &str) -> Arc<ControlFlowProjection> {
        flow_with_policy(source, b"control-region-test-flow/1")
    }

    fn regions(source: &str) -> ControlRegionProjection {
        derive_control_regions(
            flow(source),
            ControlRegionPolicyId::from_parts(&[b"control-region-test-policy/1"]).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn m4_3_linear_graph_has_exact_dual_trees_and_root_region() {
        let projection = regions("fn run() {}\n");
        assert_eq!(projection.schema(), CONTROL_REGION_SCHEMA);
        assert_eq!(projection.document().graphs().len(), 1);
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.coverage().reasons().is_empty());
        assert_eq!(graph.points().len(), 4);
        assert!(
            graph
                .points()
                .iter()
                .all(|point| point.reachable() && point.exit_reachable())
        );
        let mut dominator_depths = graph
            .points()
            .iter()
            .map(|point| point.dominator_depth().unwrap())
            .collect::<Vec<_>>();
        dominator_depths.sort();
        assert_eq!(dominator_depths, [0, 1, 2, 3]);
        let mut post_dominator_depths = graph
            .points()
            .iter()
            .map(|point| point.post_dominator_depth().unwrap())
            .collect::<Vec<_>>();
        post_dominator_depths.sort();
        assert_eq!(post_dominator_depths, [0, 1, 2, 3]);
        assert_eq!(graph.regions().len(), 1);
        assert_eq!(graph.regions()[0].kind(), StructuredControlRegionKind::Root);
        assert_eq!(graph.regions()[0].points().len(), 4);
        assert!(graph.regions()[0].parent().is_none());
        assert!(graph.regions()[0].children().is_empty());
        assert!(graph.residuals().is_empty());

        let bytes = serde_json::to_vec(projection.document()).unwrap();
        let decoded: ControlRegionDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
        assert_eq!(
            decoded.control_flow_projection_id(),
            projection.control_flow().id()
        );
        assert_eq!(
            decoded.analysis_id(),
            projection.control_flow().analysis().id().as_str()
        );
    }

    #[test]
    fn m4_3_nested_diamonds_form_a_canonical_laminar_region_tree() {
        let projection =
            regions("fn run(x: bool, y: bool) { if x { if y { 1; } else { 2; } } else { 3; } }\n");
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.residuals().is_empty());
        assert_eq!(graph.regions().len(), 3, "{:#?}", graph.regions());
        assert_eq!(
            graph
                .regions()
                .iter()
                .filter(|region| region.kind() == StructuredControlRegionKind::Root)
                .count(),
            1
        );
        assert_eq!(
            graph
                .regions()
                .iter()
                .filter(|region| region.kind() == StructuredControlRegionKind::Branch)
                .count(),
            2
        );
        let root = graph
            .regions()
            .iter()
            .find(|region| region.kind() == StructuredControlRegionKind::Root)
            .unwrap();
        assert_eq!(root.children().len(), 1);
        let outer = graph
            .regions()
            .iter()
            .find(|region| region.key() == &root.children()[0])
            .unwrap();
        assert_eq!(outer.children().len(), 1);
        let inner = graph
            .regions()
            .iter()
            .find(|region| region.key() == &outer.children()[0])
            .unwrap();
        assert!(inner.children().is_empty());
        assert!(inner.points().len() < outer.points().len());
        assert!(outer.points().len() < root.points().len());

        let source = &projection.control_flow().document().graphs()[0];
        let kinds = source
            .points()
            .iter()
            .map(|point| (point.key(), point.kind()))
            .collect::<BTreeMap<_, _>>();
        for dispatch in source.points().iter().filter(|point| {
            point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch)
        }) {
            let fact = graph
                .points()
                .iter()
                .find(|fact| fact.point() == dispatch.key())
                .unwrap();
            assert!(matches!(
                kinds[fact.immediate_post_dominator().unwrap()],
                ControlPointKind::Synthetic(ControlSyntheticPointKind::Merge)
            ));
        }
    }

    #[test]
    fn m4_3_multiple_exit_outcomes_join_only_at_the_virtual_exit() {
        let projection = regions("fn run(x: bool) { if x { return; } 1; }\n");
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        let branch = graph
            .regions()
            .iter()
            .find(|region| region.kind() == StructuredControlRegionKind::Branch)
            .expect("abrupt/normal branch region");
        assert_eq!(branch.exit(), graph.exit());
        let dispatch = branch.entry();
        let fact = graph
            .points()
            .iter()
            .find(|fact| fact.point() == dispatch)
            .unwrap();
        assert_eq!(fact.immediate_post_dominator(), Some(graph.exit()));
        assert_eq!(graph.regions().len(), 2);
        assert!(graph.residuals().is_empty());
    }

    #[test]
    fn m4_3_unreachable_suffix_and_nonterminating_cycle_use_disjoint_domains() {
        let unreachable = regions("fn run() { return; 42; }\n");
        let graph = &unreachable.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        let dead = graph
            .points()
            .iter()
            .filter(|point| !point.reachable())
            .collect::<Vec<_>>();
        assert_eq!(dead.len(), 1);
        assert!(!dead[0].exit_reachable());
        assert!(dead[0].dominators().is_empty());
        assert!(dead[0].post_dominators().is_empty());
        assert!(dead[0].immediate_dominator().is_none());
        assert!(dead[0].immediate_post_dominator().is_none());

        let nonterminating = regions("fn run() { loop {} }\n");
        let graph = &nonterminating.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert_eq!(
            graph.coverage().reasons(),
            [
                "entry-reachable points cannot reach the virtual exit",
                "executable owner has no modeled exit path",
                "terminating core has no valid structured root",
            ]
        );
        assert_eq!(
            graph
                .points()
                .iter()
                .filter(|point| point.reachable())
                .count(),
            3
        );
        assert_eq!(
            graph
                .points()
                .iter()
                .filter(|point| point.exit_reachable())
                .count(),
            2
        );
        assert_eq!(
            graph
                .points()
                .iter()
                .filter(|point| point.reachable() && point.exit_reachable())
                .count(),
            0
        );
        assert!(graph.regions().is_empty());
    }

    #[test]
    fn m4_3_loop_and_partial_source_coverage_are_numerically_preserved() {
        let loops = regions(
            "fn run(flag: bool, xs: [i32; 0]) { while flag { continue; } for _x in xs { break; } }\n",
        );
        let graph = &loops.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.residuals().is_empty());
        assert_eq!(
            graph
                .regions()
                .iter()
                .filter(|region| region.kind() == StructuredControlRegionKind::Loop)
                .count(),
            2
        );
        let source = &loops.control_flow().document().graphs()[0];
        let kinds = source
            .points()
            .iter()
            .map(|point| (point.key(), point.kind()))
            .collect::<BTreeMap<_, _>>();
        for header in source.points().iter().filter(|point| {
            point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::LoopHeader)
        }) {
            let fact = graph
                .points()
                .iter()
                .find(|fact| fact.point() == header.key())
                .unwrap();
            assert!(matches!(
                kinds[fact.immediate_post_dominator().unwrap()],
                ControlPointKind::Synthetic(ControlSyntheticPointKind::Merge)
            ));
        }

        let partial = regions("fn run() { println!(\"x\"); }\n");
        let graph = &partial.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert_eq!(
            graph.coverage().reasons(),
            ["Rust macro expansion is unavailable"]
        );
        assert_eq!(graph.regions().len(), 1);
    }

    #[test]
    fn m4_3_terminating_branch_beside_infinite_branch_is_residual_not_structured() {
        let projection = regions("fn run(x: bool) { if x { loop {} } else { 1; } }\n");
        let source = &projection.control_flow().document().graphs()[0];
        assert_eq!(source.coverage().status(), FactCoverage::Complete);
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert_eq!(
            graph.coverage().reasons(),
            [
                "entry-reachable points cannot reach the virtual exit",
                "non-laminar or invalid SESE candidates remain",
                "terminating core has no valid structured root",
            ]
        );
        assert!(graph.regions().is_empty());
        assert_eq!(graph.residuals().len(), 1, "{:#?}", graph.residuals());
        assert_eq!(
            graph.residuals()[0].kind(),
            StructuredControlRegionKind::Root
        );
        assert!(graph.residuals().iter().all(|residual| {
            residual.reason() == "candidate has an outgoing edge that bypasses its exit"
        }));
        assert_eq!(
            graph
                .points()
                .iter()
                .filter(|point| point.reachable() && !point.exit_reachable())
                .count(),
            2
        );
    }

    #[test]
    fn m4_3_document_rejects_relation_region_key_and_unknown_field_corruption() {
        let projection = regions("fn run(x: bool) { if x { 1; } else { 2; } }\n");
        let original = serde_json::to_value(projection.document()).unwrap();
        let mut mutations: Vec<JsonMutation> = vec![
            (
                "unreachable-with-dominators",
                Box::new(|value| value["graphs"][0]["points"][0]["reachable"] = false.into()),
            ),
            (
                "wrong-immediate-dominator",
                Box::new(|value| {
                    value["graphs"][0]["points"][1]["immediate_dominator"] = Value::Null
                }),
            ),
            (
                "region-key",
                Box::new(|value| {
                    value["graphs"][0]["regions"][0]["key"] =
                        format!("cre1_{}", "0".repeat(64)).into()
                }),
            ),
            (
                "noncanonical-dominators",
                Box::new(|value| {
                    let points = value["graphs"][0]["points"].as_array_mut().unwrap();
                    let point = points
                        .iter_mut()
                        .find(|point| point["dominators"].as_array().unwrap().len() > 1)
                        .unwrap();
                    point["dominators"].as_array_mut().unwrap().swap(0, 1);
                }),
            ),
            (
                "missing-region-parent",
                Box::new(|value| {
                    let regions = value["graphs"][0]["regions"].as_array_mut().unwrap();
                    let child = regions
                        .iter_mut()
                        .find(|region| !region["parent"].is_null())
                        .unwrap();
                    child["parent"] = Value::Null;
                }),
            ),
            (
                "incomplete-coverage-without-reason",
                Box::new(|value| value["graphs"][0]["coverage"]["status"] = "partial".into()),
            ),
            (
                "unknown-field",
                Box::new(|value| value["unexpected"] = true.into()),
            ),
        ];
        for (label, mutate) in &mut mutations {
            let mut corrupted = original.clone();
            mutate(&mut corrupted);
            assert!(
                serde_json::from_value::<ControlRegionDocument>(corrupted).is_err(),
                "corruption {label} passed"
            );
        }

        assert_eq!(
            projection.control_flow().analysis().parse_counts().len(),
            1,
            "region derivation must not reparse"
        );
        assert!(
            projection
                .control_flow()
                .analysis()
                .snapshot()
                .entry(Path::new("flow.rs"))
                .is_some()
        );
    }

    #[test]
    fn m4_3_projection_policy_and_source_cfg_identity_are_all_bound() {
        let source = "fn run(x: bool) { if x { 1; } else { 2; } }\n";
        let source_flow = flow(source);
        let policy_a = ControlRegionPolicyId::from_parts(&[b"region-policy-a/1"]).unwrap();
        let policy_b = ControlRegionPolicyId::from_parts(&[b"region-policy-b/1"]).unwrap();
        let first = derive_control_regions(Arc::clone(&source_flow), policy_a.clone()).unwrap();
        let repeated = derive_control_regions(Arc::clone(&source_flow), policy_a.clone()).unwrap();
        assert_eq!(
            serde_json::to_vec(first.document()).unwrap(),
            serde_json::to_vec(repeated.document()).unwrap()
        );

        let changed_policy = derive_control_regions(Arc::clone(&source_flow), policy_b).unwrap();
        assert_ne!(first.id(), changed_policy.id());
        assert_ne!(
            first.document().graphs()[0].key(),
            changed_policy.document().graphs()[0].key()
        );
        assert_ne!(
            first.document().graphs()[0].points()[0].key(),
            changed_policy.document().graphs()[0].points()[0].key()
        );
        assert_ne!(
            first.document().graphs()[0].regions()[0].key(),
            changed_policy.document().graphs()[0].regions()[0].key()
        );

        let changed_flow = flow_with_policy(source, b"different-control-flow-policy/1");
        let changed_source = derive_control_regions(changed_flow, policy_a).unwrap();
        assert_ne!(
            first.control_flow().id(),
            changed_source.control_flow().id()
        );
        assert_ne!(first.id(), changed_source.id());
        assert_ne!(
            first.document().graphs()[0].control_flow_graph(),
            changed_source.document().graphs()[0].control_flow_graph()
        );
    }
}
