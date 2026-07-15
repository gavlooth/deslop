use std::collections::BTreeSet;

use deslop_core::{Lang, SafetyClass, Span};
use deslop_parse::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, ControlEdgeKind,
    ControlEdgePrecision, ControlPointKind, ControlSyntheticPointKind, GraphEvidenceLayer,
    ProgramDependenceGraph, ProgramDependenceNode, ProgramDependenceProjection, ProjectAnalysis,
    evaluate_program_graph_recipe_eligibility,
};

use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactDirection, ImpactQueryError, ProofState, RecipeCondition, RecipeContractError,
    RecipeFixture, RecipeFixtureRole, RollbackPlan, RollbackStrategy, TransformationCandidate,
    TransformationCandidateDraft, TransformationEdit, TransformationFamily, TransformationRecipe,
    TransformationRecipeDraft, ValidationPlan, ValidationStep, ValidationStepKind,
    program_dependence_impact_cone,
};

const CONDITION_EQUIVALENT_FRAGMENT: &str = "equivalent-fragment-exact";
const CONDITION_CONDITION_ORDER: &str = "condition-order-retained";
const CONDITION_DEPENDENCIES: &str = "dependencies-preserved";
const CONDITION_EFFECT_ORDER: &str = "effect-and-drop-order-preserved";
const FORBIDDEN_UNCERTAIN_CONTROL: &str = "recovered-or-conservative-control";
const FORBIDDEN_BINDING_ESCAPE: &str = "binding-lifetime-or-drop-escape";
const FORBIDDEN_NON_STRUCTURED: &str = "non-structured-control";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FactoringKind {
    EquivalentArms,
    CommonPrefix,
    CommonSuffix,
}

impl FactoringKind {
    fn name(self) -> &'static str {
        match self {
            Self::EquivalentArms => "equivalent-arms",
            Self::CommonPrefix => "common-prefix",
            Self::CommonSuffix => "common-suffix",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchGraphEvidence {
    pub before: Vec<GraphEntityRef>,
    pub after: ExpectedGraphDelta,
    pub counter_evidence: Vec<ConditionEvidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum BranchRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("branch recipe graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("branch recipe received an inconsistent projection: {0}")]
    Projection(String),
}

pub fn equivalent_branch_factoring_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-factor-equivalent-branch-fragments".into(),
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
                CONDITION_EQUIVALENT_FRAGMENT,
                "Both arms retain an exact equivalent fragment at the same structural boundary.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_CONDITION_ORDER,
                "The proposed rewrite evaluates the original condition exactly once before the factored fragment.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_DEPENDENCIES,
                "Complete def/use evidence proves every moved binding and access remains valid.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                CONDITION_EFFECT_ORDER,
                "Complete effect evidence proves evaluation, destruction, exception, and suspension order is preserved.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_UNCERTAIN_CONTROL,
                "Recovered syntax or a conservative edge participates in the branch.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_BINDING_ESCAPE,
                "Factoring changes binding visibility, borrow extent, destruction timing, or captured state.",
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
                    description: "Build the project after the exact guarded replacement.".into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "graph-delta".into(),
                    kind: ValidationStepKind::GraphDelta,
                    description:
                        "Rebuild CFG/PST/PDG evidence and compare the expected branch changes."
                            .into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "parse".into(),
                    kind: ValidationStepKind::Parse,
                    description: "Parse the retained source after replacement.".into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "test".into(),
                    kind: ValidationStepKind::Test,
                    description:
                        "Run the project test command before accepting the review candidate.".into(),
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
                "identical-arms",
                FixtureExpectation::Candidate,
                "Both Rust if arms contain the same exact retained body.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "distinct-arms",
                FixtureExpectation::NoCandidate,
                "The two arms have no exact common boundary fragment.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "effectful-common-prefix",
                FixtureExpectation::ReviewRequired,
                "A common effectful prefix is detected but lacks production effect authority.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "reordered-fragments",
                FixtureExpectation::NoCandidate,
                "The same statements occur in a different order and are not factored.",
            ),
        ],
    })
}

pub fn detect_equivalent_branch_fragments(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, BranchRecipeError> {
    let recipe = equivalent_branch_factoring_recipe()?;
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
        .map_err(|error| BranchRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = control_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.control_flow_graph())
            .ok_or_else(|| missing("control-flow graph", graph.control_flow_graph().as_str()))?;
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
        let data_graph = data_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.data_flow_graph())
            .ok_or_else(|| missing("data-flow graph", graph.data_flow_graph().as_str()))?;

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
                .map_err(|error| BranchRecipeError::Projection(error.to_string()))?;
            let Some(parts) = branch_parts(analysis, branch)? else {
                continue;
            };
            let Some(root_node) = graph
                .nodes()
                .iter()
                .find(|node| node.point() == dispatch.key())
            else {
                continue;
            };
            let variants = factoring_variants(&parts);
            for (kind, after) in variants {
                let emission_key = (source.clone(), kind.name());
                if !emitted.insert(emission_key) {
                    continue;
                }
                let target_span = span(source);
                let root = graph_root(graph, root_node);
                let impact = program_dependence_impact_cone(
                    projection,
                    graph.key(),
                    root_node.key(),
                    ImpactDirection::Bidirectional,
                    8,
                )?;
                let exact = result(
                    CONDITION_EQUIVALENT_FRAGMENT,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                    &format!(
                        "Exact retained Rust CST text proves a {} fragment at matching arm boundaries.",
                        kind.name()
                    ),
                );
                let condition_order = result(
                    CONDITION_CONDITION_ORDER,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                    "The replacement keeps the condition before the factored fragment and evaluates it exactly once.",
                );
                let dependencies = capability_result(
                    CONDITION_DEPENDENCIES,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production Rust def/use authority is unavailable; binding movement requires review.",
                    AdapterCapability::DefUse,
                    data_graph.coverage().def_use_support(),
                    data_graph.coverage().def_use_authority(),
                );
                let effects = capability_result(
                    CONDITION_EFFECT_ORDER,
                    ProofState::Unknown,
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data_graph.key().as_str(),
                        data_graph.key().as_str(),
                    ),
                    "Production Rust effect authority is unavailable; destruction and borrow timing require review.",
                    AdapterCapability::Effects,
                    data_graph.coverage().effects_support(),
                    data_graph.coverage().effects_authority(),
                );
                let forbidden_results = vec![
                    result(
                        FORBIDDEN_UNCERTAIN_CONTROL,
                        ProofState::Disproven,
                        flow_entity(flow_graph.key().as_str(), dispatch.key().as_str()),
                        "The dispatch and both outgoing branch edges are exact and unrecovered.",
                    ),
                    capability_result(
                        FORBIDDEN_BINDING_ESCAPE,
                        ProofState::Unknown,
                        graph_entity(
                            GraphEvidenceLayer::DataFlow,
                            data_graph.key().as_str(),
                            data_graph.key().as_str(),
                        ),
                        "The missing def/use and effect capabilities cannot disprove a binding, lifetime, or drop-order escape.",
                        AdapterCapability::DefUse,
                        data_graph.coverage().def_use_support(),
                        data_graph.coverage().def_use_authority(),
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
                            entity: root,
                            node: source.clone(),
                            span: target_span,
                        },
                        eligibility: eligibility.clone(),
                        required_results: vec![exact, condition_order, dependencies, effects],
                        forbidden_results,
                        impact,
                        expected_delta: branch_delta(graph, root_node, kind),
                        edits: vec![TransformationEdit::exact_node_replacement(
                            source.clone(),
                            target_span,
                            branch.text().into(),
                            after,
                        )],
                        safety: SafetyClass::SafeWithPrecondition,
                        disposition: CandidateDisposition::ReviewRequired,
                        validation_plan: recipe.validation_plan().clone(),
                        rollback_plan: recipe.rollback_plan().clone(),
                    },
                )?);
            }
        }
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

pub fn branch_graph_evidence(candidate: &TransformationCandidate) -> Option<BranchGraphEvidence> {
    if candidate.recipe().family() != TransformationFamily::BranchControl {
        return None;
    }
    let mut before = candidate
        .required_results()
        .iter()
        .flat_map(|result| {
            result
                .evidence
                .iter()
                .map(|evidence| evidence.entity.clone())
        })
        .collect::<Vec<_>>();
    before.sort();
    before.dedup();
    let mut counter_evidence = candidate
        .forbidden_results()
        .iter()
        .flat_map(|result| result.evidence.iter().cloned())
        .collect::<Vec<_>>();
    counter_evidence
        .sort_by(|left, right| (&left.entity, &left.detail).cmp(&(&right.entity, &right.detail)));
    Some(BranchGraphEvidence {
        before,
        after: candidate.expected_delta().clone(),
        counter_evidence,
    })
}

struct BranchParts {
    condition: String,
    then_nodes: Vec<(String, String)>,
    else_nodes: Vec<(String, String)>,
    then_body: String,
}

fn branch_parts(
    analysis: &ProjectAnalysis,
    branch: deslop_parse::NodeView<'_>,
) -> Result<Option<BranchParts>, BranchRecipeError> {
    if branch.grammar().lang() != Lang::Rust
        || branch.raw_grammar_kind() != "if_expression"
        || branch.has_error()
        || branch.text().contains("//")
        || branch.text().contains("/*")
    {
        return Ok(None);
    }
    let Some(condition) = child_by_field(analysis, branch, "condition")? else {
        return Ok(None);
    };
    if matches!(condition.raw_grammar_kind(), "let_condition" | "let_chain") {
        return Ok(None);
    }
    let Some(consequence) = child_by_field(analysis, branch, "consequence")? else {
        return Ok(None);
    };
    let Some(alternative) = child_by_field(analysis, branch, "alternative")? else {
        return Ok(None);
    };
    let alternative = if alternative.raw_grammar_kind() == "else_clause" {
        named_children(analysis, alternative)?
            .into_iter()
            .find(|child| child.raw_grammar_kind() == "block")
    } else if alternative.raw_grammar_kind() == "block" {
        Some(alternative)
    } else {
        None
    };
    let Some(alternative) = alternative else {
        return Ok(None);
    };
    if consequence.raw_grammar_kind() != "block"
        || consequence.has_error()
        || alternative.has_error()
    {
        return Ok(None);
    }
    let then_children = comparable_children(analysis, consequence)?;
    let else_children = comparable_children(analysis, alternative)?;
    if then_children.is_empty() || else_children.is_empty() {
        return Ok(None);
    }
    Ok(Some(BranchParts {
        condition: condition.text().into(),
        then_nodes: then_children,
        else_nodes: else_children,
        then_body: block_body(consequence.text())?.into(),
    }))
}

fn factoring_variants(parts: &BranchParts) -> Vec<(FactoringKind, String)> {
    if parts.then_nodes == parts.else_nodes {
        return vec![(
            FactoringKind::EquivalentArms,
            format!(
                "{{ if {} {{}} else {{}}; {} }}",
                parts.condition, parts.then_body
            ),
        )];
    }
    let prefix = parts
        .then_nodes
        .iter()
        .zip(&parts.else_nodes)
        .take_while(|(left, right)| left == right)
        .count();
    let maximum_suffix = parts.then_nodes.len().min(parts.else_nodes.len()) - prefix;
    let suffix = parts
        .then_nodes
        .iter()
        .rev()
        .zip(parts.else_nodes.iter().rev())
        .take(maximum_suffix)
        .take_while(|(left, right)| left == right)
        .count();
    let mut variants = Vec::new();
    if prefix > 0 {
        let shared = render_nodes(&parts.then_nodes[..prefix]);
        let then_rest = render_block(&parts.then_nodes[prefix..]);
        let else_rest = render_block(&parts.else_nodes[prefix..]);
        variants.push((
            FactoringKind::CommonPrefix,
            format!(
                "{{ let __deslop_m5_condition = {}; {} if __deslop_m5_condition {} else {} }}",
                parts.condition, shared, then_rest, else_rest
            ),
        ));
    }
    if suffix > 0 {
        let then_end = parts.then_nodes.len() - suffix;
        let else_end = parts.else_nodes.len() - suffix;
        let shared = render_nodes(&parts.then_nodes[then_end..]);
        variants.push((
            FactoringKind::CommonSuffix,
            format!(
                "{{ if {} {} else {}; {} }}",
                parts.condition,
                render_block(&parts.then_nodes[..then_end]),
                render_block(&parts.else_nodes[..else_end]),
                shared
            ),
        ));
    }
    variants
}

fn comparable_children(
    analysis: &ProjectAnalysis,
    block: deslop_parse::NodeView<'_>,
) -> Result<Vec<(String, String)>, BranchRecipeError> {
    let children = named_children(analysis, block)?;
    if children.iter().any(|child| {
        child.is_extra()
            || child.has_error()
            || matches!(
                child.raw_grammar_kind(),
                "attribute_item" | "inner_attribute_item"
            )
    }) {
        return Ok(Vec::new());
    }
    Ok(children
        .into_iter()
        .map(|child| (child.raw_grammar_kind().into(), child.text().into()))
        .collect())
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, BranchRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| BranchRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, BranchRecipeError> {
    node.children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| BranchRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|children| {
            children
                .into_iter()
                .filter(|child| child.is_named())
                .collect()
        })
}

fn block_body(text: &str) -> Result<&str, BranchRecipeError> {
    text.strip_prefix('{')
        .and_then(|body| body.strip_suffix('}'))
        .map(str::trim)
        .ok_or_else(|| BranchRecipeError::Projection("Rust block lacks exact braces".into()))
}

fn render_nodes(nodes: &[(String, String)]) -> String {
    nodes
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_block(nodes: &[(String, String)]) -> String {
    format!("{{ {} }}", render_nodes(nodes))
}

fn exact_branch_edges(
    graph: &deslop_parse::ControlFlowGraph,
    dispatch: &deslop_parse::ControlPointKey,
) -> bool {
    let edges = graph
        .edges()
        .iter()
        .filter(|edge| edge.from() == dispatch)
        .collect::<Vec<_>>();
    edges.len() == 2
        && edges.iter().all(|edge| {
            *edge.precision() == ControlEdgePrecision::Exact
                && !edge.recovered_source()
                && !edge.recovered_predicate()
                && matches!(edge.kind(), ControlEdgeKind::Branch(_))
        })
}

fn branch_delta(
    graph: &ProgramDependenceGraph,
    node: &ProgramDependenceNode,
    kind: FactoringKind,
) -> ExpectedGraphDelta {
    ExpectedGraphDelta {
        changes: vec![ExpectedGraphChange {
            kind: GraphChangeKind::Modify,
            entity: graph_root(graph, node),
            rationale: format!(
                "Rebuild must retain one condition dispatch while replacing duplicated {} control/dependence structure.",
                kind.name()
            ),
        }],
    }
}

fn condition(key: &str, description: &str, layer: GraphEvidenceLayer) -> RecipeCondition {
    RecipeCondition {
        key: key.into(),
        description: description.into(),
        layer,
    }
}

fn fixture(
    role: RecipeFixtureRole,
    name: &str,
    expectation: FixtureExpectation,
    description: &str,
) -> RecipeFixture {
    RecipeFixture {
        role,
        name: name.into(),
        expectation,
        description: description.into(),
    }
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

fn capability_result(
    condition: &str,
    state: ProofState,
    entity: GraphEntityRef,
    detail: &str,
    capability: AdapterCapability,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
) -> ConditionResult {
    ConditionResult {
        condition: condition.into(),
        state,
        evidence: vec![ConditionEvidence {
            entity,
            detail: detail.into(),
            capability: Some(capability),
            support: Some(support),
            authority,
        }],
    }
}

fn graph_entity(layer: GraphEvidenceLayer, graph: &str, entity: &str) -> GraphEntityRef {
    GraphEntityRef {
        layer,
        graph: graph.into(),
        entity: entity.into(),
    }
}

fn flow_entity(graph: &str, point: &str) -> GraphEntityRef {
    graph_entity(GraphEvidenceLayer::ControlFlow, graph, point)
}

fn graph_root(graph: &ProgramDependenceGraph, node: &ProgramDependenceNode) -> GraphEntityRef {
    graph_entity(
        GraphEvidenceLayer::ProgramDependence,
        graph.key().as_str(),
        node.key().as_str(),
    )
}

fn span(node: &deslop_parse::NodeKey) -> Span {
    Span {
        start_line: node.anchor().start_row() as usize + 1,
        end_line: node.anchor().end_row() as usize + 1,
        start_byte: node.anchor().start_byte() as usize,
        end_byte: node.anchor().end_byte() as usize,
    }
}

fn missing(kind: &str, identity: &str) -> BranchRecipeError {
    BranchRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::{build_rust_recipe_projection, detect_rust_recipes};

    const FIXTURES: &str = r#"
fn side() {}
fn left() -> i32 { 1 }
fn right() -> i32 { 2 }
fn shared() -> i32 { 3 }

fn equivalent(flag: bool) -> i32 {
    if flag { side(); 1 } else { side(); 1 }
}

fn prefix(flag: bool) -> i32 {
    if flag { side(); left() } else { side(); right() }
}

fn suffix(flag: bool) -> i32 {
    if flag { left(); shared() } else { right(); shared() }
}

fn reordered(flag: bool) -> i32 {
    if flag { side(); shared(); left() } else { shared(); side(); right() }
}

fn comments_are_preserved_by_abstention(flag: bool) -> i32 {
    if flag { /* keep */ side(); 1 } else { /* keep */ side(); 1 }
}
"#;

    #[test]
    fn recipe_freezes_the_four_required_fixture_roles() {
        let recipe = equivalent_branch_factoring_recipe().unwrap();
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
    }

    #[test]
    fn detects_equivalent_prefix_and_suffix_without_claiming_missing_authority() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("branches.rs"), FIXTURES).unwrap();
        let candidates = detect_rust_recipes(root.path(), &[PathBuf::from("branches.rs")]).unwrap();
        assert_eq!(candidates.len(), 3);
        assert!(candidates.iter().all(|candidate| {
            candidate.recipe().name() == "rust-factor-equivalent-branch-fragments"
                && candidate.disposition() == CandidateDisposition::ReviewRequired
                && candidate.safety() == SafetyClass::SafeWithPrecondition
                && !candidate.eligibility().eligible()
        }));
        let replacements = candidates
            .iter()
            .map(|candidate| candidate.edits()[0].after.as_str())
            .collect::<Vec<_>>();
        assert!(
            replacements
                .iter()
                .any(|replacement| replacement.contains("if flag {} else {}; side(); 1"))
        );
        assert!(replacements.iter().any(|replacement| {
            replacement.contains("let __deslop_m5_condition = flag; side();")
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.contains("if flag { left(); } else { right(); }; shared()")
        }));

        for candidate in &candidates {
            assert!(candidate.required_results().iter().any(|result| {
                result.condition == CONDITION_DEPENDENCIES
                    && result.state == ProofState::Unknown
                    && result.evidence[0].capability == Some(AdapterCapability::DefUse)
                    && result.evidence[0].support == Some(CapabilitySupport::Unknown)
            }));
            let evidence = branch_graph_evidence(candidate).unwrap();
            assert!(!evidence.before.is_empty());
            assert!(!evidence.after.changes.is_empty());
            assert!(!evidence.counter_evidence.is_empty());
        }
    }

    #[test]
    fn proposed_replacements_remain_parseable_and_wire_strict() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("branches.rs");
        fs::write(&path, FIXTURES).unwrap();
        let candidates = detect_rust_recipes(root.path(), &[PathBuf::from("branches.rs")]).unwrap();
        for candidate in candidates {
            let edit = &candidate.edits()[0];
            let mut changed = FIXTURES.to_string();
            changed.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
            fs::write(&path, &changed).unwrap();
            let projection =
                build_rust_recipe_projection(root.path(), &[PathBuf::from("branches.rs")])
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

            let bytes = serde_json::to_vec(&candidate).unwrap();
            let decoded: TransformationCandidate = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(decoded, candidate);
            let mut unknown = serde_json::to_value(&candidate).unwrap();
            unknown["unexpected"] = serde_json::json!(true);
            assert!(serde_json::from_value::<TransformationCandidate>(unknown).is_err());
        }
    }
}
