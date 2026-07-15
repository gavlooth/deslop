use std::collections::BTreeSet;

use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    AdapterCapability, ControlBranchKind, ControlEdgeKind, ControlPointKind,
    ControlSyntheticPointKind, FactCoverage, GraphEvidenceLayer, ProgramDependenceGraph,
    ProgramDependenceNode, ProgramDependenceProjection, ProjectAnalysis,
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

const DEAD_LITERAL: &str = "literal-predicate-outcome-exact";
const DEAD_SELECTED: &str = "selected-arm-fragment-exact";
const DEAD_COMPILE: &str = "dead-arm-compile-effects-preserved";
const DEAD_CONTROL: &str = "recovered-or-conservative-control";
const DEAD_OPAQUE: &str = "removed-comment-attribute-or-macro";
const DEAD_NON_STRUCTURED: &str = "non-structured-control";

const CHAIN_SUBJECT: &str = "shared-subject-comparisons-exact";
const CHAIN_EXHAUSTIVE: &str = "explicit-fallback-exhaustive";
const CHAIN_REBUILD: &str = "match-dispatch-reconstructible";
const CHAIN_PST: &str = "chain-pst-nesting-retained";
const CHAIN_EQUALITY: &str = "equality-pattern-semantics-preserved";
const CHAIN_CONTROL: &str = "recovered-or-conservative-control";
const CHAIN_MOVE: &str = "subject-move-borrow-or-drop-change";
const CHAIN_EFFECT: &str = "comparison-effect-or-exception-change";
const CHAIN_NON_STRUCTURED: &str = "non-structured-control";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadArmGraphEvidence {
    pub dispatch: GraphEntityRef,
    pub selected_arm: GraphEntityRef,
    pub dead_arm: GraphEntityRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExhaustiveChainGraphEvidence {
    pub dispatches: Vec<GraphEntityRef>,
    pub pst_points: Vec<GraphEntityRef>,
    pub pst_authority: ProofState,
}

#[derive(Debug, thiserror::Error)]
pub enum TerminalBranchRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("terminal-branch graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("terminal-branch recipe received an inconsistent projection: {0}")]
    Projection(String),
}

#[derive(Debug)]
struct DeadShape<'a> {
    predicate: deslop_parse::NodeView<'a>,
    selected: deslop_parse::NodeView<'a>,
    selected_kind: ControlBranchKind,
    dead_kind: ControlBranchKind,
}

#[derive(Debug)]
struct ChainCase<'a> {
    branch: deslop_parse::NodeView<'a>,
    pattern: deslop_parse::NodeView<'a>,
    body: deslop_parse::NodeView<'a>,
}

#[derive(Debug)]
struct ChainShape<'a> {
    subject: deslop_parse::NodeView<'a>,
    cases: Vec<ChainCase<'a>>,
    fallback: deslop_parse::NodeView<'a>,
}

pub fn literal_dead_arm_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-remove-literal-dead-arm".into(),
        version: "1.0.0".into(),
        family: TransformationFamily::BranchControl,
        required_layers: branch_layers(),
        required_conditions: vec![
            condition(
                DEAD_LITERAL,
                "An exact Rust boolean literal makes one branch outcome infeasible.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                DEAD_SELECTED,
                "The selected arm is retained as one exact block expression.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                DEAD_COMPILE,
                "Complete effect evidence proves deleting the unselected syntax has no compile-time behavior.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                DEAD_CONTROL,
                "Recovered syntax or conservative dispatch control participates.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                DEAD_OPAQUE,
                "The removed arm contains a comment, attribute, or macro boundary.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                DEAD_NON_STRUCTURED,
                "Non-structured control participates in the callable.",
                GraphEvidenceLayer::NonStructuredControl,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: branch_validation("dead-arm deletion"),
        rollback_plan: branch_rollback(),
        fixtures: vec![
            fixture(
                RecipeFixtureRole::Positive,
                "literal-true-arm",
                FixtureExpectation::Candidate,
                "An explicit true predicate selects the consequence block.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "dynamic-predicate",
                FixtureExpectation::NoCandidate,
                "A dynamic predicate does not prove either arm dead.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "unknown-compile-effects",
                FixtureExpectation::ReviewRequired,
                "The runtime arm is exact but production compile/effect authority is unavailable.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "macro-in-dead-arm",
                FixtureExpectation::NoCandidate,
                "Macro or attributed syntax cannot be discarded as incidental dead text.",
            ),
        ],
    })
}

pub fn exhaustive_chain_to_match_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-convert-exhaustive-chain-to-match".into(),
        version: "1.0.0".into(),
        family: TransformationFamily::BranchControl,
        required_layers: branch_layers(),
        required_conditions: vec![
            condition(
                CHAIN_SUBJECT,
                "Every branch compares the same identifier to a distinct literal or qualified path.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CHAIN_EXHAUSTIVE,
                "An explicit final else block becomes the exact wildcard match arm.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CHAIN_REBUILD,
                "The generated unguarded final-wildcard match has exact retained CFG lowering.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CHAIN_PST,
                "PST facts retain the nested dispatch reachability boundary.",
                GraphEvidenceLayer::ControlRegions,
            ),
            condition(
                CHAIN_EQUALITY,
                "Complete semantic evidence proves equality tests and match patterns select identical values.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                CHAIN_CONTROL,
                "Recovered syntax or conservative dispatch control participates.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CHAIN_MOVE,
                "Evaluating the match subject once changes move, borrow, lifetime, or drop behavior.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                CHAIN_EFFECT,
                "Replacing equality dispatch changes effects, panic, exception, or suspension behavior.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                CHAIN_NON_STRUCTURED,
                "Non-structured control participates in the callable.",
                GraphEvidenceLayer::NonStructuredControl,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: branch_validation("exhaustive match dispatch"),
        rollback_plan: branch_rollback(),
        fixtures: vec![
            fixture(
                RecipeFixtureRole::Positive,
                "qualified-enum-chain",
                FixtureExpectation::Candidate,
                "Two qualified-path comparisons and a fallback become a final-wildcard match.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "different-subjects",
                FixtureExpectation::NoCandidate,
                "Comparisons over different identifiers cannot share a match subject.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "unknown-partialeq-semantics",
                FixtureExpectation::ReviewRequired,
                "Structural exhaustiveness is exact but equality/type semantics lack production authority.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "missing-final-fallback",
                FixtureExpectation::NoCandidate,
                "A chain without an explicit fallback has no syntax-level exhaustiveness proof.",
            ),
        ],
    })
}

pub fn detect_literal_dead_arms(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, TerminalBranchRecipeError> {
    let recipe = literal_dead_arm_recipe()?;
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
        .map_err(|error| TerminalBranchRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = flow_graph(flow, graph)?;
        let data_graph = data_graph(data_flow, graph)?;
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

        for dispatch in branch_dispatches(flow_graph) {
            let Some(source) = dispatch.source() else {
                continue;
            };
            if !exact_branch_edges(flow_graph, dispatch.key()) {
                continue;
            }
            let branch = analysis
                .node_by_key(source)
                .map_err(|error| TerminalBranchRecipeError::Projection(error.to_string()))?;
            let Some(shape) = dead_shape(analysis, branch)? else {
                continue;
            };
            let Some(selected_edge) = branch_edge(flow_graph, dispatch.key(), &shape.selected_kind)
            else {
                continue;
            };
            let Some(dead_edge) = branch_edge(flow_graph, dispatch.key(), &shape.dead_kind) else {
                continue;
            };
            let Some(root) = pdg_dispatch(graph, dispatch.key()) else {
                continue;
            };
            let evidence = DeadArmGraphEvidence {
                dispatch: flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                selected_arm: flow_entity(flow_graph.key().as_str(), selected_edge.to().as_str()),
                dead_arm: flow_entity(flow_graph.key().as_str(), dead_edge.to().as_str()),
            };
            let target_span = span(source);
            let required_results = vec![
                multi_result(
                    DEAD_LITERAL,
                    ProofState::Proven,
                    vec![
                        plain_evidence(
                            evidence.dispatch.clone(),
                            &format!(
                                "Exact predicate `{}` selects only the {:?} branch outcome.",
                                shape.predicate.text(),
                                shape.selected_kind
                            ),
                        ),
                        plain_evidence(
                            evidence.dead_arm.clone(),
                            "The opposite exact branch edge is infeasible under the retained literal value.",
                        ),
                    ],
                ),
                result(
                    DEAD_SELECTED,
                    ProofState::Proven,
                    evidence.selected_arm.clone(),
                    "The selected block bytes replace the complete if expression without reordering.",
                ),
                capability_result(
                    DEAD_COMPILE,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production Effects authority cannot prove deletion is neutral to every compile-time or type-level effect.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                ),
            ];
            let forbidden_results = vec![
                result(
                    DEAD_CONTROL,
                    ProofState::Disproven,
                    evidence.dispatch.clone(),
                    "The dispatch and both outgoing edges are exact and unrecovered.",
                ),
                result(
                    DEAD_OPAQUE,
                    ProofState::Disproven,
                    evidence.dead_arm.clone(),
                    "The complete branch was checked recursively for comments, attributes, and macros.",
                ),
                non_structured_result(DEAD_NON_STRUCTURED, non_structured_graph),
            ];
            candidates.push(TransformationCandidate::new(
                TransformationCandidateDraft {
                    recipe: recipe.clone(),
                    source: candidate_source(analysis, projection),
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
                    expected_delta: dead_delta(graph, root, &evidence),
                    edits: vec![TransformationEdit::exact_node_replacement(
                        source.clone(),
                        target_span,
                        branch.text().into(),
                        shape.selected.text().into(),
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

pub fn detect_exhaustive_chain_matches(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, TerminalBranchRecipeError> {
    let recipe = exhaustive_chain_to_match_recipe()?;
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
        .map_err(|error| TerminalBranchRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = flow_graph(flow, graph)?;
        let region_graph = regions
            .document()
            .graphs()
            .iter()
            .find(|item| item.control_flow_graph() == flow_graph.key())
            .ok_or_else(|| missing("control-region graph", flow_graph.key().as_str()))?;
        let data_graph = data_graph(data_flow, graph)?;
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

        for dispatch in branch_dispatches(flow_graph) {
            let Some(source) = dispatch.source() else {
                continue;
            };
            let branch = analysis
                .node_by_key(source)
                .map_err(|error| TerminalBranchRecipeError::Projection(error.to_string()))?;
            if nested_else_if(analysis, branch)? {
                continue;
            }
            let Some(shape) = chain_shape(analysis, branch)? else {
                continue;
            };
            let mut dispatch_nodes = Vec::new();
            let mut dispatch_entities = Vec::new();
            let mut pst_points = Vec::new();
            let mut exact = true;
            for case in &shape.cases {
                let Some(point) = flow_graph.points().iter().find(|point| {
                    point.kind()
                        == &ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch)
                        && point.source() == Some(case.branch.key())
                }) else {
                    exact = false;
                    break;
                };
                if point.recovered() || !exact_branch_edges(flow_graph, point.key()) {
                    exact = false;
                    break;
                }
                let Some(node) = pdg_dispatch(graph, point.key()) else {
                    exact = false;
                    break;
                };
                let Some(fact) = region_graph
                    .points()
                    .iter()
                    .find(|fact| fact.point() == point.key())
                else {
                    exact = false;
                    break;
                };
                dispatch_nodes.push(node);
                dispatch_entities
                    .push(flow_entity(flow_graph.key().as_str(), point.key().as_str()));
                pst_points.push(graph_entity(
                    GraphEvidenceLayer::ControlRegions,
                    region_graph.key().as_str(),
                    fact.key().as_str(),
                ));
            }
            if !exact || dispatch_nodes.len() != shape.cases.len() {
                continue;
            }
            let pst_authority = if region_graph.coverage().status() == FactCoverage::Complete {
                ProofState::Proven
            } else {
                ProofState::Unknown
            };
            let evidence = ExhaustiveChainGraphEvidence {
                dispatches: dispatch_entities,
                pst_points,
                pst_authority,
            };
            let root = dispatch_nodes[0];
            let target_span = span(source);
            let dispatch_evidence = evidence
                .dispatches
                .iter()
                .cloned()
                .map(|entity| {
                    plain_evidence(
                        entity,
                        "Exact chain dispatch over the retained shared subject.",
                    )
                })
                .collect::<Vec<_>>();
            let required_results = vec![
                multi_result(CHAIN_SUBJECT, ProofState::Proven, dispatch_evidence.clone()),
                result(
                    CHAIN_EXHAUSTIVE,
                    ProofState::Proven,
                    evidence.dispatches[0].clone(),
                    "The explicit final else block becomes a final `_` arm, so syntax-level dispatch is exhaustive.",
                ),
                result(
                    CHAIN_REBUILD,
                    ProofState::Proven,
                    evidence.dispatches[0].clone(),
                    "The generated match uses unguarded case arms and a unique final wildcard supported by exact CFG lowering.",
                ),
                multi_result(
                    CHAIN_PST,
                    pst_authority,
                    evidence
                        .pst_points
                        .iter()
                        .cloned()
                        .map(|entity| plain_evidence(entity, "Retained chain dispatch PST point."))
                        .collect(),
                ),
                capability_result(
                    CHAIN_EQUALITY,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production type/effect authority cannot prove overloaded equality is identical to pattern selection.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                ),
            ];
            let forbidden_results = vec![
                multi_result(CHAIN_CONTROL, ProofState::Disproven, dispatch_evidence),
                capability_result(
                    CHAIN_MOVE,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Missing DefUse authority cannot disprove a move, borrow, lifetime, or drop change from one subject evaluation.",
                    AdapterCapability::DefUse,
                    data_graph.coverage().def_use_support(),
                    data_graph.coverage().def_use_authority(),
                ),
                capability_result(
                    CHAIN_EFFECT,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Missing Effects authority cannot disprove custom equality effects, panic, exception, or suspension changes.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                ),
                non_structured_result(CHAIN_NON_STRUCTURED, non_structured_graph),
            ];
            candidates.push(TransformationCandidate::new(
                TransformationCandidateDraft {
                    recipe: recipe.clone(),
                    source: candidate_source(analysis, projection),
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
                    expected_delta: chain_delta(graph, &dispatch_nodes, &evidence),
                    edits: vec![TransformationEdit::exact_node_replacement(
                        source.clone(),
                        target_span,
                        branch.text().into(),
                        render_match(&shape),
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

fn dead_shape<'a>(
    analysis: &'a ProjectAnalysis,
    branch: deslop_parse::NodeView<'a>,
) -> Result<Option<DeadShape<'a>>, TerminalBranchRecipeError> {
    if !eligible_if(analysis, branch)? || contains_opaque(analysis, branch)? {
        return Ok(None);
    }
    let Some(predicate) = child_by_field(analysis, branch, "condition")? else {
        return Ok(None);
    };
    if predicate.raw_grammar_kind() != "boolean_literal" {
        return Ok(None);
    }
    let Some(consequence) = child_by_field(analysis, branch, "consequence")? else {
        return Ok(None);
    };
    let Some(alternative) = child_by_field(analysis, branch, "alternative")? else {
        return Ok(None);
    };
    let Some(alternative) = else_block(analysis, alternative)? else {
        return Ok(None);
    };
    match predicate.text().trim() {
        "true" => Ok(Some(DeadShape {
            predicate,
            selected: consequence,
            selected_kind: ControlBranchKind::True,
            dead_kind: ControlBranchKind::False,
        })),
        "false" => Ok(Some(DeadShape {
            predicate,
            selected: alternative,
            selected_kind: ControlBranchKind::False,
            dead_kind: ControlBranchKind::True,
        })),
        _ => Ok(None),
    }
}

fn chain_shape<'a>(
    analysis: &'a ProjectAnalysis,
    outer: deslop_parse::NodeView<'a>,
) -> Result<Option<ChainShape<'a>>, TerminalBranchRecipeError> {
    if !eligible_if(analysis, outer)? || contains_opaque(analysis, outer)? {
        return Ok(None);
    }
    let mut cases = Vec::new();
    let mut patterns = BTreeSet::new();
    let mut subject_text = None::<String>;
    let mut current = outer;
    let fallback;
    loop {
        if cases.len() == 6 || !eligible_if(analysis, current)? {
            return Ok(None);
        }
        let Some(condition) = child_by_field(analysis, current, "condition")? else {
            return Ok(None);
        };
        let Some((subject, pattern)) = equality_case(analysis, condition)? else {
            return Ok(None);
        };
        if subject_text
            .as_ref()
            .is_some_and(|retained| retained != subject.text())
            || !patterns.insert(pattern.text().to_string())
        {
            return Ok(None);
        }
        subject_text.get_or_insert_with(|| subject.text().to_string());
        let Some(body) = child_by_field(analysis, current, "consequence")? else {
            return Ok(None);
        };
        if body.raw_grammar_kind() != "block" || body.has_error() {
            return Ok(None);
        }
        cases.push(ChainCase {
            branch: current,
            pattern,
            body,
        });
        let Some(alternative) = child_by_field(analysis, current, "alternative")? else {
            return Ok(None);
        };
        let targets = named_children(analysis, alternative)?;
        if let Some(next) = targets
            .iter()
            .copied()
            .find(|node| node.raw_grammar_kind() == "if_expression")
        {
            current = next;
            continue;
        }
        let Some(block) = targets
            .into_iter()
            .find(|node| node.raw_grammar_kind() == "block")
        else {
            return Ok(None);
        };
        fallback = block;
        break;
    }
    if cases.len() < 2 {
        return Ok(None);
    }
    Ok(Some(ChainShape {
        subject: equality_case(
            analysis,
            child_by_field(analysis, outer, "condition")?.unwrap(),
        )?
        .unwrap()
        .0,
        cases,
        fallback,
    }))
}

fn equality_case<'a>(
    analysis: &'a ProjectAnalysis,
    condition: deslop_parse::NodeView<'a>,
) -> Result<
    Option<(deslop_parse::NodeView<'a>, deslop_parse::NodeView<'a>)>,
    TerminalBranchRecipeError,
> {
    if condition.raw_grammar_kind() != "binary_expression"
        || !condition.children().any(|id| {
            analysis
                .node(id)
                .is_ok_and(|child| !child.is_named() && child.text() == "==")
        })
    {
        return Ok(None);
    }
    let Some(left) = child_by_field(analysis, condition, "left")? else {
        return Ok(None);
    };
    let Some(right) = child_by_field(analysis, condition, "right")? else {
        return Ok(None);
    };
    if left.raw_grammar_kind() == "identifier" && pattern_allowed(right) {
        return Ok(Some((left, right)));
    }
    if right.raw_grammar_kind() == "identifier" && pattern_allowed(left) {
        return Ok(Some((right, left)));
    }
    Ok(None)
}

fn pattern_allowed(node: deslop_parse::NodeView<'_>) -> bool {
    matches!(
        node.raw_grammar_kind(),
        "scoped_identifier"
            | "integer_literal"
            | "char_literal"
            | "string_literal"
            | "boolean_literal"
    )
}

fn eligible_if(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, TerminalBranchRecipeError> {
    Ok(node.grammar().lang() == Lang::Rust
        && node.raw_grammar_kind() == "if_expression"
        && !node.has_error()
        && !node.text().contains("//")
        && !node.text().contains("/*")
        && child_by_field(analysis, node, "consequence")?
            .is_some_and(|body| body.raw_grammar_kind() == "block"))
}

fn contains_opaque(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, TerminalBranchRecipeError> {
    if matches!(
        node.raw_grammar_kind(),
        "macro_invocation" | "attribute_item" | "inner_attribute_item"
    ) {
        return Ok(true);
    }
    Ok(analysis
        .descendant_node_ids(node.id())
        .map_err(|error| TerminalBranchRecipeError::Projection(error.to_string()))?
        .any(|id| {
            analysis.node(id).is_ok_and(|item| {
                matches!(
                    item.raw_grammar_kind(),
                    "macro_invocation" | "attribute_item" | "inner_attribute_item"
                )
            })
        }))
}

fn nested_else_if(
    analysis: &ProjectAnalysis,
    branch: deslop_parse::NodeView<'_>,
) -> Result<bool, TerminalBranchRecipeError> {
    let Some(parent) = branch.parent() else {
        return Ok(false);
    };
    let parent = analysis
        .node(parent)
        .map_err(|error| TerminalBranchRecipeError::Projection(error.to_string()))?;
    Ok(parent.raw_grammar_kind() == "else_clause")
}

fn else_block<'a>(
    analysis: &'a ProjectAnalysis,
    alternative: deslop_parse::NodeView<'a>,
) -> Result<Option<deslop_parse::NodeView<'a>>, TerminalBranchRecipeError> {
    if alternative.raw_grammar_kind() == "block" {
        return Ok(Some(alternative));
    }
    Ok(named_children(analysis, alternative)?
        .into_iter()
        .find(|node| node.raw_grammar_kind() == "block"))
}

fn render_match(shape: &ChainShape<'_>) -> String {
    let mut arms = shape
        .cases
        .iter()
        .map(|case| format!("{} => {}", case.pattern.text(), case.body.text()))
        .collect::<Vec<_>>();
    arms.push(format!("_ => {}", shape.fallback.text()));
    format!("match {} {{ {} }}", shape.subject.text(), arms.join(", "))
}

fn branch_layers() -> Vec<GraphEvidenceLayer> {
    vec![
        GraphEvidenceLayer::ControlFlow,
        GraphEvidenceLayer::ControlRegions,
        GraphEvidenceLayer::NonStructuredControl,
        GraphEvidenceLayer::DataFlow,
        GraphEvidenceLayer::ProgramDependence,
    ]
}

fn branch_validation(noun: &str) -> ValidationPlan {
    ValidationPlan {
        steps: vec![
            validation(
                "build",
                ValidationStepKind::Build,
                &format!("Build after the {noun}."),
            ),
            validation(
                "graph-delta",
                ValidationStepKind::GraphDelta,
                &format!("Rebuild and compare the {noun} graph delta."),
            ),
            validation(
                "parse",
                ValidationStepKind::Parse,
                "Parse the exact replacement.",
            ),
            validation(
                "test",
                ValidationStepKind::Test,
                "Run project tests before accepting the review candidate.",
            ),
        ],
    }
}

fn branch_rollback() -> RollbackPlan {
    RollbackPlan {
        strategy: RollbackStrategy::ReverseExactEdits,
        require_revision_guards: true,
        validation_steps: vec![
            "build".into(),
            "graph-delta".into(),
            "parse".into(),
            "test".into(),
        ],
    }
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

fn flow_graph<'a>(
    flow: &'a deslop_parse::ControlFlowProjection,
    graph: &ProgramDependenceGraph,
) -> Result<&'a deslop_parse::ControlFlowGraph, TerminalBranchRecipeError> {
    flow.document()
        .graphs()
        .iter()
        .find(|item| item.key() == graph.control_flow_graph())
        .ok_or_else(|| missing("control-flow graph", graph.control_flow_graph().as_str()))
}

fn data_graph<'a>(
    data: &'a deslop_parse::DataFlowProjection,
    graph: &ProgramDependenceGraph,
) -> Result<&'a deslop_parse::DataFlowGraph, TerminalBranchRecipeError> {
    data.document()
        .graphs()
        .iter()
        .find(|item| item.key() == graph.data_flow_graph())
        .ok_or_else(|| missing("data-flow graph", graph.data_flow_graph().as_str()))
}

fn branch_dispatches(
    graph: &deslop_parse::ControlFlowGraph,
) -> impl Iterator<Item = &deslop_parse::ControlPoint> {
    graph.points().iter().filter(|point| {
        point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch)
            && !point.recovered()
    })
}

fn branch_edge<'a>(
    graph: &'a deslop_parse::ControlFlowGraph,
    dispatch: &deslop_parse::ControlPointKey,
    kind: &ControlBranchKind,
) -> Option<&'a deslop_parse::ControlEdge> {
    graph.edges().iter().find(|edge| {
        edge.from() == dispatch && edge.kind() == &ControlEdgeKind::Branch(kind.clone())
    })
}

fn pdg_dispatch<'a>(
    graph: &'a ProgramDependenceGraph,
    point: &deslop_parse::ControlPointKey,
) -> Option<&'a ProgramDependenceNode> {
    graph.nodes().iter().find(|node| node.point() == point)
}

fn candidate_source(
    analysis: &ProjectAnalysis,
    projection: &ProgramDependenceProjection,
) -> CandidateSource {
    CandidateSource {
        project_snapshot: analysis.snapshot().id().as_str().into(),
        analysis: analysis.id().as_str().into(),
        program_dependence_projection: projection.id().as_str().into(),
    }
}

fn dead_delta(
    graph: &ProgramDependenceGraph,
    root: &ProgramDependenceNode,
    evidence: &DeadArmGraphEvidence,
) -> ExpectedGraphDelta {
    ExpectedGraphDelta {
        changes: vec![
            ExpectedGraphChange {
                kind: GraphChangeKind::Remove,
                entity: graph_root(graph, root),
                rationale: "The literal branch dispatch is removed after selecting one arm.".into(),
            },
            ExpectedGraphChange {
                kind: GraphChangeKind::Remove,
                entity: evidence.dead_arm.clone(),
                rationale: "The literal-infeasible arm is absent after rebuilding.".into(),
            },
            ExpectedGraphChange {
                kind: GraphChangeKind::Preserve,
                entity: evidence.selected_arm.clone(),
                rationale: "The selected arm control fragment remains byte-exact.".into(),
            },
        ],
    }
}

fn chain_delta(
    graph: &ProgramDependenceGraph,
    nodes: &[&ProgramDependenceNode],
    evidence: &ExhaustiveChainGraphEvidence,
) -> ExpectedGraphDelta {
    let mut changes = vec![ExpectedGraphChange {
        kind: GraphChangeKind::Modify,
        entity: graph_root(graph, nodes[0]),
        rationale: "The outer conditional dispatch becomes one exact match-table dispatch.".into(),
    }];
    for node in &nodes[1..] {
        changes.push(ExpectedGraphChange {
            kind: GraphChangeKind::Remove,
            entity: graph_root(graph, node),
            rationale: "A nested equality dispatch becomes one case edge on the match table."
                .into(),
        });
    }
    for point in &evidence.pst_points {
        changes.push(ExpectedGraphChange {
            kind: GraphChangeKind::Preserve,
            entity: point.clone(),
            rationale: "The rebuilt dispatch must retain equivalent reachability boundaries."
                .into(),
        });
    }
    ExpectedGraphDelta { changes }
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, TerminalBranchRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| TerminalBranchRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, TerminalBranchRecipeError> {
    node.children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| TerminalBranchRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| items.into_iter().filter(|item| item.is_named()).collect())
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

fn non_structured_result(
    condition: &str,
    graph: &deslop_parse::NonStructuredControlGraph,
) -> ConditionResult {
    result(
        condition,
        if graph.facts().is_empty() {
            ProofState::Disproven
        } else {
            ProofState::Unknown
        },
        graph_entity(
            GraphEvidenceLayer::NonStructuredControl,
            graph.key().as_str(),
            graph.key().as_str(),
        ),
        if graph.facts().is_empty() {
            "No retained non-structured-control fact participates in this callable."
        } else {
            "Retained non-structured-control facts require manual review."
        },
    )
}

fn missing(kind: &str, identity: &str) -> TerminalBranchRecipeError {
    TerminalBranchRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::{branch_graph_evidence, build_rust_recipe_projection, detect_rust_recipes};

    const SOURCE: &str = r#"
enum Mode { A, B, C }

fn dead_true() -> i32 {
    if true { 1 } else { 2 }
}

fn dead_false() -> i32 {
    if false { 1 } else { 2 }
}

fn dispatch(mode: Mode) -> i32 {
    if mode == Mode::A { 1 } else if mode == Mode::B { 2 } else { 3 }
}

fn dynamic(flag: bool) -> i32 {
    if flag { 1 } else { 2 }
}
"#;

    fn candidates(root: &std::path::Path, recipe: &str) -> Vec<TransformationCandidate> {
        detect_rust_recipes(root, &[PathBuf::from("terminal.rs")])
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.recipe().name() == recipe)
            .collect()
    }

    #[test]
    fn recipes_freeze_four_roles_and_review_authority() {
        for recipe in [
            literal_dead_arm_recipe().unwrap(),
            exhaustive_chain_to_match_recipe().unwrap(),
        ] {
            assert_eq!(recipe.fixtures().len(), 4);
            assert_eq!(recipe.maximum_safety(), SafetyClass::SafeWithPrecondition);
            assert!(
                recipe
                    .required_layers()
                    .contains(&GraphEvidenceLayer::ProgramDependence)
            );
        }
    }

    #[test]
    fn literal_dead_arms_retain_selected_blocks_and_unknown_compile_effects() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("terminal.rs"), SOURCE).unwrap();
        let found = candidates(root.path(), "rust-remove-literal-dead-arm");
        assert_eq!(found.len(), 2);
        assert_eq!(
            found
                .iter()
                .map(|candidate| candidate.edits()[0].after.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["{ 1 }", "{ 2 }"])
        );
        for candidate in found {
            assert_eq!(
                candidate.disposition(),
                CandidateDisposition::ReviewRequired
            );
            assert!(candidate.required_results().iter().any(|item| {
                item.condition == DEAD_LITERAL && item.state == ProofState::Proven
            }));
            assert!(candidate.required_results().iter().any(|item| {
                item.condition == DEAD_COMPILE && item.state == ProofState::Unknown
            }));
            assert_eq!(
                branch_graph_evidence(&candidate)
                    .unwrap()
                    .after
                    .changes
                    .len(),
                3
            );
        }
    }

    #[test]
    fn exhaustive_chain_becomes_exact_final_wildcard_match() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("terminal.rs");
        fs::write(&path, SOURCE).unwrap();
        let found = candidates(root.path(), "rust-convert-exhaustive-chain-to-match");
        assert_eq!(found.len(), 1);
        let candidate = &found[0];
        assert_eq!(
            candidate.edits()[0].after,
            "match mode { Mode::A => { 1 }, Mode::B => { 2 }, _ => { 3 } }"
        );
        assert!(candidate.required_results().iter().any(|item| {
            item.condition == CHAIN_EXHAUSTIVE && item.state == ProofState::Proven
        }));
        assert!(
            candidate.required_results().iter().any(|item| {
                item.condition == CHAIN_EQUALITY && item.state == ProofState::Unknown
            })
        );

        let edit = &candidate.edits()[0];
        let mut changed = SOURCE.to_string();
        changed.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        fs::write(&path, changed).unwrap();
        let projection = build_rust_recipe_projection(root.path(), &[PathBuf::from("terminal.rs")])
            .unwrap()
            .unwrap();
        let flow = projection.data_flow().control_regions().control_flow();
        let rebuilt = flow
            .document()
            .graphs()
            .iter()
            .find(|graph| {
                graph.owner().file().path.as_path() == std::path::Path::new("terminal.rs") && {
                    flow.analysis()
                        .node_by_key(graph.owner())
                        .is_ok_and(|node| node.text().contains("match mode"))
                }
            })
            .unwrap();
        assert_eq!(rebuilt.coverage().status(), FactCoverage::Complete);
        assert!(
            rebuilt.edges().iter().any(|edge| {
                edge.kind() == &ControlEdgeKind::Branch(ControlBranchKind::Default)
            })
        );
    }

    #[test]
    fn macro_dead_arm_and_non_exhaustive_or_duplicate_chains_abstain() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("terminal.rs"),
            "enum M { A, B }\nfn a() { if true {} else { println!(\"dead\"); } }\n\
             fn b(m: M) { if m == M::A {} else if m == M::B {} }\n\
             fn c(m: M) { if m == M::A {} else if m == M::A {} else {} }\n",
        )
        .unwrap();
        assert!(candidates(root.path(), "rust-remove-literal-dead-arm").is_empty());
        assert!(candidates(root.path(), "rust-convert-exhaustive-chain-to-match").is_empty());
    }

    #[test]
    fn candidate_wire_rejects_automatic_promotion() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("terminal.rs"), SOURCE).unwrap();
        for recipe in [
            "rust-remove-literal-dead-arm",
            "rust-convert-exhaustive-chain-to-match",
        ] {
            let candidate = candidates(root.path(), recipe).pop().unwrap();
            let value = serde_json::to_value(&candidate).unwrap();
            let decoded: TransformationCandidate = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(decoded, candidate);
            let mut stale = value;
            stale["disposition"] = serde_json::json!("automatic");
            assert!(serde_json::from_value::<TransformationCandidate>(stale).is_err());
        }
    }
}
