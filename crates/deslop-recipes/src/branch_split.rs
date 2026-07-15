use std::collections::{BTreeMap, BTreeSet, VecDeque};

use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    AdapterCapability, CapabilitySupport, ControlPointKind, ControlSyntheticPointKind,
    GraphEvidenceLayer, ProgramDependenceEdgeKind, ProgramDependenceGraph, ProgramDependenceNode,
    ProgramDependenceNodeKey, ProgramDependenceProjection, ProjectAnalysis,
    evaluate_program_graph_recipe_eligibility,
};

use crate::branch::{
    capability_result, condition, exact_branch_edges, fixture, flow_entity, graph_entity,
    graph_root, result, span,
};
use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactDirection, ImpactQueryError, ProofState, RecipeContractError, RecipeFixtureRole,
    RollbackPlan, RollbackStrategy, TransformationCandidate, TransformationCandidateDraft,
    TransformationEdit, TransformationFamily, TransformationRecipe, TransformationRecipeDraft,
    ValidationPlan, ValidationStep, ValidationStepKind, program_dependence_impact_cone,
};

const REQUIRED_ACTIONS: &str = "direct-action-statements";
const REQUIRED_SLICES: &str = "distinct-dependence-slices";
const REQUIRED_ORDER: &str = "predicate-count-and-action-order-retained";
const REQUIRED_EFFECTS: &str = "effect-and-drop-independence";
const FORBIDDEN_CROSSING: &str = "crossing-flow-dependence";
const FORBIDDEN_SCOPE: &str = "binding-scope-or-drop-change";
const FORBIDDEN_CONTROL: &str = "recovered-or-conservative-control";
const FORBIDDEN_NON_STRUCTURED: &str = "non-structured-control";
const TEMP_NAME: &str = "__deslop_m57_condition";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDependenceSlice {
    pub action: deslop_parse::NodeKey,
    pub entities: Vec<GraphEntityRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSplitDependenceEvidence {
    pub slices: Vec<ActionDependenceSlice>,
    pub crossing_edges: Vec<GraphEntityRef>,
    pub independence: ProofState,
}

#[derive(Debug, thiserror::Error)]
pub enum BranchSplitRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("branch-split graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("branch-split recipe received an inconsistent projection: {0}")]
    Projection(String),
}

pub fn independent_branch_split_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-split-independent-branch-actions".into(),
        version: "1.0.0".into(),
        family: TransformationFamily::BranchControl,
        required_layers: vec![
            GraphEvidenceLayer::ControlFlow,
            GraphEvidenceLayer::ControlRegions,
            GraphEvidenceLayer::NonStructuredControl,
            GraphEvidenceLayer::DataFlow,
            GraphEvidenceLayer::ProgramDependence,
        ],
        required_conditions: vec![
            condition(
                REQUIRED_ACTIONS,
                "The branch contains only two or more direct call statements.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                REQUIRED_SLICES,
                "Each action belongs to a distinct complete flow-dependence slice.",
                GraphEvidenceLayer::ProgramDependence,
            ),
            condition(
                REQUIRED_ORDER,
                "Predicate count and action source order remain exact.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                REQUIRED_EFFECTS,
                "Complete effect evidence proves action scopes and drop order independent.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_CROSSING,
                "A flow edge or transitive flow slice crosses two actions.",
                GraphEvidenceLayer::ProgramDependence,
            ),
            condition(
                FORBIDDEN_SCOPE,
                "Splitting changes a binding, borrow, lifetime, or drop order.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                FORBIDDEN_CONTROL,
                "Recovered syntax or conservative branch control participates.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_NON_STRUCTURED,
                "Non-structured control participates in the callable.",
                GraphEvidenceLayer::NonStructuredControl,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: ValidationPlan {
            steps: vec![
                validation(
                    "build",
                    ValidationStepKind::Build,
                    "Build after the guarded branch split.",
                ),
                validation(
                    "graph-delta",
                    ValidationStepKind::GraphDelta,
                    "Rebuild and compare dispatch and slice changes.",
                ),
                validation(
                    "parse",
                    ValidationStepKind::Parse,
                    "Parse the exact replacement.",
                ),
                validation(
                    "test",
                    ValidationStepKind::Test,
                    "Run project tests before acceptance.",
                ),
            ],
        },
        rollback_plan: RollbackPlan {
            strategy: RollbackStrategy::ReverseExactEdits,
            require_revision_guards: true,
            validation_steps: vec![
                "build".into(),
                "graph-delta".into(),
                "parse".into(),
                "test".into(),
            ],
        },
        fixtures: vec![
            fixture(
                RecipeFixtureRole::Positive,
                "two-independent-actions",
                FixtureExpectation::Candidate,
                "Two actions have disjoint retained flow slices.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "single-action",
                FixtureExpectation::NoCandidate,
                "One action has nothing to split.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "unknown-production-pdg",
                FixtureExpectation::ReviewRequired,
                "No crossing is retained but production LocalPdg is unavailable.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "binding-between-actions",
                FixtureExpectation::NoCandidate,
                "A declaration or crossing flow prevents splitting.",
            ),
        ],
    })
}

pub fn detect_independent_branch_splits(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, BranchSplitRecipeError> {
    let recipe = independent_branch_split_recipe()?;
    let data_flow = projection.data_flow();
    let regions = data_flow.control_regions();
    let flow = regions.control_flow();
    let analysis = flow.analysis();
    let non_structured = projection.non_structured_control();
    let mut candidates = Vec::new();

    for graph in projection.document().graphs() {
        let eligibility = evaluate_program_graph_recipe_eligibility(
            projection,
            graph,
            &recipe.eligibility_requirement(),
        )
        .map_err(|error| BranchSplitRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = flow
            .document()
            .graphs()
            .iter()
            .find(|item| item.key() == graph.control_flow_graph())
            .ok_or_else(|| missing("control-flow graph", graph.control_flow_graph().as_str()))?;
        let data_graph = data_flow
            .document()
            .graphs()
            .iter()
            .find(|item| item.key() == graph.data_flow_graph())
            .ok_or_else(|| missing("data-flow graph", graph.data_flow_graph().as_str()))?;
        let non_structured_graph = non_structured
            .document()
            .graphs()
            .iter()
            .find(|item| item.key() == graph.non_structured_control_graph())
            .ok_or_else(|| {
                missing(
                    "non-structured graph",
                    graph.non_structured_control_graph().as_str(),
                )
            })?;

        for dispatch in flow_graph.points().iter().filter(|point| {
            point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch)
                && !point.recovered()
        }) {
            let Some(source) = dispatch.source() else {
                continue;
            };
            if !exact_branch_edges(flow_graph, dispatch.key()) {
                continue;
            }
            let branch = analysis
                .node_by_key(source)
                .map_err(|error| BranchSplitRecipeError::Projection(error.to_string()))?;
            let Some((predicate, actions)) = split_shape(analysis, branch)? else {
                continue;
            };
            let Some(root) = graph
                .nodes()
                .iter()
                .find(|node| node.point() == dispatch.key())
            else {
                continue;
            };
            let Some(evidence) = dependence_evidence(graph, &actions, data_graph)? else {
                continue;
            };
            if !evidence.crossing_edges.is_empty() {
                continue;
            }
            let independence = evidence.independence;
            let mut slice_evidence =
                evidence
                    .slices
                    .iter()
                    .flat_map(|slice| {
                        slice.entities.iter().cloned().map(|entity| {
                            plain_evidence(entity, "Retained action flow-slice entity.")
                        })
                    })
                    .collect::<Vec<_>>();
            if slice_evidence.is_empty() {
                continue;
            }
            slice_evidence[0].detail = if independence == ProofState::Proven {
                "Complete DefUse and LocalPdg authority prove disjoint action slices.".into()
            } else {
                "No crossing is retained, but unavailable DefUse/LocalPdg authority makes absence Unknown.".into()
            };
            let target_span = span(source);
            let required_results = vec![
                result(
                    REQUIRED_ACTIONS,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                    "Exact Rust CST contains only direct call statements.",
                ),
                multi_result(REQUIRED_SLICES, independence, slice_evidence),
                result(
                    REQUIRED_ORDER,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                    "The predicate is stored once and action source order is retained.",
                ),
                capability_result(
                    REQUIRED_EFFECTS,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production Effects cannot prove scope, temporary, or drop independence.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                ),
            ];
            let forbidden_results = vec![
                multi_result(
                    FORBIDDEN_CROSSING,
                    if independence == ProofState::Proven {
                        ProofState::Disproven
                    } else {
                        ProofState::Unknown
                    },
                    evidence
                        .slices
                        .iter()
                        .flat_map(|slice| slice.entities.iter().cloned())
                        .map(|entity| {
                            plain_evidence(
                                entity,
                                "No retained crossing reaches another action slice.",
                            )
                        })
                        .collect(),
                ),
                capability_result(
                    FORBIDDEN_SCOPE,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Missing DefUse/Effects cannot disprove a hidden scope or drop change.",
                    AdapterCapability::DefUse,
                    data_graph.coverage().def_use_support(),
                    data_graph.coverage().def_use_authority(),
                ),
                result(
                    FORBIDDEN_CONTROL,
                    ProofState::Disproven,
                    flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                    "Dispatch and outgoing edges are exact and unrecovered.",
                ),
                result(
                    FORBIDDEN_NON_STRUCTURED,
                    if non_structured_graph.facts().is_empty() {
                        ProofState::Disproven
                    } else {
                        ProofState::Unknown
                    },
                    graph_entity(
                        GraphEvidenceLayer::NonStructuredControl,
                        non_structured_graph.key().as_str(),
                        non_structured_graph.key().as_str(),
                    ),
                    if non_structured_graph.facts().is_empty() {
                        "No non-structured fact participates."
                    } else {
                        "Non-structured facts require review."
                    },
                ),
            ];
            candidates.push(TransformationCandidate::new(
                TransformationCandidateDraft {
                    recipe: recipe.clone(),
                    source: CandidateSource {
                        project_snapshot: analysis.snapshot().id().as_str().into(),
                        analysis: analysis.id().as_str().into(),
                        program_dependence_projection: projection.id().as_str().into(),
                    },
                    target: CandidateTarget {
                        entity: graph_root(graph, root),
                        node: source.clone(),
                        span: target_span,
                    },
                    eligibility: eligibility.clone(),
                    required_results,
                    forbidden_results,
                    impact: program_dependence_impact_cone(
                        projection,
                        graph.key(),
                        root.key(),
                        ImpactDirection::Bidirectional,
                        8,
                    )?,
                    expected_delta: split_delta(graph, root, &evidence),
                    edits: vec![TransformationEdit::exact_node_replacement(
                        source.clone(),
                        target_span,
                        branch.text().into(),
                        render_split(predicate.text(), &actions),
                    )],
                    safety: SafetyClass::SafeWithPrecondition,
                    disposition: CandidateDisposition::ReviewRequired,
                    validation_plan: recipe.validation_plan().clone(),
                    rollback_plan: recipe.rollback_plan().clone(),
                },
            )?);
        }
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

fn split_shape<'a>(
    analysis: &'a ProjectAnalysis,
    branch: deslop_parse::NodeView<'a>,
) -> Result<
    Option<(deslop_parse::NodeView<'a>, Vec<deslop_parse::NodeView<'a>>)>,
    BranchSplitRecipeError,
> {
    if branch.grammar().lang() != Lang::Rust
        || branch.raw_grammar_kind() != "if_expression"
        || branch.has_error()
        || branch.text().contains("//")
        || branch.text().contains("/*")
        || branch.text().contains(TEMP_NAME)
        || child_by_field(analysis, branch, "alternative")?.is_some()
    {
        return Ok(None);
    }
    let Some(predicate) = child_by_field(analysis, branch, "condition")? else {
        return Ok(None);
    };
    if contains_let_condition(analysis, predicate)? {
        return Ok(None);
    }
    let Some(block) = child_by_field(analysis, branch, "consequence")? else {
        return Ok(None);
    };
    let all = named_children(analysis, block)?;
    if all.len() < 2 || all.len() > 8 {
        return Ok(None);
    }
    for action in &all {
        if !direct_call_statement(analysis, *action)? {
            return Ok(None);
        }
    }
    Ok(Some((predicate, all)))
}

fn contains_let_condition(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, BranchSplitRecipeError> {
    if matches!(node.raw_grammar_kind(), "let_condition" | "let_chain") {
        return Ok(true);
    }
    Ok(analysis
        .descendant_node_ids(node.id())
        .map_err(|error| BranchSplitRecipeError::Projection(error.to_string()))?
        .any(|id| {
            analysis
                .node(id)
                .is_ok_and(|item| matches!(item.raw_grammar_kind(), "let_condition" | "let_chain"))
        }))
}

fn direct_call_statement(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, BranchSplitRecipeError> {
    if node.raw_grammar_kind() != "expression_statement" || node.has_error() {
        return Ok(false);
    }
    let children = named_children(analysis, node)?;
    Ok(children.len() == 1 && children[0].raw_grammar_kind() == "call_expression")
}

fn dependence_evidence(
    graph: &ProgramDependenceGraph,
    actions: &[deslop_parse::NodeView<'_>],
    data: &deslop_parse::DataFlowGraph,
) -> Result<Option<BranchSplitDependenceEvidence>, BranchSplitRecipeError> {
    let mut roots = Vec::new();
    for action in actions {
        let anchor = action.key().anchor();
        let nodes = graph
            .nodes()
            .iter()
            .filter(|node| {
                node.source().is_some_and(|source| {
                    source.anchor().start_byte() >= anchor.start_byte()
                        && source.anchor().end_byte() <= anchor.end_byte()
                })
            })
            .map(|node| node.key().clone())
            .collect::<BTreeSet<_>>();
        if nodes.is_empty() {
            return Ok(None);
        }
        roots.push(nodes);
    }
    let slices = flow_closures(graph, &roots);
    let crossings = slice_crossings(graph, &slices);
    let authoritative = data.coverage().def_use_support() == CapabilitySupport::Provided
        && graph.coverage().local_pdg_support() == CapabilitySupport::Provided
        && data.coverage().def_use_authority().is_some()
        && graph.coverage().local_pdg_authority().is_some();
    Ok(Some(BranchSplitDependenceEvidence {
        slices: actions
            .iter()
            .zip(&slices)
            .map(|(action, nodes)| ActionDependenceSlice {
                action: action.key().clone(),
                entities: nodes
                    .iter()
                    .map(|node| {
                        graph_entity(
                            GraphEvidenceLayer::ProgramDependence,
                            graph.key().as_str(),
                            node.as_str(),
                        )
                    })
                    .collect(),
            })
            .collect(),
        crossing_edges: crossings
            .iter()
            .map(|edge| {
                graph_entity(
                    GraphEvidenceLayer::ProgramDependence,
                    graph.key().as_str(),
                    edge,
                )
            })
            .collect(),
        independence: if authoritative {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
    }))
}

fn flow_closures(
    graph: &ProgramDependenceGraph,
    roots: &[BTreeSet<ProgramDependenceNodeKey>],
) -> Vec<BTreeSet<ProgramDependenceNodeKey>> {
    roots
        .iter()
        .map(|roots| {
            let mut seen = roots.clone();
            let mut queue = VecDeque::from_iter(roots.iter().cloned());
            while let Some(current) = queue.pop_front() {
                for next in graph.edges().iter().filter_map(|edge| match edge.kind() {
                    ProgramDependenceEdgeKind::Flow { .. } if edge.from() == &current => {
                        Some(edge.to())
                    }
                    ProgramDependenceEdgeKind::Flow { .. } if edge.to() == &current => {
                        Some(edge.from())
                    }
                    _ => None,
                }) {
                    if seen.insert(next.clone()) {
                        queue.push_back(next.clone());
                    }
                }
            }
            seen
        })
        .collect()
}

fn slice_crossings(
    graph: &ProgramDependenceGraph,
    slices: &[BTreeSet<ProgramDependenceNodeKey>],
) -> Vec<String> {
    let mut owner = BTreeMap::new();
    let mut crossings = BTreeSet::new();
    for (index, slice) in slices.iter().enumerate() {
        for node in slice {
            if owner
                .insert(node, index)
                .is_some_and(|prior| prior != index)
            {
                crossings.insert(format!("slice-overlap:{}", node.as_str()));
            }
        }
    }
    for edge in graph
        .edges()
        .iter()
        .filter(|edge| matches!(edge.kind(), ProgramDependenceEdgeKind::Flow { .. }))
    {
        if let (Some(left), Some(right)) = (owner.get(edge.from()), owner.get(edge.to()))
            && left != right
        {
            crossings.insert(edge.key().as_str().into());
        }
    }
    crossings.into_iter().collect()
}

fn render_split(predicate: &str, actions: &[deslop_parse::NodeView<'_>]) -> String {
    let branches = actions
        .iter()
        .map(|action| format!("if {TEMP_NAME} {{ {} }}", action.text()))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{{ let {TEMP_NAME} = {predicate}; {branches} }}")
}

fn split_delta(
    graph: &ProgramDependenceGraph,
    root: &ProgramDependenceNode,
    evidence: &BranchSplitDependenceEvidence,
) -> ExpectedGraphDelta {
    let mut changes = vec![ExpectedGraphChange {
        kind: GraphChangeKind::Remove,
        entity: graph_root(graph, root),
        rationale: "The compound dispatch becomes ordered per-action dispatches.".into(),
    }];
    for entity in evidence.slices.iter().flat_map(|slice| &slice.entities) {
        changes.push(ExpectedGraphChange {
            kind: GraphChangeKind::Preserve,
            entity: entity.clone(),
            rationale: "The action dependence entity must remain after splitting.".into(),
        });
    }
    ExpectedGraphDelta { changes }
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, BranchSplitRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| BranchSplitRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}
fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, BranchSplitRecipeError> {
    node.children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| BranchSplitRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.into_iter().filter(|item| item.is_named()).collect())
}
fn validation(key: &str, kind: ValidationStepKind, description: &str) -> ValidationStep {
    ValidationStep {
        key: key.into(),
        kind,
        description: description.into(),
        command: None,
        required: true,
    }
}
fn plain_evidence(entity: GraphEntityRef, detail: &str) -> ConditionEvidence {
    ConditionEvidence {
        entity,
        detail: detail.into(),
        capability: None,
        support: None,
        authority: None,
    }
}
fn multi_result(
    condition: &str,
    state: ProofState,
    evidence: Vec<ConditionEvidence>,
) -> ConditionResult {
    ConditionResult {
        condition: condition.into(),
        state,
        evidence,
    }
}
fn missing(kind: &str, identity: &str) -> BranchSplitRecipeError {
    BranchSplitRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{branch_graph_evidence, build_rust_recipe_projection, detect_rust_recipes};
    use std::{fs, path::PathBuf};

    const SOURCE: &str = "fn a() {}\nfn b() {}\nfn run(flag: bool) { if flag { a(); b(); } }\n";
    fn candidates(root: &std::path::Path) -> Vec<TransformationCandidate> {
        detect_rust_recipes(root, &[PathBuf::from("split.rs")])
            .unwrap()
            .into_iter()
            .filter(|candidate| {
                candidate.recipe().name() == "rust-split-independent-branch-actions"
            })
            .collect()
    }

    #[test]
    fn candidate_retains_unknown_production_independence() {
        assert_eq!(
            independent_branch_split_recipe().unwrap().fixtures().len(),
            4
        );
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("split.rs"), SOURCE).unwrap();
        let found = candidates(root.path());
        assert_eq!(found.len(), 1);
        let candidate = &found[0];
        assert_eq!(
            candidate.disposition(),
            CandidateDisposition::ReviewRequired
        );
        assert!(
            candidate
                .required_results()
                .iter()
                .any(|item| item.condition == REQUIRED_SLICES && item.state == ProofState::Unknown)
        );
        assert!(
            candidate.edits()[0]
                .after
                .contains("let __deslop_m57_condition = flag")
        );
        assert!(
            branch_graph_evidence(candidate)
                .unwrap()
                .after
                .changes
                .len()
                >= 3
        );
    }

    #[test]
    fn replacement_reparses_and_wire_is_strict() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("split.rs");
        fs::write(&path, SOURCE).unwrap();
        let candidate = candidates(root.path()).pop().unwrap();
        let edit = &candidate.edits()[0];
        let mut changed = SOURCE.to_string();
        changed.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        fs::write(&path, changed).unwrap();
        let projection = build_rust_recipe_projection(root.path(), &[PathBuf::from("split.rs")])
            .unwrap()
            .unwrap();
        let analysis = projection
            .data_flow()
            .control_regions()
            .control_flow()
            .analysis();
        assert!(
            analysis
                .node_ids()
                .all(|id| analysis.node(id).is_ok_and(|node| !node.has_error()))
        );
        let value = serde_json::to_value(&candidate).unwrap();
        let decoded: TransformationCandidate = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded, candidate);
        let mut stale = value;
        stale["disposition"] = serde_json::json!("automatic");
        assert!(serde_json::from_value::<TransformationCandidate>(stale).is_err());
    }

    #[test]
    fn non_action_shapes_abstain() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("split.rs"), "fn a() {} fn b() {} fn run(f: bool) { if f { a(); } if f { let x = 1; a(); } if f { /*keep*/ a(); b(); } if f { a(); b(); } else { a(); } }").unwrap();
        assert!(candidates(root.path()).is_empty());
    }
}
