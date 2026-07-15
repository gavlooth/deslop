use std::collections::BTreeSet;

use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    AdapterCapability, ControlPointKind, ControlSyntheticPointKind, GraphEvidenceLayer,
    ProgramDependenceGraph, ProgramDependenceNode, ProgramDependenceProjection, ProjectAnalysis,
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

const CONDITION_TRUTH_EQUIVALENT: &str = "short-circuit-truth-equivalent";
const CONDITION_EVALUATION_ORDER: &str = "left-to-right-evaluation-count";
const CONDITION_BODIES_EXACT: &str = "branch-bodies-exact";
const CONDITION_EXCEPTION_ORDER: &str = "exception-suspension-order-preserved";
const FORBIDDEN_UNCERTAIN_CONTROL: &str = "recovered-or-conservative-control";
const FORBIDDEN_BINDING_SCOPE: &str = "condition-binding-scope-change";
const FORBIDDEN_EFFECT_DIVERGENCE: &str = "effect-or-exception-divergence";
const FORBIDDEN_NON_STRUCTURED: &str = "non-structured-control";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeKind {
    AndNoFallback,
    AndSharedFallback,
    OrSharedSuccess,
}

impl MergeKind {
    fn name(self) -> &'static str {
        match self {
            Self::AndNoFallback => "and-no-fallback",
            Self::AndSharedFallback => "and-shared-fallback",
            Self::OrSharedSuccess => "or-shared-success",
        }
    }

    fn operator(self) -> &'static str {
        match self {
            Self::AndNoFallback | Self::AndSharedFallback => "&&",
            Self::OrSharedSuccess => "||",
        }
    }
}

#[derive(Debug)]
struct MergeVariant {
    kind: MergeKind,
    inner: deslop_parse::NodeKey,
    replacement: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConditionMergeRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("adjacent-condition recipe graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("adjacent-condition recipe received an inconsistent projection: {0}")]
    Projection(String),
}

pub fn adjacent_condition_merge_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-merge-adjacent-conditions".into(),
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
                CONDITION_TRUTH_EQUIVALENT,
                "The nested branch truth table is exactly equivalent to Rust && or || short-circuiting.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_EVALUATION_ORDER,
                "The right predicate remains conditional and both predicates retain left-to-right evaluation count.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_BODIES_EXACT,
                "Success and fallback bodies are retained byte-exact at equivalent outcomes.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_EXCEPTION_ORDER,
                "Complete effect evidence proves panic, exception, abrupt-exit, and suspension behavior is preserved.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_UNCERTAIN_CONTROL,
                "Recovered syntax or a conservative edge participates in either branch.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_BINDING_SCOPE,
                "A let condition or let chain binds a name whose scope would change after merging.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_EFFECT_DIVERGENCE,
                "Merging changes effect, panic, exception, abrupt-exit, or suspension order.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                FORBIDDEN_NON_STRUCTURED,
                "A non-structured control fact participates in the callable.",
                GraphEvidenceLayer::NonStructuredControl,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: ValidationPlan {
            steps: vec![
                ValidationStep {
                    key: "build".into(),
                    kind: ValidationStepKind::Build,
                    description: "Build the project after the guarded short-circuit replacement."
                        .into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "graph-delta".into(),
                    kind: ValidationStepKind::GraphDelta,
                    description:
                        "Rebuild graph evidence and verify two dispatches collapse to one.".into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "parse".into(),
                    kind: ValidationStepKind::Parse,
                    description: "Parse the exact retained source after replacement.".into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "test".into(),
                    kind: ValidationStepKind::Test,
                    description: "Run project tests before accepting the review candidate.".into(),
                    command: None,
                    required: true,
                },
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
                "nested-and",
                FixtureExpectation::Candidate,
                "A nested if with no fallback becomes one && condition.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "different-fallbacks",
                FixtureExpectation::NoCandidate,
                "Different nested and outer fallbacks cannot be collapsed.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "effectful-right-predicate",
                FixtureExpectation::ReviewRequired,
                "The right predicate remains short-circuited but lacks production effect authority.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "let-binding-condition",
                FixtureExpectation::NoCandidate,
                "A condition binding cannot cross the merged condition scope boundary.",
            ),
        ],
    })
}

pub fn detect_adjacent_condition_merges(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, ConditionMergeRecipeError> {
    let recipe = adjacent_condition_merge_recipe()?;
    let data_flow = projection.data_flow();
    let regions = data_flow.control_regions();
    let control_flow = regions.control_flow();
    let analysis = control_flow.analysis();
    let non_structured = projection.non_structured_control();
    let mut emitted = BTreeSet::new();
    let mut candidates = Vec::new();

    for graph in projection.document().graphs() {
        let eligibility = evaluate_program_graph_recipe_eligibility(
            projection,
            graph,
            &recipe.eligibility_requirement(),
        )
        .map_err(|error| ConditionMergeRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = control_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.control_flow_graph())
            .ok_or_else(|| missing("control-flow graph", graph.control_flow_graph().as_str()))?;
        let data_graph = data_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.data_flow_graph())
            .ok_or_else(|| missing("data-flow graph", graph.data_flow_graph().as_str()))?;
        let non_structured_graph = non_structured
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.non_structured_control_graph())
            .ok_or_else(|| {
                missing(
                    "non-structured graph",
                    graph.non_structured_control_graph().as_str(),
                )
            })?;

        for outer_dispatch in flow_graph.points().iter().filter(|point| {
            point.kind() == &ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch)
                && !point.recovered()
        }) {
            let Some(outer_key) = outer_dispatch.source() else {
                continue;
            };
            if !exact_branch_edges(flow_graph, outer_dispatch.key()) {
                continue;
            }
            let outer = analysis
                .node_by_key(outer_key)
                .map_err(|error| ConditionMergeRecipeError::Projection(error.to_string()))?;
            let Some(variant) = merge_variant(analysis, outer)? else {
                continue;
            };
            if !emitted.insert(outer_key.clone()) {
                continue;
            }
            let Some(inner_dispatch) = flow_graph.points().iter().find(|point| {
                point.kind()
                    == &ControlPointKind::Synthetic(ControlSyntheticPointKind::BranchDispatch)
                    && point.source() == Some(&variant.inner)
            }) else {
                continue;
            };
            if inner_dispatch.recovered() || !exact_branch_edges(flow_graph, inner_dispatch.key()) {
                continue;
            }
            let Some(outer_node) = graph
                .nodes()
                .iter()
                .find(|node| node.point() == outer_dispatch.key())
            else {
                continue;
            };
            let Some(inner_node) = graph
                .nodes()
                .iter()
                .find(|node| node.point() == inner_dispatch.key())
            else {
                continue;
            };
            let target_span = span(outer_key);
            let impact = program_dependence_impact_cone(
                projection,
                graph.key(),
                outer_node.key(),
                ImpactDirection::Bidirectional,
                8,
            )?;
            let dispatch_evidence = vec![
                ConditionEvidence {
                    entity: flow_entity(flow_graph.key().as_str(), outer_dispatch.key().as_str()),
                    detail: format!(
                        "The outer dispatch is the left operand of the exact {} short-circuit form.",
                        variant.kind.operator()
                    ),
                    capability: None,
                    support: None,
                    authority: None,
                },
                ConditionEvidence {
                    entity: flow_entity(flow_graph.key().as_str(), inner_dispatch.key().as_str()),
                    detail: "The inner dispatch is evaluated only on the same short-circuit path as before."
                        .into(),
                    capability: None,
                    support: None,
                    authority: None,
                },
            ];
            let required_results = vec![
                multi_result(
                    CONDITION_TRUTH_EQUIVALENT,
                    ProofState::Proven,
                    dispatch_evidence.clone(),
                ),
                multi_result(
                    CONDITION_EVALUATION_ORDER,
                    ProofState::Proven,
                    dispatch_evidence,
                ),
                result(
                    CONDITION_BODIES_EXACT,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), outer_dispatch.key().as_str()),
                    &format!(
                        "Exact retained block bytes satisfy the {} body equivalence rule.",
                        variant.kind.name()
                    ),
                ),
                capability_result(
                    CONDITION_EXCEPTION_ORDER,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production Rust Effects authority is unavailable; panic, exception, and suspension equivalence remains review evidence.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                ),
            ];
            let forbidden_results = vec![
                multi_result(
                    FORBIDDEN_UNCERTAIN_CONTROL,
                    ProofState::Disproven,
                    vec![
                        plain_evidence(
                            flow_entity(flow_graph.key().as_str(), outer_dispatch.key().as_str()),
                            "The outer dispatch and both outgoing edges are exact and unrecovered.",
                        ),
                        plain_evidence(
                            flow_entity(flow_graph.key().as_str(), inner_dispatch.key().as_str()),
                            "The inner dispatch and both outgoing edges are exact and unrecovered.",
                        ),
                    ],
                ),
                result(
                    FORBIDDEN_BINDING_SCOPE,
                    ProofState::Disproven,
                    flow_entity(flow_graph.key().as_str(), outer_dispatch.key().as_str()),
                    "Both conditions were checked recursively and contain no let condition or let chain.",
                ),
                capability_result(
                    FORBIDDEN_EFFECT_DIVERGENCE,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Missing Effects authority cannot disprove a hidden panic, exception, abrupt-exit, or suspension divergence.",
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
                        entity: graph_root(graph, outer_node),
                        node: outer_key.clone(),
                        span: target_span,
                    },
                    eligibility: eligibility.clone(),
                    required_results,
                    forbidden_results,
                    impact,
                    expected_delta: merge_delta(graph, outer_node, inner_node, variant.kind),
                    edits: vec![TransformationEdit::exact_node_replacement(
                        outer_key.clone(),
                        target_span,
                        outer.text().into(),
                        variant.replacement,
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

fn merge_variant(
    analysis: &ProjectAnalysis,
    outer: deslop_parse::NodeView<'_>,
) -> Result<Option<MergeVariant>, ConditionMergeRecipeError> {
    if !eligible_if(outer) {
        return Ok(None);
    }
    let Some(left) = child_by_field(analysis, outer, "condition")? else {
        return Ok(None);
    };
    if contains_condition_binding(analysis, left)? {
        return Ok(None);
    }
    let Some(outer_then) = child_by_field(analysis, outer, "consequence")? else {
        return Ok(None);
    };
    let outer_else = child_by_field(analysis, outer, "alternative")?
        .map(|alternative| else_target(analysis, alternative))
        .transpose()?
        .flatten();

    if let Some(inner) = single_if_in_block(analysis, outer_then)? {
        let Some(right) = child_by_field(analysis, inner, "condition")? else {
            return Ok(None);
        };
        if !eligible_if(inner) || contains_condition_binding(analysis, right)? {
            return Ok(None);
        }
        let Some(inner_then) = child_by_field(analysis, inner, "consequence")? else {
            return Ok(None);
        };
        let inner_else = child_by_field(analysis, inner, "alternative")?
            .map(|alternative| else_target(analysis, alternative))
            .transpose()?
            .flatten();
        match (outer_else, inner_else) {
            (None, None) => {
                return Ok(Some(MergeVariant {
                    kind: MergeKind::AndNoFallback,
                    inner: inner.key().clone(),
                    replacement: render_merge(
                        left.text(),
                        "&&",
                        right.text(),
                        inner_then.text(),
                        None,
                    ),
                }));
            }
            (Some(outer_fallback), Some(inner_fallback))
                if outer_fallback.raw_grammar_kind() == "block"
                    && inner_fallback.raw_grammar_kind() == "block"
                    && outer_fallback.text() == inner_fallback.text() =>
            {
                return Ok(Some(MergeVariant {
                    kind: MergeKind::AndSharedFallback,
                    inner: inner.key().clone(),
                    replacement: render_merge(
                        left.text(),
                        "&&",
                        right.text(),
                        inner_then.text(),
                        Some(outer_fallback.text()),
                    ),
                }));
            }
            _ => {}
        }
    }

    let Some(inner) =
        outer_else.filter(|alternative| alternative.raw_grammar_kind() == "if_expression")
    else {
        return Ok(None);
    };
    if !eligible_if(inner) {
        return Ok(None);
    }
    let Some(right) = child_by_field(analysis, inner, "condition")? else {
        return Ok(None);
    };
    if contains_condition_binding(analysis, right)? {
        return Ok(None);
    }
    let Some(inner_then) = child_by_field(analysis, inner, "consequence")? else {
        return Ok(None);
    };
    if outer_then.raw_grammar_kind() != "block" || outer_then.text() != inner_then.text() {
        return Ok(None);
    }
    let inner_else = child_by_field(analysis, inner, "alternative")?
        .map(|alternative| else_target(analysis, alternative))
        .transpose()?
        .flatten();
    let fallback = inner_else.map(|fallback| fallback.text().to_owned());
    Ok(Some(MergeVariant {
        kind: MergeKind::OrSharedSuccess,
        inner: inner.key().clone(),
        replacement: render_merge(
            left.text(),
            "||",
            right.text(),
            outer_then.text(),
            fallback.as_deref(),
        ),
    }))
}

fn eligible_if(node: deslop_parse::NodeView<'_>) -> bool {
    node.grammar().lang() == Lang::Rust
        && node.raw_grammar_kind() == "if_expression"
        && !node.has_error()
        && !node.text().contains("//")
        && !node.text().contains("/*")
}

fn contains_condition_binding(
    analysis: &ProjectAnalysis,
    condition: deslop_parse::NodeView<'_>,
) -> Result<bool, ConditionMergeRecipeError> {
    if matches!(condition.raw_grammar_kind(), "let_condition" | "let_chain") {
        return Ok(true);
    }
    for descendant in analysis
        .descendant_node_ids(condition.id())
        .map_err(|error| ConditionMergeRecipeError::Projection(error.to_string()))?
    {
        let view = analysis
            .node(descendant)
            .map_err(|error| ConditionMergeRecipeError::Projection(error.to_string()))?;
        if matches!(view.raw_grammar_kind(), "let_condition" | "let_chain") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn single_if_in_block<'a>(
    analysis: &'a ProjectAnalysis,
    block: deslop_parse::NodeView<'a>,
) -> Result<Option<deslop_parse::NodeView<'a>>, ConditionMergeRecipeError> {
    if block.raw_grammar_kind() != "block" || block.has_error() {
        return Ok(None);
    }
    let children = named_children(analysis, block)?;
    if children.len() != 1 {
        return Ok(None);
    }
    if children[0].raw_grammar_kind() == "if_expression" {
        return Ok(Some(children[0]));
    }
    if children[0].raw_grammar_kind() != "expression_statement" {
        return Ok(None);
    }
    let statement_children = named_children(analysis, children[0])?;
    Ok((statement_children.len() == 1
        && statement_children[0].raw_grammar_kind() == "if_expression")
        .then_some(statement_children[0]))
}

fn else_target<'a>(
    analysis: &'a ProjectAnalysis,
    alternative: deslop_parse::NodeView<'a>,
) -> Result<Option<deslop_parse::NodeView<'a>>, ConditionMergeRecipeError> {
    if alternative.raw_grammar_kind() != "else_clause" {
        return Ok(Some(alternative));
    }
    Ok(named_children(analysis, alternative)?
        .into_iter()
        .find(|child| matches!(child.raw_grammar_kind(), "block" | "if_expression")))
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, ConditionMergeRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| ConditionMergeRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, ConditionMergeRecipeError> {
    node.children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| ConditionMergeRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|children| {
            children
                .into_iter()
                .filter(|child| child.is_named())
                .collect()
        })
}

fn render_merge(
    left: &str,
    operator: &str,
    right: &str,
    success: &str,
    fallback: Option<&str>,
) -> String {
    let fallback = fallback
        .map(|body| format!(" else {body}"))
        .unwrap_or_default();
    format!("if ({left}) {operator} ({right}) {success}{fallback}")
}

fn merge_delta(
    graph: &ProgramDependenceGraph,
    outer: &ProgramDependenceNode,
    inner: &ProgramDependenceNode,
    kind: MergeKind,
) -> ExpectedGraphDelta {
    ExpectedGraphDelta {
        changes: vec![
            ExpectedGraphChange {
                kind: GraphChangeKind::Modify,
                entity: graph_root(graph, outer),
                rationale: format!(
                    "The outer dispatch becomes the retained {} short-circuit dispatch.",
                    kind.operator()
                ),
            },
            ExpectedGraphChange {
                kind: GraphChangeKind::Remove,
                entity: graph_root(graph, inner),
                rationale: "The nested dispatch is represented by the right short-circuit operand after rebuilding."
                    .into(),
            },
        ],
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

fn missing(kind: &str, identity: &str) -> ConditionMergeRecipeError {
    ConditionMergeRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use deslop_parse::CapabilitySupport;

    use super::*;
    use crate::{branch_graph_evidence, build_rust_recipe_projection, detect_rust_recipes};

    const FIXTURES: &str = r#"
fn act() {}
fn other() {}
fn left() -> bool { true }
fn right() -> bool { true }

fn and_no_fallback(a: bool, b: bool) {
    if a { if b { act(); } }
}

fn and_shared_fallback(a: bool, b: bool) {
    if a { if b { act(); } else { other(); } } else { other(); }
}

fn or_shared_success(a: bool, b: bool) {
    if a { act(); } else if b { act(); } else { other(); }
}

fn mismatched_fallback(a: bool, b: bool) {
    if a { if b { act(); } else { other(); } } else { act(); }
}

fn reordered_effects() {
    if left() { if right() { act(); } }
}
"#;

    fn merge_candidates(root: &std::path::Path) -> Vec<TransformationCandidate> {
        detect_rust_recipes(root, &[PathBuf::from("conditions.rs")])
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.recipe().name() == "rust-merge-adjacent-conditions")
            .collect()
    }

    #[test]
    fn recipe_freezes_four_roles_and_semantic_layers() {
        let recipe = adjacent_condition_merge_recipe().unwrap();
        assert_eq!(recipe.family(), TransformationFamily::BranchControl);
        assert_eq!(recipe.maximum_safety(), SafetyClass::SafeWithPrecondition);
        assert_eq!(recipe.fixtures().len(), 4);
        assert_eq!(
            recipe
                .fixtures()
                .iter()
                .map(|fixture| fixture.role)
                .collect::<Vec<_>>(),
            vec![
                RecipeFixtureRole::Positive,
                RecipeFixtureRole::NoOp,
                RecipeFixtureRole::MinimalCounterexample,
                RecipeFixtureRole::AdversarialNearMiss,
            ]
        );
        assert!(
            recipe
                .required_layers()
                .contains(&GraphEvidenceLayer::DataFlow)
        );
        assert!(
            recipe
                .required_layers()
                .contains(&GraphEvidenceLayer::ProgramDependence)
        );
    }

    #[test]
    fn detects_and_or_forms_with_exact_short_circuit_order_and_unknown_effects() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("conditions.rs"), FIXTURES).unwrap();
        let candidates = merge_candidates(root.path());
        let replacements = candidates
            .iter()
            .map(|candidate| candidate.edits()[0].after.as_str())
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 4, "{replacements:#?}");
        assert_eq!(
            replacements
                .iter()
                .filter(|replacement| replacement.contains("&&"))
                .count(),
            3
        );
        assert_eq!(
            replacements
                .iter()
                .filter(|replacement| replacement.contains("||"))
                .count(),
            1
        );
        for candidate in &candidates {
            assert_eq!(
                candidate.disposition(),
                CandidateDisposition::ReviewRequired
            );
            assert_eq!(candidate.safety(), SafetyClass::SafeWithPrecondition);
            assert!(!candidate.eligibility().eligible());
            assert!(candidate.required_results().iter().any(|result| {
                result.condition == CONDITION_EVALUATION_ORDER
                    && result.state == ProofState::Proven
                    && result.evidence.len() == 2
            }));
            assert!(candidate.required_results().iter().any(|result| {
                result.condition == CONDITION_EXCEPTION_ORDER
                    && result.state == ProofState::Unknown
                    && result.evidence[0].capability == Some(AdapterCapability::Effects)
                    && result.evidence[0].support == Some(CapabilitySupport::Unknown)
            }));
            let evidence = branch_graph_evidence(candidate).unwrap();
            assert!(evidence.before.len() >= 2);
            assert_eq!(evidence.after.changes.len(), 2);
            assert!(!evidence.counter_evidence.is_empty());
        }
    }

    #[test]
    fn replacements_reparse_and_strict_wire_rejects_mutation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("conditions.rs");
        fs::write(&path, FIXTURES).unwrap();
        for candidate in merge_candidates(root.path()) {
            let edit = &candidate.edits()[0];
            let mut changed = FIXTURES.to_string();
            changed.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
            fs::write(&path, &changed).unwrap();
            let projection =
                build_rust_recipe_projection(root.path(), &[PathBuf::from("conditions.rs")])
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
                    .all(|node| analysis.node(node).is_ok_and(|view| !view.has_error()))
            );
            fs::write(&path, FIXTURES).unwrap();

            let encoded = serde_json::to_value(&candidate).unwrap();
            let decoded: TransformationCandidate = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(decoded, candidate);
            let mut stale = encoded;
            stale["required_results"][0]["state"] = serde_json::json!("unknown");
            assert!(serde_json::from_value::<TransformationCandidate>(stale).is_err());
        }
    }

    #[test]
    fn let_condition_and_comment_forms_abstain() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("conditions.rs"),
            "fn run(a: Option<bool>, b: bool) { if let Some(value) = a { if b { let _ = value; } } }\n\
             fn commented(a: bool, b: bool) { if a { /* preserve */ if b {} } }\n",
        )
        .unwrap();
        assert!(merge_candidates(root.path()).is_empty());
    }
}
