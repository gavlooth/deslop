use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use deslop_parse::{
    BuildContextId, CanonicalRoleSet, ControlFlowPolicyId, ControlRegionPolicyId, DataFlowBuilder,
    DataFlowEffectDraft, DataFlowGraphDraft, DataFlowPolicyId, DiscoveryPolicy,
    FactCoverageEvidence, NameNamespace, NamespacePolicy, NonStructuredControlPolicyId,
    ProgramDependencePolicyId, ProgramDependenceProjection, ProjectAnalysis,
    ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec, ResolutionPolicyId,
    ResolutionProjection, RootSpec, ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind,
    ScopeSpec, derive_control_regions, derive_non_structured_control_regions,
    derive_program_dependence, lower_control_flow,
};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::{
    TransformationCandidate, detect_equivalent_branch_fragments,
    detect_unreachable_literal_statements,
};

const SCOPE_LIMITATION: &str =
    "production Rust scope authority is unavailable to this control-only recipe";
pub const RECIPE_DETECTION_REPORT_SCHEMA: &str = "deslop.recipe-detection/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeAbstention {
    pub path: PathBuf,
    pub stage: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDetectionReport {
    pub schema: String,
    pub selected_rust_files: usize,
    pub analyzed_rust_files: usize,
    pub candidates: Vec<TransformationCandidate>,
    pub abstentions: Vec<RecipeAbstention>,
}

/// Detect the production Rust recipe set from a canonical, retained graph projection.
///
/// Only Rust source files selected by `paths` enter the projection. This prevents an
/// unsupported language elsewhere in a repository from changing target eligibility.
pub fn detect_rust_recipes(root: &Path, paths: &[PathBuf]) -> Result<Vec<TransformationCandidate>> {
    let report = detect_rust_recipe_report(root, paths)?;
    if !report.abstentions.is_empty() {
        let details = report
            .abstentions
            .iter()
            .map(|abstention| format!("{}: {}", abstention.path.display(), abstention.reason))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("recipe detection abstained: {details}");
    }
    Ok(report.candidates)
}

pub fn detect_rust_recipe_report(root: &Path, paths: &[PathBuf]) -> Result<RecipeDetectionReport> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve recipe root {}", root.display()))?;
    let logical_files = collect_rust_files(&root, paths)?;
    let selected_rust_files = logical_files.len();
    let (mut candidates, analyzed_rust_files, mut abstentions) =
        match build_rust_recipe_projection(&root, &logical_files) {
            Ok(Some(projection)) => match detect_projection_recipes(&projection) {
                Ok(provisional) => {
                    // Candidate wires must remain reconstructible from their exact target file.
                    // The combined projection is only the cheap discovery pass; files with an
                    // opportunity are rebuilt alone before any authority crosses the boundary.
                    let candidate_paths = provisional
                        .iter()
                        .map(|candidate| candidate.target().node.file().path.clone())
                        .collect::<BTreeSet<_>>();
                    let mut stable = Vec::new();
                    let mut local_abstentions = Vec::new();
                    for path in candidate_paths {
                        match detect_one_file(&root, &path) {
                            Ok(detected) => stable.extend(detected),
                            Err(error) => local_abstentions.push(RecipeAbstention {
                                path,
                                stage: "candidate-rebuild".to_string(),
                                reason: format!("{error:#}"),
                            }),
                        }
                    }
                    (stable, selected_rust_files, local_abstentions)
                }
                Err(error) => {
                    fallback_file_detection(&root, &logical_files, Some(error.to_string()))
                }
            },
            Ok(None) => (Vec::new(), selected_rust_files, Vec::new()),
            Err(error) => {
                fallback_file_detection(&root, &logical_files, Some(format!("{error:#}")))
            }
        };
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    abstentions.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(RecipeDetectionReport {
        schema: RECIPE_DETECTION_REPORT_SCHEMA.to_string(),
        selected_rust_files,
        analyzed_rust_files,
        candidates,
        abstentions,
    })
}

fn fallback_file_detection(
    root: &Path,
    logical_files: &[PathBuf],
    combined_reason: Option<String>,
) -> (Vec<TransformationCandidate>, usize, Vec<RecipeAbstention>) {
    let mut candidates = Vec::new();
    let mut abstentions = Vec::new();
    let mut analyzed = 0;
    for logical in logical_files {
        match detect_one_file(root, logical) {
            Ok(detected) => {
                analyzed += 1;
                candidates.extend(detected);
            }
            Err(error) => abstentions.push(RecipeAbstention {
                path: logical.clone(),
                stage: "isolated-fallback".to_string(),
                reason: match &combined_reason {
                    Some(combined) => format!("combined projection failed ({combined}); isolated projection failed: {error:#}"),
                    None => format!("{error:#}"),
                },
            }),
        }
    }
    (candidates, analyzed, abstentions)
}

fn detect_one_file(root: &Path, logical: &PathBuf) -> Result<Vec<TransformationCandidate>> {
    let Some(projection) = build_rust_recipe_projection(root, std::slice::from_ref(logical))?
    else {
        return Ok(Vec::new());
    };
    detect_projection_recipes(&projection)
}

fn detect_projection_recipes(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>> {
    let mut candidates = detect_unreachable_literal_statements(projection)?;
    candidates.extend(detect_equivalent_branch_fragments(projection)?);
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

/// Build the retained production graph used by recipe detection.
///
/// `None` means the selected Rust sources contain no executable owner and therefore
/// cannot contain a candidate for the current recipe.
pub fn build_rust_recipe_projection(
    root: &Path,
    paths: &[PathBuf],
) -> Result<Option<Arc<ProgramDependenceProjection>>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve recipe root {}", root.display()))?;
    let logical_files = collect_rust_files(&root, paths)?;
    if logical_files.is_empty() {
        return Ok(None);
    }

    let planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base: root.clone(),
        root: RootSpec::Explicit(root),
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::ExactLogicalFiles(logical_files),
        discovery: DiscoveryPolicy::Canonical,
    })?;
    let analysis = ProjectAnalysis::build(planner.build()?.snapshot)?;
    let lowered = lower_control_flow(
        Arc::clone(&analysis),
        ControlFlowPolicyId::from_parts(&[b"deslop-rust-recipe-cfg/1"])?,
    )?;
    let Some(flow_projection) = lowered.projection() else {
        return Ok(None);
    };
    let flow = Arc::new(flow_projection.clone());
    let regions = Arc::new(derive_control_regions(
        Arc::clone(&flow),
        ControlRegionPolicyId::from_parts(&[b"deslop-rust-recipe-regions/1"])?,
    )?);

    let incomplete = FactCoverageEvidence::partial(SCOPE_LIMITATION)?;
    let namespace = NamespacePolicy::new(vec![NameNamespace::Value], vec![])?;
    let mut scopes = ScopeGraphBuilder::new(
        Arc::clone(&analysis),
        BuildContextId::from_parts(&[b"deslop-rust-recipe-build/1"])?,
        ScopeFactPolicyId::from_parts(&[b"deslop-rust-recipe-scope/1"])?,
    )?;
    let mut file_scopes = BTreeMap::new();
    for file in analysis.files() {
        let root_node = analysis
            .file_node_ids(&file.key().path)
            .and_then(|nodes| {
                nodes.into_iter().find(|node| {
                    analysis
                        .node(*node)
                        .is_ok_and(|view| view.raw_grammar_kind() == "source_file")
                })
            })
            .with_context(|| {
                format!("missing Rust source root for {}", file.key().path.display())
            })?;
        let scope = scopes.add_scope(
            root_node,
            roles(&analysis, root_node)?,
            incomplete.clone(),
            ScopeDraft {
                kind: ScopeKind::File,
                parent: None,
                namespace_policy: namespace.clone(),
            },
        )?;
        file_scopes.insert(file.key().path.clone(), scope);
    }
    for graph in flow.document().graphs() {
        let owner = analysis.node_by_key(graph.owner())?.id();
        let parent = file_scopes
            .get(&graph.owner().file().path)
            .copied()
            .with_context(|| {
                format!(
                    "missing file scope for executable owner {}",
                    graph.owner().file().path.display()
                )
            })?;
        scopes.add_scope(
            owner,
            roles(&analysis, owner)?,
            incomplete.clone(),
            ScopeDraft {
                kind: ScopeKind::Callable,
                parent: Some(parent),
                namespace_policy: namespace.clone(),
            },
        )?;
    }

    let resolution = Arc::new(ResolutionProjection::build(
        Arc::new(scopes.build()?),
        ResolutionPolicyId::from_parts(&[b"deslop-rust-recipe-resolution/1"])?,
    )?);
    let mut data_flow = DataFlowBuilder::new(
        Arc::clone(&regions),
        resolution,
        DataFlowPolicyId::from_parts(&[b"deslop-rust-recipe-data-flow/1"])?,
    )?;
    for graph in flow.document().graphs() {
        data_flow.add_graph(DataFlowGraphDraft {
            control_flow_graph: graph.key().clone(),
            definitions: vec![],
            accesses: vec![],
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
        })?;
    }
    let data_flow = Arc::new(data_flow.build()?);
    let non_structured = Arc::new(derive_non_structured_control_regions(
        regions,
        NonStructuredControlPolicyId::from_parts(&[b"deslop-rust-recipe-non-structured/1"])?,
    )?);
    Ok(Some(Arc::new(derive_program_dependence(
        data_flow,
        non_structured,
        ProgramDependencePolicyId::from_parts(&[b"deslop-rust-recipe-pdg/1"])?,
    )?)))
}

fn collect_rust_files(root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let selected = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let mut files = BTreeSet::new();
    for selected_path in selected {
        reject_noncanonical_input(&selected_path)?;
        let physical = if selected_path.is_absolute() {
            selected_path
        } else {
            root.join(selected_path)
        };
        let physical = physical
            .canonicalize()
            .with_context(|| format!("failed to resolve recipe path {}", physical.display()))?;
        if !physical.starts_with(root) {
            bail!(
                "recipe path {} is outside root {}",
                physical.display(),
                root.display()
            );
        }
        if physical.is_file() {
            add_rust_file(root, &physical, &mut files)?;
            continue;
        }
        if !physical.is_dir() {
            bail!(
                "recipe path {} is neither a file nor directory",
                physical.display()
            );
        }
        for entry in WalkBuilder::new(&physical).standard_filters(true).build() {
            let entry = entry.with_context(|| {
                format!(
                    "failed while discovering Rust sources below {}",
                    physical.display()
                )
            })?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                add_rust_file(root, entry.path(), &mut files)?;
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn add_rust_file(root: &Path, path: &Path, files: &mut BTreeSet<PathBuf>) -> Result<()> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return Ok(());
    }
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to resolve Rust source {}", path.display()))?;
    let logical = canonical.strip_prefix(root).with_context(|| {
        format!(
            "Rust source {} is outside root {}",
            canonical.display(),
            root.display()
        )
    })?;
    if logical.as_os_str().is_empty() {
        bail!("Rust source path cannot equal the recipe root");
    }
    files.insert(logical.to_path_buf());
    Ok(())
}

fn reject_noncanonical_input(path: &Path) -> Result<()> {
    if path.components().any(|component| {
        matches!(component, Component::ParentDir | Component::CurDir) && path != Path::new(".")
    }) {
        bail!("recipe path {} is not canonical", path.display());
    }
    Ok(())
}

fn roles(analysis: &Arc<ProjectAnalysis>, node: deslop_parse::NodeId) -> Result<CanonicalRoleSet> {
    let path = analysis.node(node)?.path().to_path_buf();
    analysis
        .canonical_role_projection(&path)?
        .facts()
        .iter()
        .find(|fact| fact.node() == node)
        .map(|fact| fact.roles())
        .context("canonical role projection omitted a scope node")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn production_builder_detects_only_selected_rust_sources() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("candidate.rs"),
            "fn run() { return; 1; }\n",
        )
        .unwrap();
        fs::write(
            root.path().join("peer.py"),
            "def run():\n    return\n    1\n",
        )
        .unwrap();

        let candidates =
            detect_rust_recipes(root.path(), &[PathBuf::from("candidate.rs")]).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].target().node.file().path,
            PathBuf::from("candidate.rs")
        );
    }

    #[test]
    fn production_builder_rejects_noncanonical_and_foreign_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("candidate.rs"),
            "fn run() { return; 1; }\n",
        )
        .unwrap();
        assert!(detect_rust_recipes(root.path(), &[PathBuf::from("src/../candidate.rs")]).is_err());

        let foreign = tempfile::tempdir().unwrap();
        fs::write(foreign.path().join("foreign.rs"), "fn run() {}\n").unwrap();
        assert!(detect_rust_recipes(root.path(), &[foreign.path().join("foreign.rs")]).is_err());
    }

    #[test]
    fn unrelated_nonterminating_rust_graph_cannot_contaminate_target_authority() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("candidate.rs"),
            "fn run() { return; 1; }\n",
        )
        .unwrap();
        fs::write(root.path().join("peer.rs"), "fn peer() { loop {} }\n").unwrap();

        let isolated = detect_rust_recipes(root.path(), &[PathBuf::from("candidate.rs")]).unwrap();
        let combined = detect_rust_recipes(
            root.path(),
            &[PathBuf::from("candidate.rs"), PathBuf::from("peer.rs")],
        )
        .unwrap();
        assert_eq!(combined, isolated);
        assert_eq!(combined.len(), 1);
    }
}
