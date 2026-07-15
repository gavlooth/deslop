use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    DataFlowAccessKind, FactCoverage, GraphEligibilityDecision, GraphEvidenceLayer,
    ProgramDependenceGraph, ProgramDependenceNode, ProgramDependenceProjection, ProjectAnalysis,
    evaluate_program_graph_recipe_eligibility,
};

use crate::branch::{condition, fixture, graph_entity, span};
use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactDirection, ImpactQueryError, ProofState, RecipeContractError, RecipeFixtureRole,
    RollbackPlan, RollbackStrategy, TransformationCandidate, TransformationCandidateDraft,
    TransformationEdit, TransformationFamily, TransformationRecipe, TransformationRecipeDraft,
    ValidationPlan, ValidationStep, ValidationStepKind, program_dependence_impact_cone,
};

const DEF_USE: &str = "complete-exact-def-use-frontier";
const EFFECTS: &str = "complete-empty-effect-frontier";
const SINGLE_USE: &str = "adjacent-single-read-use";
const CLOSED: &str = "closed-primitive-initializer";
const NO_REFS: &str = "complete-empty-reference-set";
const LITERAL: &str = "direct-unused-literal-expression";
const LOCATION: &str = "diagnostic-and-panic-location-reviewed";
const PARTIAL: &str = "partial-or-uncertain-semantic-frontier";
const EFFECTFUL: &str = "call-memory-drop-or-control-effect";
const BAD_SHAPE: &str = "typed-mutable-pattern-macro-or-recovered-shape";

#[derive(Debug, thiserror::Error)]
pub enum LocalCleanupRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("local-cleanup graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("local-cleanup projection is inconsistent: {0}")]
    Projection(String),
}

#[derive(Clone, Copy)]
enum CleanupKind {
    InlineTemporary,
    RemoveExpression,
    RemoveDeadLocal,
}

pub fn inline_single_use_temporary_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    cleanup_recipe(CleanupKind::InlineTemporary)
}

pub fn remove_unused_pure_expression_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    cleanup_recipe(CleanupKind::RemoveExpression)
}

pub fn remove_independent_dead_local_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    cleanup_recipe(CleanupKind::RemoveDeadLocal)
}

pub fn detect_local_cleanup_candidates(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, LocalCleanupRecipeError> {
    let temporary = inline_single_use_temporary_recipe()?;
    let expression = remove_unused_pure_expression_recipe()?;
    let dead_local = remove_independent_dead_local_recipe()?;
    let data_flow = projection.data_flow();
    let analysis = data_flow.control_regions().control_flow().analysis();
    let scope = data_flow.resolution().scope_graph();
    let mut candidates = Vec::new();

    for graph in projection.document().graphs() {
        let data = data_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.data_flow_graph())
            .ok_or_else(|| missing("data-flow graph", graph.data_flow_graph().as_str()))?;
        if graph.coverage().status() != FactCoverage::Complete
            || !graph.gaps().is_empty()
            || data.coverage().status() != FactCoverage::Complete
            || data
                .effects()
                .iter()
                .any(|item| item.uncertainty().is_some())
            || data
                .accesses()
                .iter()
                .any(|item| item.uncertainty().is_some())
        {
            continue;
        }
        let temporary_eligibility = eligibility(projection, graph, &temporary)?;
        let expression_eligibility = eligibility(projection, graph, &expression)?;
        let dead_local_eligibility = eligibility(projection, graph, &dead_local)?;
        if !temporary_eligibility.eligible()
            || !expression_eligibility.eligible()
            || !dead_local_eligibility.eligible()
        {
            continue;
        }

        for root in graph.nodes().iter().filter(|node| node.reachable()) {
            let Some(source) = root.source() else {
                continue;
            };
            let view = analysis
                .node_by_key(source)
                .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))?;
            if view.grammar().lang() != Lang::Rust || view.has_error() {
                continue;
            }

            if direct_literal_statement(analysis, view)? && empty_point(data, root.point(), false) {
                candidates.push(removal_candidate(
                    projection,
                    graph,
                    root,
                    data,
                    view,
                    expression.clone(),
                    expression_eligibility.clone(),
                    LITERAL,
                    "The direct reachable literal statement has no retained semantic output.",
                )?);
            }

            let Some(local) = simple_local(analysis, view)? else {
                continue;
            };
            let definitions = data
                .definitions()
                .iter()
                .filter(|definition| definition.point() == root.point())
                .filter(|definition| {
                    scope
                        .facts()
                        .iter()
                        .find(|fact| fact.key() == definition.source_fact())
                        .and_then(|fact| analysis.node_key(fact.node()).ok())
                        == Some(local.pattern.key())
                })
                .collect::<Vec<_>>();
            if definitions.len() != 1 || !empty_point(data, root.point(), true) {
                continue;
            }
            let definition = definitions[0];
            let accesses = data
                .accesses()
                .iter()
                .filter(|access| access.symbol() == Some(definition.symbol()))
                .collect::<Vec<_>>();

            if accesses.is_empty() && is_closed_literal(analysis, local.value)? {
                candidates.push(removal_candidate(
                    projection,
                    graph,
                    root,
                    data,
                    local.statement,
                    dead_local.clone(),
                    dead_local_eligibility.clone(),
                    NO_REFS,
                    "The exact local definition has no retained symbol access.",
                )?);
            }

            if accesses.len() != 1 || !is_closed_expression(analysis, local.value)? {
                continue;
            }
            let access = accesses[0];
            if access.kind() != DataFlowAccessKind::Read
                || access.reaching_definitions() != [definition.key().clone()]
            {
                continue;
            }
            let reference = scope
                .facts()
                .iter()
                .find(|fact| fact.key() == access.reference())
                .ok_or_else(|| missing("reference fact", access.reference().as_str()))?;
            let use_node = analysis
                .node(reference.node())
                .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))?;
            let Some(use_statement) = direct_body_child(analysis, use_node)? else {
                continue;
            };
            if use_node.raw_grammar_kind() != "identifier"
                || !adjacent(analysis, local.statement, use_statement)?
                || forbidden_use_ancestor(analysis, use_node, use_statement)?
            {
                continue;
            }
            let use_root = graph
                .nodes()
                .iter()
                .find(|node| node.point() == access.point())
                .ok_or_else(|| missing("PDG use node", access.point().as_str()))?;
            candidates.push(inline_candidate(
                projection,
                graph,
                root,
                use_root,
                data,
                definition,
                access,
                local,
                use_node,
                temporary.clone(),
                temporary_eligibility.clone(),
            )?);
        }
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

#[derive(Clone, Copy)]
struct LocalShape<'a> {
    statement: deslop_parse::NodeView<'a>,
    pattern: deslop_parse::NodeView<'a>,
    value: deslop_parse::NodeView<'a>,
}

#[allow(clippy::too_many_arguments)]
fn inline_candidate(
    projection: &ProgramDependenceProjection,
    graph: &ProgramDependenceGraph,
    definition_root: &ProgramDependenceNode,
    use_root: &ProgramDependenceNode,
    data: &deslop_parse::DataFlowGraph,
    definition: &deslop_parse::DataFlowDefinition,
    access: &deslop_parse::DataFlowAccess,
    local: LocalShape<'_>,
    use_node: deslop_parse::NodeView<'_>,
    recipe: TransformationRecipe,
    eligibility: GraphEligibilityDecision,
) -> Result<TransformationCandidate, LocalCleanupRecipeError> {
    let analysis = projection
        .data_flow()
        .control_regions()
        .control_flow()
        .analysis();
    let target = pdg_entity(graph, use_root);
    let definition_entity = data_entity(data, definition.key().as_str());
    let access_entity = data_entity(data, access.key().as_str());
    Ok(TransformationCandidate::new(
        TransformationCandidateDraft {
            recipe: recipe.clone(),
            source: candidate_source(projection, analysis),
            target: CandidateTarget {
                entity: target.clone(),
                node: use_node.key().clone(),
                span: span(use_node.key()),
            },
            eligibility,
            required_results: vec![
                proven(
                    DEF_USE,
                    access_entity.clone(),
                    "The sole Read has exactly this reaching definition.",
                ),
                proven(
                    EFFECTS,
                    data_entity(data, definition_root.data_flow_point().as_str()),
                    "The initializer point has no other access, boundary, or effect.",
                ),
                proven(
                    SINGLE_USE,
                    control_entity(graph, use_root.point()),
                    "The sole use is in the immediately following body statement.",
                ),
                proven(
                    CLOSED,
                    control_entity(graph, definition_root.point()),
                    "The initializer is a closed primitive expression.",
                ),
                result(
                    LOCATION,
                    ProofState::Unknown,
                    control_entity(graph, use_root.point()),
                    "Diagnostic and panic source locations require review.",
                ),
            ],
            forbidden_results: disproven_common(graph, data, definition_root),
            impact: program_dependence_impact_cone(
                projection,
                graph.key(),
                use_root.key(),
                ImpactDirection::Bidirectional,
                8,
            )?,
            expected_delta: ExpectedGraphDelta {
                changes: vec![
                    change(
                        GraphChangeKind::Remove,
                        definition_entity,
                        "Remove the temporary definition.",
                    ),
                    change(
                        GraphChangeKind::Remove,
                        access_entity,
                        "Replace the temporary symbol read.",
                    ),
                    change(
                        GraphChangeKind::Modify,
                        target,
                        "Insert the exact parenthesized initializer.",
                    ),
                ],
            },
            edits: vec![
                TransformationEdit::exact_node_deletion(
                    local.statement.key().clone(),
                    span(local.statement.key()),
                    local.statement.text().into(),
                ),
                TransformationEdit::exact_node_replacement(
                    use_node.key().clone(),
                    span(use_node.key()),
                    use_node.text().into(),
                    format!("({})", local.value.text()),
                ),
            ],
            safety: SafetyClass::SafeWithPrecondition,
            disposition: CandidateDisposition::ReviewRequired,
            validation_plan: recipe.validation_plan().clone(),
            rollback_plan: recipe.rollback_plan().clone(),
        },
    )?)
}

#[allow(clippy::too_many_arguments)]
fn removal_candidate(
    projection: &ProgramDependenceProjection,
    graph: &ProgramDependenceGraph,
    root: &ProgramDependenceNode,
    data: &deslop_parse::DataFlowGraph,
    statement: deslop_parse::NodeView<'_>,
    recipe: TransformationRecipe,
    eligibility: GraphEligibilityDecision,
    shape_condition: &str,
    shape_detail: &str,
) -> Result<TransformationCandidate, LocalCleanupRecipeError> {
    let analysis = projection
        .data_flow()
        .control_regions()
        .control_flow()
        .analysis();
    let target = pdg_entity(graph, root);
    let data_point = data_entity(data, root.data_flow_point().as_str());
    let control_point = control_entity(graph, root.point());
    let mut required = vec![
        proven(
            DEF_USE,
            data_point.clone(),
            "Complete exact def/use closes this point.",
        ),
        proven(
            EFFECTS,
            data_point,
            "Complete effects retain no boundary or effect at this point.",
        ),
        proven(
            shape_condition,
            if shape_condition == NO_REFS {
                data_entity(data, root.data_flow_point().as_str())
            } else {
                control_point.clone()
            },
            shape_detail,
        ),
    ];
    if shape_condition == NO_REFS {
        required.push(proven(
            CLOSED,
            control_point,
            "The initializer is one closed primitive literal.",
        ));
    }
    Ok(TransformationCandidate::new(
        TransformationCandidateDraft {
            recipe: recipe.clone(),
            source: candidate_source(projection, analysis),
            target: CandidateTarget {
                entity: target.clone(),
                node: statement.key().clone(),
                span: span(statement.key()),
            },
            eligibility,
            required_results: required,
            forbidden_results: disproven_common(graph, data, root),
            impact: program_dependence_impact_cone(
                projection,
                graph.key(),
                root.key(),
                ImpactDirection::Bidirectional,
                4,
            )?,
            expected_delta: ExpectedGraphDelta {
                changes: vec![change(
                    GraphChangeKind::Remove,
                    target,
                    "Remove the independent semantically empty statement.",
                )],
            },
            edits: vec![TransformationEdit::exact_node_deletion(
                statement.key().clone(),
                span(statement.key()),
                statement.text().into(),
            )],
            safety: SafetyClass::SafeAuto,
            disposition: CandidateDisposition::Automatic,
            validation_plan: recipe.validation_plan().clone(),
            rollback_plan: recipe.rollback_plan().clone(),
        },
    )?)
}

fn simple_local<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Option<LocalShape<'a>>, LocalCleanupRecipeError> {
    if node.raw_grammar_kind() != "let_declaration"
        || !direct_block_child(analysis, node)
        || bad_syntax(analysis, node)?
        || field(analysis, node, "type")?.is_some()
        || field(analysis, node, "alternative")?.is_some()
    {
        return Ok(None);
    }
    let (Some(pattern), Some(value)) = (
        field(analysis, node, "pattern")?,
        field(analysis, node, "value")?,
    ) else {
        return Ok(None);
    };
    if pattern.raw_grammar_kind() != "identifier" {
        return Ok(None);
    }
    Ok(Some(LocalShape {
        statement: node,
        pattern,
        value,
    }))
}

fn direct_literal_statement(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, LocalCleanupRecipeError> {
    if node.raw_grammar_kind() != "expression_statement"
        || !direct_block_child(analysis, node)
        || bad_syntax(analysis, node)?
    {
        return Ok(false);
    }
    let children = named_children(analysis, node)?;
    Ok(children.len() == 1 && is_closed_literal(analysis, children[0])?)
}

fn is_closed_literal(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, LocalCleanupRecipeError> {
    if matches!(
        node.raw_grammar_kind(),
        "integer_literal"
            | "float_literal"
            | "boolean_literal"
            | "char_literal"
            | "string_literal"
            | "raw_string_literal"
    ) {
        return Ok(!node.has_error());
    }
    if !matches!(
        node.raw_grammar_kind(),
        "parenthesized_expression" | "unary_expression"
    ) || bad_syntax(analysis, node)?
    {
        return Ok(false);
    }
    let children = named_children(analysis, node)?;
    Ok(children.len() == 1 && is_closed_literal(analysis, children[0])?)
}

fn is_closed_expression(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, LocalCleanupRecipeError> {
    if is_closed_literal(analysis, node)? {
        return Ok(true);
    }
    if node.raw_grammar_kind() != "binary_expression" || bad_syntax(analysis, node)? {
        return Ok(false);
    }
    let children = named_children(analysis, node)?;
    Ok(children.len() == 2
        && is_closed_expression(analysis, children[0])?
        && is_closed_expression(analysis, children[1])?)
}

fn empty_point(
    data: &deslop_parse::DataFlowGraph,
    point: &deslop_parse::ControlPointKey,
    allow_one_definition: bool,
) -> bool {
    data.definitions()
        .iter()
        .filter(|item| item.point() == point)
        .count()
        == usize::from(allow_one_definition)
        && data.accesses().iter().all(|item| item.point() != point)
        && data.boundaries().iter().all(|item| item.point() != point)
        && data
            .effects()
            .iter()
            .find(|item| item.point() == point)
            .is_some_and(|item| item.uncertainty().is_none() && item.effects().is_empty())
}

fn direct_block_child(analysis: &ProjectAnalysis, node: deslop_parse::NodeView<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        analysis
            .node(parent)
            .is_ok_and(|parent| parent.raw_grammar_kind() == "block")
    })
}

fn direct_body_child<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Option<deslop_parse::NodeView<'a>>, LocalCleanupRecipeError> {
    let mut current = node;
    loop {
        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        let parent = analysis
            .node(parent)
            .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))?;
        if parent.raw_grammar_kind() == "block" {
            return Ok(Some(current));
        }
        if parent.raw_grammar_kind() == "function_item" {
            return Ok(None);
        }
        current = parent;
    }
}

fn adjacent(
    analysis: &ProjectAnalysis,
    definition: deslop_parse::NodeView<'_>,
    usage: deslop_parse::NodeView<'_>,
) -> Result<bool, LocalCleanupRecipeError> {
    if definition.parent() != usage.parent() {
        return Ok(false);
    }
    let Some(body) = definition.parent().and_then(|id| analysis.node(id).ok()) else {
        return Ok(false);
    };
    Ok(named_children(analysis, body)?
        .windows(2)
        .any(|pair| pair[0].id() == definition.id() && pair[1].id() == usage.id()))
}

fn forbidden_use_ancestor(
    analysis: &ProjectAnalysis,
    use_node: deslop_parse::NodeView<'_>,
    statement: deslop_parse::NodeView<'_>,
) -> Result<bool, LocalCleanupRecipeError> {
    let mut current = use_node;
    while current.id() != statement.id() {
        if matches!(
            current.raw_grammar_kind(),
            "macro_invocation"
                | "closure_expression"
                | "async_block"
                | "unsafe_block"
                | "attribute_item"
                | "inner_attribute_item"
        ) {
            return Ok(true);
        }
        let Some(parent) = current.parent() else {
            return Ok(true);
        };
        current = analysis
            .node(parent)
            .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))?;
    }
    bad_syntax(analysis, statement)
}

fn bad_syntax(
    analysis: &ProjectAnalysis,
    node: deslop_parse::NodeView<'_>,
) -> Result<bool, LocalCleanupRecipeError> {
    if node.has_error() || matches!(node.raw_grammar_kind(), "line_comment" | "block_comment") {
        return Ok(true);
    }
    Ok(analysis
        .descendant_node_ids(node.id())
        .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))?
        .any(|id| {
            analysis.node(id).is_ok_and(|child| {
                child.has_error()
                    || matches!(child.raw_grammar_kind(), "line_comment" | "block_comment")
            })
        }))
}

fn field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    name: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, LocalCleanupRecipeError> {
    for child in node.children() {
        let child = analysis
            .node(child)
            .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))?;
        if child.field() == Some(name) {
            return Ok(Some(child));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, LocalCleanupRecipeError> {
    node.children()
        .map(|id| {
            analysis
                .node(id)
                .map_err(|error| LocalCleanupRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|nodes| nodes.into_iter().filter(|node| node.is_named()).collect())
}

fn cleanup_recipe(kind: CleanupKind) -> Result<TransformationRecipe, RecipeContractError> {
    let (name, maximum_safety, required_conditions, fixtures) = match kind {
        CleanupKind::InlineTemporary => (
            "rust-inline-exact-single-use-temporary",
            SafetyClass::SafeWithPrecondition,
            vec![
                req(
                    DEF_USE,
                    "Complete def/use proves one definition and its only reaching read.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    EFFECTS,
                    "The initializer point has a complete empty effect frontier.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    SINGLE_USE,
                    "The sole use is in the immediately following direct-body statement.",
                    GraphEvidenceLayer::ControlFlow,
                ),
                req(
                    CLOSED,
                    "The immutable untyped initializer is a closed primitive expression.",
                    GraphEvidenceLayer::ControlFlow,
                ),
                req(
                    LOCATION,
                    "Diagnostic and panic source locations require review.",
                    GraphEvidenceLayer::ControlFlow,
                ),
            ],
            fixtures_for(kind),
        ),
        CleanupKind::RemoveExpression => (
            "rust-remove-unused-pure-literal-expression",
            SafetyClass::SafeAuto,
            vec![
                req(
                    DEF_USE,
                    "Complete def/use retains no definition or access at the expression.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    EFFECTS,
                    "Complete effects retain no boundary or effect at the expression.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    LITERAL,
                    "The reachable direct-body statement contains only a literal.",
                    GraphEvidenceLayer::ControlFlow,
                ),
            ],
            fixtures_for(kind),
        ),
        CleanupKind::RemoveDeadLocal => (
            "rust-remove-independent-unused-literal-local",
            SafetyClass::SafeAuto,
            vec![
                req(
                    DEF_USE,
                    "Complete def/use retains one exact local definition.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    NO_REFS,
                    "The local symbol has no retained access.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    EFFECTS,
                    "The literal initializer has an empty effect frontier.",
                    GraphEvidenceLayer::DataFlow,
                ),
                req(
                    CLOSED,
                    "The immutable untyped initializer is one closed literal.",
                    GraphEvidenceLayer::ControlFlow,
                ),
            ],
            fixtures_for(kind),
        ),
    };
    TransformationRecipe::new(TransformationRecipeDraft {
        name: name.into(),
        version: "1.0.0".into(),
        family: TransformationFamily::FunctionExpression,
        required_layers: vec![
            GraphEvidenceLayer::ControlFlow,
            GraphEvidenceLayer::ControlRegions,
            GraphEvidenceLayer::NonStructuredControl,
            GraphEvidenceLayer::DataFlow,
            GraphEvidenceLayer::ProgramDependence,
        ],
        required_conditions,
        forbidden_conditions: vec![
            condition(
                PARTIAL,
                "Partial or uncertain DefUse, Effects, or LocalPdg blocks cleanup.",
                GraphEvidenceLayer::ProgramDependence,
            ),
            condition(
                EFFECTFUL,
                "A call, memory, allocation, drop, or control effect blocks cleanup.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                BAD_SHAPE,
                "Typed, mutable, destructuring, macro, comment, or recovered syntax blocks cleanup.",
                GraphEvidenceLayer::ControlFlow,
            ),
        ],
        maximum_safety,
        validation_plan: validation_plan(),
        rollback_plan: rollback_plan(),
        fixtures,
    })
}

fn fixtures_for(kind: CleanupKind) -> Vec<crate::RecipeFixture> {
    let (positive, no_op, minimal, adversarial) = match kind {
        CleanupKind::InlineTemporary => (
            (
                "adjacent-single-use",
                "One exact local reaches one adjacent read.",
            ),
            ("two-reads", "A second read keeps the binding live."),
            (
                "intervening-statement",
                "Evaluation cannot move across another statement.",
            ),
            (
                "foreign-reaching-definition",
                "Spelling cannot replace exact definition identity.",
            ),
        ),
        CleanupKind::RemoveExpression => (
            (
                "unused-literal",
                "A literal statement has no retained semantic output.",
            ),
            ("call-statement", "A call is not a removable literal."),
            (
                "operator-statement",
                "Panic-capable operators are not automatically deleted.",
            ),
            ("partial-effects", "Missing effects cannot prove purity."),
        ),
        CleanupKind::RemoveDeadLocal => (
            (
                "unused-literal-local",
                "A literal local has no retained use or effect.",
            ),
            ("live-local", "A read keeps the definition live."),
            (
                "call-initializer",
                "An unused call result does not make the call removable.",
            ),
            (
                "typed-pattern",
                "Type and pattern boundaries need stronger authority.",
            ),
        ),
    };
    vec![
        fixture(
            RecipeFixtureRole::Positive,
            positive.0,
            FixtureExpectation::Candidate,
            positive.1,
        ),
        fixture(
            RecipeFixtureRole::NoOp,
            no_op.0,
            FixtureExpectation::NoCandidate,
            no_op.1,
        ),
        fixture(
            RecipeFixtureRole::MinimalCounterexample,
            minimal.0,
            FixtureExpectation::NoCandidate,
            minimal.1,
        ),
        fixture(
            RecipeFixtureRole::AdversarialNearMiss,
            adversarial.0,
            FixtureExpectation::NoCandidate,
            adversarial.1,
        ),
    ]
}

fn eligibility(
    projection: &ProgramDependenceProjection,
    graph: &ProgramDependenceGraph,
    recipe: &TransformationRecipe,
) -> Result<GraphEligibilityDecision, LocalCleanupRecipeError> {
    evaluate_program_graph_recipe_eligibility(projection, graph, &recipe.eligibility_requirement())
        .map_err(|error| LocalCleanupRecipeError::Eligibility(error.to_string()))
}

fn req(key: &str, description: &str, layer: GraphEvidenceLayer) -> crate::RecipeCondition {
    condition(key, description, layer)
}

fn validation_plan() -> ValidationPlan {
    ValidationPlan {
        steps: vec![
            validation(
                "build",
                ValidationStepKind::Build,
                "Build the exact local cleanup.",
            ),
            validation(
                "graph-delta",
                ValidationStepKind::GraphDelta,
                "Verify the expected def/use/effect delta.",
            ),
            validation(
                "parse",
                ValidationStepKind::Parse,
                "Parse the exact edit transaction.",
            ),
            validation(
                "test",
                ValidationStepKind::Test,
                "Run project tests after cleanup.",
            ),
        ],
    }
}

fn rollback_plan() -> RollbackPlan {
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

fn candidate_source(
    projection: &ProgramDependenceProjection,
    analysis: &ProjectAnalysis,
) -> CandidateSource {
    CandidateSource {
        project_snapshot: analysis.snapshot().id().as_str().into(),
        analysis: analysis.id().as_str().into(),
        program_dependence_projection: projection.id().as_str().into(),
    }
}

fn pdg_entity(graph: &ProgramDependenceGraph, node: &ProgramDependenceNode) -> GraphEntityRef {
    graph_entity(
        GraphEvidenceLayer::ProgramDependence,
        graph.key().as_str(),
        node.key().as_str(),
    )
}

fn data_entity(data: &deslop_parse::DataFlowGraph, key: &str) -> GraphEntityRef {
    graph_entity(GraphEvidenceLayer::DataFlow, data.key().as_str(), key)
}

fn control_entity(
    graph: &ProgramDependenceGraph,
    point: &deslop_parse::ControlPointKey,
) -> GraphEntityRef {
    graph_entity(
        GraphEvidenceLayer::ControlFlow,
        graph.control_flow_graph().as_str(),
        point.as_str(),
    )
}

fn proven(condition: &str, entity: GraphEntityRef, detail: &str) -> ConditionResult {
    result(condition, ProofState::Proven, entity, detail)
}

fn result(
    condition: &str,
    state: ProofState,
    entity: GraphEntityRef,
    detail: &str,
) -> ConditionResult {
    ConditionResult {
        condition: condition.into(),
        state,
        evidence: vec![ConditionEvidence {
            entity,
            detail: detail.into(),
            capability: None,
            support: None,
            authority: None,
        }],
    }
}

fn disproven_common(
    graph: &ProgramDependenceGraph,
    data: &deslop_parse::DataFlowGraph,
    root: &ProgramDependenceNode,
) -> Vec<ConditionResult> {
    vec![
        result(
            PARTIAL,
            ProofState::Disproven,
            pdg_entity(graph, root),
            "Local PDG/data coverage is Complete with no gap or uncertainty.",
        ),
        result(
            EFFECTFUL,
            ProofState::Disproven,
            data_entity(data, data.key().as_str()),
            "The selected point has the required empty effect frontier.",
        ),
        result(
            BAD_SHAPE,
            ProofState::Disproven,
            control_entity(graph, root.point()),
            "The exact CST is direct, untyped, immutable, comment-free, and unrecovered.",
        ),
    ]
}

fn change(kind: GraphChangeKind, entity: GraphEntityRef, rationale: &str) -> ExpectedGraphChange {
    ExpectedGraphChange {
        kind,
        entity,
        rationale: rationale.into(),
    }
}

fn missing(kind: &str, identity: &str) -> LocalCleanupRecipeError {
    LocalCleanupRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::process::Command;
    use std::sync::Arc;

    use deslop_lang::Registry;
    use deslop_parse::{
        BindingDraft, BindingForm, BindingTargetDraft, BuildContextId, ControlEdgeDraft,
        ControlEdgeKind, ControlEdgePrecision, ControlExitOutcome, ControlFlowBuilder,
        ControlFlowCoverageEvidence, ControlFlowGraphDraft, ControlFlowOwnerKind,
        ControlFlowPolicyId, ControlPointDraft, ControlPointKind, ControlSyntheticPointKind,
        DataFlowAccessDraft, DataFlowBuilder, DataFlowDefinitionDraft, DataFlowEffectDraft,
        DataFlowGraphDraft, DataFlowPolicyId, DeclarationDraft, FactCoverageEvidence, Mutability,
        NameNamespace, NamespacePolicy, NonStructuredControlPolicyId, ProgramDependencePolicyId,
        ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft, ReferenceRole, RepositoryId,
        ResolutionPolicyId, ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind,
        VisibilityDraft, VisibilityKind, derive_control_regions,
        derive_non_structured_control_regions, derive_program_dependence,
    };

    use super::*;
    use crate::inline_helper::tests::INLINE_TEST_PACK;

    struct Fixture {
        source: String,
        projection: deslop_parse::ProgramDependenceProjection,
    }

    fn fixture() -> Fixture {
        let source = "fn run() -> i32 {\n\
                      \x20   let temporary = 1 + 2;\n\
                      \x20   let result = temporary * 3;\n\
                      \x20   99;\n\
                      \x20   let unused = 7;\n\
                      \x20   let live = 5;\n\
                      \x20   let first = live;\n\
                      \x20   let second = live;\n\
                      \x20   let typed: i32 = 11;\n\
                      \x20   1 + 2;\n\
                      \x20   result + first + second\n\
                      }\n\
                      fn main() { println!(\"{}\", run()); }\n"
            .to_string();
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::default();
        registry.register(&INLINE_TEST_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("local-cleanup-test").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("fixture.inliners", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let nodes = |kind: &str| {
            analysis
                .node_ids()
                .filter(|id| analysis.node(*id).unwrap().raw_grammar_kind() == kind)
                .collect::<Vec<_>>()
        };
        let run = nodes("function_item")[0];
        let body = nodes("block")[0];
        let source_root = nodes("source_file")[0];
        let body_children = named_children(&analysis, analysis.node(body).unwrap()).unwrap();
        assert_eq!(body_children.len(), 10);
        let roles = |node| {
            let path = analysis.node(node).unwrap().path().to_path_buf();
            analysis
                .canonical_role_projection(&path)
                .unwrap()
                .facts()
                .iter()
                .find(|fact| fact.node() == node)
                .unwrap()
                .roles()
        };
        let complete = FactCoverageEvidence::complete();
        let namespaces = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let mut scopes = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"local-cleanup-build"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"local-cleanup-scope"]).unwrap(),
        )
        .unwrap();
        let file_scope = scopes
            .add_scope(
                source_root,
                roles(source_root),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let run_scope = scopes
            .add_scope(
                run,
                roles(run),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();

        let names = [
            "temporary",
            "result",
            "unused",
            "live",
            "first",
            "second",
            "typed",
        ];
        let mut declaration_ids = BTreeMap::new();
        let mut binding_ids = BTreeMap::new();
        let mut reference_ids = BTreeMap::<String, Vec<_>>::new();
        for name in names {
            let mut identifiers = analysis
                .node_ids()
                .filter(|id| {
                    let node = analysis.node(*id).unwrap();
                    node.raw_grammar_kind() == "identifier" && node.text() == name
                })
                .collect::<Vec<_>>();
            identifiers.sort_by_key(|id| analysis.node(*id).unwrap().span().start_byte());
            let pattern = identifiers[0];
            let declaration = scopes
                .add_declaration(
                    pattern,
                    roles(pattern),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: name.into(),
                        lookup_key: name.into(),
                        namespace: NameNamespace::Value,
                        scope: run_scope,
                        visibility: VisibilityDraft {
                            kind: VisibilityKind::Scope,
                            boundary: Some(run_scope),
                            adapter_rule: None,
                        },
                        modifiers: vec![],
                    },
                )
                .unwrap();
            let binding = scopes
                .add_binding(
                    pattern,
                    roles(pattern),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(declaration),
                        form: BindingForm::Declaration,
                        timing: deslop_parse::BindingTiming::AfterInitializer,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            declaration_ids.insert(name.to_string(), declaration);
            binding_ids.insert(name.to_string(), binding);
            for identifier in identifiers.into_iter().skip(1) {
                let reference = scopes
                    .add_reference(
                        identifier,
                        roles(identifier),
                        complete.clone(),
                        ReferenceDraft {
                            original_spelling: name.into(),
                            segments: vec![name.into()],
                            namespace: NameNamespace::Value,
                            scope: run_scope,
                            role: ReferenceRole::Read,
                        },
                    )
                    .unwrap();
                reference_ids
                    .entry(name.into())
                    .or_default()
                    .push((identifier, reference));
            }
        }
        let scopes = Arc::new(scopes.build().unwrap());
        let fact_key = |id| scopes.fact(id).unwrap().key().clone();
        let declaration_keys = declaration_ids
            .into_iter()
            .map(|(name, id)| (name, fact_key(id)))
            .collect::<BTreeMap<_, _>>();
        let binding_keys = binding_ids
            .into_iter()
            .map(|(name, id)| (name, fact_key(id)))
            .collect::<BTreeMap<_, _>>();
        let reference_keys = reference_ids
            .into_iter()
            .map(|(name, values)| {
                (
                    name,
                    values
                        .into_iter()
                        .map(|(node, id)| (node, fact_key(id)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let resolution = Arc::new(
            deslop_parse::ResolutionProjection::build(
                scopes,
                ResolutionPolicyId::from_parts(&[b"local-cleanup-resolution"]).unwrap(),
            )
            .unwrap(),
        );
        assert!(resolution.results().iter().all(|result| {
            result.wire().status() == deslop_parse::ResolutionStatus::Unique
                && result.wire().coverage().status() == FactCoverage::Complete
        }));

        let mut flow = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"local-cleanup-flow"]).unwrap(),
        );
        let mut points = vec![ControlPointDraft {
            kind: ControlPointKind::Entry,
            source: None,
            ordinal: 0,
        }];
        points.extend(
            body_children
                .iter()
                .enumerate()
                .map(|(ordinal, node)| ControlPointDraft {
                    kind: ControlPointKind::Syntax,
                    source: Some(node.id()),
                    ordinal: ordinal as u32,
                }),
        );
        points.push(ControlPointDraft {
            kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
            source: Some(body),
            ordinal: 0,
        });
        points.push(ControlPointDraft {
            kind: ControlPointKind::Exit,
            source: None,
            ordinal: 0,
        });
        let edges = (0..points.len() - 1)
            .map(|from| ControlEdgeDraft {
                from,
                to: from + 1,
                kind: if from == 0 {
                    ControlEdgeKind::Entry
                } else if from + 1 == points.len() - 1 {
                    ControlEdgeKind::Exit(ControlExitOutcome::Normal)
                } else {
                    ControlEdgeKind::Normal
                },
                source: run,
                predicate: None,
                precision: ControlEdgePrecision::Exact,
            })
            .collect();
        flow.add_graph(ControlFlowGraphDraft {
            owner: run,
            owner_kind: ControlFlowOwnerKind::Callable,
            coverage: ControlFlowCoverageEvidence::complete(),
            points,
            edges,
        })
        .unwrap();
        let flow = Arc::new(flow.build().unwrap());
        let flow_graph = &flow.document().graphs()[0];
        let point_for_node = |node| {
            let statement = direct_body_child(&analysis, analysis.node(node).unwrap())
                .unwrap()
                .unwrap();
            flow_graph
                .points()
                .iter()
                .find(|point| point.source() == Some(statement.key()))
                .unwrap()
                .key()
                .clone()
        };
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                deslop_parse::ControlRegionPolicyId::from_parts(&[b"local-cleanup-regions"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let mut data = DataFlowBuilder::new(
            Arc::clone(&regions),
            resolution,
            DataFlowPolicyId::from_parts(&[b"local-cleanup-data"]).unwrap(),
        )
        .unwrap();
        let definitions = names
            .iter()
            .map(|name| {
                let pattern = analysis
                    .node_ids()
                    .find(|id| {
                        let node = analysis.node(*id).unwrap();
                        node.raw_grammar_kind() == "identifier"
                            && node.text() == *name
                            && field(
                                &analysis,
                                analysis.node(node.parent().unwrap()).unwrap(),
                                "pattern",
                            )
                            .ok()
                            .flatten()
                            .is_some_and(|candidate| candidate.id() == *id)
                    })
                    .unwrap();
                DataFlowDefinitionDraft {
                    point: point_for_node(pattern),
                    declaration: declaration_keys[*name].clone(),
                    source_fact: binding_keys[*name].clone(),
                    ordinal: 0,
                }
            })
            .collect::<Vec<_>>();
        let mut accesses = Vec::new();
        let mut access_ordinals = definitions
            .iter()
            .map(|definition| (definition.point.clone(), 1_u32))
            .collect::<BTreeMap<_, _>>();
        for (name, references) in &reference_keys {
            for (node, reference) in references {
                let point = point_for_node(*node);
                let ordinal = access_ordinals.entry(point.clone()).or_insert(0_u32);
                accesses.push(DataFlowAccessDraft {
                    point,
                    reference: reference.clone(),
                    kind: DataFlowAccessKind::Read,
                    ordinal: *ordinal,
                });
                *ordinal += 1;
            }
            assert_ne!(name, "unused");
        }
        data.add_graph(DataFlowGraphDraft {
            control_flow_graph: flow_graph.key().clone(),
            definitions,
            accesses,
            boundaries: vec![],
            effects: flow_graph
                .points()
                .iter()
                .map(|point| DataFlowEffectDraft {
                    point: point.key().clone(),
                    effects: vec![],
                    uncertainty: None,
                })
                .collect(),
        })
        .unwrap();
        let data = Arc::new(data.build().unwrap());
        let non_structured = Arc::new(
            derive_non_structured_control_regions(
                regions,
                NonStructuredControlPolicyId::from_parts(&[b"local-cleanup-non-structured"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let projection = derive_program_dependence(
            data,
            non_structured,
            ProgramDependencePolicyId::from_parts(&[b"local-cleanup-pdg"]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            projection.document().graphs()[0].coverage().status(),
            FactCoverage::Complete
        );
        Fixture { source, projection }
    }

    fn apply_all(source: &str, candidates: &[TransformationCandidate]) -> String {
        let mut output = source.to_string();
        let mut edits = candidates
            .iter()
            .flat_map(|candidate| candidate.edits())
            .collect::<Vec<_>>();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.span.start_byte));
        for edit in edits {
            assert_eq!(
                &output[edit.span.start_byte..edit.span.end_byte],
                edit.before
            );
            output.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        }
        output
    }

    fn run_rust(source: &str) -> String {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("fixture.rs");
        let binary = root.path().join("fixture");
        std::fs::write(&path, source).unwrap();
        let build = Command::new("rustc")
            .args(["--edition=2024", "-Awarnings"])
            .arg(&path)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let output = Command::new(binary).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn complete_fixture_emits_three_disjoint_behavior_preserving_cleanups() {
        let fixture = fixture();
        let candidates = detect_local_cleanup_candidates(&fixture.projection).unwrap();
        assert_eq!(candidates.len(), 3);
        let by_name = candidates
            .iter()
            .map(|candidate| (candidate.recipe().name(), candidate))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(by_name.len(), 3);
        assert_eq!(
            by_name["rust-inline-exact-single-use-temporary"].disposition(),
            CandidateDisposition::ReviewRequired
        );
        for name in [
            "rust-remove-unused-pure-literal-expression",
            "rust-remove-independent-unused-literal-local",
        ] {
            assert_eq!(by_name[name].disposition(), CandidateDisposition::Automatic);
        }
        let rewritten = apply_all(&fixture.source, &candidates);
        assert!(!rewritten.contains("let temporary"));
        assert!(!rewritten.contains("99;"));
        assert!(!rewritten.contains("let unused"));
        assert!(rewritten.contains("let result = (1 + 2) * 3;"));
        assert!(rewritten.contains("let typed: i32 = 11;"));
        assert!(rewritten.contains("1 + 2;"));
        assert_eq!(run_rust(&fixture.source), run_rust(&rewritten));
        assert_eq!(run_rust(&rewritten).trim(), "19");
    }
}
