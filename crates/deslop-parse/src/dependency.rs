use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    CapabilityAuthority, FactCoverage, NameNamespace, ProjectionId, ResolutionEndpoint,
    ResolutionPolicyId, ResolutionProjection, ResolutionResultKey, ResolutionStatus, ScopeFactData,
    ScopeFactKey, Visibility,
};

pub const DEPENDENCY_SCHEMA: &str = "deslop.dependency/1";
pub const DEPENDENCY_POLICY_SCHEMA: &str = "deslop.dependency-policy/1";

const POLICY_DOMAIN: &str = "deslop dependency policy v1";
const NODE_DOMAIN: &str = "deslop dependency node v1";
const EDGE_DOMAIN: &str = "deslop dependency edge v1";
const GAP_DOMAIN: &str = "deslop dependency gap v1";

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

digest_id!(DependencyPolicyId, "dpp1_");
digest_id!(DependencyNodeKey, "dpn1_");
digest_id!(DependencyEdgeKey, "dpe1_");
digest_id!(DependencyGapKey, "dpx1_");

impl DependencyPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, DependencyBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(DependencyBuildError::Invalid(
                "dependency policy identity requires nonempty parts".into(),
            ));
        }
        Ok(Self(derive_id(POLICY_DOMAIN, "dpp1_", parts)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DependencyNodeKind {
    File {
        path: PathBuf,
    },
    Module {
        package_id: String,
        target_id: String,
        source_root: String,
        module_path: Vec<String>,
    },
    Package {
        package_id: String,
    },
    BuildTarget {
        package_id: String,
        target_id: String,
    },
    LocalApi {
        declaration: ScopeFactKey,
        name: String,
        namespace: NameNamespace,
        visibility: Visibility,
        file: PathBuf,
        exports: Vec<ScopeFactKey>,
    },
    ExternalApi {
        provider: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyNode {
    key: DependencyNodeKey,
    kind: DependencyNodeKind,
    source_facts: Vec<ScopeFactKey>,
}

impl DependencyNode {
    pub fn key(&self) -> &DependencyNodeKey {
        &self.key
    }

    pub fn kind(&self) -> &DependencyNodeKind {
        &self.kind
    }

    pub fn source_facts(&self) -> &[ScopeFactKey] {
        &self.source_facts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyEdgeKind {
    PackageContainsTarget,
    TargetContainsModule,
    ModuleContainsFile,
    FileDependency,
    ModuleDependency,
    PackageDependency,
    BuildTargetDependency,
    ApiUse,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyEvidence {
    pub resolution: Option<ResolutionResultKey>,
    pub source_facts: Vec<ScopeFactKey>,
    pub authority: Option<CapabilityAuthority>,
    pub coverage: FactCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyEdge {
    key: DependencyEdgeKey,
    from: DependencyNodeKey,
    to: DependencyNodeKey,
    kind: DependencyEdgeKind,
    evidence: Vec<DependencyEvidence>,
}

impl DependencyEdge {
    pub fn key(&self) -> &DependencyEdgeKey {
        &self.key
    }

    pub fn from(&self) -> &DependencyNodeKey {
        &self.from
    }

    pub fn to(&self) -> &DependencyNodeKey {
        &self.to
    }

    pub fn kind(&self) -> DependencyEdgeKind {
        self.kind
    }

    pub fn evidence(&self) -> &[DependencyEvidence] {
        &self.evidence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DependencyGapKind {
    FileWithoutModule {
        path: PathBuf,
    },
    DuplicateFileOwnership {
        path: PathBuf,
        modules: Vec<ScopeFactKey>,
    },
    IncompleteExports {
        module: ScopeFactKey,
        status: FactCoverage,
        reasons: Vec<String>,
    },
    IncompleteResolution {
        result: ResolutionResultKey,
        status: ResolutionStatus,
        coverage: FactCoverage,
        dynamic_boundaries: Vec<ScopeFactKey>,
    },
    UnsupportedEndpoint {
        result: ResolutionResultKey,
        endpoint: ResolutionEndpoint,
    },
    EndpointWithoutFile {
        result: ResolutionResultKey,
        fact: ScopeFactKey,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyGap {
    key: DependencyGapKey,
    kind: DependencyGapKind,
}

impl DependencyGap {
    pub fn key(&self) -> &DependencyGapKey {
        &self.key
    }

    pub fn kind(&self) -> &DependencyGapKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

impl DependencyCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    resolution_projection_id: ProjectionId,
    resolution_policy: ResolutionPolicyId,
    build_context: crate::BuildContextId,
    policy: DependencyPolicyId,
    coverage: DependencyCoverageEvidence,
    nodes: Vec<DependencyNode>,
    edges: Vec<DependencyEdge>,
    gaps: Vec<DependencyGap>,
}

impl DependencyDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn resolution_projection_id(&self) -> &ProjectionId {
        &self.resolution_projection_id
    }

    pub fn resolution_policy(&self) -> &ResolutionPolicyId {
        &self.resolution_policy
    }

    pub fn build_context(&self) -> &crate::BuildContextId {
        &self.build_context
    }

    pub fn policy(&self) -> &DependencyPolicyId {
        &self.policy
    }

    pub fn coverage(&self) -> &DependencyCoverageEvidence {
        &self.coverage
    }

    pub fn nodes(&self) -> &[DependencyNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[DependencyEdge] {
        &self.edges
    }

    pub fn gaps(&self) -> &[DependencyGap] {
        &self.gaps
    }

    fn validate(&self) -> Result<(), DependencyBuildError> {
        if self.schema != DEPENDENCY_SCHEMA {
            return Err(invalid(format!(
                "unsupported dependency schema {}",
                self.schema
            )));
        }
        validate_digest(self.projection_id.as_str(), "pj1_")?;
        validate_digest(self.resolution_projection_id.as_str(), "pj1_")?;
        validate_digest(self.policy.as_str(), "dpp1_")?;
        validate_digest(&self.analysis_id, "pa1_")?;
        validate_sorted("dependency nodes", &self.nodes, |node| node.key.as_str())?;
        validate_sorted("dependency edges", &self.edges, |edge| edge.key.as_str())?;
        validate_sorted("dependency gaps", &self.gaps, |gap| gap.key.as_str())?;
        let node_keys = self
            .nodes
            .iter()
            .map(|node| &node.key)
            .collect::<BTreeSet<_>>();
        for node in &self.nodes {
            validate_texts(&node.source_facts)?;
            if node.key != make_node_key(&node.kind)? {
                return Err(invalid("dependency node key does not match kind"));
            }
            validate_node_kind(&node.kind)?;
        }
        for edge in &self.edges {
            if !node_keys.contains(&edge.from) || !node_keys.contains(&edge.to) {
                return Err(invalid("dependency edge endpoint is absent"));
            }
            if edge.from == edge.to {
                return Err(invalid("dependency edge cannot be a self-edge"));
            }
            if edge.evidence.is_empty() || !strictly_sorted(&edge.evidence) {
                return Err(invalid(
                    "dependency edge evidence is not canonical and distinct",
                ));
            }
            for evidence in &edge.evidence {
                validate_texts(&evidence.source_facts)?;
                if evidence.coverage == FactCoverage::Complete && evidence.authority.is_none() {
                    return Err(invalid("complete dependency evidence requires authority"));
                }
            }
            if edge.key
                != make_edge_key(
                    &self.policy,
                    &edge.from,
                    &edge.to,
                    edge.kind,
                    &edge.evidence,
                )?
            {
                return Err(invalid("dependency edge key does not match payload"));
            }
        }
        for gap in &self.gaps {
            if gap.key != make_gap_key(&self.policy, &gap.kind)? {
                return Err(invalid("dependency gap key does not match payload"));
            }
        }
        validate_coverage(&self.coverage, &self.gaps)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    resolution_projection_id: ProjectionId,
    resolution_policy: ResolutionPolicyId,
    build_context: crate::BuildContextId,
    policy: DependencyPolicyId,
    coverage: DependencyCoverageEvidence,
    nodes: Vec<DependencyNode>,
    edges: Vec<DependencyEdge>,
    gaps: Vec<DependencyGap>,
}

impl<'de> Deserialize<'de> for DependencyDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = DependencyDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            resolution_projection_id: wire.resolution_projection_id,
            resolution_policy: wire.resolution_policy,
            build_context: wire.build_context,
            policy: wire.policy,
            coverage: wire.coverage,
            nodes: wire.nodes,
            edges: wire.edges,
            gaps: wire.gaps,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct DependencyProjection {
    id: ProjectionId,
    resolution: Arc<ResolutionProjection>,
    policy: DependencyPolicyId,
    document: DependencyDocument,
}

impl DependencyProjection {
    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn resolution(&self) -> &Arc<ResolutionProjection> {
        &self.resolution
    }

    pub fn policy(&self) -> &DependencyPolicyId {
        &self.policy
    }

    pub fn document(&self) -> &DependencyDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for DependencyBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid dependency evidence: {detail}"),
            Self::Identity(detail) => write!(formatter, "dependency identity error: {detail}"),
        }
    }
}

impl std::error::Error for DependencyBuildError {}

pub fn derive_dependencies(
    resolution: Arc<ResolutionProjection>,
    policy: DependencyPolicyId,
) -> Result<DependencyProjection, DependencyBuildError> {
    let scope = resolution.scope_graph();
    let analysis = scope.analysis();
    let facts = scope.facts();
    let facts_by_key = facts
        .iter()
        .map(|fact| (fact.key(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut nodes = BTreeMap::<DependencyNodeKey, DependencyNode>::new();
    let mut edge_evidence = BTreeMap::<
        (DependencyNodeKey, DependencyNodeKey, DependencyEdgeKind),
        BTreeSet<DependencyEvidence>,
    >::new();
    let mut gap_kinds = BTreeSet::new();

    let mut file_nodes = BTreeMap::new();
    for file in analysis.files() {
        let path = file.key().path.clone();
        let key = add_node(
            &mut nodes,
            DependencyNodeKind::File { path: path.clone() },
            vec![],
        )?;
        file_nodes.insert(path, key);
    }

    let mut module_by_fact = BTreeMap::new();
    let mut module_owners = BTreeMap::<PathBuf, Vec<(ScopeFactKey, DependencyNodeKey)>>::new();
    let mut module_parts =
        BTreeMap::<DependencyNodeKey, (DependencyNodeKey, DependencyNodeKey)>::new();
    for fact in facts {
        let ScopeFactData::BuildModule {
            package_id,
            target_id,
            source_root,
            module_path,
            file_scopes,
            export_coverage,
        } = fact.data()
        else {
            continue;
        };
        let module = add_node(
            &mut nodes,
            DependencyNodeKind::Module {
                package_id: package_id.clone(),
                target_id: target_id.clone(),
                source_root: source_root.clone(),
                module_path: module_path.clone(),
            },
            vec![fact.key().clone()],
        )?;
        let package = add_node(
            &mut nodes,
            DependencyNodeKind::Package {
                package_id: package_id.clone(),
            },
            vec![fact.key().clone()],
        )?;
        let target = add_node(
            &mut nodes,
            DependencyNodeKind::BuildTarget {
                package_id: package_id.clone(),
                target_id: target_id.clone(),
            },
            vec![fact.key().clone()],
        )?;
        module_by_fact.insert(fact.key().clone(), module.clone());
        module_parts.insert(module.clone(), (package.clone(), target.clone()));
        let containment = build_evidence(
            None,
            vec![fact.key().clone()],
            fact.evidence().authority,
            fact.evidence().coverage.status,
        );
        add_edge_evidence(
            &mut edge_evidence,
            package,
            target.clone(),
            DependencyEdgeKind::PackageContainsTarget,
            containment.clone(),
        );
        add_edge_evidence(
            &mut edge_evidence,
            target,
            module.clone(),
            DependencyEdgeKind::TargetContainsModule,
            containment.clone(),
        );
        for file_scope in file_scopes {
            let Some(file_fact) = facts_by_key.get(file_scope) else {
                return Err(invalid(format!(
                    "BuildModule {} names absent file scope {}",
                    fact.key().as_str(),
                    file_scope.as_str()
                )));
            };
            let path = file_fact.evidence().node_key.file().path.clone();
            let Some(file_node) = file_nodes.get(&path).cloned() else {
                return Err(invalid(format!(
                    "BuildModule file {} is absent from analysis",
                    path.display()
                )));
            };
            module_owners
                .entry(path)
                .or_default()
                .push((fact.key().clone(), module.clone()));
            add_edge_evidence(
                &mut edge_evidence,
                module.clone(),
                file_node,
                DependencyEdgeKind::ModuleContainsFile,
                containment.clone(),
            );
        }
        if export_coverage.status != FactCoverage::Complete {
            gap_kinds.insert(DependencyGapKind::IncompleteExports {
                module: fact.key().clone(),
                status: export_coverage.status,
                reasons: export_coverage.reason.iter().cloned().collect(),
            });
        }
    }

    let mut unique_module = BTreeMap::new();
    for (path, owners) in &module_owners {
        let mut canonical = owners.clone();
        canonical.sort();
        canonical.dedup();
        if canonical.len() == 1 {
            unique_module.insert(path.clone(), canonical[0].1.clone());
        } else {
            gap_kinds.insert(DependencyGapKind::DuplicateFileOwnership {
                path: path.clone(),
                modules: canonical.into_iter().map(|(fact, _)| fact).collect(),
            });
        }
    }
    for path in file_nodes.keys() {
        if !module_owners.contains_key(path) {
            gap_kinds.insert(DependencyGapKind::FileWithoutModule { path: path.clone() });
        }
    }

    for record in resolution.results() {
        let result = record.wire();
        if result.coverage().status() != FactCoverage::Complete
            || result.status() != ResolutionStatus::Unique
            || result.preferred().is_none_or(|preferred| {
                preferred.status() != ResolutionStatus::Unique || preferred.endpoints().len() != 1
            })
            || !result.dynamic_boundaries().is_empty()
        {
            gap_kinds.insert(DependencyGapKind::IncompleteResolution {
                result: result.key().clone(),
                status: result.status(),
                coverage: result.coverage().status(),
                dynamic_boundaries: result.dynamic_boundaries().to_vec(),
            });
            continue;
        }
        let endpoint = result.preferred().expect("checked preferred").endpoints()[0].clone();
        let source_path = result.reference_evidence().node_key.file().path.clone();
        let source_file = file_nodes.get(&source_path).cloned().ok_or_else(|| {
            invalid(format!(
                "reference source file {} is absent",
                source_path.display()
            ))
        })?;
        let evidence = build_evidence(
            Some(result.key().clone()),
            result.source_facts().to_vec(),
            result.authority(),
            result.coverage().status(),
        );
        match endpoint.clone() {
            ResolutionEndpoint::Declaration(declaration)
            | ResolutionEndpoint::Definition(declaration) => {
                let declaration = canonical_declaration(&facts_by_key, &declaration)?;
                let fact = facts_by_key.get(&declaration).ok_or_else(|| {
                    invalid(format!(
                        "resolution endpoint {} is absent",
                        declaration.as_str()
                    ))
                })?;
                let ScopeFactData::Declaration {
                    original_name,
                    namespace,
                    visibility,
                    ..
                } = fact.data()
                else {
                    return Err(invalid("canonical API endpoint is not a declaration"));
                };
                let target_path = fact.evidence().node_key.file().path.clone();
                let Some(target_file) = file_nodes.get(&target_path).cloned() else {
                    gap_kinds.insert(DependencyGapKind::EndpointWithoutFile {
                        result: result.key().clone(),
                        fact: declaration,
                    });
                    continue;
                };
                let mut exports = facts
                    .iter()
                    .filter_map(|candidate| match candidate.data() {
                        ScopeFactData::Export {
                            local_target: Some(target),
                            ..
                        } if target == &declaration
                            && candidate.evidence().coverage.status == FactCoverage::Complete
                            && candidate.evidence().authority.is_some() =>
                        {
                            Some(candidate.key().clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                exports.sort();
                exports.dedup();
                let api = add_node(
                    &mut nodes,
                    DependencyNodeKind::LocalApi {
                        declaration: declaration.clone(),
                        name: original_name.clone(),
                        namespace: namespace.clone(),
                        visibility: visibility.clone(),
                        file: target_path.clone(),
                        exports: exports.clone(),
                    },
                    std::iter::once(declaration).chain(exports).collect(),
                )?;
                add_edge_evidence(
                    &mut edge_evidence,
                    source_file.clone(),
                    api,
                    DependencyEdgeKind::ApiUse,
                    evidence.clone(),
                );
                add_level_dependencies(
                    &mut edge_evidence,
                    &unique_module,
                    &module_parts,
                    DependencyFilePair {
                        source_path: &source_path,
                        target_path: &target_path,
                        source_file: &source_file,
                        target_file: &target_file,
                    },
                    evidence,
                );
            }
            ResolutionEndpoint::Module(module_fact) => {
                let Some(target_module) = module_by_fact.get(&module_fact).cloned() else {
                    gap_kinds.insert(DependencyGapKind::UnsupportedEndpoint {
                        result: result.key().clone(),
                        endpoint,
                    });
                    continue;
                };
                if let Some(source_module) = unique_module.get(&source_path)
                    && source_module != &target_module
                {
                    add_edge_evidence(
                        &mut edge_evidence,
                        source_module.clone(),
                        target_module.clone(),
                        DependencyEdgeKind::ModuleDependency,
                        evidence.clone(),
                    );
                    add_parent_dependencies(
                        &mut edge_evidence,
                        &module_parts,
                        source_module,
                        &target_module,
                        evidence,
                    );
                }
            }
            ResolutionEndpoint::External(provider) => {
                let api = add_node(
                    &mut nodes,
                    DependencyNodeKind::ExternalApi { provider },
                    vec![],
                )?;
                add_edge_evidence(
                    &mut edge_evidence,
                    source_file,
                    api,
                    DependencyEdgeKind::ApiUse,
                    evidence,
                );
            }
            ResolutionEndpoint::MergedDeclarations(_) => {
                gap_kinds.insert(DependencyGapKind::UnsupportedEndpoint {
                    result: result.key().clone(),
                    endpoint,
                });
            }
        }
    }

    let mut nodes = nodes.into_values().collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.key.cmp(&right.key));
    let mut edges = edge_evidence
        .into_iter()
        .map(|((from, to, kind), evidence)| {
            make_edge(&policy, from, to, kind, evidence.into_iter().collect())
        })
        .collect::<Result<Vec<_>, _>>()?;
    edges.sort_by(|left, right| left.key.cmp(&right.key));
    let mut gaps = gap_kinds
        .into_iter()
        .map(|kind| make_gap(&policy, kind))
        .collect::<Result<Vec<_>, _>>()?;
    gaps.sort_by(|left, right| left.key.cmp(&right.key));
    let coverage = make_coverage(&gaps);
    let payload = serde_json::to_vec(&(
        resolution.id(),
        resolution.resolution_policy(),
        scope.build_context(),
        &policy,
        &coverage,
        &nodes,
        &edges,
        &gaps,
    ))
    .map_err(|error| DependencyBuildError::Identity(error.to_string()))?;
    let id = analysis
        .derive_projection_id(
            DEPENDENCY_SCHEMA,
            &payload,
            resolution.id().as_str().as_bytes(),
        )
        .map_err(|error| DependencyBuildError::Identity(error.to_string()))?;
    let document = DependencyDocument {
        schema: DEPENDENCY_SCHEMA.into(),
        projection_id: id.clone(),
        analysis_id: analysis.id().as_str().into(),
        resolution_projection_id: resolution.id().clone(),
        resolution_policy: resolution.resolution_policy().clone(),
        build_context: scope.build_context().clone(),
        policy: policy.clone(),
        coverage,
        nodes,
        edges,
        gaps,
    };
    document.validate()?;
    Ok(DependencyProjection {
        id,
        resolution,
        policy,
        document,
    })
}

fn canonical_declaration(
    facts: &BTreeMap<&ScopeFactKey, &crate::ScopeFactRecord>,
    endpoint: &ScopeFactKey,
) -> Result<ScopeFactKey, DependencyBuildError> {
    let fact = facts
        .get(endpoint)
        .ok_or_else(|| invalid(format!("endpoint {} is absent", endpoint.as_str())))?;
    match fact.data() {
        ScopeFactData::Declaration { .. } => Ok(endpoint.clone()),
        ScopeFactData::Definition { declaration, .. } => Ok(declaration.clone()),
        _ => Err(invalid(format!(
            "endpoint {} is not declaration/definition",
            endpoint.as_str()
        ))),
    }
}

struct DependencyFilePair<'a> {
    source_path: &'a PathBuf,
    target_path: &'a PathBuf,
    source_file: &'a DependencyNodeKey,
    target_file: &'a DependencyNodeKey,
}

fn add_level_dependencies(
    edges: &mut BTreeMap<
        (DependencyNodeKey, DependencyNodeKey, DependencyEdgeKind),
        BTreeSet<DependencyEvidence>,
    >,
    modules: &BTreeMap<PathBuf, DependencyNodeKey>,
    parts: &BTreeMap<DependencyNodeKey, (DependencyNodeKey, DependencyNodeKey)>,
    files: DependencyFilePair<'_>,
    evidence: DependencyEvidence,
) {
    if files.source_file != files.target_file {
        add_edge_evidence(
            edges,
            files.source_file.clone(),
            files.target_file.clone(),
            DependencyEdgeKind::FileDependency,
            evidence.clone(),
        );
    }
    let (Some(source_module), Some(target_module)) = (
        modules.get(files.source_path),
        modules.get(files.target_path),
    ) else {
        return;
    };
    if source_module != target_module {
        add_edge_evidence(
            edges,
            source_module.clone(),
            target_module.clone(),
            DependencyEdgeKind::ModuleDependency,
            evidence.clone(),
        );
        add_parent_dependencies(edges, parts, source_module, target_module, evidence);
    }
}

fn add_parent_dependencies(
    edges: &mut BTreeMap<
        (DependencyNodeKey, DependencyNodeKey, DependencyEdgeKind),
        BTreeSet<DependencyEvidence>,
    >,
    parts: &BTreeMap<DependencyNodeKey, (DependencyNodeKey, DependencyNodeKey)>,
    source_module: &DependencyNodeKey,
    target_module: &DependencyNodeKey,
    evidence: DependencyEvidence,
) {
    let (Some((source_package, source_target)), Some((target_package, target_target))) =
        (parts.get(source_module), parts.get(target_module))
    else {
        return;
    };
    if source_package != target_package {
        add_edge_evidence(
            edges,
            source_package.clone(),
            target_package.clone(),
            DependencyEdgeKind::PackageDependency,
            evidence.clone(),
        );
    }
    if source_target != target_target {
        add_edge_evidence(
            edges,
            source_target.clone(),
            target_target.clone(),
            DependencyEdgeKind::BuildTargetDependency,
            evidence,
        );
    }
}

fn add_node(
    nodes: &mut BTreeMap<DependencyNodeKey, DependencyNode>,
    kind: DependencyNodeKind,
    source_facts: Vec<ScopeFactKey>,
) -> Result<DependencyNodeKey, DependencyBuildError> {
    let key = make_node_key(&kind)?;
    let node = nodes.entry(key.clone()).or_insert_with(|| DependencyNode {
        key: key.clone(),
        kind,
        source_facts: vec![],
    });
    node.source_facts.extend(source_facts);
    node.source_facts.sort();
    node.source_facts.dedup();
    Ok(key)
}

fn add_edge_evidence(
    edges: &mut BTreeMap<
        (DependencyNodeKey, DependencyNodeKey, DependencyEdgeKind),
        BTreeSet<DependencyEvidence>,
    >,
    from: DependencyNodeKey,
    to: DependencyNodeKey,
    kind: DependencyEdgeKind,
    evidence: DependencyEvidence,
) {
    if from != to {
        edges.entry((from, to, kind)).or_default().insert(evidence);
    }
}

fn build_evidence(
    resolution: Option<ResolutionResultKey>,
    mut source_facts: Vec<ScopeFactKey>,
    authority: Option<CapabilityAuthority>,
    coverage: FactCoverage,
) -> DependencyEvidence {
    source_facts.sort();
    source_facts.dedup();
    DependencyEvidence {
        resolution,
        source_facts,
        authority,
        coverage,
    }
}

fn make_node_key(kind: &DependencyNodeKind) -> Result<DependencyNodeKey, DependencyBuildError> {
    let payload = serde_json::to_vec(kind)
        .map_err(|error| DependencyBuildError::Identity(error.to_string()))?;
    Ok(DependencyNodeKey(derive_id(
        NODE_DOMAIN,
        "dpn1_",
        &[&payload],
    )))
}

fn make_edge(
    policy: &DependencyPolicyId,
    from: DependencyNodeKey,
    to: DependencyNodeKey,
    kind: DependencyEdgeKind,
    evidence: Vec<DependencyEvidence>,
) -> Result<DependencyEdge, DependencyBuildError> {
    let key = make_edge_key(policy, &from, &to, kind, &evidence)?;
    Ok(DependencyEdge {
        key,
        from,
        to,
        kind,
        evidence,
    })
}

fn make_edge_key(
    policy: &DependencyPolicyId,
    from: &DependencyNodeKey,
    to: &DependencyNodeKey,
    kind: DependencyEdgeKind,
    evidence: &[DependencyEvidence],
) -> Result<DependencyEdgeKey, DependencyBuildError> {
    let payload = serde_json::to_vec(&(policy, from, to, kind, evidence))
        .map_err(|error| DependencyBuildError::Identity(error.to_string()))?;
    Ok(DependencyEdgeKey(derive_id(
        EDGE_DOMAIN,
        "dpe1_",
        &[&payload],
    )))
}

fn make_gap(
    policy: &DependencyPolicyId,
    kind: DependencyGapKind,
) -> Result<DependencyGap, DependencyBuildError> {
    let key = make_gap_key(policy, &kind)?;
    Ok(DependencyGap { key, kind })
}

fn make_gap_key(
    policy: &DependencyPolicyId,
    kind: &DependencyGapKind,
) -> Result<DependencyGapKey, DependencyBuildError> {
    let payload = serde_json::to_vec(&(policy, kind))
        .map_err(|error| DependencyBuildError::Identity(error.to_string()))?;
    Ok(DependencyGapKey(derive_id(
        GAP_DOMAIN,
        "dpx1_",
        &[&payload],
    )))
}

fn make_coverage(gaps: &[DependencyGap]) -> DependencyCoverageEvidence {
    let mut reasons = gaps.iter().map(gap_reason).collect::<Vec<_>>();
    reasons.sort();
    reasons.dedup();
    DependencyCoverageEvidence {
        status: gaps
            .iter()
            .map(gap_coverage)
            .max_by_key(|coverage| coverage_severity(*coverage))
            .unwrap_or(FactCoverage::Complete),
        reasons,
    }
}

fn gap_reason(gap: &DependencyGap) -> String {
    match &gap.kind {
        DependencyGapKind::FileWithoutModule { path } => {
            format!("file {} has no BuildModule owner", path.display())
        }
        DependencyGapKind::DuplicateFileOwnership { path, .. } => {
            format!("file {} has duplicate BuildModule owners", path.display())
        }
        DependencyGapKind::IncompleteExports { module, status, .. } => format!(
            "module {} export coverage is {}",
            module.as_str(),
            fact_coverage_label(*status)
        ),
        DependencyGapKind::IncompleteResolution {
            result,
            status,
            coverage,
            ..
        } => format!(
            "resolution {} is {} with {} coverage",
            result.as_str(),
            resolution_status_label(*status),
            fact_coverage_label(*coverage)
        ),
        DependencyGapKind::UnsupportedEndpoint { result, .. } => {
            format!("resolution {} has unsupported endpoint", result.as_str())
        }
        DependencyGapKind::EndpointWithoutFile { result, fact } => format!(
            "resolution {} endpoint {} has no analyzed file",
            result.as_str(),
            fact.as_str()
        ),
    }
}

fn gap_coverage(gap: &DependencyGap) -> FactCoverage {
    match &gap.kind {
        DependencyGapKind::IncompleteExports { status, .. }
        | DependencyGapKind::IncompleteResolution {
            coverage: status, ..
        } => *status,
        DependencyGapKind::UnsupportedEndpoint { .. } => FactCoverage::Unsupported,
        DependencyGapKind::FileWithoutModule { .. }
        | DependencyGapKind::DuplicateFileOwnership { .. }
        | DependencyGapKind::EndpointWithoutFile { .. } => FactCoverage::Partial,
    }
}

fn coverage_severity(coverage: FactCoverage) -> u8 {
    match coverage {
        FactCoverage::Complete => 0,
        FactCoverage::Partial => 1,
        FactCoverage::Unsupported => 2,
        FactCoverage::Failed => 3,
    }
}

fn fact_coverage_label(coverage: FactCoverage) -> &'static str {
    match coverage {
        FactCoverage::Complete => "complete",
        FactCoverage::Partial => "partial",
        FactCoverage::Unsupported => "unsupported",
        FactCoverage::Failed => "failed",
    }
}

fn resolution_status_label(status: ResolutionStatus) -> &'static str {
    match status {
        ResolutionStatus::Unique => "unique",
        ResolutionStatus::Ambiguous => "ambiguous",
        ResolutionStatus::Unresolved => "unresolved",
        ResolutionStatus::Unknown => "unknown",
        ResolutionStatus::Conflict => "conflict",
    }
}

fn validate_node_kind(kind: &DependencyNodeKind) -> Result<(), DependencyBuildError> {
    match kind {
        DependencyNodeKind::File { path } if path.as_os_str().is_empty() => {
            Err(invalid("file dependency path is empty"))
        }
        DependencyNodeKind::Module {
            package_id,
            target_id,
            source_root,
            module_path,
        } => {
            validate_text(package_id)?;
            validate_text(target_id)?;
            validate_text(source_root)?;
            if module_path.is_empty() {
                return Err(invalid("module path is empty"));
            }
            for segment in module_path {
                validate_text(segment)?;
            }
            Ok(())
        }
        DependencyNodeKind::Package { package_id } => validate_text(package_id),
        DependencyNodeKind::BuildTarget {
            package_id,
            target_id,
        } => {
            validate_text(package_id)?;
            validate_text(target_id)
        }
        DependencyNodeKind::LocalApi { name, exports, .. } => {
            validate_text(name)?;
            if !strictly_sorted(exports) && !exports.is_empty() {
                return Err(invalid("API exports are not canonical"));
            }
            Ok(())
        }
        DependencyNodeKind::ExternalApi { provider } => validate_text(provider),
        DependencyNodeKind::File { .. } => Ok(()),
    }
}

fn validate_coverage(
    coverage: &DependencyCoverageEvidence,
    gaps: &[DependencyGap],
) -> Result<(), DependencyBuildError> {
    let expected = make_coverage(gaps);
    if coverage != &expected {
        return Err(invalid("dependency coverage does not match gaps"));
    }
    Ok(())
}

fn validate_sorted<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), DependencyBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        Err(invalid(format!("{label} are not canonical and distinct")))
    } else {
        Ok(())
    }
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn validate_texts<T: Ord>(values: &[T]) -> Result<(), DependencyBuildError> {
    if !strictly_sorted(values) && !values.is_empty() {
        return Err(invalid(
            "dependency source facts are not canonical and distinct",
        ));
    }
    Ok(())
}

fn validate_text(value: &str) -> Result<(), DependencyBuildError> {
    if value.trim().is_empty() || value.trim() != value {
        Err(invalid("dependency text must be canonical and nonempty"))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), DependencyBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(invalid(format!("identity must start with {prefix}")));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(
            "identity must contain a canonical 32-byte hexadecimal digest",
        ));
    }
    Ok(())
}

fn invalid(detail: impl Into<String>) -> DependencyBuildError {
    DependencyBuildError::Invalid(detail.into())
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

    use deslop_lang::Registry;

    use super::*;
    use crate::resolution::tests::COMPLETE_RESOLUTION_PACK;
    use crate::{
        BuildContextId, BuildModuleDraft, DeclarationDraft, ExportDraft, FactCoverageEvidence,
        ImportDraft, ImportForm, NamespacePolicy, ProjectAnalysis, ProjectSnapshotBuilder,
        ReferenceDraft, ReferenceRole, RepositoryId, ResolutionProjection, ScopeDraft,
        ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind, SemanticArtifactId, SemanticProviderDraft,
        SemanticProviderKind, SemanticResolutionFactBuilder, SemanticResolutionFactDraft,
        VisibilityDraft, VisibilityKind,
    };

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum FixtureEndpoint {
        ProviderDeclaration,
        ConsumerDeclaration,
        External,
        AdapterOnly,
    }

    pub(crate) fn dependency_fixture(
        export_coverage: FactCoverageEvidence,
        endpoint: FixtureEndpoint,
        duplicate_provider_owner: bool,
    ) -> DependencyProjection {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::default();
        registry.register(&COMPLETE_RESOLUTION_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("dependency-projection-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay(
            "consumer.resolutionrs",
            b"fn consume() { imported; }\n".to_vec(),
        )
        .unwrap()
        .with_overlay("provider.resolutionrs", b"fn imported() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let complete = FactCoverageEvidence::complete();
        let namespaces =
            NamespacePolicy::new(vec![NameNamespace::Value, NameNamespace::Module], vec![])
                .unwrap();
        let consumer_root = node_by_kind(&analysis, "consumer.resolutionrs", "source_file");
        let provider_root = node_by_kind(&analysis, "provider.resolutionrs", "source_file");
        let reference_node = node_by_text(&analysis, "consumer.resolutionrs", "imported");
        let declaration_node = node_by_text(&analysis, "provider.resolutionrs", "imported");
        let mut builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"dependency-build-context"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"dependency-scope-policy/1"]).unwrap(),
        )
        .unwrap();
        let consumer_scope = builder
            .add_scope(
                consumer_root,
                roles(&analysis, consumer_root),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let provider_scope = builder
            .add_scope(
                provider_root,
                roles(&analysis, provider_root),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces,
                },
            )
            .unwrap();
        let declaration = builder
            .add_declaration(
                declaration_node,
                roles(&analysis, declaration_node),
                complete.clone(),
                DeclarationDraft {
                    original_name: "imported".into(),
                    lookup_key: "imported".into(),
                    namespace: NameNamespace::Value,
                    scope: provider_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        let consumer_declaration = (endpoint == FixtureEndpoint::ConsumerDeclaration).then(|| {
            builder
                .add_declaration(
                    reference_node,
                    roles(&analysis, reference_node),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "imported".into(),
                        lookup_key: "imported".into(),
                        namespace: NameNamespace::Value,
                        scope: consumer_scope,
                        visibility: VisibilityDraft {
                            kind: VisibilityKind::Private,
                            boundary: None,
                            adapter_rule: None,
                        },
                        modifiers: vec![],
                    },
                )
                .unwrap()
        });
        builder
            .add_export(
                declaration_node,
                roles(&analysis, declaration_node),
                complete.clone(),
                ExportDraft {
                    scope: provider_scope,
                    local_target: Some(declaration),
                    local_name: Some("imported".into()),
                    exported_name: "imported".into(),
                    reexport_segments: vec![],
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    conditions: vec![],
                },
            )
            .unwrap();
        builder
            .add_import(
                consumer_root,
                roles(&analysis, consumer_root),
                complete.clone(),
                ImportDraft {
                    scope: consumer_scope,
                    module_segments: vec!["dep".into()],
                    form: ImportForm::Selective,
                    alias: None,
                    selected_names: vec!["imported".into()],
                    conditions: vec![],
                },
            )
            .unwrap();
        let reference = builder
            .add_reference(
                reference_node,
                roles(&analysis, reference_node),
                complete.clone(),
                ReferenceDraft {
                    original_spelling: "imported".into(),
                    segments: vec!["imported".into()],
                    namespace: NameNamespace::Value,
                    scope: consumer_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        for (node, scope, package, target, source_root, module) in [
            (
                consumer_root,
                consumer_scope,
                "app-package",
                "app-bin",
                "app/src",
                "app",
            ),
            (
                provider_root,
                provider_scope,
                "dep-package",
                "dep-lib",
                "dep/src",
                "dep",
            ),
        ] {
            builder
                .add_build_module(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    BuildModuleDraft {
                        package_id: package.into(),
                        target_id: target.into(),
                        source_root: source_root.into(),
                        module_path: vec![module.into()],
                        file_scopes: vec![scope],
                        export_coverage: export_coverage.clone(),
                    },
                )
                .unwrap();
        }
        if duplicate_provider_owner {
            builder
                .add_build_module(
                    provider_root,
                    roles(&analysis, provider_root),
                    complete.clone(),
                    BuildModuleDraft {
                        package_id: "other-package".into(),
                        target_id: "other-lib".into(),
                        source_root: "other/src".into(),
                        module_path: vec!["other".into()],
                        file_scopes: vec![provider_scope],
                        export_coverage: export_coverage.clone(),
                    },
                )
                .unwrap();
        }
        let scope = Arc::new(builder.build().unwrap());
        let resolution_policy =
            ResolutionPolicyId::from_parts(&[b"dependency-resolution-policy/1"]).unwrap();
        let resolution = if endpoint != FixtureEndpoint::AdapterOnly {
            let mut semantic = SemanticResolutionFactBuilder::new(Arc::clone(&scope));
            let provider = semantic
                .add_provider(SemanticProviderDraft {
                    kind: SemanticProviderKind::Compiler,
                    name: "dependency-fixture-compiler".into(),
                    version: "1.0.0".into(),
                    executable_artifact: semantic_artifact(b"compiler-executable"),
                    configuration_artifact: semantic_artifact(b"compiler-configuration"),
                    project_model_artifact: Some(semantic_artifact(b"compiler-project-model")),
                    project_model_coverage: complete.clone(),
                })
                .unwrap();
            let endpoint = match endpoint {
                FixtureEndpoint::ProviderDeclaration => {
                    ResolutionEndpoint::Declaration(scope.fact(declaration).unwrap().key().clone())
                }
                FixtureEndpoint::ConsumerDeclaration => ResolutionEndpoint::Declaration(
                    scope
                        .fact(consumer_declaration.expect("consumer declaration requested"))
                        .unwrap()
                        .key()
                        .clone(),
                ),
                FixtureEndpoint::External => {
                    ResolutionEndpoint::External("std::fmt::Display".into())
                }
                FixtureEndpoint::AdapterOnly => unreachable!("checked above"),
            };
            semantic
                .add_fact(SemanticResolutionFactDraft {
                    provider,
                    reference: scope.fact(reference).unwrap().key().clone(),
                    result_artifact: semantic_artifact(b"compiler-resolution-result"),
                    status: ResolutionStatus::Unique,
                    endpoints: vec![endpoint],
                    coverage: complete,
                    diagnostics: vec!["compiler retained exact cross-package endpoint".into()],
                })
                .unwrap();
            Arc::new(
                ResolutionProjection::build_with_semantic_facts(
                    scope,
                    resolution_policy,
                    Arc::new(semantic.finish().unwrap()),
                )
                .unwrap(),
            )
        } else {
            Arc::new(ResolutionProjection::build(scope, resolution_policy).unwrap())
        };
        derive_dependencies(
            resolution,
            DependencyPolicyId::from_parts(&[b"dependency-policy/1"]).unwrap(),
        )
        .unwrap()
    }

    fn node_by_kind(analysis: &ProjectAnalysis, path: &str, kind: &str) -> crate::NodeId {
        analysis
            .node_ids()
            .find(|node| {
                let view = analysis.node(*node).unwrap();
                view.path() == Path::new(path) && view.raw_kind() == kind
            })
            .unwrap()
    }

    fn node_by_text(analysis: &ProjectAnalysis, path: &str, text: &str) -> crate::NodeId {
        analysis
            .node_ids()
            .find(|node| {
                let view = analysis.node(*node).unwrap();
                view.path() == Path::new(path) && view.text() == text
            })
            .unwrap()
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

    fn semantic_artifact(label: &[u8]) -> SemanticArtifactId {
        SemanticArtifactId::from_parts(&[label]).unwrap()
    }

    #[test]
    fn exact_resolution_projects_all_dependency_levels_and_api_use() {
        let projection = dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            false,
        );
        let document = projection.document();
        assert_eq!(
            document.coverage().status(),
            FactCoverage::Complete,
            "{:#?}",
            document.gaps()
        );
        assert!(document.gaps().is_empty());
        assert_eq!(document.nodes().len(), 9);
        assert_eq!(document.edges().len(), 11);
        for (kind, expected) in [
            (DependencyEdgeKind::PackageContainsTarget, 2),
            (DependencyEdgeKind::TargetContainsModule, 2),
            (DependencyEdgeKind::ModuleContainsFile, 2),
            (DependencyEdgeKind::FileDependency, 1),
            (DependencyEdgeKind::ModuleDependency, 1),
            (DependencyEdgeKind::PackageDependency, 1),
            (DependencyEdgeKind::BuildTargetDependency, 1),
            (DependencyEdgeKind::ApiUse, 1),
        ] {
            assert_eq!(
                document
                    .edges()
                    .iter()
                    .filter(|edge| edge.kind() == kind)
                    .count(),
                expected,
                "unexpected {kind:?} edge count"
            );
        }
        let package_edge = document
            .edges()
            .iter()
            .find(|edge| edge.kind() == DependencyEdgeKind::PackageDependency)
            .unwrap();
        assert!(matches!(
            document
                .nodes()
                .iter()
                .find(|node| node.key() == package_edge.from())
                .unwrap()
                .kind(),
            DependencyNodeKind::Package { package_id } if package_id == "app-package"
        ));
        assert!(matches!(
            document
                .nodes()
                .iter()
                .find(|node| node.key() == package_edge.to())
                .unwrap()
                .kind(),
            DependencyNodeKind::Package { package_id } if package_id == "dep-package"
        ));
        for edge in document.edges().iter().filter(|edge| {
            matches!(
                edge.kind(),
                DependencyEdgeKind::FileDependency
                    | DependencyEdgeKind::ModuleDependency
                    | DependencyEdgeKind::PackageDependency
                    | DependencyEdgeKind::BuildTargetDependency
                    | DependencyEdgeKind::ApiUse
            )
        }) {
            assert!(edge.evidence().iter().all(|evidence| {
                evidence.resolution.is_some()
                    && evidence.authority.is_some()
                    && evidence.coverage == FactCoverage::Complete
            }));
        }
        assert!(document.nodes().iter().any(|node| matches!(
            node.kind(),
            DependencyNodeKind::LocalApi { name, exports, .. }
                if name == "imported" && exports.len() == 1
        )));
    }

    #[test]
    fn projection_is_deterministic_round_trips_and_rejects_tampering() {
        let first = dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            false,
        );
        let policy = first.policy().clone();
        let second = derive_dependencies(Arc::clone(first.resolution()), policy).unwrap();
        assert_eq!(first.id(), second.id());
        let bytes = serde_json::to_vec(first.document()).unwrap();
        assert_eq!(bytes, serde_json::to_vec(second.document()).unwrap());
        let decoded: DependencyDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());

        let mut tampered = serde_json::to_value(first.document()).unwrap();
        tampered["nodes"][0]["key"] = serde_json::Value::String(format!("dpn1_{}", "0".repeat(64)));
        assert!(serde_json::from_value::<DependencyDocument>(tampered).is_err());
    }

    #[test]
    fn incomplete_export_authority_downgrades_and_withholds_dependencies() {
        let projection = dependency_fixture(
            FactCoverageEvidence::partial("dependency fixture exports are incomplete").unwrap(),
            FixtureEndpoint::AdapterOnly,
            false,
        );
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Partial);
        assert!(!document.coverage().reasons().is_empty());
        assert!(document.gaps().iter().any(|gap| matches!(
            gap.kind(),
            DependencyGapKind::IncompleteExports { status, reasons, .. }
                if *status == FactCoverage::Partial
                    && reasons == &["dependency fixture exports are incomplete"]
        )));
        assert!(document.gaps().iter().any(|gap| matches!(
            gap.kind(),
            DependencyGapKind::IncompleteResolution { status, coverage, .. }
                if *status == ResolutionStatus::Unknown && *coverage == FactCoverage::Partial
        )));
        assert!(!document.edges().iter().any(|edge| matches!(
            edge.kind(),
            DependencyEdgeKind::FileDependency
                | DependencyEdgeKind::ModuleDependency
                | DependencyEdgeKind::PackageDependency
                | DependencyEdgeKind::BuildTargetDependency
                | DependencyEdgeKind::ApiUse
        )));
    }

    #[test]
    fn duplicate_ownership_withholds_module_and_parent_dependencies() {
        let projection = dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            true,
        );
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Partial);
        assert!(document.gaps().iter().any(|gap| matches!(
            gap.kind(),
            DependencyGapKind::DuplicateFileOwnership { path, modules }
                if path == Path::new("provider.resolutionrs") && modules.len() == 2
        )));
        assert!(
            document
                .edges()
                .iter()
                .any(|edge| edge.kind() == DependencyEdgeKind::FileDependency)
        );
        assert!(!document.edges().iter().any(|edge| matches!(
            edge.kind(),
            DependencyEdgeKind::ModuleDependency
                | DependencyEdgeKind::PackageDependency
                | DependencyEdgeKind::BuildTargetDependency
        )));
    }

    #[test]
    fn same_file_api_use_does_not_create_same_level_self_dependencies() {
        let projection = dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ConsumerDeclaration,
            false,
        );
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Complete);
        assert_eq!(
            document
                .edges()
                .iter()
                .filter(|edge| edge.kind() == DependencyEdgeKind::ApiUse)
                .count(),
            1
        );
        assert!(!document.edges().iter().any(|edge| matches!(
            edge.kind(),
            DependencyEdgeKind::FileDependency
                | DependencyEdgeKind::ModuleDependency
                | DependencyEdgeKind::PackageDependency
                | DependencyEdgeKind::BuildTargetDependency
        )));
    }

    #[test]
    fn external_api_use_never_invents_build_identity() {
        let projection = dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::External,
            false,
        );
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Complete);
        assert!(document.nodes().iter().any(|node| matches!(
            node.kind(),
            DependencyNodeKind::ExternalApi { provider }
                if provider == "std::fmt::Display"
        )));
        assert_eq!(
            document
                .edges()
                .iter()
                .filter(|edge| edge.kind() == DependencyEdgeKind::ApiUse)
                .count(),
            1
        );
        assert!(!document.edges().iter().any(|edge| matches!(
            edge.kind(),
            DependencyEdgeKind::FileDependency
                | DependencyEdgeKind::ModuleDependency
                | DependencyEdgeKind::PackageDependency
                | DependencyEdgeKind::BuildTargetDependency
        )));
    }
}
