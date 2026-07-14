use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use deslop_lang::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, ControlAbruptForm,
    ControlEvaluationOrder, ControlFlowAction, ControlFlowOwnerRuleKind, ControlLoopForm,
};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    FactCoverage, GrammarSelection, LanguageAdapterIdentity, NodeId, NodeKey, ProjectAnalysis,
    ProjectionId,
};

pub const CONTROL_FLOW_SCHEMA: &str = "deslop.control-flow/1";
pub const CONTROL_FLOW_POLICY_SCHEMA: &str = "deslop.control-flow-policy/1";

const POLICY_ID_DOMAIN: &str = "deslop control-flow policy v1";
const GRAPH_KEY_DOMAIN: &str = "deslop control-flow graph key v1";
const POINT_KEY_DOMAIN: &str = "deslop control-flow point key v1";
const EDGE_KEY_DOMAIN: &str = "deslop control-flow edge key v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ControlFlowPolicyId(String);

impl ControlFlowPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ControlFlowBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(ControlFlowBuildError::Invalid(
                "control-flow policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_parts_id(POLICY_ID_DOMAIN, "cfp1_", parts)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ControlFlowPolicyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "cfp1_").map_err(D::Error::custom)?;
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

digest_key!(ControlFlowGraphKey, "cfg1_");
digest_key!(ControlPointKey, "cpt1_");
digest_key!(ControlEdgeKey, "ced1_");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

impl ControlFlowCoverageEvidence {
    pub fn complete() -> Self {
        Self {
            status: FactCoverage::Complete,
            reasons: Vec::new(),
        }
    }

    pub fn partial(reasons: Vec<String>) -> Result<Self, ControlFlowBuildError> {
        Self::incomplete(FactCoverage::Partial, reasons)
    }

    pub fn unsupported(reasons: Vec<String>) -> Result<Self, ControlFlowBuildError> {
        Self::incomplete(FactCoverage::Unsupported, reasons)
    }

    pub fn failed(reasons: Vec<String>) -> Result<Self, ControlFlowBuildError> {
        Self::incomplete(FactCoverage::Failed, reasons)
    }

    fn incomplete(
        status: FactCoverage,
        mut reasons: Vec<String>,
    ) -> Result<Self, ControlFlowBuildError> {
        reasons.sort();
        let evidence = Self { status, reasons };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    fn validate(&self) -> Result<(), ControlFlowBuildError> {
        match (self.status, self.reasons.is_empty()) {
            (FactCoverage::Complete, true) => Ok(()),
            (FactCoverage::Complete, false) => Err(ControlFlowBuildError::Invalid(
                "complete control-flow coverage cannot carry uncertainty reasons".into(),
            )),
            (_, true) => Err(ControlFlowBuildError::Invalid(
                "incomplete control-flow coverage requires an exact reason".into(),
            )),
            (_, false) => {
                validate_strings("control-flow coverage reason", &self.reasons)?;
                validate_canonical_distinct("control-flow coverage reasons", &self.reasons)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlFlowOwnerKind {
    Callable,
    Initializer,
    ModuleInitializer,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlSyntheticPointKind {
    NoOp,
    BranchDispatch,
    Merge,
    LoopHeader,
    HandlerDispatch,
    FinallyDispatch,
    AbruptDispatch,
    Suspension,
    Resume,
    ExitDispatch,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
pub enum ControlPointKind {
    Entry,
    Exit,
    Syntax,
    Synthetic(ControlSyntheticPointKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlExitOutcome {
    Normal,
    Exceptional,
    Abrupt,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlBranchKind {
    True,
    False,
    Case { label: String },
    Default,
    GuardPassed,
    GuardFailed,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlLoopKind {
    Enter,
    Body,
    Back,
    ConditionFalse,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlExceptionalKind {
    Throw,
    Propagate,
    Handler,
    FinallyEnter,
    FinallyResume,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlAbruptKind {
    Return,
    Break { label: Option<String> },
    Continue { label: Option<String> },
    Goto { label: String },
    Terminate,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlSuspensionKind {
    AwaitReady,
    AwaitPending,
    Yield,
    Suspend,
    Resume,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "family", content = "detail", rename_all = "kebab-case")]
pub enum ControlEdgeKind {
    Entry,
    Exit(ControlExitOutcome),
    Normal,
    Branch(ControlBranchKind),
    Loop(ControlLoopKind),
    Exceptional(ControlExceptionalKind),
    Abrupt(ControlAbruptKind),
    Suspension(ControlSuspensionKind),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "precision", content = "reason", rename_all = "kebab-case")]
pub enum ControlEdgePrecision {
    Exact,
    Conservative(String),
}

impl ControlEdgePrecision {
    pub fn conservative(reason: impl Into<String>) -> Result<Self, ControlFlowBuildError> {
        let precision = Self::Conservative(reason.into());
        precision.validate()?;
        Ok(precision)
    }

    fn validate(&self) -> Result<(), ControlFlowBuildError> {
        if let Self::Conservative(reason) = self {
            validate_text("conservative control-edge reason", reason)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ControlPointDraft {
    pub kind: ControlPointKind,
    pub source: Option<NodeId>,
    pub ordinal: u32,
}

#[derive(Debug, Clone)]
pub struct ControlEdgeDraft {
    pub from: usize,
    pub to: usize,
    pub kind: ControlEdgeKind,
    pub source: NodeId,
    pub predicate: Option<NodeId>,
    pub precision: ControlEdgePrecision,
}

#[derive(Debug, Clone)]
pub struct ControlFlowGraphDraft {
    pub owner: NodeId,
    pub owner_kind: ControlFlowOwnerKind,
    pub coverage: ControlFlowCoverageEvidence,
    pub points: Vec<ControlPointDraft>,
    pub edges: Vec<ControlEdgeDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlPoint {
    key: ControlPointKey,
    kind: ControlPointKind,
    source: Option<NodeKey>,
    ordinal: u32,
    recovered: bool,
}

impl ControlPoint {
    pub fn key(&self) -> &ControlPointKey {
        &self.key
    }

    pub fn kind(&self) -> &ControlPointKind {
        &self.kind
    }

    pub fn source(&self) -> Option<&NodeKey> {
        self.source.as_ref()
    }

    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub fn recovered(&self) -> bool {
        self.recovered
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEdge {
    key: ControlEdgeKey,
    from: ControlPointKey,
    to: ControlPointKey,
    kind: ControlEdgeKind,
    source: NodeKey,
    predicate: Option<NodeKey>,
    precision: ControlEdgePrecision,
    recovered_source: bool,
    recovered_predicate: bool,
}

impl ControlEdge {
    pub fn key(&self) -> &ControlEdgeKey {
        &self.key
    }

    pub fn from(&self) -> &ControlPointKey {
        &self.from
    }

    pub fn to(&self) -> &ControlPointKey {
        &self.to
    }

    pub fn kind(&self) -> &ControlEdgeKind {
        &self.kind
    }

    pub fn source(&self) -> &NodeKey {
        &self.source
    }

    pub fn predicate(&self) -> Option<&NodeKey> {
        self.predicate.as_ref()
    }

    pub fn precision(&self) -> &ControlEdgePrecision {
        &self.precision
    }

    pub fn recovered_source(&self) -> bool {
        self.recovered_source
    }

    pub fn recovered_predicate(&self) -> bool {
        self.recovered_predicate
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowGraph {
    key: ControlFlowGraphKey,
    owner: NodeKey,
    owner_kind: ControlFlowOwnerKind,
    grammar: GrammarSelection,
    adapter: LanguageAdapterIdentity,
    capability_support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    recovered_owner: bool,
    coverage: ControlFlowCoverageEvidence,
    entry: ControlPointKey,
    exit: ControlPointKey,
    points: Vec<ControlPoint>,
    edges: Vec<ControlEdge>,
}

impl ControlFlowGraph {
    pub fn key(&self) -> &ControlFlowGraphKey {
        &self.key
    }

    pub fn owner(&self) -> &NodeKey {
        &self.owner
    }

    pub fn owner_kind(&self) -> &ControlFlowOwnerKind {
        &self.owner_kind
    }

    pub fn grammar(&self) -> &GrammarSelection {
        &self.grammar
    }

    pub fn adapter(&self) -> &LanguageAdapterIdentity {
        &self.adapter
    }

    pub fn capability_support(&self) -> CapabilitySupport {
        self.capability_support
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn recovered_owner(&self) -> bool {
        self.recovered_owner
    }

    pub fn coverage(&self) -> &ControlFlowCoverageEvidence {
        &self.coverage
    }

    pub fn entry(&self) -> &ControlPointKey {
        &self.entry
    }

    pub fn exit(&self) -> &ControlPointKey {
        &self.exit
    }

    pub fn points(&self) -> &[ControlPoint] {
        &self.points
    }

    pub fn edges(&self) -> &[ControlEdge] {
        &self.edges
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    policy: ControlFlowPolicyId,
    graphs: Vec<ControlFlowGraph>,
}

impl ControlFlowDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn policy(&self) -> &ControlFlowPolicyId {
        &self.policy
    }

    pub fn graphs(&self) -> &[ControlFlowGraph] {
        &self.graphs
    }

    fn validate(&self) -> Result<(), ControlFlowBuildError> {
        if self.schema != CONTROL_FLOW_SCHEMA {
            return Err(ControlFlowBuildError::Invalid(format!(
                "unsupported control-flow schema {}",
                self.schema
            )));
        }
        validate_digest_id(self.projection_id.as_str(), "pj1_")?;
        validate_digest_id(&self.analysis_id, "pa1_")?;
        if self.graphs.is_empty() {
            return Err(ControlFlowBuildError::Invalid(
                "control-flow document cannot be empty".into(),
            ));
        }
        validate_sorted_unique_by_key("control-flow graphs", &self.graphs, |graph| {
            graph.key.as_str()
        })?;
        let mut owners = BTreeSet::new();
        for graph in &self.graphs {
            if !owners.insert(graph.owner.clone()) {
                return Err(ControlFlowBuildError::Invalid(
                    "control-flow document contains more than one graph for an owner".into(),
                ));
            }
            validate_graph(&self.policy, graph)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlFlowDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    policy: ControlFlowPolicyId,
    graphs: Vec<ControlFlowGraph>,
}

impl<'de> Deserialize<'de> for ControlFlowDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ControlFlowDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            policy: wire.policy,
            graphs: wire.graphs,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct ControlFlowProjection {
    id: ProjectionId,
    analysis: Arc<ProjectAnalysis>,
    policy: ControlFlowPolicyId,
    document: ControlFlowDocument,
}

impl ControlFlowProjection {
    pub fn schema(&self) -> &'static str {
        CONTROL_FLOW_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn analysis(&self) -> &Arc<ProjectAnalysis> {
        &self.analysis
    }

    pub fn policy(&self) -> &ControlFlowPolicyId {
        &self.policy
    }

    pub fn document(&self) -> &ControlFlowDocument {
        &self.document
    }
}

#[derive(Debug)]
pub struct ControlFlowBuilder {
    analysis: Arc<ProjectAnalysis>,
    policy: ControlFlowPolicyId,
    graphs: Vec<ControlFlowGraph>,
}

impl ControlFlowBuilder {
    pub fn new(analysis: Arc<ProjectAnalysis>, policy: ControlFlowPolicyId) -> Self {
        Self {
            analysis,
            policy,
            graphs: Vec::new(),
        }
    }

    pub fn add_graph(
        &mut self,
        draft: ControlFlowGraphDraft,
    ) -> Result<ControlFlowGraphKey, ControlFlowBuildError> {
        let owner = node_evidence(&self.analysis, draft.owner)?;
        validate_owner_kind(&draft.owner_kind)?;
        draft.coverage.validate()?;
        let entry = self
            .analysis
            .snapshot()
            .entry(owner.key.file().path.as_path())
            .ok_or_else(|| {
                ControlFlowBuildError::Node("graph owner file is not retained".into())
            })?;
        let adapter = entry.language_adapter_identity().ok_or_else(|| {
            ControlFlowBuildError::Node("graph owner has no stored language adapter".into())
        })?;
        let declaration = adapter
            .capabilities()
            .declaration(AdapterCapability::ControlFlow);

        let mut points = Vec::with_capacity(draft.points.len());
        for point in draft.points {
            validate_point_kind(&point.kind)?;
            let source = point
                .source
                .map(|node| node_evidence(&self.analysis, node))
                .transpose()?;
            if source
                .as_ref()
                .is_some_and(|source| source.key.file() != owner.key.file())
            {
                return Err(ControlFlowBuildError::ForeignFileNode);
            }
            let mut wire = ControlPoint {
                key: ControlPointKey(String::new()),
                kind: point.kind,
                source: source.as_ref().map(|source| source.key.clone()),
                ordinal: point.ordinal,
                recovered: source.as_ref().is_some_and(|source| source.recovered),
            };
            wire.key = derive_point_key(&self.policy, &owner.key, adapter, &wire)?;
            points.push(wire);
        }

        let original_point_keys = points
            .iter()
            .map(|point| point.key.clone())
            .collect::<Vec<_>>();
        let mut edges = Vec::with_capacity(draft.edges.len());
        for edge in draft.edges {
            validate_edge_kind(&edge.kind)?;
            edge.precision.validate()?;
            let from = original_point_keys.get(edge.from).ok_or(
                ControlFlowBuildError::PointOutOfRange {
                    requested: edge.from,
                    point_count: original_point_keys.len(),
                },
            )?;
            let to =
                original_point_keys
                    .get(edge.to)
                    .ok_or(ControlFlowBuildError::PointOutOfRange {
                        requested: edge.to,
                        point_count: original_point_keys.len(),
                    })?;
            let source = node_evidence(&self.analysis, edge.source)?;
            let predicate = edge
                .predicate
                .map(|node| node_evidence(&self.analysis, node))
                .transpose()?;
            if source.key.file() != owner.key.file()
                || predicate
                    .as_ref()
                    .is_some_and(|predicate| predicate.key.file() != owner.key.file())
            {
                return Err(ControlFlowBuildError::ForeignFileNode);
            }
            let mut wire = ControlEdge {
                key: ControlEdgeKey(String::new()),
                from: from.clone(),
                to: to.clone(),
                kind: edge.kind,
                source: source.key,
                predicate: predicate.as_ref().map(|predicate| predicate.key.clone()),
                precision: edge.precision,
                recovered_source: source.recovered,
                recovered_predicate: predicate.is_some_and(|predicate| predicate.recovered),
            };
            wire.key = derive_edge_key(&self.policy, &owner.key, adapter, &wire)?;
            edges.push(wire);
        }

        points.sort_by(|left, right| left.key.cmp(&right.key));
        edges.sort_by(|left, right| left.key.cmp(&right.key));
        let entries = points
            .iter()
            .filter(|point| point.kind == ControlPointKind::Entry)
            .map(|point| point.key.clone())
            .collect::<Vec<_>>();
        let exits = points
            .iter()
            .filter(|point| point.kind == ControlPointKind::Exit)
            .map(|point| point.key.clone())
            .collect::<Vec<_>>();
        if entries.len() != 1 || exits.len() != 1 {
            return Err(ControlFlowBuildError::Invalid(
                "control-flow graph requires exactly one entry and one exit point".into(),
            ));
        }
        let mut graph = ControlFlowGraph {
            key: ControlFlowGraphKey(String::new()),
            owner: owner.key,
            owner_kind: draft.owner_kind,
            grammar: owner.grammar,
            adapter: adapter.clone(),
            capability_support: declaration.support(),
            authority: declaration.authority(),
            recovered_owner: owner.recovered,
            coverage: draft.coverage,
            entry: entries[0].clone(),
            exit: exits[0].clone(),
            points,
            edges,
        };
        graph.key = derive_graph_key(&self.policy, &graph)?;
        validate_graph(&self.policy, &graph)?;
        if self
            .graphs
            .iter()
            .any(|existing| existing.owner == graph.owner)
        {
            return Err(ControlFlowBuildError::DuplicateOwner);
        }
        let key = graph.key.clone();
        self.graphs.push(graph);
        Ok(key)
    }

    pub fn build(mut self) -> Result<ControlFlowProjection, ControlFlowBuildError> {
        if self.graphs.is_empty() {
            return Err(ControlFlowBuildError::Invalid(
                "control-flow projection cannot be empty".into(),
            ));
        }
        self.graphs.sort_by(|left, right| left.key.cmp(&right.key));
        let payload = serde_json::to_vec(&(&self.policy, &self.graphs))
            .map_err(|error| ControlFlowBuildError::Identity(error.to_string()))?;
        let capabilities = capability_identity_bytes(&self.graphs);
        let id = self
            .analysis
            .derive_projection_id(CONTROL_FLOW_SCHEMA, &payload, &capabilities)
            .map_err(|error| ControlFlowBuildError::Identity(error.to_string()))?;
        let document = ControlFlowDocument {
            schema: CONTROL_FLOW_SCHEMA.to_string(),
            projection_id: id.clone(),
            analysis_id: self.analysis.id().as_str().to_string(),
            policy: self.policy.clone(),
            graphs: self.graphs,
        };
        document.validate()?;
        Ok(ControlFlowProjection {
            id,
            analysis: self.analysis,
            policy: self.policy,
            document,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowLoweringGap {
    path: PathBuf,
    adapter_schema: String,
    support: CapabilitySupport,
    reason: String,
}

impl ControlFlowLoweringGap {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn adapter_schema(&self) -> &str {
        &self.adapter_schema
    }

    pub fn support(&self) -> CapabilitySupport {
        self.support
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone)]
pub struct ControlFlowLoweringResult {
    projection: Option<ControlFlowProjection>,
    gaps: Vec<ControlFlowLoweringGap>,
}

impl ControlFlowLoweringResult {
    pub fn projection(&self) -> Option<&ControlFlowProjection> {
        self.projection.as_ref()
    }

    pub fn gaps(&self) -> &[ControlFlowLoweringGap] {
        &self.gaps
    }
}

/// Lower every executable owner whose exact stored adapter provides an applicable rule pack.
///
/// Files whose adapters declare ControlFlow Unknown or Unsupported remain explicit gaps. This function never
/// falls back to canonical roles, control-query captures, source-text heuristics, or the legacy project graph.
pub fn lower_control_flow(
    analysis: Arc<ProjectAnalysis>,
    policy: ControlFlowPolicyId,
) -> Result<ControlFlowLoweringResult, ControlFlowBuildError> {
    let mut drafts = Vec::new();
    let mut gaps = Vec::new();
    for file in analysis.files() {
        let path = file.key().path.clone();
        let identity = analysis
            .snapshot()
            .entry(&path)
            .and_then(|entry| entry.language_adapter_identity())
            .ok_or_else(|| {
                ControlFlowBuildError::Node(format!(
                    "source {} has no stored adapter identity",
                    path.display()
                ))
            })?;
        let rules = identity.control_flow_rules();
        if rules.support() != CapabilitySupport::Provided {
            gaps.push(ControlFlowLoweringGap {
                path,
                adapter_schema: identity.schema().to_string(),
                support: rules.support(),
                reason: format!(
                    "adapter {} declares ControlFlow {}",
                    identity.name(),
                    rules.support().as_str()
                ),
            });
            continue;
        }
        for owner in analysis.node_ids().filter(|node| {
            analysis.node(*node).is_ok_and(|view| {
                view.path() == path && rules.owner_rule(view.raw_kind(), view.text()).is_some()
            })
        }) {
            drafts.push(lower_owner(&analysis, owner, rules)?);
        }
    }
    gaps.sort();
    let projection = if drafts.is_empty() {
        None
    } else {
        let mut builder = ControlFlowBuilder::new(Arc::clone(&analysis), policy);
        for draft in drafts {
            builder.add_graph(draft)?;
        }
        Some(builder.build()?)
    };
    Ok(ControlFlowLoweringResult { projection, gaps })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingExitKind {
    Normal,
    Return,
    Break(Option<String>),
    Continue(Option<String>),
    Terminate,
}

#[derive(Debug, Clone)]
struct PendingExit {
    point: usize,
    kind: PendingExitKind,
    source: NodeId,
    precision: ControlEdgePrecision,
}

#[derive(Debug)]
struct LoweredFragment {
    start: usize,
    exits: Vec<PendingExit>,
}

struct OwnerLowerer<'a> {
    analysis: &'a ProjectAnalysis,
    rules: &'a deslop_lang::LanguageControlFlowRulePack,
    owner: NodeId,
    points: Vec<ControlPointDraft>,
    edges: Vec<ControlEdgeDraft>,
    uncertainty: BTreeSet<String>,
}

impl<'a> OwnerLowerer<'a> {
    fn push_point(&mut self, kind: ControlPointKind, source: Option<NodeId>) -> usize {
        let index = self.points.len();
        let ordinal = if matches!(kind, ControlPointKind::Entry | ControlPointKind::Exit) {
            0
        } else {
            self.points
                .iter()
                .filter(|point| point.kind == kind && point.source == source)
                .count() as u32
        };
        self.points.push(ControlPointDraft {
            kind,
            source,
            ordinal,
        });
        index
    }

    fn push_edge(
        &mut self,
        from: usize,
        to: usize,
        kind: ControlEdgeKind,
        source: NodeId,
        predicate: Option<NodeId>,
        precision: ControlEdgePrecision,
    ) {
        self.edges.push(ControlEdgeDraft {
            from,
            to,
            kind,
            source,
            predicate,
            precision,
        });
    }

    fn lower(&mut self, node: NodeId) -> Result<LoweredFragment, ControlFlowBuildError> {
        let view = self
            .analysis
            .node(node)
            .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
        if view.has_error() || view.is_error() || view.is_missing() {
            self.uncertainty
                .insert(format!("recovered syntax intersects {}", view.raw_kind()));
        }
        if node != self.owner
            && self
                .rules
                .owner_rule(view.raw_kind(), view.text())
                .is_some()
        {
            return Ok(self.leaf(node, ControlEdgePrecision::Exact));
        }
        let Some(rule) = self.rules.rule(view.raw_kind(), view.text()) else {
            return Ok(self.leaf(node, ControlEdgePrecision::Exact));
        };
        match rule.action() {
            ControlFlowAction::Sequence => self.lower_sequence(node),
            ControlFlowAction::Branch {
                condition_field,
                consequence_field,
                alternative_field,
            } => self.lower_branch(
                node,
                condition_field,
                consequence_field,
                alternative_field.as_deref(),
            ),
            ControlFlowAction::Loop {
                form,
                condition_field,
                body_field,
                alternative_field,
                label_kind,
            } => self.lower_loop(
                node,
                *form,
                condition_field.as_deref(),
                body_field,
                alternative_field.as_deref(),
                label_kind.as_deref(),
            ),
            ControlFlowAction::Abrupt {
                form,
                value_field,
                label_kind,
            } => self.lower_abrupt(node, form, value_field.as_deref(), label_kind.as_deref()),
            ControlFlowAction::OpaqueBoundary { reason } => {
                self.uncertainty.insert(reason.clone());
                let precision = ControlEdgePrecision::conservative(reason.clone())?;
                Ok(self.leaf(node, precision))
            }
            ControlFlowAction::Match { .. }
            | ControlFlowAction::Exceptional { .. }
            | ControlFlowAction::Suspension { .. }
            | ControlFlowAction::AdapterDefined { .. } => {
                let reason = format!(
                    "{} lowering is retained but not implemented by the shared M4.2 traversal",
                    view.raw_kind()
                );
                self.uncertainty.insert(reason.clone());
                Ok(self.leaf(node, ControlEdgePrecision::conservative(reason)?))
            }
        }
    }

    fn scan_uncertainty(&mut self, node: NodeId) -> Result<(), ControlFlowBuildError> {
        let view = self
            .analysis
            .node(node)
            .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
        if view.has_error() || view.is_error() || view.is_missing() {
            self.uncertainty
                .insert(format!("recovered syntax intersects {}", view.raw_kind()));
        }
        if node != self.owner
            && self
                .rules
                .owner_rule(view.raw_kind(), view.text())
                .is_some()
        {
            return Ok(());
        }
        if let Some(rule) = self.rules.rule(view.raw_kind(), view.text()) {
            match rule.action() {
                ControlFlowAction::OpaqueBoundary { reason } => {
                    self.uncertainty.insert(reason.clone());
                }
                ControlFlowAction::Match { .. }
                | ControlFlowAction::Exceptional { .. }
                | ControlFlowAction::Suspension { .. }
                | ControlFlowAction::AdapterDefined { .. } => {
                    self.uncertainty.insert(format!(
                        "{} lowering is retained but not implemented by the shared M4.2 traversal",
                        view.raw_kind()
                    ));
                }
                _ => {}
            }
        }
        let children = view.children().collect::<Vec<_>>();
        for child in children {
            self.scan_uncertainty(child)?;
        }
        Ok(())
    }

    fn contains_unlowered_control(&self, node: NodeId) -> Result<bool, ControlFlowBuildError> {
        let view = self
            .analysis
            .node(node)
            .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
        if node != self.owner
            && self
                .rules
                .owner_rule(view.raw_kind(), view.text())
                .is_some()
        {
            return Ok(false);
        }
        if self
            .rules
            .rule(view.raw_kind(), view.text())
            .is_some_and(|rule| {
                matches!(
                    rule.action(),
                    ControlFlowAction::Branch { .. }
                        | ControlFlowAction::Match { .. }
                        | ControlFlowAction::Loop { .. }
                        | ControlFlowAction::Abrupt { .. }
                        | ControlFlowAction::Exceptional { .. }
                        | ControlFlowAction::Suspension { .. }
                        | ControlFlowAction::AdapterDefined { .. }
                )
            })
        {
            return Ok(true);
        }
        for child in view.children() {
            if self.contains_unlowered_control(child)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn leaf(&mut self, node: NodeId, precision: ControlEdgePrecision) -> LoweredFragment {
        let point = self.push_point(ControlPointKind::Syntax, Some(node));
        LoweredFragment {
            start: point,
            exits: vec![PendingExit {
                point,
                kind: PendingExitKind::Normal,
                source: node,
                precision,
            }],
        }
    }

    fn lower_sequence(&mut self, node: NodeId) -> Result<LoweredFragment, ControlFlowBuildError> {
        let children = named_children(self.analysis, node)?;
        if children.is_empty() {
            let point = self.push_point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::NoOp),
                Some(node),
            );
            return Ok(LoweredFragment {
                start: point,
                exits: vec![PendingExit {
                    point,
                    kind: PendingExitKind::Normal,
                    source: node,
                    precision: ControlEdgePrecision::Exact,
                }],
            });
        }
        let mut fragments = children
            .into_iter()
            .map(|child| self.lower(child))
            .collect::<Result<Vec<_>, _>>()?;
        let start = fragments[0].start;
        let mut carried = Vec::new();
        let mut current = fragments.remove(0);
        for next in fragments {
            let mut reached_next = false;
            for exit in current.exits {
                if exit.kind == PendingExitKind::Normal {
                    reached_next = true;
                    self.push_edge(
                        exit.point,
                        next.start,
                        ControlEdgeKind::Normal,
                        exit.source,
                        None,
                        exit.precision,
                    );
                } else {
                    carried.push(exit);
                }
            }
            current = if reached_next {
                next
            } else {
                LoweredFragment {
                    start: next.start,
                    exits: Vec::new(),
                }
            };
        }
        carried.extend(current.exits);
        Ok(LoweredFragment {
            start,
            exits: carried,
        })
    }

    fn lower_branch(
        &mut self,
        node: NodeId,
        condition_field: &str,
        consequence_field: &str,
        alternative_field: Option<&str>,
    ) -> Result<LoweredFragment, ControlFlowBuildError> {
        let condition = required_child(self.analysis, node, condition_field)?;
        if self.contains_unlowered_control(condition)? {
            self.uncertainty
                .insert("nested control in branch condition is not lowered yet".into());
        }
        let consequence = required_child(self.analysis, node, consequence_field)?;
        let alternative = alternative_field
            .map(|field| child_by_field(self.analysis, node, field))
            .transpose()?
            .flatten();
        let dispatch = self.push_point(
            ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch),
            Some(node),
        );
        let merge = self.push_point(
            ControlPointKind::Synthetic(ControlSyntheticPointKind::Merge),
            Some(node),
        );
        let then_fragment = self.lower(consequence)?;
        self.push_edge(
            dispatch,
            then_fragment.start,
            ControlEdgeKind::Branch(ControlBranchKind::True),
            node,
            Some(condition),
            ControlEdgePrecision::Exact,
        );
        let mut exits = Vec::new();
        let mut merge_reachable = self.join_branch_exits(then_fragment.exits, merge, &mut exits);
        if let Some(alternative) = alternative {
            let else_fragment = self.lower(alternative)?;
            self.push_edge(
                dispatch,
                else_fragment.start,
                ControlEdgeKind::Branch(ControlBranchKind::False),
                node,
                Some(condition),
                ControlEdgePrecision::Exact,
            );
            merge_reachable |= self.join_branch_exits(else_fragment.exits, merge, &mut exits);
        } else {
            self.push_edge(
                dispatch,
                merge,
                ControlEdgeKind::Branch(ControlBranchKind::False),
                node,
                Some(condition),
                ControlEdgePrecision::Exact,
            );
            merge_reachable = true;
        }
        if merge_reachable {
            exits.push(PendingExit {
                point: merge,
                kind: PendingExitKind::Normal,
                source: node,
                precision: ControlEdgePrecision::Exact,
            });
        }
        Ok(LoweredFragment {
            start: dispatch,
            exits,
        })
    }

    fn join_branch_exits(
        &mut self,
        branch_exits: Vec<PendingExit>,
        merge: usize,
        carried: &mut Vec<PendingExit>,
    ) -> bool {
        let mut merge_reachable = false;
        for exit in branch_exits {
            if exit.kind == PendingExitKind::Normal {
                merge_reachable = true;
                self.push_edge(
                    exit.point,
                    merge,
                    ControlEdgeKind::Normal,
                    exit.source,
                    None,
                    exit.precision,
                );
            } else {
                carried.push(exit);
            }
        }
        merge_reachable
    }

    fn lower_loop(
        &mut self,
        node: NodeId,
        form: ControlLoopForm,
        condition_field: Option<&str>,
        body_field: &str,
        alternative_field: Option<&str>,
        label_kind: Option<&str>,
    ) -> Result<LoweredFragment, ControlFlowBuildError> {
        let condition = condition_field
            .map(|field| required_child(self.analysis, node, field))
            .transpose()?;
        if let Some(condition) = condition
            && self.contains_unlowered_control(condition)?
        {
            self.uncertainty
                .insert("nested control in loop condition is not lowered yet".into());
        }
        let body = required_child(self.analysis, node, body_field)?;
        let alternative = alternative_field
            .map(|field| child_by_field(self.analysis, node, field))
            .transpose()?
            .flatten();
        let loop_label = label_kind
            .map(|kind| child_text_by_kind(self.analysis, node, kind))
            .transpose()?
            .flatten();
        let header = self.push_point(
            ControlPointKind::Synthetic(ControlSyntheticPointKind::LoopHeader),
            Some(node),
        );
        let after = self.push_point(
            ControlPointKind::Synthetic(ControlSyntheticPointKind::Merge),
            Some(node),
        );
        let body_fragment = self.lower(body)?;
        self.push_edge(
            header,
            body_fragment.start,
            ControlEdgeKind::Loop(ControlLoopKind::Body),
            node,
            condition,
            ControlEdgePrecision::Exact,
        );
        let mut exits = Vec::new();
        let mut has_break = false;
        for exit in body_fragment.exits {
            match exit.kind {
                PendingExitKind::Normal => self.push_edge(
                    exit.point,
                    header,
                    ControlEdgeKind::Loop(ControlLoopKind::Back),
                    exit.source,
                    None,
                    exit.precision,
                ),
                PendingExitKind::Continue(ref label)
                    if label.is_none() || label.as_ref() == loop_label.as_ref() =>
                {
                    self.push_edge(
                        exit.point,
                        header,
                        ControlEdgeKind::Abrupt(ControlAbruptKind::Continue {
                            label: label.clone(),
                        }),
                        exit.source,
                        None,
                        exit.precision,
                    );
                }
                PendingExitKind::Break(ref label)
                    if label.is_none() || label.as_ref() == loop_label.as_ref() =>
                {
                    has_break = true;
                    self.push_edge(
                        exit.point,
                        after,
                        ControlEdgeKind::Abrupt(ControlAbruptKind::Break {
                            label: label.clone(),
                        }),
                        exit.source,
                        None,
                        exit.precision,
                    );
                }
                _ => exits.push(exit),
            }
        }
        if form != ControlLoopForm::Infinite {
            if let Some(alternative) = alternative {
                let alternative_fragment = self.lower(alternative)?;
                self.push_edge(
                    header,
                    alternative_fragment.start,
                    ControlEdgeKind::Loop(ControlLoopKind::ConditionFalse),
                    node,
                    condition,
                    ControlEdgePrecision::Exact,
                );
                for exit in alternative_fragment.exits {
                    if exit.kind == PendingExitKind::Normal {
                        self.push_edge(
                            exit.point,
                            after,
                            ControlEdgeKind::Normal,
                            exit.source,
                            None,
                            exit.precision,
                        );
                    } else {
                        exits.push(exit);
                    }
                }
            } else {
                self.push_edge(
                    header,
                    after,
                    ControlEdgeKind::Loop(ControlLoopKind::ConditionFalse),
                    node,
                    condition,
                    ControlEdgePrecision::Exact,
                );
            }
        }
        if form != ControlLoopForm::Infinite || has_break {
            exits.push(PendingExit {
                point: after,
                kind: PendingExitKind::Normal,
                source: node,
                precision: ControlEdgePrecision::Exact,
            });
        }
        Ok(LoweredFragment {
            start: header,
            exits,
        })
    }

    fn lower_abrupt(
        &mut self,
        node: NodeId,
        form: &ControlAbruptForm,
        value_field: Option<&str>,
        label_kind: Option<&str>,
    ) -> Result<LoweredFragment, ControlFlowBuildError> {
        let point = self.push_point(ControlPointKind::Syntax, Some(node));
        let label = label_kind
            .map(|kind| child_text_by_kind(self.analysis, node, kind))
            .transpose()?
            .flatten();
        let declared_value = value_field
            .map(|field| child_by_field(self.analysis, node, field))
            .transpose()?
            .flatten();
        let value_children = if let Some(value) = declared_value {
            vec![value]
        } else {
            named_children(self.analysis, node)?
                .into_iter()
                .filter(|child| {
                    self.analysis
                        .node(*child)
                        .is_ok_and(|view| label_kind.is_none_or(|kind| view.raw_kind() != kind))
                })
                .collect()
        };
        let mut has_unlowered_value_control = false;
        for value in value_children {
            has_unlowered_value_control |= self.contains_unlowered_control(value)?;
        }
        let precision = if has_unlowered_value_control {
            let reason = format!(
                "nested control in {} value is not lowered yet",
                self.analysis
                    .node(node)
                    .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?
                    .raw_kind()
            );
            self.uncertainty.insert(reason.clone());
            ControlEdgePrecision::conservative(reason)?
        } else {
            ControlEdgePrecision::Exact
        };
        let kind = match form {
            ControlAbruptForm::Return => PendingExitKind::Return,
            ControlAbruptForm::Break => PendingExitKind::Break(label),
            ControlAbruptForm::Continue => PendingExitKind::Continue(label),
            ControlAbruptForm::Terminate => PendingExitKind::Terminate,
            ControlAbruptForm::Goto | ControlAbruptForm::AdapterDefined { .. } => {
                let reason = format!(
                    "{} abrupt target is not statically modeled",
                    self.analysis
                        .node(node)
                        .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?
                        .raw_kind()
                );
                self.uncertainty.insert(reason.clone());
                return Ok(LoweredFragment {
                    start: point,
                    exits: vec![PendingExit {
                        point,
                        kind: PendingExitKind::Normal,
                        source: node,
                        precision: ControlEdgePrecision::conservative(reason)?,
                    }],
                });
            }
        };
        Ok(LoweredFragment {
            start: point,
            exits: vec![PendingExit {
                point,
                kind,
                source: node,
                precision,
            }],
        })
    }
}

fn lower_owner(
    analysis: &ProjectAnalysis,
    owner: NodeId,
    rules: &deslop_lang::LanguageControlFlowRulePack,
) -> Result<ControlFlowGraphDraft, ControlFlowBuildError> {
    let owner_view = analysis
        .node(owner)
        .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
    let owner_rule = rules
        .owner_rule(owner_view.raw_kind(), owner_view.text())
        .ok_or_else(|| ControlFlowBuildError::Invalid("missing exact owner rule".into()))?;
    let body = required_child(analysis, owner, owner_rule.body_field())?;
    let mut lowerer = OwnerLowerer {
        analysis,
        rules,
        owner,
        points: Vec::new(),
        edges: Vec::new(),
        uncertainty: BTreeSet::new(),
    };
    if matches!(
        rules.evaluation_order(),
        Some(ControlEvaluationOrder::Unspecified)
    ) {
        lowerer
            .uncertainty
            .insert("adapter evaluation order is unspecified".into());
    }
    let entry = lowerer.push_point(ControlPointKind::Entry, None);
    let exit = lowerer.push_point(ControlPointKind::Exit, None);
    lowerer.scan_uncertainty(body)?;
    let fragment = lowerer.lower(body)?;
    lowerer.push_edge(
        entry,
        fragment.start,
        ControlEdgeKind::Entry,
        owner,
        None,
        ControlEdgePrecision::Exact,
    );
    let mut dispatches = BTreeMap::new();
    for pending in fragment.exits {
        let outcome = match pending.kind {
            PendingExitKind::Normal => ControlExitOutcome::Normal,
            PendingExitKind::Return | PendingExitKind::Terminate => ControlExitOutcome::Abrupt,
            PendingExitKind::Break(_) | PendingExitKind::Continue(_) => {
                let reason = "break/continue escaped its owning loop".to_string();
                lowerer.uncertainty.insert(reason.clone());
                ControlExitOutcome::Abrupt
            }
        };
        let dispatch = *dispatches.entry(outcome).or_insert_with(|| {
            lowerer.push_point(
                ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                Some(owner),
            )
        });
        let kind = match pending.kind {
            PendingExitKind::Normal => ControlEdgeKind::Normal,
            PendingExitKind::Return => ControlEdgeKind::Abrupt(ControlAbruptKind::Return),
            PendingExitKind::Terminate => ControlEdgeKind::Abrupt(ControlAbruptKind::Terminate),
            PendingExitKind::Break(label) => {
                ControlEdgeKind::Abrupt(ControlAbruptKind::Break { label })
            }
            PendingExitKind::Continue(label) => {
                ControlEdgeKind::Abrupt(ControlAbruptKind::Continue { label })
            }
        };
        lowerer.push_edge(
            pending.point,
            dispatch,
            kind,
            pending.source,
            None,
            pending.precision,
        );
    }
    if dispatches.is_empty() {
        let reason = "executable owner has no modeled exit path".to_string();
        lowerer.uncertainty.insert(reason.clone());
        let dispatch = lowerer.push_point(
            ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
            Some(owner),
        );
        dispatches.insert(ControlExitOutcome::Normal, dispatch);
    }
    for (outcome, dispatch) in dispatches {
        lowerer.push_edge(
            dispatch,
            exit,
            ControlEdgeKind::Exit(outcome),
            owner,
            None,
            ControlEdgePrecision::Exact,
        );
    }
    let coverage = if lowerer.uncertainty.is_empty() {
        ControlFlowCoverageEvidence::complete()
    } else {
        ControlFlowCoverageEvidence::partial(lowerer.uncertainty.into_iter().collect())?
    };
    let owner_kind = match owner_rule.kind() {
        ControlFlowOwnerRuleKind::Callable => ControlFlowOwnerKind::Callable,
        ControlFlowOwnerRuleKind::Initializer => ControlFlowOwnerKind::Initializer,
        ControlFlowOwnerRuleKind::ModuleInitializer => ControlFlowOwnerKind::ModuleInitializer,
        ControlFlowOwnerRuleKind::AdapterDefined { schema, name } => {
            ControlFlowOwnerKind::AdapterDefined {
                schema: schema.clone(),
                name: name.clone(),
            }
        }
    };
    Ok(ControlFlowGraphDraft {
        owner,
        owner_kind,
        coverage,
        points: lowerer.points,
        edges: lowerer.edges,
    })
}

fn named_children(
    analysis: &ProjectAnalysis,
    node: NodeId,
) -> Result<Vec<NodeId>, ControlFlowBuildError> {
    let view = analysis
        .node(node)
        .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
    let mut children = Vec::new();
    for child in view.children() {
        let child_view = analysis
            .node(child)
            .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
        if child_view.is_named() && !child_view.is_extra() {
            children.push(child);
        }
    }
    Ok(children)
}

fn child_by_field(
    analysis: &ProjectAnalysis,
    node: NodeId,
    field: &str,
) -> Result<Option<NodeId>, ControlFlowBuildError> {
    let view = analysis
        .node(node)
        .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
    for child in view.children() {
        let child_view = analysis
            .node(child)
            .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
        if child_view.field() == Some(field) {
            return Ok(Some(child));
        }
    }
    Ok(None)
}

fn child_text_by_kind(
    analysis: &ProjectAnalysis,
    node: NodeId,
    raw_kind: &str,
) -> Result<Option<String>, ControlFlowBuildError> {
    let view = analysis
        .node(node)
        .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
    for child in view.children() {
        let child_view = analysis
            .node(child)
            .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
        if child_view.raw_kind() == raw_kind {
            return Ok(Some(child_view.text().to_string()));
        }
    }
    Ok(None)
}

fn required_child(
    analysis: &ProjectAnalysis,
    node: NodeId,
    field: &str,
) -> Result<NodeId, ControlFlowBuildError> {
    child_by_field(analysis, node, field)?.ok_or_else(|| {
        let raw_kind = analysis
            .node(node)
            .map(|view| view.raw_kind().to_string())
            .unwrap_or_else(|_| "<foreign>".into());
        ControlFlowBuildError::Invalid(format!(
            "control-flow rule expected field {field} on {raw_kind}"
        ))
    })
}

#[derive(Debug, Clone)]
struct NodeEvidence {
    key: NodeKey,
    grammar: GrammarSelection,
    recovered: bool,
}

fn node_evidence(
    analysis: &ProjectAnalysis,
    node: NodeId,
) -> Result<NodeEvidence, ControlFlowBuildError> {
    let view = analysis
        .node(node)
        .map_err(|error| ControlFlowBuildError::Node(error.to_string()))?;
    Ok(NodeEvidence {
        key: view.key().clone(),
        grammar: view.grammar().clone(),
        recovered: view.has_error() || view.is_error() || view.is_missing(),
    })
}

fn validate_graph(
    policy: &ControlFlowPolicyId,
    graph: &ControlFlowGraph,
) -> Result<(), ControlFlowBuildError> {
    validate_owner_kind(&graph.owner_kind)?;
    if !graph.owner.is_supported() {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow graph owner has an unsupported node key".into(),
        ));
    }
    if graph.grammar != graph.owner.file().grammar {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow graph grammar disagrees with its owner node".into(),
        ));
    }
    graph
        .adapter
        .capabilities()
        .validate()
        .map_err(ControlFlowBuildError::Invalid)?;
    if graph.adapter.schema() != graph.adapter.capabilities().adapter_schema() {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow adapter schema disagrees with its capability manifest".into(),
        ));
    }
    let declaration = graph
        .adapter
        .capabilities()
        .declaration(AdapterCapability::ControlFlow);
    if declaration.support() != graph.capability_support
        || declaration.authority() != graph.authority
    {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow evidence disagrees with the stored capability declaration".into(),
        ));
    }
    graph.coverage.validate()?;
    validate_coverage_support(graph.coverage.status, graph.capability_support)?;
    if graph.coverage.status == FactCoverage::Complete {
        if !matches!(
            graph.authority,
            Some(
                CapabilityAuthority::Adapter
                    | CapabilityAuthority::LanguageServer
                    | CapabilityAuthority::Compiler
            )
        ) {
            return Err(ControlFlowBuildError::Invalid(
                "complete control flow requires static adapter, language-server, or compiler authority"
                    .into(),
            ));
        }
        if graph.recovered_owner {
            return Err(ControlFlowBuildError::Invalid(
                "a recovered owner cannot have complete control-flow coverage".into(),
            ));
        }
    }
    if graph.points.len() < 3 {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow graph requires entry, exit, and at least one body point".into(),
        ));
    }
    validate_sorted_unique_by_key("control points", &graph.points, |point| point.key.as_str())?;
    validate_sorted_unique_by_key("control edges", &graph.edges, |edge| edge.key.as_str())?;

    let mut points = BTreeMap::new();
    let mut entry_count = 0;
    let mut exit_count = 0;
    for point in &graph.points {
        validate_point(policy, &graph.owner, &graph.adapter, point)?;
        if point
            .source
            .as_ref()
            .is_some_and(|source| source.file() != graph.owner.file())
        {
            return Err(ControlFlowBuildError::ForeignFileNode);
        }
        if point
            .source
            .as_ref()
            .is_some_and(|source| !node_is_within(source, &graph.owner))
        {
            return Err(ControlFlowBuildError::OutsideOwnerRegion);
        }
        match point.kind {
            ControlPointKind::Entry => entry_count += 1,
            ControlPointKind::Exit => exit_count += 1,
            _ => {}
        }
        points.insert(point.key.clone(), &point.kind);
    }
    if entry_count != 1 || exit_count != 1 {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow graph requires exactly one entry and one exit point".into(),
        ));
    }
    if points.get(&graph.entry) != Some(&&ControlPointKind::Entry)
        || points.get(&graph.exit) != Some(&&ControlPointKind::Exit)
    {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow boundary keys do not identify the unique entry and exit".into(),
        ));
    }

    let mut semantic_edges = BTreeSet::new();
    let mut entry_edges = 0;
    let mut exit_edges = 0;
    for edge in &graph.edges {
        validate_edge(policy, &graph.owner, &graph.adapter, edge)?;
        if edge.source.file() != graph.owner.file()
            || edge
                .predicate
                .as_ref()
                .is_some_and(|predicate| predicate.file() != graph.owner.file())
        {
            return Err(ControlFlowBuildError::ForeignFileNode);
        }
        if !node_is_within(&edge.source, &graph.owner)
            || edge
                .predicate
                .as_ref()
                .is_some_and(|predicate| !node_is_within(predicate, &graph.owner))
        {
            return Err(ControlFlowBuildError::OutsideOwnerRegion);
        }
        let from_kind = points
            .get(&edge.from)
            .ok_or_else(|| ControlFlowBuildError::DanglingPoint(edge.from.as_str().into()))?;
        let to_kind = points
            .get(&edge.to)
            .ok_or_else(|| ControlFlowBuildError::DanglingPoint(edge.to.as_str().into()))?;
        match edge.kind {
            ControlEdgeKind::Entry => {
                entry_edges += 1;
                if **from_kind != ControlPointKind::Entry || **to_kind == ControlPointKind::Entry {
                    return Err(ControlFlowBuildError::Invalid(
                        "entry edges must leave the unique entry point".into(),
                    ));
                }
            }
            ControlEdgeKind::Exit(_) => {
                exit_edges += 1;
                if **to_kind != ControlPointKind::Exit || **from_kind == ControlPointKind::Exit {
                    return Err(ControlFlowBuildError::Invalid(
                        "exit edges must enter the unique exit point".into(),
                    ));
                }
            }
            _ => {
                if matches!(
                    **from_kind,
                    ControlPointKind::Entry | ControlPointKind::Exit
                ) || matches!(**to_kind, ControlPointKind::Entry | ControlPointKind::Exit)
                {
                    return Err(ControlFlowBuildError::Invalid(
                        "only entry/exit edge families may touch virtual boundaries".into(),
                    ));
                }
            }
        }
        if **to_kind == ControlPointKind::Entry || **from_kind == ControlPointKind::Exit {
            return Err(ControlFlowBuildError::Invalid(
                "entry cannot have incoming flow and exit cannot have outgoing flow".into(),
            ));
        }
        if !semantic_edges.insert((edge.from.clone(), edge.to.clone(), edge.kind.clone())) {
            return Err(ControlFlowBuildError::Invalid(
                "control-flow graph contains a duplicate semantic edge".into(),
            ));
        }
        if graph.coverage.status == FactCoverage::Complete
            && (!matches!(edge.precision, ControlEdgePrecision::Exact)
                || edge.recovered_source
                || edge.recovered_predicate)
        {
            return Err(ControlFlowBuildError::Invalid(
                "complete control flow cannot retain conservative or recovered edge evidence"
                    .into(),
            ));
        }
    }
    if entry_edges == 0 || exit_edges == 0 {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow graph requires explicit entry and exit edges".into(),
        ));
    }
    if graph.coverage.status == FactCoverage::Complete
        && graph.points.iter().any(|point| point.recovered)
    {
        return Err(ControlFlowBuildError::Invalid(
            "complete control flow cannot retain recovered control points".into(),
        ));
    }
    let expected = derive_graph_key(policy, graph)?;
    if expected != graph.key {
        return Err(ControlFlowBuildError::Invalid(
            "control-flow graph key does not bind its complete payload".into(),
        ));
    }
    Ok(())
}

fn validate_point(
    policy: &ControlFlowPolicyId,
    owner: &NodeKey,
    adapter: &LanguageAdapterIdentity,
    point: &ControlPoint,
) -> Result<(), ControlFlowBuildError> {
    validate_point_kind(&point.kind)?;
    match (&point.kind, &point.source, point.ordinal, point.recovered) {
        (ControlPointKind::Entry | ControlPointKind::Exit, None, 0, false) => {}
        (ControlPointKind::Entry | ControlPointKind::Exit, ..) => {
            return Err(ControlFlowBuildError::Invalid(
                "virtual entry/exit points require no source, ordinal zero, and no recovery".into(),
            ));
        }
        (ControlPointKind::Syntax | ControlPointKind::Synthetic(_), Some(source), _, _) => {
            if !source.is_supported() {
                return Err(ControlFlowBuildError::Invalid(
                    "control point has an unsupported source node key".into(),
                ));
            }
        }
        (_, None, _, _) => {
            return Err(ControlFlowBuildError::Invalid(
                "non-virtual control points require exact source-node evidence".into(),
            ));
        }
    }
    let expected = derive_point_key(policy, owner, adapter, point)?;
    if expected != point.key {
        return Err(ControlFlowBuildError::Invalid(
            "control-point key does not bind its complete payload".into(),
        ));
    }
    Ok(())
}

fn validate_edge(
    policy: &ControlFlowPolicyId,
    owner: &NodeKey,
    adapter: &LanguageAdapterIdentity,
    edge: &ControlEdge,
) -> Result<(), ControlFlowBuildError> {
    validate_edge_kind(&edge.kind)?;
    edge.precision.validate()?;
    if !edge.source.is_supported()
        || edge
            .predicate
            .as_ref()
            .is_some_and(|node| !node.is_supported())
    {
        return Err(ControlFlowBuildError::Invalid(
            "control edge has unsupported source-node evidence".into(),
        ));
    }
    if edge.predicate.is_none() && edge.recovered_predicate {
        return Err(ControlFlowBuildError::Invalid(
            "control edge claims recovered predicate without predicate evidence".into(),
        ));
    }
    let expected = derive_edge_key(policy, owner, adapter, edge)?;
    if expected != edge.key {
        return Err(ControlFlowBuildError::Invalid(
            "control-edge key does not bind its complete payload".into(),
        ));
    }
    Ok(())
}

fn validate_coverage_support(
    coverage: FactCoverage,
    support: CapabilitySupport,
) -> Result<(), ControlFlowBuildError> {
    match (coverage, support) {
        (FactCoverage::Complete, CapabilitySupport::Provided) => Ok(()),
        (FactCoverage::Complete, _) => Err(ControlFlowBuildError::Invalid(
            "complete control-flow coverage requires a provided capability".into(),
        )),
        (
            FactCoverage::Unsupported,
            CapabilitySupport::Provided | CapabilitySupport::Unsupported,
        ) => Ok(()),
        (FactCoverage::Unsupported, CapabilitySupport::Unknown) => {
            Err(ControlFlowBuildError::Invalid(
                "unsupported control-flow coverage requires an explicit adapter declaration".into(),
            ))
        }
        (FactCoverage::Partial, CapabilitySupport::Unsupported) => {
            Err(ControlFlowBuildError::Invalid(
                "partial control-flow coverage contradicts unsupported capability".into(),
            ))
        }
        (FactCoverage::Failed, CapabilitySupport::Unsupported) => {
            Err(ControlFlowBuildError::Invalid(
                "failed control-flow coverage contradicts unsupported capability".into(),
            ))
        }
        _ => Ok(()),
    }
}

fn validate_owner_kind(kind: &ControlFlowOwnerKind) -> Result<(), ControlFlowBuildError> {
    if let ControlFlowOwnerKind::AdapterDefined { schema, name } = kind {
        validate_adapter_pair("control-flow owner kind", schema, name)?;
    }
    Ok(())
}

fn validate_point_kind(kind: &ControlPointKind) -> Result<(), ControlFlowBuildError> {
    if let ControlPointKind::Synthetic(ControlSyntheticPointKind::AdapterDefined { schema, name }) =
        kind
    {
        validate_adapter_pair("synthetic control-point kind", schema, name)?;
    }
    Ok(())
}

fn validate_edge_kind(kind: &ControlEdgeKind) -> Result<(), ControlFlowBuildError> {
    match kind {
        ControlEdgeKind::Branch(ControlBranchKind::Case { label }) => {
            validate_text("control branch case label", label)
        }
        ControlEdgeKind::Branch(ControlBranchKind::AdapterDefined { schema, name }) => {
            validate_adapter_pair("control branch kind", schema, name)
        }
        ControlEdgeKind::Exceptional(ControlExceptionalKind::AdapterDefined { schema, name }) => {
            validate_adapter_pair("exceptional control kind", schema, name)
        }
        ControlEdgeKind::Abrupt(
            ControlAbruptKind::Break { label } | ControlAbruptKind::Continue { label },
        ) => {
            if let Some(label) = label {
                validate_text("abrupt control label", label)?;
            }
            Ok(())
        }
        ControlEdgeKind::Abrupt(ControlAbruptKind::Goto { label }) => {
            validate_text("goto control label", label)
        }
        ControlEdgeKind::Abrupt(ControlAbruptKind::AdapterDefined { schema, name }) => {
            validate_adapter_pair("abrupt control kind", schema, name)
        }
        ControlEdgeKind::Suspension(ControlSuspensionKind::AdapterDefined { schema, name }) => {
            validate_adapter_pair("suspension control kind", schema, name)
        }
        _ => Ok(()),
    }
}

#[derive(Serialize)]
struct PointKeyPayload<'a> {
    policy: &'a ControlFlowPolicyId,
    owner: &'a NodeKey,
    adapter: &'a LanguageAdapterIdentity,
    kind: &'a ControlPointKind,
    source: &'a Option<NodeKey>,
    ordinal: u32,
    recovered: bool,
}

fn derive_point_key(
    policy: &ControlFlowPolicyId,
    owner: &NodeKey,
    adapter: &LanguageAdapterIdentity,
    point: &ControlPoint,
) -> Result<ControlPointKey, ControlFlowBuildError> {
    derive_serialized_id(
        POINT_KEY_DOMAIN,
        "cpt1_",
        &PointKeyPayload {
            policy,
            owner,
            adapter,
            kind: &point.kind,
            source: &point.source,
            ordinal: point.ordinal,
            recovered: point.recovered,
        },
    )
    .map(ControlPointKey)
}

#[derive(Serialize)]
struct EdgeKeyPayload<'a> {
    policy: &'a ControlFlowPolicyId,
    owner: &'a NodeKey,
    adapter: &'a LanguageAdapterIdentity,
    from: &'a ControlPointKey,
    to: &'a ControlPointKey,
    kind: &'a ControlEdgeKind,
    source: &'a NodeKey,
    predicate: &'a Option<NodeKey>,
    precision: &'a ControlEdgePrecision,
    recovered_source: bool,
    recovered_predicate: bool,
}

fn derive_edge_key(
    policy: &ControlFlowPolicyId,
    owner: &NodeKey,
    adapter: &LanguageAdapterIdentity,
    edge: &ControlEdge,
) -> Result<ControlEdgeKey, ControlFlowBuildError> {
    derive_serialized_id(
        EDGE_KEY_DOMAIN,
        "ced1_",
        &EdgeKeyPayload {
            policy,
            owner,
            adapter,
            from: &edge.from,
            to: &edge.to,
            kind: &edge.kind,
            source: &edge.source,
            predicate: &edge.predicate,
            precision: &edge.precision,
            recovered_source: edge.recovered_source,
            recovered_predicate: edge.recovered_predicate,
        },
    )
    .map(ControlEdgeKey)
}

#[derive(Serialize)]
struct GraphKeyPayload<'a> {
    policy: &'a ControlFlowPolicyId,
    owner: &'a NodeKey,
    owner_kind: &'a ControlFlowOwnerKind,
    grammar: &'a GrammarSelection,
    adapter: &'a LanguageAdapterIdentity,
    capability_support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
    recovered_owner: bool,
    coverage: &'a ControlFlowCoverageEvidence,
    entry: &'a ControlPointKey,
    exit: &'a ControlPointKey,
    points: &'a [ControlPoint],
    edges: &'a [ControlEdge],
}

fn derive_graph_key(
    policy: &ControlFlowPolicyId,
    graph: &ControlFlowGraph,
) -> Result<ControlFlowGraphKey, ControlFlowBuildError> {
    derive_serialized_id(
        GRAPH_KEY_DOMAIN,
        "cfg1_",
        &GraphKeyPayload {
            policy,
            owner: &graph.owner,
            owner_kind: &graph.owner_kind,
            grammar: &graph.grammar,
            adapter: &graph.adapter,
            capability_support: graph.capability_support,
            authority: graph.authority,
            recovered_owner: graph.recovered_owner,
            coverage: &graph.coverage,
            entry: &graph.entry,
            exit: &graph.exit,
            points: &graph.points,
            edges: &graph.edges,
        },
    )
    .map(ControlFlowGraphKey)
}

fn derive_serialized_id<T: Serialize>(
    domain: &str,
    prefix: &str,
    value: &T,
) -> Result<String, ControlFlowBuildError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| ControlFlowBuildError::Identity(error.to_string()))?;
    Ok(derive_parts_id(domain, prefix, &[&bytes]))
}

fn derive_parts_id(domain: &str, prefix: &str, parts: &[&[u8]]) -> String {
    let mut hasher = blake3::Hasher::new();
    hash_part(&mut hasher, domain.as_bytes());
    for part in parts {
        hash_part(&mut hasher, part);
    }
    format!("{prefix}{}", hasher.finalize().to_hex())
}

fn hash_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn capability_identity_bytes(graphs: &[ControlFlowGraph]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for graph in graphs {
        for value in [
            graph.owner.file().path.to_string_lossy().as_bytes(),
            graph.adapter.schema().as_bytes(),
            graph.capability_support.as_str().as_bytes(),
            graph
                .authority
                .map_or("", CapabilityAuthority::as_str)
                .as_bytes(),
        ] {
            bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
            bytes.extend_from_slice(value);
        }
    }
    bytes
}

fn node_is_within(node: &NodeKey, owner: &NodeKey) -> bool {
    node.file() == owner.file()
        && node.anchor().start_byte() >= owner.anchor().start_byte()
        && node.anchor().end_byte() <= owner.anchor().end_byte()
}

fn validate_sorted_unique_by_key<T, F>(
    label: &str,
    values: &[T],
    key: F,
) -> Result<(), ControlFlowBuildError>
where
    F: Fn(&T) -> &str,
{
    for pair in values.windows(2) {
        match key(&pair[0]).cmp(key(&pair[1])) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => {
                return Err(ControlFlowBuildError::Invalid(format!(
                    "{label} contain duplicate keys"
                )));
            }
            std::cmp::Ordering::Greater => {
                return Err(ControlFlowBuildError::Invalid(format!(
                    "{label} are not in canonical key order"
                )));
            }
        }
    }
    Ok(())
}

fn validate_canonical_distinct(
    label: &str,
    values: &[String],
) -> Result<(), ControlFlowBuildError> {
    for pair in values.windows(2) {
        match pair[0].cmp(&pair[1]) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => {
                return Err(ControlFlowBuildError::Invalid(format!(
                    "{label} contain duplicates"
                )));
            }
            std::cmp::Ordering::Greater => {
                return Err(ControlFlowBuildError::Invalid(format!(
                    "{label} are not in canonical order"
                )));
            }
        }
    }
    Ok(())
}

fn validate_strings(label: &str, values: &[String]) -> Result<(), ControlFlowBuildError> {
    for value in values {
        validate_text(label, value)?;
    }
    Ok(())
}

fn validate_adapter_pair(
    label: &str,
    schema: &str,
    name: &str,
) -> Result<(), ControlFlowBuildError> {
    validate_text(&format!("{label} schema"), schema)?;
    validate_text(&format!("{label} name"), name)
}

fn validate_text(label: &str, value: &str) -> Result<(), ControlFlowBuildError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(ControlFlowBuildError::Invalid(format!(
            "{label} must be nonempty control-free text"
        )));
    }
    Ok(())
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), ControlFlowBuildError> {
    if !value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(ControlFlowBuildError::Invalid(format!(
            "identity must be canonical lowercase {prefix}<64-hex>"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowBuildError {
    Invalid(String),
    Node(String),
    ForeignFileNode,
    OutsideOwnerRegion,
    PointOutOfRange {
        requested: usize,
        point_count: usize,
    },
    DanglingPoint(String),
    DuplicateOwner,
    Identity(String),
}

impl fmt::Display for ControlFlowBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid control flow: {detail}"),
            Self::Node(detail) => write!(formatter, "invalid control-flow node: {detail}"),
            Self::ForeignFileNode => {
                formatter.write_str("control-flow node belongs to a different file than its owner")
            }
            Self::OutsideOwnerRegion => formatter
                .write_str("control-flow node is outside its executable owner's source region"),
            Self::PointOutOfRange {
                requested,
                point_count,
            } => write!(
                formatter,
                "control point index {requested} is outside point count {point_count}"
            ),
            Self::DanglingPoint(key) => {
                write!(formatter, "control edge references missing point {key}")
            }
            Self::DuplicateOwner => {
                formatter.write_str("control-flow projection already contains this owner")
            }
            Self::Identity(detail) => write!(formatter, "control-flow identity failed: {detail}"),
        }
    }
}

impl std::error::Error for ControlFlowBuildError {}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use deslop_core::Lang;
    use deslop_lang::{
        CLOJURE_PACK, CapabilityDeclaration, ControlEvaluationOrder, ControlFlowAction,
        ControlFlowOwnerRule, ControlFlowOwnerRuleKind, ControlFlowRule, ControlFlowSyntaxSelector,
        DialectDeclaration, GrammarDescriptor, JAVASCRIPT_PACK, JULIA_PACK, LangPack,
        LanguageAdapterCapabilityManifest, LanguageControlFlowRulePack, PYTHON_PACK, RUST_PACK,
        RegionSpan, Registry, TYPESCRIPT_PACK,
    };
    use serde_json::Value;
    use std::path::Path;

    type JsonMutation = (&'static str, Box<dyn Fn(&mut Value)>);
    type DraftMutation = (
        &'static str,
        Box<dyn Fn(&ProjectAnalysis, NodeId, &mut ControlFlowGraphDraft)>,
    );

    struct ControlFlowTestPack;

    static CONTROL_FLOW_TEST_PACK: ControlFlowTestPack = ControlFlowTestPack;

    impl LangPack for ControlFlowTestPack {
        fn name(&self) -> &'static str {
            "control-flow-test-rust"
        }

        fn adapter_schema(&self) -> &'static str {
            RUST_PACK.adapter_schema()
        }

        fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
            LanguageAdapterCapabilityManifest::current_syntax(self.adapter_schema())
                .with_declaration(CapabilityDeclaration::provided(
                    AdapterCapability::ControlFlow,
                    CapabilityAuthority::Adapter,
                ))
                .unwrap()
        }

        fn control_flow_rule_pack(&self) -> LanguageControlFlowRulePack {
            LanguageControlFlowRulePack::provided(
                self.adapter_schema(),
                CapabilityAuthority::Adapter,
                vec![DialectDeclaration::new(
                    "rust",
                    "tree-sitter-rust",
                    "0.24.0",
                )],
                ControlEvaluationOrder::LeftToRight,
                vec![ControlFlowOwnerRule::new(
                    ControlFlowSyntaxSelector::new("function_item", None),
                    ControlFlowOwnerRuleKind::Callable,
                    "body",
                )],
                vec![
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("block", None),
                        ControlFlowAction::Sequence,
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("if_expression", None),
                        ControlFlowAction::Branch {
                            condition_field: "condition".into(),
                            consequence_field: "consequence".into(),
                            alternative_field: Some("alternative".into()),
                        },
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("loop_expression", None),
                        ControlFlowAction::Loop {
                            form: ControlLoopForm::Infinite,
                            condition_field: None,
                            body_field: "body".into(),
                            alternative_field: None,
                            label_kind: Some("label".into()),
                        },
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("macro_invocation", None),
                        ControlFlowAction::OpaqueBoundary {
                            reason: "Rust macro expansion is unavailable".into(),
                        },
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("break_expression", None),
                        ControlFlowAction::Abrupt {
                            form: ControlAbruptForm::Break,
                            value_field: None,
                            label_kind: Some("label".into()),
                        },
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("expression_statement", None),
                        ControlFlowAction::Sequence,
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("else_clause", None),
                        ControlFlowAction::Sequence,
                    ),
                    ControlFlowRule::new(
                        ControlFlowSyntaxSelector::new("return_expression", None),
                        ControlFlowAction::Abrupt {
                            form: ControlAbruptForm::Return,
                            value_field: None,
                            label_kind: None,
                        },
                    ),
                ],
            )
            .unwrap()
        }

        fn lang(&self) -> Lang {
            Lang::Rust
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["cflow"]
        }

        fn grammar(&self) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar()
        }

        fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
            Some(GrammarDescriptor::new(
                Lang::Rust,
                "rust",
                "tree-sitter-rust",
                "0.24.0",
            ))
        }

        fn line_comments(&self) -> &'static [&'static str] {
            &["//"]
        }

        fn metrics_regions(&self) -> &'static [&'static str] {
            &[]
        }

        fn metrics_branches(&self) -> &'static [&'static str] {
            &[]
        }

        fn metrics_nesting(&self) -> &'static [&'static str] {
            &[]
        }

        fn metrics_flow_breaks(&self) -> &'static [&'static str] {
            &[]
        }

        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            &[]
        }

        fn enclosing_region(&self, node: tree_sitter::Node<'_>, _text: &str) -> Option<RegionSpan> {
            Some(RegionSpan {
                start_line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
            })
        }
    }

    fn analysis(provided: bool) -> Arc<ProjectAnalysis> {
        if provided {
            return provided_analysis(
                "fn run(x: bool) { if x { loop { break; } } else { return; } }\n",
            );
        }
        let root = tempfile::tempdir().unwrap();
        let registry = Registry::default();
        let snapshot = crate::ProjectSnapshotBuilder::new(
            root.path(),
            crate::RepositoryId::explicit("control-flow-schema-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("flow.py", b"def run():\n    pass\n".to_vec())
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn provided_analysis(source: &str) -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::default();
        registry.register(&CONTROL_FLOW_TEST_PACK);
        let snapshot = crate::ProjectSnapshotBuilder::new(
            root.path(),
            crate::RepositoryId::explicit("control-flow-schema-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("flow.cflow", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn production_rust_analysis(source: &str) -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let snapshot = crate::ProjectSnapshotBuilder::new(
            root.path(),
            crate::RepositoryId::explicit("control-flow-production-rust-test").unwrap(),
        )
        .unwrap()
        .with_registry(Registry::default())
        .with_overlay("flow.rs", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn node_by_kind(analysis: &ProjectAnalysis, kind: &str) -> NodeId {
        analysis
            .node_ids()
            .find(|node| analysis.node(*node).unwrap().raw_kind() == kind)
            .unwrap_or_else(|| panic!("missing {kind}"))
    }

    pub(crate) fn complete_projection() -> ControlFlowProjection {
        let analysis = analysis(true);
        let owner = node_by_kind(&analysis, "function_item");
        let condition = node_by_kind(&analysis, "identifier");
        let branch = node_by_kind(&analysis, "if_expression");
        let loop_node = node_by_kind(&analysis, "loop_expression");
        let break_node = node_by_kind(&analysis, "break_expression");
        let return_node = node_by_kind(&analysis, "return_expression");
        let policy =
            ControlFlowPolicyId::from_parts(&[b"hand-labelled-all-edge-families/1"]).unwrap();
        let mut builder = ControlFlowBuilder::new(Arc::clone(&analysis), policy);
        let points = vec![
            ControlPointDraft {
                kind: ControlPointKind::Entry,
                source: None,
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch),
                source: Some(branch),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(condition),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::LoopHeader),
                source: Some(loop_node),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(break_node),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Syntax,
                source: Some(return_node),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::HandlerDispatch),
                source: Some(owner),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::Suspension),
                source: Some(owner),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::Resume),
                source: Some(owner),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                source: Some(owner),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Exit,
                source: None,
                ordinal: 0,
            },
        ];
        let edge = |from, to, kind, source, predicate| ControlEdgeDraft {
            from,
            to,
            kind,
            source,
            predicate,
            precision: ControlEdgePrecision::Exact,
        };
        let edges = vec![
            edge(0, 1, ControlEdgeKind::Entry, owner, None),
            edge(1, 2, ControlEdgeKind::Normal, branch, None),
            edge(
                2,
                3,
                ControlEdgeKind::Branch(ControlBranchKind::True),
                condition,
                Some(condition),
            ),
            edge(
                2,
                5,
                ControlEdgeKind::Branch(ControlBranchKind::False),
                condition,
                Some(condition),
            ),
            edge(
                3,
                4,
                ControlEdgeKind::Loop(ControlLoopKind::Body),
                loop_node,
                Some(loop_node),
            ),
            edge(
                4,
                3,
                ControlEdgeKind::Loop(ControlLoopKind::Back),
                break_node,
                None,
            ),
            edge(
                4,
                9,
                ControlEdgeKind::Abrupt(ControlAbruptKind::Break { label: None }),
                break_node,
                None,
            ),
            edge(
                5,
                9,
                ControlEdgeKind::Abrupt(ControlAbruptKind::Return),
                return_node,
                None,
            ),
            edge(
                1,
                6,
                ControlEdgeKind::Exceptional(ControlExceptionalKind::Handler),
                branch,
                None,
            ),
            edge(
                6,
                9,
                ControlEdgeKind::Exceptional(ControlExceptionalKind::Propagate),
                owner,
                None,
            ),
            edge(
                1,
                7,
                ControlEdgeKind::Suspension(ControlSuspensionKind::Suspend),
                owner,
                None,
            ),
            edge(
                7,
                8,
                ControlEdgeKind::Suspension(ControlSuspensionKind::Resume),
                owner,
                None,
            ),
            edge(8, 9, ControlEdgeKind::Normal, owner, None),
            edge(
                9,
                10,
                ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                owner,
                None,
            ),
        ];
        builder
            .add_graph(ControlFlowGraphDraft {
                owner,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points,
                edges,
            })
            .unwrap();
        builder.build().unwrap()
    }

    fn minimal_draft(owner: NodeId) -> ControlFlowGraphDraft {
        ControlFlowGraphDraft {
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
                    kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::NoOp),
                    source: Some(owner),
                    ordinal: 0,
                },
                ControlPointDraft {
                    kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
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
                    kind: ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                    source: owner,
                    predicate: None,
                    precision: ControlEdgePrecision::Exact,
                },
            ],
        }
    }

    #[test]
    fn m4_8_exception_suspension_and_early_exit_families_remain_typed() {
        let projection = complete_projection();
        let graph = &projection.document().graphs()[0];
        let family_counts = graph.edges().iter().fold([0usize; 4], |mut counts, edge| {
            match edge.kind() {
                ControlEdgeKind::Normal => counts[0] += 1,
                ControlEdgeKind::Exceptional(_) => counts[1] += 1,
                ControlEdgeKind::Suspension(_) => counts[2] += 1,
                ControlEdgeKind::Abrupt(_) => counts[3] += 1,
                _ => {}
            }
            counts
        });
        assert_eq!(family_counts, [2, 2, 2, 2]);
        for kind in [
            ControlEdgeKind::Exceptional(ControlExceptionalKind::Handler),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::Propagate),
            ControlEdgeKind::Suspension(ControlSuspensionKind::Suspend),
            ControlEdgeKind::Suspension(ControlSuspensionKind::Resume),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Break { label: None }),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Return),
        ] {
            assert_eq!(
                graph
                    .edges()
                    .iter()
                    .filter(|edge| edge.kind() == &kind)
                    .count(),
                1,
                "missing or collapsed advanced edge {kind:?}"
            );
        }
    }

    #[test]
    fn m4_8_production_await_and_closure_remain_explicitly_partial() {
        let analysis = production_rust_analysis(
            "async fn run(x: i32) -> i32 { let captured = || x; yield x; helper().await; captured() }\n",
        );
        let lowered = lower_control_flow(
            analysis,
            ControlFlowPolicyId::from_parts(&[b"m4.8-await-closure/1"]).unwrap(),
        )
        .unwrap();
        assert!(lowered.gaps().is_empty());
        let projection = lowered.projection().unwrap();
        assert_eq!(projection.document().graphs().len(), 2);
        assert!(projection.document().graphs().iter().any(|graph| {
            graph.coverage().status() == FactCoverage::Partial
                && graph.coverage().reasons().iter().any(|reason| {
                    reason == "await_expression lowering is retained but not implemented by the shared M4.2 traversal"
                })
                && graph.coverage().reasons().iter().any(|reason| {
                    reason == "yield_expression lowering is retained but not implemented by the shared M4.2 traversal"
                })
        }));
        assert!(projection.document().graphs().iter().all(|graph| {
            !graph
                .edges()
                .iter()
                .any(|edge| matches!(edge.kind(), ControlEdgeKind::Suspension(_)))
        }));
    }

    #[test]
    fn m4_8_return_never_falls_through_to_following_syntax() {
        let analysis = production_rust_analysis("fn run() { return; helper(); }\n");
        let return_node = node_by_kind(&analysis, "return_expression");
        let call_node = node_by_kind(&analysis, "call_expression");
        let lowered = lower_control_flow(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"m4.8-early-return/1"]).unwrap(),
        )
        .unwrap();
        let graph = &lowered.projection().unwrap().document().graphs()[0];
        let return_key = analysis.node_key(return_node).unwrap();
        let call_key = analysis.node_key(call_node).unwrap();
        let return_point = graph
            .points()
            .iter()
            .find(|point| point.source() == Some(return_key))
            .unwrap();
        assert!(graph.edges().iter().any(|edge| {
            edge.from() == return_point.key()
                && edge.kind() == &ControlEdgeKind::Abrupt(ControlAbruptKind::Return)
        }));
        assert!(!graph.edges().iter().any(|edge| {
            edge.from() == return_point.key() && edge.kind() == &ControlEdgeKind::Normal
        }));
        assert!(
            graph
                .points()
                .iter()
                .find(|point| point.source() == Some(call_key))
                .is_none_or(|point| !graph.edges().iter().any(|edge| edge.to() == point.key()))
        );
    }

    #[test]
    fn m4_1_all_edge_families_are_distinct_stable_and_strict() {
        let projection = complete_projection();
        let document = projection.document();
        assert_eq!(document.schema(), CONTROL_FLOW_SCHEMA);
        assert_eq!(document.analysis_id(), projection.analysis().id().as_str());
        let graph = &document.graphs()[0];
        let families = graph
            .edges()
            .iter()
            .map(|edge| match edge.kind() {
                ControlEdgeKind::Entry => "entry",
                ControlEdgeKind::Exit(_) => "exit",
                ControlEdgeKind::Normal => "normal",
                ControlEdgeKind::Branch(_) => "branch",
                ControlEdgeKind::Loop(_) => "loop",
                ControlEdgeKind::Exceptional(_) => "exceptional",
                ControlEdgeKind::Abrupt(_) => "abrupt",
                ControlEdgeKind::Suspension(_) => "suspension",
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(families.len(), 8);
        assert_eq!(graph.capability_support(), CapabilitySupport::Provided);
        assert_eq!(graph.authority(), Some(CapabilityAuthority::Adapter));
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert_eq!(graph.points().len(), 11);
        assert_eq!(graph.edges().len(), 14);

        let first = serde_json::to_vec(document).unwrap();
        let decoded: ControlFlowDocument = serde_json::from_slice(&first).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), first);
        assert!(
            decoded.graphs()[0]
                .points()
                .windows(2)
                .all(|pair| pair[0].key() < pair[1].key())
        );
        assert!(
            decoded.graphs()[0]
                .edges()
                .windows(2)
                .all(|pair| pair[0].key() < pair[1].key())
        );

        let mut unknown: Value = serde_json::from_slice(&first).unwrap();
        unknown["unexpected"] = Value::Bool(true);
        assert!(serde_json::from_value::<ControlFlowDocument>(unknown).is_err());
    }

    #[test]
    fn m4_1_payload_topology_and_authority_corruption_fail_closed() {
        let projection = complete_projection();
        let original = serde_json::to_value(projection.document()).unwrap();
        let mutations: Vec<JsonMutation> = vec![
            (
                "schema",
                Box::new(|value| value["schema"] = Value::String("deslop.control-flow/0".into())),
            ),
            (
                "graph-key",
                Box::new(|value| {
                    value["graphs"][0]["key"] = Value::String(format!("cfg1_{}", "0".repeat(64)))
                }),
            ),
            (
                "point-key",
                Box::new(|value| {
                    value["graphs"][0]["points"][0]["key"] =
                        Value::String(format!("cpt1_{}", "0".repeat(64)))
                }),
            ),
            (
                "edge-key",
                Box::new(|value| {
                    value["graphs"][0]["edges"][0]["key"] =
                        Value::String(format!("ced1_{}", "0".repeat(64)))
                }),
            ),
            (
                "coverage-reason",
                Box::new(|value| {
                    value["graphs"][0]["coverage"]["reasons"] = serde_json::json!(["not complete"])
                }),
            ),
            (
                "capability-support",
                Box::new(|value| {
                    value["graphs"][0]["capability_support"] = Value::String("unknown".into())
                }),
            ),
            (
                "syntax-authority",
                Box::new(|value| {
                    value["graphs"][0]["authority"] = Value::String("syntax".into());
                    let capabilities =
                        value["graphs"][0]["adapter"]["capabilities"]["capabilities"]
                            .as_array_mut()
                            .unwrap();
                    let declaration = capabilities
                        .iter_mut()
                        .find(|declaration| declaration["capability"] == "control-flow")
                        .unwrap();
                    declaration["authority"] = Value::String("syntax".into());
                }),
            ),
            (
                "dangling",
                Box::new(|value| {
                    value["graphs"][0]["edges"][0]["to"] =
                        Value::String(format!("cpt1_{}", "1".repeat(64)))
                }),
            ),
            (
                "noncanonical-points",
                Box::new(|value| {
                    value["graphs"][0]["points"]
                        .as_array_mut()
                        .unwrap()
                        .swap(0, 1)
                }),
            ),
            (
                "duplicate-edge",
                Box::new(|value| {
                    let duplicate = value["graphs"][0]["edges"][0].clone();
                    value["graphs"][0]["edges"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }),
            ),
        ];
        for (label, mutate) in mutations {
            let mut value = original.clone();
            mutate(&mut value);
            assert!(
                serde_json::from_value::<ControlFlowDocument>(value).is_err(),
                "corruption {label} unexpectedly passed"
            );
        }
    }

    #[test]
    fn m4_1_edge_subkind_catalog_round_trips_without_collapse() {
        let kinds = vec![
            ControlEdgeKind::Entry,
            ControlEdgeKind::Exit(ControlExitOutcome::Normal),
            ControlEdgeKind::Exit(ControlExitOutcome::Exceptional),
            ControlEdgeKind::Exit(ControlExitOutcome::Abrupt),
            ControlEdgeKind::Exit(ControlExitOutcome::Suspended),
            ControlEdgeKind::Normal,
            ControlEdgeKind::Branch(ControlBranchKind::True),
            ControlEdgeKind::Branch(ControlBranchKind::False),
            ControlEdgeKind::Branch(ControlBranchKind::Case {
                label: "red".into(),
            }),
            ControlEdgeKind::Branch(ControlBranchKind::Default),
            ControlEdgeKind::Branch(ControlBranchKind::GuardPassed),
            ControlEdgeKind::Branch(ControlBranchKind::GuardFailed),
            ControlEdgeKind::Branch(ControlBranchKind::AdapterDefined {
                schema: "test.branch/1".into(),
                name: "pattern-fallback".into(),
            }),
            ControlEdgeKind::Loop(ControlLoopKind::Enter),
            ControlEdgeKind::Loop(ControlLoopKind::Body),
            ControlEdgeKind::Loop(ControlLoopKind::Back),
            ControlEdgeKind::Loop(ControlLoopKind::ConditionFalse),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::Throw),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::Propagate),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::Handler),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::FinallyEnter),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::FinallyResume),
            ControlEdgeKind::Exceptional(ControlExceptionalKind::AdapterDefined {
                schema: "test.exception/1".into(),
                name: "filter".into(),
            }),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Return),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Break { label: None }),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Continue {
                label: Some("outer".into()),
            }),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Goto {
                label: "done".into(),
            }),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Terminate),
            ControlEdgeKind::Abrupt(ControlAbruptKind::AdapterDefined {
                schema: "test.abrupt/1".into(),
                name: "nonlocal-return".into(),
            }),
            ControlEdgeKind::Suspension(ControlSuspensionKind::AwaitReady),
            ControlEdgeKind::Suspension(ControlSuspensionKind::AwaitPending),
            ControlEdgeKind::Suspension(ControlSuspensionKind::Yield),
            ControlEdgeKind::Suspension(ControlSuspensionKind::Suspend),
            ControlEdgeKind::Suspension(ControlSuspensionKind::Resume),
            ControlEdgeKind::Suspension(ControlSuspensionKind::AdapterDefined {
                schema: "test.suspension/1".into(),
                name: "generator-close".into(),
            }),
        ];
        let encoded = kinds
            .iter()
            .map(|kind| {
                validate_edge_kind(kind).unwrap();
                serde_json::to_string(kind).unwrap()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(encoded.len(), kinds.len());
        for kind in kinds {
            let bytes = serde_json::to_vec(&kind).unwrap();
            assert_eq!(
                serde_json::from_slice::<ControlEdgeKind>(&bytes).unwrap(),
                kind
            );
        }
    }

    #[test]
    fn m4_1_builder_rejects_boundary_duplicates_conservative_complete_and_outside_owner() {
        let scenarios: Vec<DraftMutation> = vec![
            (
                "entry-boundary",
                Box::new(|_, _, draft| {
                    draft.edges[0].from = 1;
                    draft.edges[0].to = 2;
                }),
            ),
            (
                "duplicate-semantic-edge",
                Box::new(|_, _, draft| draft.edges.push(draft.edges[1].clone())),
            ),
            (
                "conservative-complete",
                Box::new(|_, _, draft| {
                    draft.edges[1].precision =
                        ControlEdgePrecision::conservative("unknown dispatch").unwrap();
                }),
            ),
            (
                "outside-owner",
                Box::new(|analysis, _, draft| {
                    draft.points[1].source = Some(node_by_kind(analysis, "source_file"));
                }),
            ),
        ];
        for (label, mutate) in scenarios {
            let analysis = analysis(true);
            let owner = node_by_kind(&analysis, "function_item");
            let mut draft = minimal_draft(owner);
            mutate(&analysis, owner, &mut draft);
            let policy = ControlFlowPolicyId::from_parts(&[label.as_bytes()]).unwrap();
            let mut builder = ControlFlowBuilder::new(analysis, policy);
            assert!(builder.add_graph(draft).is_err(), "scenario {label} passed");
        }
    }

    #[test]
    fn m4_1_production_unknown_capability_cannot_claim_complete_flow() {
        let analysis = analysis(false);
        let owner = node_by_kind(&analysis, "function_definition");
        let policy = ControlFlowPolicyId::from_parts(&[b"production-unknown/1"]).unwrap();
        let points = vec![
            ControlPointDraft {
                kind: ControlPointKind::Entry,
                source: None,
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::NoOp),
                source: Some(owner),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                source: Some(owner),
                ordinal: 0,
            },
            ControlPointDraft {
                kind: ControlPointKind::Exit,
                source: None,
                ordinal: 0,
            },
        ];
        let edges = vec![
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
                kind: ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                source: owner,
                predicate: None,
                precision: ControlEdgePrecision::Exact,
            },
        ];
        let mut builder = ControlFlowBuilder::new(analysis, policy);
        let error = builder
            .add_graph(ControlFlowGraphDraft {
                owner,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points,
                edges,
            })
            .unwrap_err();
        assert!(error.to_string().contains("provided capability"), "{error}");

        for pack in [
            &CLOJURE_PACK as &dyn LangPack,
            &JULIA_PACK,
            &PYTHON_PACK,
            &JAVASCRIPT_PACK,
            &TYPESCRIPT_PACK,
        ] {
            let declaration = pack
                .capability_manifest()
                .declaration(AdapterCapability::ControlFlow)
                .clone();
            assert_eq!(
                declaration.support(),
                CapabilitySupport::Unknown,
                "{}",
                pack.name()
            );
            assert_eq!(declaration.authority(), None, "{}", pack.name());
        }
        let rust = RUST_PACK.capability_manifest();
        let declaration = rust.declaration(AdapterCapability::ControlFlow);
        assert_eq!(declaration.support(), CapabilitySupport::Provided);
        assert_eq!(declaration.authority(), Some(CapabilityAuthority::Adapter));
        assert_eq!(RUST_PACK.control_flow_rule_pack().rules().len(), 17);
        let source = include_str!("control_flow.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!source.contains("deslop_graph"));
        assert!(!source.contains("GraphProjection"));
        assert!(!source.contains("deslop.graph/2"));
    }

    #[test]
    fn m4_2_stored_rule_pack_lowers_owned_sequence_branch_loop_and_abrupt_flow() {
        let analysis = analysis(true);
        let policy = ControlFlowPolicyId::from_parts(&[b"m4.2-owned-rule-lowering/1"]).unwrap();
        let lowered = lower_control_flow(Arc::clone(&analysis), policy).unwrap();
        assert!(lowered.gaps().is_empty());
        let projection = lowered.projection().expect("provided test adapter graph");
        assert_eq!(projection.analysis().id(), analysis.id());
        let graph = &projection.document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert_eq!(graph.points().len(), 10, "{graph:#?}");
        assert_eq!(graph.edges().len(), 10);
        let mut counts = BTreeMap::new();
        for edge in graph.edges() {
            let family = match edge.kind() {
                ControlEdgeKind::Entry => "entry",
                ControlEdgeKind::Exit(_) => "exit",
                ControlEdgeKind::Normal => "normal",
                ControlEdgeKind::Branch(_) => "branch",
                ControlEdgeKind::Loop(_) => "loop",
                ControlEdgeKind::Exceptional(_) => "exceptional",
                ControlEdgeKind::Abrupt(_) => "abrupt",
                ControlEdgeKind::Suspension(_) => "suspension",
            };
            *counts.entry(family).or_insert(0usize) += 1;
        }
        assert_eq!(
            counts,
            BTreeMap::from([
                ("abrupt", 2),
                ("branch", 2),
                ("entry", 1),
                ("exit", 2),
                ("loop", 1),
                ("normal", 2),
            ])
        );
        assert!(graph.edges().iter().any(|edge| {
            edge.kind() == &ControlEdgeKind::Abrupt(ControlAbruptKind::Break { label: None })
        }));
        assert!(
            graph
                .edges()
                .iter()
                .any(|edge| { edge.kind() == &ControlEdgeKind::Abrupt(ControlAbruptKind::Return) })
        );
        assert_eq!(analysis.parse_counts().len(), 1);
        assert_eq!(
            serde_json::to_vec(projection.document()).unwrap(),
            serde_json::to_vec(
                lower_control_flow(
                    analysis,
                    ControlFlowPolicyId::from_parts(&[b"m4.2-owned-rule-lowering/1"]).unwrap(),
                )
                .unwrap()
                .projection()
                .unwrap()
                .document()
            )
            .unwrap()
        );
    }

    #[test]
    fn m4_2_production_rust_rules_lower_exact_fixture_and_labeled_outer_break() {
        let exact = production_rust_analysis(
            "fn run(x: bool) { if x { loop { break; } } else { return; } }\n",
        );
        let identity = exact
            .snapshot()
            .entry(Path::new("flow.rs"))
            .unwrap()
            .language_adapter_identity()
            .unwrap();
        assert_eq!(identity.schema(), "deslop-lang-adapter/3");
        assert_eq!(identity.control_flow_rules().rules().len(), 17);
        let lowered = lower_control_flow(
            Arc::clone(&exact),
            ControlFlowPolicyId::from_parts(&[b"m4.2-production-rust-exact/1"]).unwrap(),
        )
        .unwrap();
        assert!(lowered.gaps().is_empty());
        let graph = &lowered.projection().unwrap().document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert_eq!(graph.points().len(), 10);
        assert_eq!(graph.edges().len(), 10);

        let labeled =
            production_rust_analysis("fn run() { 'outer: loop { loop { break 'outer; } } }\n");
        let lowered = lower_control_flow(
            labeled,
            ControlFlowPolicyId::from_parts(&[b"m4.2-production-rust-label/1"]).unwrap(),
        )
        .unwrap();
        let graph = &lowered.projection().unwrap().document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.edges().iter().any(|edge| {
            edge.kind()
                == &ControlEdgeKind::Abrupt(ControlAbruptKind::Break {
                    label: Some("'outer".into()),
                })
        }));
    }

    #[test]
    fn m4_2_production_rust_unmodeled_values_and_calls_are_partial() {
        for (source, expected_reason) in [
            (
                "fn run(flag: bool) -> i32 { return if flag { 1 } else { 2 }; }\n",
                "nested control in return_expression value is not lowered yet",
            ),
            (
                "fn run() { helper(); }\n",
                "Rust call unwind behavior requires callee effects and panic strategy",
            ),
            (
                "fn run() { println!(\"x\"); }\n",
                "Rust macro expansion is unavailable",
            ),
        ] {
            let analysis = production_rust_analysis(source);
            let lowered = lower_control_flow(
                analysis,
                ControlFlowPolicyId::from_parts(&[expected_reason.as_bytes()]).unwrap(),
            )
            .unwrap();
            let graph = &lowered.projection().unwrap().document().graphs()[0];
            assert_eq!(graph.coverage().status(), FactCoverage::Partial);
            assert!(
                graph
                    .coverage()
                    .reasons()
                    .iter()
                    .any(|reason| reason == expected_reason),
                "{:#?}",
                graph.coverage()
            );
        }

        let simple_value = production_rust_analysis("fn run() -> i32 { return 1; }\n");
        let lowered = lower_control_flow(
            simple_value,
            ControlFlowPolicyId::from_parts(&[b"m4.2-production-rust-simple-return/1"]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            lowered.projection().unwrap().document().graphs()[0]
                .coverage()
                .status(),
            FactCoverage::Complete
        );
    }

    #[test]
    fn m4_2_production_rust_does_not_fabricate_normal_flow_after_abrupt_paths() {
        for source in [
            "fn run() { return; 42; }\n",
            "fn run(x: bool) { if x { return; } else { return; } }\n",
        ] {
            let analysis = production_rust_analysis(source);
            let lowered = lower_control_flow(
                analysis,
                ControlFlowPolicyId::from_parts(&[source.as_bytes()]).unwrap(),
            )
            .unwrap();
            let graph = &lowered.projection().unwrap().document().graphs()[0];
            assert_eq!(graph.coverage().status(), FactCoverage::Complete);
            assert!(
                graph.edges().iter().any(|edge| {
                    edge.kind() == &ControlEdgeKind::Exit(ControlExitOutcome::Abrupt)
                })
            );
            assert!(
                !graph.edges().iter().any(|edge| {
                    edge.kind() == &ControlEdgeKind::Exit(ControlExitOutcome::Normal)
                })
            );
        }
    }

    #[test]
    fn m4_2_production_rust_covers_while_for_match_and_nested_predicate_boundaries() {
        let loops = production_rust_analysis(
            "fn run(flag: bool, xs: [i32; 0]) { while flag { continue; } for _x in xs { break; } }\n",
        );
        let lowered = lower_control_flow(
            loops,
            ControlFlowPolicyId::from_parts(&[b"m4.2-production-rust-loop-forms/1"]).unwrap(),
        )
        .unwrap();
        let graph = &lowered.projection().unwrap().document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Complete);
        assert!(graph.edges().iter().any(|edge| {
            edge.kind() == &ControlEdgeKind::Abrupt(ControlAbruptKind::Continue { label: None })
        }));
        assert!(graph.edges().iter().any(|edge| {
            edge.kind() == &ControlEdgeKind::Abrupt(ControlAbruptKind::Break { label: None })
        }));
        assert_eq!(
            graph
                .edges()
                .iter()
                .filter(|edge| {
                    edge.kind() == &ControlEdgeKind::Loop(ControlLoopKind::ConditionFalse)
                })
                .count(),
            2
        );

        for (source, expected_reason) in [
            (
                "fn run(flag: bool) { match flag { true => return, false => {} } }\n",
                "match_expression lowering is retained but not implemented by the shared M4.2 traversal",
            ),
            (
                "fn run(a: bool, b: bool) { if if a { b } else { false } {} }\n",
                "nested control in branch condition is not lowered yet",
            ),
        ] {
            let analysis = production_rust_analysis(source);
            let lowered = lower_control_flow(
                analysis,
                ControlFlowPolicyId::from_parts(&[expected_reason.as_bytes()]).unwrap(),
            )
            .unwrap();
            let graph = &lowered.projection().unwrap().document().graphs()[0];
            assert_eq!(graph.coverage().status(), FactCoverage::Partial);
            assert!(
                graph
                    .coverage()
                    .reasons()
                    .iter()
                    .any(|reason| reason == expected_reason),
                "{:#?}",
                graph.coverage()
            );
        }
    }

    #[test]
    fn m4_2_unknown_production_adapter_is_an_explicit_gap_without_projection() {
        let analysis = analysis(false);
        let policy = ControlFlowPolicyId::from_parts(&[b"m4.2-production-gap/1"]).unwrap();
        let lowered = lower_control_flow(analysis, policy).unwrap();
        assert!(lowered.projection().is_none());
        assert_eq!(lowered.gaps().len(), 1);
        assert_eq!(lowered.gaps()[0].path(), Path::new("flow.py"));
        assert_eq!(lowered.gaps()[0].support(), CapabilitySupport::Unknown);
        assert!(
            lowered.gaps()[0]
                .reason()
                .contains("declares ControlFlow unknown")
        );
    }

    #[test]
    fn m4_2_default_registry_dispatches_every_production_pack_at_its_declared_tier() {
        let root = tempfile::tempdir().unwrap();
        let mut builder = crate::ProjectSnapshotBuilder::new(
            root.path(),
            crate::RepositoryId::explicit("control-flow-all-pack-dispatch-test").unwrap(),
        )
        .unwrap()
        .with_registry(Registry::default());
        for (path, source) in [
            ("flow.clj", "(defn run [] nil)\n"),
            ("flow.jl", "function run()\nend\n"),
            ("flow.py", "def run():\n    pass\n"),
            ("flow.js", "function run() {}\n"),
            ("flow.ts", "function run(): void {}\n"),
            ("flow.rs", "fn run() {}\n"),
        ] {
            builder = builder
                .with_overlay(path, source.as_bytes().to_vec())
                .unwrap();
        }
        let analysis = ProjectAnalysis::build(builder.build().unwrap()).unwrap();
        let lowered = lower_control_flow(
            analysis,
            ControlFlowPolicyId::from_parts(&[b"m4.2-all-pack-dispatch/1"]).unwrap(),
        )
        .unwrap();
        let projection = lowered.projection().expect("Rust Provided graph");
        assert_eq!(projection.document().graphs().len(), 1);
        assert_eq!(
            projection.document().graphs()[0].coverage().status(),
            FactCoverage::Complete
        );
        assert_eq!(
            lowered
                .gaps()
                .iter()
                .map(|gap| gap.path().to_path_buf())
                .collect::<Vec<_>>(),
            ["flow.clj", "flow.jl", "flow.js", "flow.py", "flow.ts"]
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
        assert!(lowered.gaps().iter().all(|gap| {
            gap.support() == CapabilitySupport::Unknown
                && gap.reason().contains("declares ControlFlow unknown")
        }));
    }

    #[test]
    fn m4_2_opaque_rule_downgrades_coverage_and_retains_conservative_edge() {
        let analysis = provided_analysis("fn run() { println!(\"x\"); }\n");
        let policy = ControlFlowPolicyId::from_parts(&[b"m4.2-opaque-boundary/1"]).unwrap();
        let lowered = lower_control_flow(analysis, policy).unwrap();
        let graph = &lowered.projection().unwrap().document().graphs()[0];
        assert_eq!(graph.coverage().status(), FactCoverage::Partial);
        assert_eq!(
            graph.coverage().reasons(),
            ["Rust macro expansion is unavailable"]
        );
        assert_eq!(
            graph
                .edges()
                .iter()
                .filter(|edge| matches!(edge.precision(), ControlEdgePrecision::Conservative(_)))
                .count(),
            1
        );
        assert!(graph.edges().iter().any(|edge| {
            matches!(
                edge.precision(),
                ControlEdgePrecision::Conservative(reason)
                    if reason == "Rust macro expansion is unavailable"
            )
        }));
    }
}
