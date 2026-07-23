//! The contract graph projection (`deslop.contract-graph/2`).
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md`. This is a projection
//! beside the syntactic dependency projection (`deslop.graph/2`), with its
//! own identity derived through the existing `derive_projection_id`
//! mechanism. It maps the revision-pinned contract facts extracted by
//! [`deslop_parse::ContractChangeHistory`] into language-neutral role nodes
//! and semantic edges, and provides the traversal the refactor-defect
//! analysis needs: from any function to the consumers, tests, verifiers,
//! telemetry, and publication surfaces that depend on it.
//!
//! Evidence discipline: every node and edge carries its provider and
//! capability. Reference edges are leaf-name syntactic candidates — never
//! resolution proof — and same-named targets stay visible as ambiguous
//! candidates rather than being collapsed first-wins. Role classification
//! for telemetry and status surfaces is lexical supporting evidence and is
//! marked `Partial`; structural roles (tests by location, verifiers by
//! assertion facts) are marked `Complete`. Files without a contract query
//! surface as coverage reasons, never silent absences.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use deslop_core::Span;
use deslop_core::refactor_defect::{CapabilityLevel, ContractEdgeKind, ContractRole, FactProvider};
use deslop_parse::{
    ContractFunction, ContractSnapshot, FactCoverage, ProjectAnalysis, ProjectionId,
};
use serde::{Deserialize, Serialize};

/// Wire schema identifier for the contract graph.
pub const CONTRACT_GRAPH_SCHEMA: &str = "deslop.contract-graph/2";

const CONTRACT_PROJECTION_SCHEMA: &str = "deslop.contract-graph.projection/2";
const CONTRACT_CAPABILITIES: &[u8] =
    b"contract=deslop.contract-snapshot/2;multi-role=true;unresolved-endpoints=true";

/// Observation-surface object names classifying telemetry producers.
/// Mirrors the analyzer's classification sets; lexical supporting evidence.
const TELEMETRY_SURFACES: &[&str] = &[
    "metrics",
    "metric",
    "statsd",
    "prometheus",
    "telemetry",
    "logger",
    "log",
    "gauge",
    "counter",
    "histogram",
];

/// Publication-surface object names classifying status/identity publishers.
const STATUS_SURFACES: &[&str] = &["status", "health", "heartbeat", "watchdog", "registry"];

/// One contract node: a function or module-level acceptance surface with
/// its primary language-neutral role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractGraphNode {
    /// Deterministic node id: `<path>::<name>@<start-byte>`.
    pub id: String,
    pub role: ContractRole,
    /// Every independently evidenced role. `role` remains the deterministic
    /// primary rendering role; consumers must use this set when a function
    /// participates in more than one contract surface.
    pub roles: Vec<ContractRole>,
    pub name: String,
    pub path: PathBuf,
    pub span: Span,
    pub fingerprint: String,
    pub provider: FactProvider,
    /// `Complete` for structural roles; `Partial` where the role rests on
    /// lexical surface classification (telemetry, status publication).
    pub capability: CapabilityLevel,
}

/// Edge confidence: leaf-name reference matching is syntactic nomination,
/// and multiple same-named targets stay ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContractEdgeConfidence {
    Syntactic,
    Ambiguous,
    Unresolved,
}

/// One semantic edge between contract nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractGraphEdge {
    pub from: String,
    pub to: String,
    pub kind: ContractEdgeKind,
    /// The reference or config token evidencing this edge.
    pub token: String,
    pub confidence: ContractEdgeConfidence,
    pub provider: FactProvider,
    pub capability: CapabilityLevel,
}

/// The contract graph for one revision (`deslop.contract-graph/2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractGraph {
    pub schema: String,
    pub coverage: FactCoverage,
    /// Why coverage is not complete; empty exactly when coverage is.
    pub coverage_reasons: Vec<String>,
    /// Standing limits of this projection, stated in every payload.
    pub notes: Vec<String>,
    pub nodes: Vec<ContractGraphNode>,
    pub edges: Vec<ContractGraphEdge>,
}

/// The contract graph bound to the exact analysis it projects.
#[derive(Debug)]
pub struct ContractGraphProjection {
    pub id: ProjectionId,
    pub analysis: Arc<ProjectAnalysis>,
    pub graph: ContractGraph,
}

impl std::ops::Deref for ContractGraphProjection {
    type Target = ContractGraph;

    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

impl ContractGraph {
    /// Every node whose dependency edge reaches `node_id`, with the edge
    /// that reaches it: the traversal from an owner to its consumers,
    /// tests, verifiers, telemetry, and publication surfaces.
    pub fn dependents_of(&self, node_id: &str) -> Vec<(&ContractGraphNode, &ContractGraphEdge)> {
        let by_id: BTreeMap<&str, &ContractGraphNode> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        self.edges
            .iter()
            .filter(|edge| edge.to == node_id)
            .filter_map(|edge| by_id.get(edge.from.as_str()).map(|node| (*node, edge)))
            .collect()
    }

    /// Nodes by role, for review tooling.
    pub fn nodes_with_role(&self, role: ContractRole) -> Vec<&ContractGraphNode> {
        self.nodes
            .iter()
            .filter(|node| node.roles.contains(&role))
            .collect()
    }
}

fn token_leaf(token: &str) -> &str {
    token.rsplit('.').next().unwrap_or(token)
}

fn observation_surface(token: &str, surfaces: &[&str]) -> bool {
    let Some((object, _leaf)) = token.rsplit_once('.') else {
        return false;
    };
    let object_leaf = token_leaf(object).to_ascii_lowercase();
    surfaces.contains(&object_leaf.as_str())
}

fn is_test_function(path: &std::path::Path, name: &str) -> bool {
    if name.starts_with("test_") {
        return true;
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem.starts_with("test_") || stem.ends_with("_test") {
        return true;
    }
    path.components().any(|component| {
        let text = component.as_os_str().to_str().unwrap_or_default();
        text == "test" || text == "tests"
    })
}

/// The primary role and role capability of one function. Structural roles
/// are complete; surface classifications are partial (lexical evidence).
fn classify(
    path: &std::path::Path,
    function: &ContractFunction,
) -> (ContractRole, Vec<ContractRole>, CapabilityLevel) {
    let mut roles = vec![ContractRole::Consumer];
    if is_test_function(path, &function.name) {
        roles.push(ContractRole::TestEntryPoint);
    }
    let lexical_fail_loud_gate = function.assertions == 0 && function.admission_guard.fail_loud;
    if function.assertions > 0 || lexical_fail_loud_gate {
        roles.push(ContractRole::Verifier);
    }
    if function
        .references
        .iter()
        .any(|token| observation_surface(token, STATUS_SURFACES))
    {
        roles.push(ContractRole::RuntimeIdentity);
    }
    if function
        .references
        .iter()
        .any(|token| observation_surface(token, TELEMETRY_SURFACES))
    {
        roles.push(ContractRole::TelemetrySurface);
    }
    let lower = function.name.to_ascii_lowercase();
    if ["build_", "create_", "write_", "serialize_", "emit_"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        roles.push(ContractRole::Producer);
    }
    if [
        "decide", "step", "train", "run", "resume", "execute", "apply",
    ]
    .iter()
    .any(|part| lower == *part || lower.starts_with(&format!("{part}_")))
    {
        roles.push(ContractRole::Owner);
    }
    roles.sort_by_key(|role| format!("{role:?}"));
    roles.dedup();
    let primary = if roles.contains(&ContractRole::TestEntryPoint) {
        ContractRole::TestEntryPoint
    } else if roles.contains(&ContractRole::Verifier) {
        ContractRole::Verifier
    } else if roles.contains(&ContractRole::RuntimeIdentity) {
        ContractRole::RuntimeIdentity
    } else if roles.contains(&ContractRole::TelemetrySurface) {
        ContractRole::TelemetrySurface
    } else if roles.contains(&ContractRole::Producer) {
        ContractRole::Producer
    } else if roles.contains(&ContractRole::Owner) {
        ContractRole::Owner
    } else {
        ContractRole::Consumer
    };
    let capability = if roles.contains(&ContractRole::RuntimeIdentity)
        || roles.contains(&ContractRole::TelemetrySurface)
        || roles.contains(&ContractRole::Producer)
        || roles.contains(&ContractRole::Owner)
        || lexical_fail_loud_gate
    {
        CapabilityLevel::Partial
    } else {
        CapabilityLevel::Complete
    };
    (primary, roles, capability)
}

/// The edge kind a dependency from a node with `role` represents.
fn edge_kind_for(role: ContractRole) -> ContractEdgeKind {
    match role {
        ContractRole::TestEntryPoint => ContractEdgeKind::Exercises,
        ContractRole::Verifier => ContractEdgeKind::Verifies,
        ContractRole::TelemetrySurface => ContractEdgeKind::Observes,
        ContractRole::RuntimeIdentity => ContractEdgeKind::Publishes,
        _ => ContractEdgeKind::Consumes,
    }
}

/// Build the contract graph for on-disk paths, mirroring the planner flow
/// used by `graph_paths`.
pub fn contract_graph_paths(paths: &[PathBuf]) -> Result<ContractGraph> {
    use deslop_parse::{
        DiscoveryPolicy, ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec, RootSpec,
        ScopeSpec,
    };
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base: std::env::current_dir().context("resolve contract-graph base")?,
        root: RootSpec::Auto,
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::Requested(paths),
        discovery: DiscoveryPolicy::LegacyRespectIgnore,
    })?;
    let built = planner.build()?;
    let analysis = ProjectAnalysis::build(built.snapshot)?;
    Ok(contract_graph_analysis(analysis)?.graph)
}

/// Build the contract graph projection for one exact analysis.
pub fn contract_graph_analysis(analysis: Arc<ProjectAnalysis>) -> Result<ContractGraphProjection> {
    let snapshot = ContractSnapshot::from_analysis("current", &analysis)
        .map_err(|error| anyhow::anyhow!("contract extraction failed: {error}"))?;
    let id = analysis
        .derive_projection_id(CONTRACT_PROJECTION_SCHEMA, b"{}", CONTRACT_CAPABILITIES)
        .context("derive contract projection identity")?;
    let revision = snapshot.revision_contracts();

    let mut nodes: Vec<ContractGraphNode> = Vec::new();
    // Function name -> node ids (all same-named candidates stay visible).
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for file in &revision.files {
        for function in &file.functions {
            let (role, roles, capability) = classify(&file.path, function);
            let id = format!(
                "{}::{}@{}",
                file.path.display(),
                function.name,
                function.span.start_byte
            );
            by_name
                .entry(function.name.as_str())
                .or_default()
                .push(nodes.len());
            nodes.push(ContractGraphNode {
                id,
                role,
                roles,
                name: function.name.clone(),
                path: file.path.clone(),
                span: function.span,
                fingerprint: function.fingerprint.clone(),
                provider: FactProvider::TreeSitter,
                capability,
            });
        }
    }
    // Every observed configuration key has one real graph endpoint. Module
    // acceptance surfaces and function reads converge on the same node id.
    let mut config_nodes: BTreeMap<String, (PathBuf, deslop_core::Span)> = BTreeMap::new();
    for file in &revision.files {
        for (token, span) in &file.module_config_keys {
            config_nodes
                .entry(token.clone())
                .or_insert_with(|| (file.path.clone(), *span));
        }
        for function in &file.functions {
            for token in &function.config_keys {
                config_nodes
                    .entry(token.clone())
                    .or_insert_with(|| (file.path.clone(), function.span));
            }
        }
    }
    for (token, (path, span)) in config_nodes {
        nodes.push(ContractGraphNode {
            id: format!("config:{token}"),
            role: ContractRole::ConfigParameter,
            roles: vec![ContractRole::ConfigParameter],
            name: token.clone(),
            path,
            span,
            fingerprint: format!("module-token:{token}"),
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Complete,
        });
    }

    // Preserve unresolved terminal references as typed endpoints rather than
    // silently dropping them. They carry Unknown authority and cannot prove
    // an external binding or negative reachability.
    let mut unresolved: BTreeMap<String, (PathBuf, deslop_core::Span)> = BTreeMap::new();
    for file in &revision.files {
        for function in &file.functions {
            for token in &function.references {
                if !by_name.contains_key(token_leaf(token)) {
                    unresolved
                        .entry(token.clone())
                        .or_insert_with(|| (file.path.clone(), function.span));
                }
            }
        }
    }
    for (token, (path, span)) in unresolved {
        nodes.push(ContractGraphNode {
            id: format!("unresolved:{token}"),
            role: ContractRole::UnresolvedEndpoint,
            roles: vec![ContractRole::UnresolvedEndpoint],
            name: token.clone(),
            path,
            span,
            fingerprint: format!("unresolved-token:{token}"),
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Unknown,
        });
    }

    let mut edges: Vec<ContractGraphEdge> = Vec::new();
    for file in &revision.files {
        for function in &file.functions {
            let from = format!(
                "{}::{}@{}",
                file.path.display(),
                function.name,
                function.span.start_byte
            );
            let (role, _, _) = classify(&file.path, function);
            let kind = edge_kind_for(role);
            for token in &function.references {
                let leaf = token_leaf(token);
                let Some(candidates) = by_name.get(leaf) else {
                    edges.push(ContractGraphEdge {
                        from: from.clone(),
                        to: format!("unresolved:{token}"),
                        kind,
                        token: token.clone(),
                        confidence: ContractEdgeConfidence::Unresolved,
                        provider: FactProvider::TreeSitter,
                        capability: CapabilityLevel::Unknown,
                    });
                    continue;
                };
                let confidence = if candidates.len() == 1 {
                    ContractEdgeConfidence::Syntactic
                } else {
                    ContractEdgeConfidence::Ambiguous
                };
                for target in candidates {
                    if nodes[*target].id == from {
                        continue;
                    }
                    edges.push(ContractGraphEdge {
                        from: from.clone(),
                        to: nodes[*target].id.clone(),
                        kind,
                        token: token.clone(),
                        confidence,
                        provider: FactProvider::TreeSitter,
                        capability: CapabilityLevel::Partial,
                    });
                }
            }
            for key in &function.config_keys {
                edges.push(ContractGraphEdge {
                    from: from.clone(),
                    to: format!("config:{key}"),
                    kind: ContractEdgeKind::Reads,
                    token: key.clone(),
                    confidence: ContractEdgeConfidence::Syntactic,
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Partial,
                });
            }
        }
    }
    edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.token).cmp(&(&right.from, &right.to, &right.token))
    });

    let graph = ContractGraph {
        schema: CONTRACT_GRAPH_SCHEMA.to_string(),
        coverage: snapshot.coverage,
        coverage_reasons: snapshot.reasons,
        notes: vec![
            "reference edges are leaf-name syntactic candidates, not resolution proof; \
             same-named targets are retained as ambiguous"
                .to_string(),
            "telemetry and runtime-identity roles rest on lexical surface classification \
             and carry partial capability"
                .to_string(),
            "unresolved reference endpoints are retained with unknown capability; they are not proven external"
                .to_string(),
        ],
        nodes,
        edges,
    };
    Ok(ContractGraphProjection {
        id,
        analysis,
        graph,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_parse::{ProjectSnapshotBuilder, RepositoryId};

    fn analysis(files: &[(&str, &[u8])]) -> Arc<ProjectAnalysis> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("contract-graph-test").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        ProjectAnalysis::build(builder.build().unwrap()).unwrap()
    }

    const SOURCE: &[u8] = br#"import os


def decide(model, candidates):
    return raw_score(model, candidates)


def raw_score(model, candidate):
    return model.forward(candidate)


def release_check(model):
    assert raw_score(model, probe()) > 0


def report_health(model):
    metrics.gauge("activity", raw_score(model, probe()))


def publish_status(model):
    status.publish(raw_score(model, probe()))


def load_config():
    return os.environ["THRESHOLD"]
"#;

    const TEST_SOURCE: &[u8] = br#"def test_decide():
    assert decide(model(), [candidate()]) is not None
"#;

    fn build() -> ContractGraphProjection {
        contract_graph_analysis(analysis(&[
            ("scoring.py", SOURCE),
            ("test_scoring.py", TEST_SOURCE),
        ]))
        .unwrap()
    }

    #[test]
    fn roles_are_classified_with_honest_capability() {
        let projection = build();
        let role_of = |name: &str| {
            projection
                .graph
                .nodes
                .iter()
                .find(|node| node.name == name)
                .unwrap_or_else(|| panic!("missing node {name}"))
        };
        assert_eq!(role_of("decide").role, ContractRole::Owner);
        assert!(role_of("decide").roles.contains(&ContractRole::Consumer));
        assert!(role_of("decide").roles.contains(&ContractRole::Owner));
        assert_eq!(role_of("release_check").role, ContractRole::Verifier);
        assert_eq!(
            role_of("release_check").capability,
            CapabilityLevel::Complete
        );
        let health = role_of("report_health");
        assert_eq!(health.role, ContractRole::TelemetrySurface);
        assert!(health.roles.contains(&ContractRole::Consumer));
        assert!(health.roles.contains(&ContractRole::TelemetrySurface));
        assert_eq!(health.capability, CapabilityLevel::Partial);
        let status = role_of("publish_status");
        assert_eq!(status.role, ContractRole::RuntimeIdentity);
        assert_eq!(status.capability, CapabilityLevel::Partial);
        assert_eq!(role_of("test_decide").role, ContractRole::TestEntryPoint);
    }

    #[test]
    fn julia_throw_gate_is_a_partial_verifier() {
        let projection = contract_graph_analysis(analysis(&[(
            "gate.jl",
            b"function require_activity(x)\n    x.metric_count > 0 || throw(ArgumentError(\"empty\"))\nend\n",
        )]))
        .unwrap();
        let gate = projection
            .graph
            .nodes
            .iter()
            .find(|node| node.name == "require_activity")
            .expect("Julia gate node");
        assert!(gate.roles.contains(&ContractRole::Verifier));
        assert_eq!(gate.capability, CapabilityLevel::Partial);
    }

    #[test]
    fn dependents_traversal_reaches_every_surface_kind() {
        let projection = build();
        let raw_score = projection
            .graph
            .nodes
            .iter()
            .find(|node| node.name == "raw_score")
            .unwrap();
        let dependents = projection.graph.dependents_of(&raw_score.id);
        let kinds: Vec<ContractEdgeKind> = dependents.iter().map(|(_, edge)| edge.kind).collect();
        assert!(kinds.contains(&ContractEdgeKind::Consumes));
        assert!(kinds.contains(&ContractEdgeKind::Verifies));
        assert!(kinds.contains(&ContractEdgeKind::Observes));
        assert!(kinds.contains(&ContractEdgeKind::Publishes));
        let decide = projection
            .graph
            .nodes
            .iter()
            .find(|node| node.name == "decide")
            .unwrap();
        let test_dependents = projection.graph.dependents_of(&decide.id);
        assert!(
            test_dependents
                .iter()
                .any(|(node, edge)| node.role == ContractRole::TestEntryPoint
                    && edge.kind == ContractEdgeKind::Exercises)
        );
    }

    #[test]
    fn config_reads_and_module_surfaces_are_nodes_and_edges() {
        let projection = build();
        assert!(
            projection
                .graph
                .edges
                .iter()
                .any(|edge| edge.kind == ContractEdgeKind::Reads && edge.token == "THRESHOLD")
        );
        assert!(
            projection
                .graph
                .nodes
                .iter()
                .any(|node| node.id == "config:THRESHOLD")
        );
        assert!(
            projection
                .graph
                .edges
                .iter()
                .all(|edge| { projection.graph.nodes.iter().any(|node| node.id == edge.to) })
        );
    }

    #[test]
    fn projection_identity_and_wire_are_deterministic() {
        let analysis = analysis(&[("scoring.py", SOURCE)]);
        let first = contract_graph_analysis(Arc::clone(&analysis)).unwrap();
        let second = contract_graph_analysis(analysis).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(
            serde_json::to_string(&first.graph).unwrap(),
            serde_json::to_string(&second.graph).unwrap()
        );
        assert_eq!(first.graph.schema, CONTRACT_GRAPH_SCHEMA);
    }

    #[test]
    fn ambiguous_same_named_targets_stay_visible() {
        let projection = contract_graph_analysis(analysis(&[
            (
                "a.py",
                b"def helper():\n    return 1\n\n\ndef caller():\n    return helper()\n",
            ),
            ("b.py", b"def helper():\n    return 2\n"),
        ]))
        .unwrap();
        let ambiguous: Vec<&ContractGraphEdge> = projection
            .graph
            .edges
            .iter()
            .filter(|edge| edge.token == "helper")
            .collect();
        assert_eq!(ambiguous.len(), 2, "both candidates stay visible");
        assert!(
            ambiguous
                .iter()
                .all(|edge| edge.confidence == ContractEdgeConfidence::Ambiguous)
        );
    }

    #[test]
    fn unsupported_language_is_a_coverage_reason() {
        let projection =
            contract_graph_analysis(analysis(&[("lib.rs", b"fn main() {}\n")])).unwrap();
        assert_eq!(projection.graph.coverage, FactCoverage::Partial);
        assert!(
            projection.graph.coverage_reasons[0].contains("no contract query"),
            "{:?}",
            projection.graph.coverage_reasons
        );
    }
}
