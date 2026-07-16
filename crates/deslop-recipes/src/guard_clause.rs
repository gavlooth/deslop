use std::collections::{BTreeSet, VecDeque};

use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    AdapterCapability, ControlAbruptKind, ControlBranchKind, ControlEdgeKind, ControlEdgePrecision,
    ControlExitOutcome, ControlPointKind, ControlSyntheticPointKind, FactCoverage,
    GraphEvidenceLayer, ProgramDependenceGraph, ProgramDependenceNode, ProgramDependenceProjection,
    ProjectAnalysis, evaluate_program_graph_recipe_eligibility,
};

use crate::branch::{
    capability_result, condition, fixture, flow_entity, graph_entity, graph_root, result, span,
};
use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactDirection, ImpactQueryError, ProofState, RecipeContractError, RecipeFixtureRole,
    RollbackPlan, RollbackStrategy, TransformationCandidate, TransformationCandidateDraft,
    TransformationEdit, TransformationFamily, TransformationRecipe, TransformationRecipeDraft,
    ValidationPlan, ValidationStep, ValidationStepKind, program_dependence_impact_cone,
};

const REQUIRED_EXIT: &str = "guard-arm-abrupt-exit-exact";
const REQUIRED_PST: &str = "pst-continuation-boundary-exact";
const REQUIRED_PREDICATE: &str = "predicate-count-and-polarity-preserved";
const REQUIRED_SCOPE: &str = "continuation-scope-and-effects-preserved";
const FORBIDDEN_CONTROL: &str = "recovered-or-conservative-control";
const FORBIDDEN_BINDING: &str = "binding-lifetime-or-drop-change";
const FORBIDDEN_EFFECT: &str = "effect-exception-or-suspension-change";
const FORBIDDEN_NON_STRUCTURED: &str = "non-structured-control";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardKind {
    ThenTerminates,
    ElseTerminates,
}

impl GuardKind {
    fn name(self) -> &'static str {
        match self {
            Self::ThenTerminates => "then-guard",
            Self::ElseTerminates => "inverted-else-guard",
        }
    }

    fn terminating_branch(self) -> ControlBranchKind {
        match self {
            Self::ThenTerminates => ControlBranchKind::True,
            Self::ElseTerminates => ControlBranchKind::False,
        }
    }

    fn continuing_branch(self) -> ControlBranchKind {
        match self {
            Self::ThenTerminates => ControlBranchKind::False,
            Self::ElseTerminates => ControlBranchKind::True,
        }
    }
}

#[derive(Debug)]
struct GuardShape<'a> {
    kind: GuardKind,
    predicate: deslop_parse::NodeView<'a>,
    guard_block: deslop_parse::NodeView<'a>,
    continuation_block: deslop_parse::NodeView<'a>,
    abrupt: deslop_parse::NodeView<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardClauseExitEvidence {
    pub dispatch: GraphEntityRef,
    pub merge: GraphEntityRef,
    pub abrupt: GraphEntityRef,
    pub exit_dispatch: GraphEntityRef,
    pub pst_points: Vec<GraphEntityRef>,
    pub pst_authority: ProofState,
}

#[derive(Debug, thiserror::Error)]
pub enum GuardClauseRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("guard-clause graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("guard-clause recipe received an inconsistent projection: {0}")]
    Projection(String),
}

pub fn guard_clause_inversion_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-invert-guard-clause".into(),
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
                REQUIRED_EXIT,
                "One exact branch arm consists of a direct abrupt exit.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                REQUIRED_PST,
                "Complete PST facts prove the other arm reaches the branch continuation boundary.",
                GraphEvidenceLayer::ControlRegions,
            ),
            condition(
                REQUIRED_PREDICATE,
                "The predicate is evaluated once with exactly the required polarity.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                REQUIRED_SCOPE,
                "Complete def/use and effect evidence proves flattening the continuation block preserves semantics.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_CONTROL,
                "Recovered syntax or conservative control participates in either path.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_BINDING,
                "Moving the continuation changes binding visibility, borrow extent, lifetime, or drop timing.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                FORBIDDEN_EFFECT,
                "Inversion changes effect, exception, panic, abrupt-exit, or suspension order.",
                GraphEvidenceLayer::DataFlow,
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
                    "Build after flattening the guarded continuation.",
                ),
                validation(
                    "graph-delta",
                    ValidationStepKind::GraphDelta,
                    "Rebuild CFG/PST/PDG evidence and compare the guard boundary.",
                ),
                validation(
                    "parse",
                    ValidationStepKind::Parse,
                    "Parse the exact guard-clause replacement.",
                ),
                validation(
                    "test",
                    ValidationStepKind::Test,
                    "Run project tests before accepting the review candidate.",
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
                "direct-return-guard",
                FixtureExpectation::Candidate,
                "A direct returning arm guards a statement-only continuation.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "both-arms-continue",
                FixtureExpectation::NoCandidate,
                "Neither arm has an exact abrupt exit.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "unknown-flattened-scope",
                FixtureExpectation::ReviewRequired,
                "PST control is exact but production DefUse and Effects authority is unavailable.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "tail-valued-if",
                FixtureExpectation::NoCandidate,
                "A value-producing tail if cannot be flattened as a statement guard.",
            ),
        ],
    })
}

pub fn detect_guard_clause_inversions(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, GuardClauseRecipeError> {
    let recipe = guard_clause_inversion_recipe()?;
    let data_flow = projection.data_flow();
    let regions = data_flow.control_regions();
    let flow = regions.control_flow();
    let analysis = flow.analysis();
    let non_structured = projection.non_structured_control();
    let mut emitted = BTreeSet::new();
    let mut candidates = Vec::new();

    for graph in projection.document().graphs() {
        let eligibility = evaluate_program_graph_recipe_eligibility(
            projection,
            graph,
            &recipe.eligibility_requirement(),
        )
        .map_err(|error| GuardClauseRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = flow
            .document()
            .graphs()
            .iter()
            .find(|item| item.key() == graph.control_flow_graph())
            .ok_or_else(|| missing("control-flow graph", graph.control_flow_graph().as_str()))?;
        let region_graph = regions
            .document()
            .graphs()
            .iter()
            .find(|item| item.control_flow_graph() == flow_graph.key())
            .ok_or_else(|| missing("control-region graph", flow_graph.key().as_str()))?;
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
            if !emitted.insert(source.clone()) {
                continue;
            }
            let branch = analysis
                .node_by_key(source)
                .map_err(|error| GuardClauseRecipeError::Projection(error.to_string()))?;
            let Some(shape) = guard_shape(analysis, branch)? else {
                continue;
            };
            let Some(merge) = flow_graph.points().iter().find(|point| {
                point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::Merge)
                    && point.source() == Some(source)
                    && !point.recovered()
            }) else {
                continue;
            };
            let Some(abrupt) = flow_graph.points().iter().find(|point| {
                point.kind() == &ControlPointKind::Syntax
                    && point.source() == Some(shape.abrupt.key())
                    && !point.recovered()
            }) else {
                continue;
            };
            let Some((terminating, continuing)) = exact_branch_pair(
                flow_graph,
                dispatch.key(),
                shape.kind.terminating_branch(),
                shape.kind.continuing_branch(),
            ) else {
                continue;
            };
            if terminating.to() != abrupt.key()
                || !exact_path_reaches(flow_graph, continuing.to(), merge.key())
            {
                continue;
            }
            let Some(exit_dispatch) = exact_abrupt_exit(flow_graph, abrupt.key()) else {
                continue;
            };
            let Some(exit_evidence) = pst_exit_evidence(
                region_graph,
                flow_graph,
                dispatch.key(),
                merge.key(),
                abrupt.key(),
                exit_dispatch,
            ) else {
                continue;
            };
            let Some(root) = graph
                .nodes()
                .iter()
                .find(|node| node.point() == dispatch.key())
            else {
                continue;
            };

            let target_span = span(source);
            let pst_evidence = exit_evidence
                .pst_points
                .iter()
                .cloned()
                .map(|entity| {
                    plain_evidence(
                        entity,
                        "Complete PST reachability and post-dominance retain the continuation boundary.",
                    )
                })
                .collect();
            let required_results = vec![
                multi_result(
                    REQUIRED_EXIT,
                    ProofState::Proven,
                    vec![
                        plain_evidence(
                            exit_evidence.abrupt.clone(),
                            "The selected arm is one direct exact return/terminate point.",
                        ),
                        plain_evidence(
                            exit_evidence.exit_dispatch.clone(),
                            "The abrupt edge reaches the callable abrupt-exit dispatch and virtual exit.",
                        ),
                    ],
                ),
                multi_result(REQUIRED_PST, exit_evidence.pst_authority, pst_evidence),
                result(
                    REQUIRED_PREDICATE,
                    ProofState::Proven,
                    exit_evidence.dispatch.clone(),
                    if shape.kind == GuardKind::ElseTerminates {
                        "The original predicate is evaluated once and boolean-negated to select the same terminating else arm."
                    } else {
                        "The original predicate is evaluated once with unchanged polarity for the terminating then arm."
                    },
                ),
                capability_result(
                    REQUIRED_SCOPE,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production DefUse/Effects authority cannot prove flattened binding, borrow, temporary, or drop semantics.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                ),
            ];
            let forbidden_results = vec![
                multi_result(
                    FORBIDDEN_CONTROL,
                    ProofState::Disproven,
                    vec![
                        plain_evidence(
                            exit_evidence.dispatch.clone(),
                            "Both dispatch edges and the selected paths are exact and unrecovered.",
                        ),
                        plain_evidence(
                            exit_evidence.merge.clone(),
                            "The continuation reaches the exact branch merge.",
                        ),
                    ],
                ),
                capability_result(
                    FORBIDDEN_BINDING,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Missing DefUse authority cannot disprove binding, lifetime, borrow, or drop changes.",
                    AdapterCapability::DefUse,
                    data_graph.coverage().def_use_support(),
                    data_graph.coverage().def_use_authority(),
                ),
                capability_result(
                    FORBIDDEN_EFFECT,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Missing Effects authority cannot disprove a hidden effect, panic, exception, or suspension change.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
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
                        "No retained non-structured-control fact participates in this callable."
                    } else {
                        "Retained non-structured-control facts require manual review."
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
                        subtree_fingerprint: None,
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
                    expected_delta: guard_delta(graph, root, &exit_evidence, shape.kind),
                    edits: vec![TransformationEdit::exact_node_replacement(
                        source.clone(),
                        target_span,
                        branch.text().into(),
                        render_guard(&shape)?,
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

fn guard_shape<'a>(
    analysis: &'a ProjectAnalysis,
    branch: deslop_parse::NodeView<'a>,
) -> Result<Option<GuardShape<'a>>, GuardClauseRecipeError> {
    if branch.grammar().lang() != Lang::Rust
        || branch.raw_grammar_kind() != "if_expression"
        || branch.has_error()
        || branch.text().contains("//")
        || branch.text().contains("/*")
    {
        return Ok(None);
    }
    let Some(parent) = branch.parent() else {
        return Ok(None);
    };
    let parent = analysis
        .node(parent)
        .map_err(|error| GuardClauseRecipeError::Projection(error.to_string()))?;
    let parent_kind = parent.raw_grammar_kind();
    if parent_kind != "expression_statement" {
        return Ok(None);
    }
    let Some(predicate) = child_by_field(analysis, branch, "condition")? else {
        return Ok(None);
    };
    if contains_let_condition(analysis, predicate)? {
        return Ok(None);
    }
    let Some(then_block) = child_by_field(analysis, branch, "consequence")? else {
        return Ok(None);
    };
    let Some(alternative) = child_by_field(analysis, branch, "alternative")? else {
        return Ok(None);
    };
    let Some(else_block) = else_block(analysis, alternative)? else {
        return Ok(None);
    };
    if then_block.raw_grammar_kind() != "block" || then_block.has_error() || else_block.has_error()
    {
        return Ok(None);
    }
    let then_abrupt = direct_abrupt(analysis, then_block)?;
    let else_abrupt = direct_abrupt(analysis, else_block)?;
    match (then_abrupt, else_abrupt) {
        (Some(abrupt), None) if statement_only_continuation(analysis, else_block)? => {
            Ok(Some(GuardShape {
                kind: GuardKind::ThenTerminates,
                predicate,
                guard_block: then_block,
                continuation_block: else_block,
                abrupt,
            }))
        }
        (None, Some(abrupt)) if statement_only_continuation(analysis, then_block)? => {
            Ok(Some(GuardShape {
                kind: GuardKind::ElseTerminates,
                predicate,
                guard_block: else_block,
                continuation_block: then_block,
                abrupt,
            }))
        }
        _ => Ok(None),
    }
}

fn direct_abrupt<'a>(
    analysis: &'a ProjectAnalysis,
    block: deslop_parse::NodeView<'a>,
) -> Result<Option<deslop_parse::NodeView<'a>>, GuardClauseRecipeError> {
    let children = named_children(analysis, block)?;
    if children.len() != 1 {
        return Ok(None);
    }
    let mut node = children[0];
    if node.raw_grammar_kind() == "expression_statement" {
        let nested = named_children(analysis, node)?;
        if nested.len() != 1 {
            return Ok(None);
        }
        node = nested[0];
    }
    Ok((node.raw_grammar_kind() == "return_expression" && !node.has_error()).then_some(node))
}

fn statement_only_continuation(
    analysis: &ProjectAnalysis,
    block: deslop_parse::NodeView<'_>,
) -> Result<bool, GuardClauseRecipeError> {
    let children = named_children(analysis, block)?;
    Ok(!children.is_empty()
        && children.len() <= 8
        && children
            .iter()
            .all(|child| !child.has_error() && child.text().trim_end().ends_with(';')))
}

fn contains_let_condition(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, GuardClauseRecipeError> {
    if matches!(node.raw_grammar_kind(), "let_condition" | "let_chain") {
        return Ok(true);
    }
    Ok(analysis
        .descendant_node_ids(node.id())
        .map_err(|error| GuardClauseRecipeError::Projection(error.to_string()))?
        .any(|id| {
            analysis
                .node(id)
                .is_ok_and(|item| matches!(item.raw_grammar_kind(), "let_condition" | "let_chain"))
        }))
}

fn else_block<'a>(
    analysis: &'a ProjectAnalysis,
    alternative: deslop_parse::NodeView<'a>,
) -> Result<Option<deslop_parse::NodeView<'a>>, GuardClauseRecipeError> {
    if alternative.raw_grammar_kind() == "block" {
        return Ok(Some(alternative));
    }
    if alternative.raw_grammar_kind() != "else_clause" {
        return Ok(None);
    }
    Ok(named_children(analysis, alternative)?
        .into_iter()
        .find(|child| child.raw_grammar_kind() == "block"))
}

fn exact_branch_pair<'a>(
    graph: &'a deslop_parse::ControlFlowGraph,
    dispatch: &deslop_parse::ControlPointKey,
    terminating_kind: ControlBranchKind,
    continuing_kind: ControlBranchKind,
) -> Option<(&'a deslop_parse::ControlEdge, &'a deslop_parse::ControlEdge)> {
    let edges = graph
        .edges()
        .iter()
        .filter(|edge| edge.from() == dispatch)
        .collect::<Vec<_>>();
    if edges.len() != 2 || edges.iter().any(|edge| !exact_edge(edge)) {
        return None;
    }
    let terminating = edges
        .iter()
        .copied()
        .find(|edge| edge.kind() == &ControlEdgeKind::Branch(terminating_kind.clone()))?;
    let continuing = edges
        .iter()
        .copied()
        .find(|edge| edge.kind() == &ControlEdgeKind::Branch(continuing_kind.clone()))?;
    Some((terminating, continuing))
}

fn exact_abrupt_exit<'a>(
    graph: &'a deslop_parse::ControlFlowGraph,
    abrupt: &deslop_parse::ControlPointKey,
) -> Option<&'a deslop_parse::ControlPointKey> {
    let outgoing = graph
        .edges()
        .iter()
        .filter(|edge| edge.from() == abrupt)
        .collect::<Vec<_>>();
    if outgoing.len() != 1 {
        return None;
    }
    let abrupt_edge = outgoing[0];
    if !exact_edge(abrupt_edge)
        || !matches!(
            abrupt_edge.kind(),
            ControlEdgeKind::Abrupt(ControlAbruptKind::Return | ControlAbruptKind::Terminate)
        )
    {
        return None;
    }
    let dispatch = graph.points().iter().find(|point| {
        point.key() == abrupt_edge.to()
            && point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch)
            && !point.recovered()
    })?;
    let exit_edges = graph
        .edges()
        .iter()
        .filter(|edge| edge.from() == dispatch.key())
        .collect::<Vec<_>>();
    (exit_edges.len() == 1
        && exit_edges[0].to() == graph.exit()
        && exit_edges[0].kind() == &ControlEdgeKind::Exit(ControlExitOutcome::Abrupt)
        && exact_edge(exit_edges[0]))
    .then_some(dispatch.key())
}

fn exact_path_reaches(
    graph: &deslop_parse::ControlFlowGraph,
    start: &deslop_parse::ControlPointKey,
    target: &deslop_parse::ControlPointKey,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(point) = queue.pop_front() {
        if point == target {
            return true;
        }
        if !seen.insert(point.clone()) {
            continue;
        }
        let outgoing = graph
            .edges()
            .iter()
            .filter(|edge| edge.from() == point)
            .collect::<Vec<_>>();
        if outgoing.iter().any(|edge| !exact_edge(edge)) {
            return false;
        }
        for edge in outgoing {
            queue.push_back(edge.to());
        }
    }
    false
}

fn pst_exit_evidence(
    region: &deslop_parse::ControlRegionGraph,
    flow: &deslop_parse::ControlFlowGraph,
    dispatch: &deslop_parse::ControlPointKey,
    merge: &deslop_parse::ControlPointKey,
    abrupt: &deslop_parse::ControlPointKey,
    exit_dispatch: &deslop_parse::ControlPointKey,
) -> Option<GuardClauseExitEvidence> {
    let points = [dispatch, merge, abrupt, exit_dispatch]
        .into_iter()
        .map(|point| region.points().iter().find(|fact| fact.point() == point))
        .collect::<Option<Vec<_>>>()?;
    if points.iter().any(|fact| {
        !fact.reachable()
            || !fact.exit_reachable()
            || !fact.post_dominators().contains(&flow.exit().clone())
    }) {
        return None;
    }
    Some(GuardClauseExitEvidence {
        dispatch: flow_entity(flow.key().as_str(), dispatch.as_str()),
        merge: flow_entity(flow.key().as_str(), merge.as_str()),
        abrupt: flow_entity(flow.key().as_str(), abrupt.as_str()),
        exit_dispatch: flow_entity(flow.key().as_str(), exit_dispatch.as_str()),
        pst_points: points
            .iter()
            .map(|fact| {
                graph_entity(
                    GraphEvidenceLayer::ControlRegions,
                    region.key().as_str(),
                    fact.key().as_str(),
                )
            })
            .collect(),
        pst_authority: if region.coverage().status() == FactCoverage::Complete {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
    })
}

fn exact_edge(edge: &deslop_parse::ControlEdge) -> bool {
    edge.precision() == &ControlEdgePrecision::Exact
        && !edge.recovered_source()
        && !edge.recovered_predicate()
}

fn render_guard(shape: &GuardShape<'_>) -> Result<String, GuardClauseRecipeError> {
    let continuation = block_body(shape.continuation_block.text())?;
    let predicate = match shape.kind {
        GuardKind::ThenTerminates => shape.predicate.text().to_string(),
        GuardKind::ElseTerminates => format!("!({})", shape.predicate.text()),
    };
    Ok(format!(
        "if {predicate} {} {continuation}",
        shape.guard_block.text()
    ))
}

fn block_body(text: &str) -> Result<&str, GuardClauseRecipeError> {
    text.strip_prefix('{')
        .and_then(|body| body.strip_suffix('}'))
        .map(str::trim)
        .ok_or_else(|| GuardClauseRecipeError::Projection("Rust block lacks exact braces".into()))
}

fn guard_delta(
    graph: &ProgramDependenceGraph,
    root: &ProgramDependenceNode,
    evidence: &GuardClauseExitEvidence,
    kind: GuardKind,
) -> ExpectedGraphDelta {
    ExpectedGraphDelta {
        changes: vec![
            ExpectedGraphChange {
                kind: GraphChangeKind::Modify,
                entity: graph_root(graph, root),
                rationale: format!(
                    "The dispatch becomes the exact {} with one retained abrupt arm.",
                    kind.name()
                ),
            },
            ExpectedGraphChange {
                kind: GraphChangeKind::Modify,
                entity: evidence.merge.clone(),
                rationale: "The continuation boundary moves before the flattened statements after rebuilding."
                    .into(),
            },
            ExpectedGraphChange {
                kind: GraphChangeKind::Preserve,
                entity: evidence.abrupt.clone(),
                rationale: "The direct abrupt exit must remain attached to the same guard outcome."
                    .into(),
            },
            ExpectedGraphChange {
                kind: GraphChangeKind::Preserve,
                entity: evidence.exit_dispatch.clone(),
                rationale: "The callable abrupt-exit outcome must remain.".into(),
            },
        ],
    }
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, GuardClauseRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| GuardClauseRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, GuardClauseRecipeError> {
    node.children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| GuardClauseRecipeError::Projection(error.to_string()))
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

fn missing(kind: &str, identity: &str) -> GuardClauseRecipeError {
    GuardClauseRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::{branch_graph_evidence, build_rust_recipe_projection, detect_rust_recipes};

    const SOURCE: &str = r#"
fn act() {}

fn then_guard(flag: bool) {
    if flag { return; } else { let _value = 1; }
}

fn else_guard(flag: bool) {
    if flag { let _value = 1; } else { return; }
}

fn call_guard(flag: bool) {
    if flag { act(); } else { return; }
}

fn no_exit(flag: bool) {
    if flag { act(); } else { act(); }
}

fn tail_value(flag: bool) -> i32 {
    if flag { 1 } else { 2 }
}
"#;

    fn candidates(root: &std::path::Path) -> Vec<TransformationCandidate> {
        detect_rust_recipes(root, &[PathBuf::from("guards.rs")])
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.recipe().name() == "rust-invert-guard-clause")
            .collect()
    }

    #[test]
    fn recipe_freezes_four_roles_and_pst_boundary() {
        let recipe = guard_clause_inversion_recipe().unwrap();
        assert_eq!(recipe.fixtures().len(), 4);
        assert!(
            recipe
                .required_layers()
                .contains(&GraphEvidenceLayer::ControlRegions)
        );
        assert_eq!(recipe.maximum_safety(), SafetyClass::SafeWithPrecondition);
    }

    #[test]
    fn detects_both_guard_polarities_with_exact_exit_and_unknown_scope() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("guards.rs"), SOURCE).unwrap();
        let found = candidates(root.path());
        let replacements = found
            .iter()
            .map(|candidate| candidate.edits()[0].after.as_str())
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 2, "{replacements:#?}");
        assert!(
            replacements
                .iter()
                .any(|replacement| replacement.starts_with("if flag { return; }"))
        );
        assert!(
            replacements
                .iter()
                .any(|replacement| replacement.starts_with("if !(flag) { return; }"))
        );
        let proven_pst = found
            .iter()
            .filter(|candidate| {
                candidate
                    .required_results()
                    .iter()
                    .any(|item| item.condition == REQUIRED_PST && item.state == ProofState::Proven)
            })
            .count();
        let unknown_pst = found
            .iter()
            .filter(|candidate| {
                candidate
                    .required_results()
                    .iter()
                    .any(|item| item.condition == REQUIRED_PST && item.state == ProofState::Unknown)
            })
            .count();
        assert_eq!((proven_pst, unknown_pst), (2, 0));
        for candidate in found {
            assert_eq!(
                candidate.disposition(),
                CandidateDisposition::ReviewRequired
            );
            assert!(candidate.required_results().iter().any(|item| {
                item.condition == REQUIRED_EXIT && item.state == ProofState::Proven
            }));
            assert!(
                candidate
                    .required_results()
                    .iter()
                    .any(|item| { item.condition == REQUIRED_PST && item.evidence.len() == 4 })
            );
            assert!(candidate.required_results().iter().any(|item| {
                item.condition == REQUIRED_SCOPE && item.state == ProofState::Unknown
            }));
            assert_eq!(
                branch_graph_evidence(&candidate)
                    .unwrap()
                    .after
                    .changes
                    .len(),
                4
            );
        }
    }

    #[test]
    fn replacement_reparses_and_wire_is_strict() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("guards.rs");
        fs::write(&path, SOURCE).unwrap();
        for candidate in candidates(root.path()) {
            let edit = &candidate.edits()[0];
            let mut changed = SOURCE.to_string();
            changed.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
            fs::write(&path, changed).unwrap();
            let projection =
                build_rust_recipe_projection(root.path(), &[PathBuf::from("guards.rs")])
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
            fs::write(&path, SOURCE).unwrap();

            let value = serde_json::to_value(&candidate).unwrap();
            let decoded: TransformationCandidate = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(decoded, candidate);
            let mut stale = value;
            stale["disposition"] = serde_json::json!("automatic");
            assert!(serde_json::from_value::<TransformationCandidate>(stale).is_err());
        }
    }

    #[test]
    fn comments_let_conditions_and_tail_values_abstain() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("guards.rs"),
            "fn act() {}\nfn a(value: Option<bool>) { if let Some(flag) = value { act(); } else { return; } }\n\
             fn b(flag: bool) { if flag { /* keep */ act(); } else { return; } }\n\
             fn c(flag: bool) -> i32 { if flag { 1 } else { 2 } }\n",
        )
        .unwrap();
        assert!(candidates(root.path()).is_empty());
    }
}
