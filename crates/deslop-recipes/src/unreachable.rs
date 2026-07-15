use std::collections::BTreeSet;

use deslop_core::{Lang, SafetyClass, Span};
use deslop_parse::{
    ControlPointKind, GraphEvidenceLayer, ProgramDependenceGraph, ProgramDependenceNode,
    ProgramDependenceProjection, ProjectAnalysis, evaluate_program_graph_recipe_eligibility,
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

const CONDITION_ENTRY_UNREACHABLE: &str = "entry-unreachable";
const CONDITION_INERT_LITERAL: &str = "inert-literal-statement";
const CONDITION_RETAINED_SOURCE: &str = "retained-source-exact";
const CONDITION_STRUCTURED: &str = "structured-reachability-exact";
const FORBIDDEN_CONTROL: &str = "conservative-or-recovered-control";
const FORBIDDEN_NON_STRUCTURED: &str = "non-structured-control";
const FORBIDDEN_SEMANTIC_FORM: &str = "referential-or-effectful-form";

#[derive(Debug, thiserror::Error)]
pub enum UnreachableRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("unreachable recipe graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("unreachable recipe received an inconsistent projection: {0}")]
    Projection(String),
}

pub fn unreachable_literal_statement_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-remove-unreachable-literal-statement".into(),
        version: "1.0.0".into(),
        family: TransformationFamily::CloneCeremonyDeadCode,
        required_layers: vec![
            GraphEvidenceLayer::ControlFlow,
            GraphEvidenceLayer::ControlRegions,
            GraphEvidenceLayer::NonStructuredControl,
        ],
        required_conditions: vec![
            condition(
                CONDITION_ENTRY_UNREACHABLE,
                "The retained control point is unreachable from callable entry.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_INERT_LITERAL,
                "The target is an exact Rust expression statement containing only one inert literal.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_RETAINED_SOURCE,
                "The edit bytes and span come from the retained source revision.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                CONDITION_STRUCTURED,
                "The unreachable relation is exact and has no structured-control residual.",
                GraphEvidenceLayer::ControlRegions,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_CONTROL,
                "A conservative control edge or recovered syntax participates in the target.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_NON_STRUCTURED,
                "A non-structured control fact participates in the callable.",
                GraphEvidenceLayer::NonStructuredControl,
            ),
            condition(
                FORBIDDEN_SEMANTIC_FORM,
                "The target form can define, reference, invoke, mutate, allocate, or otherwise produce effects.",
                GraphEvidenceLayer::ControlFlow,
            ),
        ],
        maximum_safety: SafetyClass::SafeAuto,
        validation_plan: ValidationPlan {
            steps: vec![
                ValidationStep {
                    key: "graph-delta".into(),
                    kind: ValidationStepKind::GraphDelta,
                    description:
                        "Rebuild the retained graph chain and compare the expected removals.".into(),
                    command: None,
                    required: true,
                },
                ValidationStep {
                    key: "parse".into(),
                    kind: ValidationStepKind::Parse,
                    description: "Parse the exact source after applying the guarded deletion."
                        .into(),
                    command: None,
                    required: true,
                },
            ],
        },
        rollback_plan: RollbackPlan {
            strategy: RollbackStrategy::ReverseExactEdits,
            require_revision_guards: true,
            validation_steps: vec!["graph-delta".into(), "parse".into()],
        },
        fixtures: vec![
            fixture(
                RecipeFixtureRole::Positive,
                "literal-after-return",
                FixtureExpectation::Candidate,
                "An inert Rust literal expression statement follows an exact return.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "all-reachable",
                FixtureExpectation::NoCandidate,
                "The same literal statement is reachable from callable entry.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "call-after-return",
                FixtureExpectation::NoCandidate,
                "An unreachable call can still carry compile-time and semantic effects.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "conditional-return",
                FixtureExpectation::NoCandidate,
                "A literal after a conditional return remains reachable on another path.",
            ),
        ],
    })
}

/// Detect exact, entry-unreachable Rust literal statements that cannot carry name, type,
/// allocation, call, mutation, or runtime effects.
///
/// The detector intentionally refuses broader dead syntax. Declarations, calls, macros, operators,
/// and composite expressions require complete def/use and effect authority before they can become a
/// separate recipe family.
pub fn detect_unreachable_literal_statements(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, UnreachableRecipeError> {
    let recipe = unreachable_literal_statement_recipe()?;
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
        .map_err(|error| UnreachableRecipeError::Eligibility(error.to_string()))?;
        if !eligibility.eligible() {
            continue;
        }
        let flow_graph = control_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.control_flow_graph())
            .ok_or_else(|| missing("control-flow graph", graph.control_flow_graph().as_str()))?;
        let region_graph = regions
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.control_region_graph())
            .ok_or_else(|| {
                missing(
                    "control-region graph",
                    graph.control_region_graph().as_str(),
                )
            })?;
        let non_structured_graph = non_structured
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.non_structured_control_graph())
            .ok_or_else(|| {
                missing(
                    "non-structured-control graph",
                    graph.non_structured_control_graph().as_str(),
                )
            })?;
        let data_flow_graph = data_flow
            .document()
            .graphs()
            .iter()
            .find(|candidate| candidate.key() == graph.data_flow_graph())
            .ok_or_else(|| missing("data-flow graph", graph.data_flow_graph().as_str()))?;

        for node in graph.nodes().iter().filter(|node| !node.reachable()) {
            let Some(source) = node.source() else {
                continue;
            };
            let Some(statement) = inert_literal_statement(analysis, source)? else {
                continue;
            };
            let statement_key = statement.key().clone();
            if !emitted.insert(statement_key.clone()) {
                continue;
            }
            let point = flow_graph
                .points()
                .iter()
                .find(|point| point.key() == node.point())
                .ok_or_else(|| missing("control point", node.point().as_str()))?;
            if point.kind() != &ControlPointKind::Syntax || point.recovered() {
                continue;
            }
            let relation = region_graph
                .points()
                .iter()
                .find(|relation| relation.key() == node.control_region_point())
                .ok_or_else(|| {
                    missing("control-region point", node.control_region_point().as_str())
                })?;
            if relation.reachable()
                || !relation.dominators().is_empty()
                || relation.immediate_dominator().is_some()
                || !region_graph.residuals().is_empty()
                || !non_structured_graph.facts().is_empty()
            {
                continue;
            }

            let span = span(statement.key());
            let before = statement.text().to_owned();
            let root = graph_root(graph, node);
            let impact = program_dependence_impact_cone(
                projection,
                graph.key(),
                node.key(),
                ImpactDirection::Bidirectional,
                8,
            )?;
            let required_results = vec![
                result(
                    CONDITION_ENTRY_UNREACHABLE,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), node.point().as_str()),
                    "The retained PDG and control-region relation both mark this point entry-unreachable.",
                ),
                result(
                    CONDITION_INERT_LITERAL,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), node.point().as_str()),
                    "The exact Rust CST is one expression statement with one allowlisted literal child.",
                ),
                result(
                    CONDITION_RETAINED_SOURCE,
                    ProofState::Proven,
                    flow_entity(flow_graph.key().as_str(), node.point().as_str()),
                    "The target NodeKey, source bytes, span, and revision guard share one retained revision.",
                ),
                result(
                    CONDITION_STRUCTURED,
                    ProofState::Proven,
                    GraphEntityRef {
                        layer: GraphEvidenceLayer::ControlRegions,
                        graph: region_graph.key().as_str().into(),
                        entity: relation.key().as_str().into(),
                    },
                    "Complete control-region evidence has no residual and gives the unreachable point no dominator.",
                ),
            ];
            let forbidden_results = vec![
                result(
                    FORBIDDEN_CONTROL,
                    ProofState::Disproven,
                    flow_entity(flow_graph.key().as_str(), node.point().as_str()),
                    "Graph eligibility rejected all conservative edges and the exact target point is not recovered.",
                ),
                result(
                    FORBIDDEN_NON_STRUCTURED,
                    ProofState::Disproven,
                    GraphEntityRef {
                        layer: GraphEvidenceLayer::NonStructuredControl,
                        graph: non_structured_graph.key().as_str().into(),
                        entity: non_structured_graph.key().as_str().into(),
                    },
                    "Complete non-structured-control evidence contains no facts for this callable.",
                ),
                result(
                    FORBIDDEN_SEMANTIC_FORM,
                    ProofState::Disproven,
                    flow_entity(flow_graph.key().as_str(), node.point().as_str()),
                    "The allowlist excludes declarations, names, calls, macros, operators, aggregates, and control forms.",
                ),
            ];
            let expected_delta = expected_delta(
                graph,
                flow_graph,
                region_graph,
                data_flow_graph.key().as_str(),
                span,
            );
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
                        node: statement_key.clone(),
                        span,
                    },
                    eligibility: eligibility.clone(),
                    required_results,
                    forbidden_results,
                    impact,
                    expected_delta,
                    edits: vec![TransformationEdit::exact_node_deletion(
                        statement_key,
                        span,
                        before,
                    )],
                    safety: SafetyClass::SafeAuto,
                    disposition: CandidateDisposition::Automatic,
                    validation_plan: recipe.validation_plan().clone(),
                    rollback_plan: recipe.rollback_plan().clone(),
                },
            )?);
        }
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
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

fn flow_entity(graph: &str, point: &str) -> GraphEntityRef {
    GraphEntityRef {
        layer: GraphEvidenceLayer::ControlFlow,
        graph: graph.into(),
        entity: point.into(),
    }
}

fn graph_root(graph: &ProgramDependenceGraph, node: &ProgramDependenceNode) -> GraphEntityRef {
    GraphEntityRef {
        layer: GraphEvidenceLayer::ProgramDependence,
        graph: graph.key().as_str().into(),
        entity: node.key().as_str().into(),
    }
}

fn inert_literal_statement<'a>(
    analysis: &'a ProjectAnalysis,
    source: &deslop_parse::NodeKey,
) -> Result<Option<deslop_parse::NodeView<'a>>, UnreachableRecipeError> {
    let mut node = analysis
        .node_by_key(source)
        .map_err(|error| UnreachableRecipeError::Projection(error.to_string()))?;
    if node.raw_grammar_kind() != "expression_statement" {
        let Some(parent) = node.parent() else {
            return Ok(None);
        };
        node = analysis
            .node(parent)
            .map_err(|error| UnreachableRecipeError::Projection(error.to_string()))?;
    }
    if node.grammar().lang() != Lang::Rust
        || node.raw_grammar_kind() != "expression_statement"
        || node.has_error()
    {
        return Ok(None);
    }
    let named_children = node
        .children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| UnreachableRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|child| child.is_named())
        .collect::<Vec<_>>();
    if named_children.len() != 1
        || !matches!(
            named_children[0].raw_grammar_kind(),
            "integer_literal"
                | "float_literal"
                | "boolean_literal"
                | "char_literal"
                | "string_literal"
                | "raw_string_literal"
        )
        || named_children[0].has_error()
    {
        return Ok(None);
    }
    Ok(Some(node))
}

fn span(node: &deslop_parse::NodeKey) -> Span {
    Span {
        start_line: node.anchor().start_row() as usize + 1,
        end_line: node.anchor().end_row() as usize + 1,
        start_byte: node.anchor().start_byte() as usize,
        end_byte: node.anchor().end_byte() as usize,
    }
}

fn expected_delta(
    graph: &ProgramDependenceGraph,
    flow: &deslop_parse::ControlFlowGraph,
    regions: &deslop_parse::ControlRegionGraph,
    data_flow_graph: &str,
    target: Span,
) -> ExpectedGraphDelta {
    let contains = |source: &deslop_parse::NodeKey| {
        source.anchor().start_byte() as usize >= target.start_byte
            && source.anchor().end_byte() as usize <= target.end_byte
    };
    let removed_points = flow
        .points()
        .iter()
        .filter(|point| point.source().is_some_and(&contains))
        .map(|point| point.key())
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    for point in &removed_points {
        changes.push(removal(
            GraphEvidenceLayer::ControlFlow,
            flow.key().as_str(),
            point.as_str(),
            "The deleted syntax no longer contributes this control point.",
        ));
    }
    for relation in regions
        .points()
        .iter()
        .filter(|relation| removed_points.contains(relation.point()))
    {
        changes.push(removal(
            GraphEvidenceLayer::ControlRegions,
            regions.key().as_str(),
            relation.key().as_str(),
            "The deleted control point no longer has retained reachability relations.",
        ));
    }
    for node in graph
        .nodes()
        .iter()
        .filter(|node| node.source().is_some_and(&contains))
    {
        changes.push(removal(
            GraphEvidenceLayer::DataFlow,
            data_flow_graph,
            node.data_flow_point().as_str(),
            "The deleted syntax no longer contributes a data-flow point.",
        ));
        changes.push(removal(
            GraphEvidenceLayer::ProgramDependence,
            graph.key().as_str(),
            node.key().as_str(),
            "The deleted syntax no longer contributes a program-dependence node.",
        ));
    }
    ExpectedGraphDelta { changes }
}

fn removal(
    layer: GraphEvidenceLayer,
    graph: &str,
    entity: &str,
    rationale: &str,
) -> ExpectedGraphChange {
    ExpectedGraphChange {
        kind: GraphChangeKind::Remove,
        entity: GraphEntityRef {
            layer,
            graph: graph.into(),
            entity: entity.into(),
        },
        rationale: rationale.into(),
    }
}

fn missing(kind: &str, identity: &str) -> UnreachableRecipeError {
    UnreachableRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Instant;

    use deslop_parse::{
        BuildContextId, CanonicalRoleSet, ControlFlowPolicyId, ControlRegionPolicyId,
        DataFlowBuilder, DataFlowEffectDraft, DataFlowGraphDraft, DataFlowPolicyId,
        FactCoverageEvidence, NameNamespace, NamespacePolicy, NonStructuredControlPolicyId,
        ProgramDependencePolicyId, ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId,
        ResolutionPolicyId, ResolutionProjection, ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder,
        ScopeKind, derive_control_regions, derive_non_structured_control_regions,
        derive_program_dependence, lower_control_flow,
    };
    use serde_json::json;

    use crate::{
        B7Thresholds, EvaluationObservation, ImpactDirection, evaluate_recipe_observations,
        frozen_unreachable_rust_cases, frozen_unreachable_rust_manifest,
        program_dependence_impact_cone,
    };

    use super::{
        CandidateDisposition, TransformationCandidate, detect_unreachable_literal_statements,
        unreachable_literal_statement_recipe,
    };

    fn graph(source: &str) -> Arc<deslop_parse::ProgramDependenceProjection> {
        let root = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("unreachable-literal-recipe-test").unwrap(),
        )
        .unwrap()
        .with_overlay("fixture.rs", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let flow = Arc::new(
            lower_control_flow(
                Arc::clone(&analysis),
                ControlFlowPolicyId::from_parts(&[b"unreachable-literal-recipe-cfg/1"]).unwrap(),
            )
            .unwrap()
            .projection()
            .unwrap()
            .clone(),
        );
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                ControlRegionPolicyId::from_parts(&[b"unreachable-literal-recipe-regions/1"])
                    .unwrap(),
            )
            .unwrap(),
        );

        let source_file = node_by_kind(&analysis, "source_file");
        let function = node_by_kind(&analysis, "function_item");
        let incomplete = FactCoverageEvidence::partial(
            "production Rust scope authority is unavailable to this control-only recipe fixture",
        )
        .unwrap();
        let namespace = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let mut scopes = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"unreachable-literal-recipe-build"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"unreachable-literal-recipe-scope/1"]).unwrap(),
        )
        .unwrap();
        let file_scope = scopes
            .add_scope(
                source_file,
                roles(&analysis, source_file),
                incomplete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespace.clone(),
                },
            )
            .unwrap();
        scopes
            .add_scope(
                function,
                roles(&analysis, function),
                incomplete,
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespace,
                },
            )
            .unwrap();
        let resolution = Arc::new(
            ResolutionProjection::build(
                Arc::new(scopes.build().unwrap()),
                ResolutionPolicyId::from_parts(&[b"unreachable-literal-recipe-resolution/1"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let mut data_flow = DataFlowBuilder::new(
            Arc::clone(&regions),
            resolution,
            DataFlowPolicyId::from_parts(&[b"unreachable-literal-recipe-data-flow/1"]).unwrap(),
        )
        .unwrap();
        for flow_graph in flow.document().graphs() {
            data_flow
                .add_graph(DataFlowGraphDraft {
                    control_flow_graph: flow_graph.key().clone(),
                    definitions: vec![],
                    accesses: vec![],
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
        }
        let data_flow = Arc::new(data_flow.build().unwrap());
        let non_structured = Arc::new(
            derive_non_structured_control_regions(
                regions,
                NonStructuredControlPolicyId::from_parts(&[
                    b"unreachable-literal-recipe-non-structured/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        Arc::new(
            derive_program_dependence(
                data_flow,
                non_structured,
                ProgramDependencePolicyId::from_parts(&[b"unreachable-literal-recipe-pdg/1"])
                    .unwrap(),
            )
            .unwrap(),
        )
    }

    fn node_by_kind(analysis: &ProjectAnalysis, kind: &str) -> deslop_parse::NodeId {
        analysis
            .node_ids()
            .find(|node| analysis.node(*node).unwrap().raw_grammar_kind() == kind)
            .unwrap()
    }

    fn roles(analysis: &Arc<ProjectAnalysis>, node: deslop_parse::NodeId) -> CanonicalRoleSet {
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

    #[test]
    fn exact_four_role_fixture_matrix_runs_through_retained_graphs() {
        let cases = [
            ("fn run() { return; 1; }\n", 1),
            ("fn run() { 1; }\n", 0),
            ("fn run() { return; side_effect(); }\n", 0),
            ("fn run(x: bool) { if x { return; } 1; }\n", 0),
        ];
        for (source, expected) in cases {
            assert_eq!(
                detect_unreachable_literal_statements(&graph(source))
                    .unwrap()
                    .len(),
                expected,
                "fixture source: {source}"
            );
        }
        assert_eq!(
            unreachable_literal_statement_recipe()
                .unwrap()
                .fixtures()
                .len(),
            4
        );
    }

    #[test]
    fn positive_candidate_is_guarded_automatic_and_strictly_content_bound() {
        let candidates =
            detect_unreachable_literal_statements(&graph("fn run() { return; 1; }\n")).unwrap();
        let candidate = &candidates[0];
        assert_eq!(candidate.disposition(), CandidateDisposition::Automatic);
        assert_eq!(candidate.edits().len(), 1);
        assert_eq!(candidate.edits()[0].before, "1;");
        assert_eq!(candidate.edits()[0].after, "");
        assert!(candidate.eligibility().eligible());
        assert!(
            candidate
                .impact()
                .entities
                .contains(&candidate.target().entity)
        );
        assert!(
            candidate
                .expected_delta()
                .changes
                .iter()
                .all(|change| change.kind == crate::GraphChangeKind::Remove)
        );

        let bytes = serde_json::to_vec(candidate).unwrap();
        let decoded: TransformationCandidate = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(&decoded, candidate);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);

        let mut stale = serde_json::to_value(candidate).unwrap();
        stale["disposition"] = json!("review-required");
        assert!(serde_json::from_value::<TransformationCandidate>(stale).is_err());

        let mut unsafe_automatic = serde_json::to_value(candidate).unwrap();
        unsafe_automatic["required_results"][0]["state"] = json!("unknown");
        let error = serde_json::from_value::<TransformationCandidate>(unsafe_automatic)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unproven obligation"), "{error}");

        let mut unknown_field = serde_json::to_value(candidate).unwrap();
        unknown_field["unexpected"] = json!(true);
        assert!(serde_json::from_value::<TransformationCandidate>(unknown_field).is_err());
    }

    #[test]
    fn impact_cone_walks_exact_pdg_edges_in_both_directions() {
        let projection = graph("fn run(flag: bool) { if flag { 1; } else { 2; } 3; }\n");
        let graph = &projection.document().graphs()[0];
        let edge = graph.edges().first().unwrap();

        let outgoing = program_dependence_impact_cone(
            &projection,
            graph.key(),
            edge.from(),
            ImpactDirection::Outgoing,
            1,
        )
        .unwrap();
        assert!(
            outgoing
                .entities
                .iter()
                .any(|entity| entity.entity == edge.to().as_str())
        );

        let incoming = program_dependence_impact_cone(
            &projection,
            graph.key(),
            edge.to(),
            ImpactDirection::Incoming,
            1,
        )
        .unwrap();
        assert!(
            incoming
                .entities
                .iter()
                .any(|entity| entity.entity == edge.from().as_str())
        );
    }

    #[test]
    fn guarded_patch_validates_expected_removals_and_exact_rollback() {
        let source = "fn run() { return; 1; }\n";
        let original = graph(source);
        let candidate = detect_unreachable_literal_statements(&original)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let edit = &candidate.edits()[0];
        assert_eq!(
            &source[edit.span.start_byte..edit.span.end_byte],
            edit.before
        );
        let edited = format!(
            "{}{}{}",
            &source[..edit.span.start_byte],
            edit.after,
            &source[edit.span.end_byte..]
        );
        let rebuilt = graph(&edited);
        let analysis = rebuilt
            .data_flow()
            .control_regions()
            .control_flow()
            .analysis();
        assert!(
            analysis
                .node_ids()
                .all(|node| !analysis.node(node).unwrap().has_error())
        );
        assert!(
            detect_unreachable_literal_statements(&rebuilt)
                .unwrap()
                .is_empty()
        );
        for change in &candidate.expected_delta().changes {
            let retained = match change.entity.layer {
                deslop_parse::GraphEvidenceLayer::ControlFlow => rebuilt
                    .data_flow()
                    .control_regions()
                    .control_flow()
                    .document()
                    .graphs()
                    .iter()
                    .flat_map(|graph| graph.points())
                    .any(|point| point.key().as_str() == change.entity.entity),
                deslop_parse::GraphEvidenceLayer::ControlRegions => rebuilt
                    .data_flow()
                    .control_regions()
                    .document()
                    .graphs()
                    .iter()
                    .flat_map(|graph| graph.points())
                    .any(|point| point.key().as_str() == change.entity.entity),
                deslop_parse::GraphEvidenceLayer::DataFlow => rebuilt
                    .data_flow()
                    .document()
                    .graphs()
                    .iter()
                    .flat_map(|graph| graph.points())
                    .any(|point| point.key().as_str() == change.entity.entity),
                deslop_parse::GraphEvidenceLayer::ProgramDependence => rebuilt
                    .document()
                    .graphs()
                    .iter()
                    .flat_map(|graph| graph.nodes())
                    .any(|node| node.key().as_str() == change.entity.entity),
                deslop_parse::GraphEvidenceLayer::NonStructuredControl
                | deslop_parse::GraphEvidenceLayer::SystemDependence => false,
            };
            assert!(!retained, "stale expected removal survived: {change:?}");
        }

        let rolled_back = format!(
            "{}{}{}",
            &edited[..edit.span.start_byte],
            edit.before,
            &edited[edit.span.start_byte..]
        );
        assert_eq!(rolled_back, source);
        let restored = detect_unreachable_literal_statements(&graph(&rolled_back)).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id(), candidate.id());
    }

    #[test]
    #[ignore = "release-mode 2,000-case evidence gate; run explicitly with --release --ignored"]
    fn frozen_b2_slice_runs_once_and_meets_recipe_specific_b7_gates() {
        let manifest = frozen_unreachable_rust_manifest().unwrap();
        let cases = frozen_unreachable_rust_cases().unwrap();
        let source_bytes = cases.iter().map(|case| case.source.len()).sum::<usize>();
        assert!(source_bytes <= manifest.resource_budget.maximum_source_bytes);

        let started = Instant::now();
        let mut observations = Vec::with_capacity(cases.len());
        let mut candidate_count = 0;
        for cluster in cases.chunks_exact(manifest.variants_per_cluster) {
            assert!(
                cluster
                    .iter()
                    .all(|case| case.cluster == cluster[0].cluster)
            );
            let source = cluster
                .iter()
                .map(|case| case.source.as_str())
                .collect::<String>();
            let candidates = detect_unreachable_literal_statements(&graph(&source)).unwrap();
            candidate_count += candidates.len();
            let mut by_line = BTreeMap::new();
            for candidate in &candidates {
                assert!(
                    by_line
                        .insert(candidate.target().span.start_line, candidate)
                        .is_none()
                );
            }
            observations.extend(cluster.iter().enumerate().map(|(index, case)| {
                let candidate = by_line.get(&(index + 1)).copied();
                if let Some(candidate) = candidate {
                    assert_eq!(
                        Some(candidate.edits()[0].before.as_str()),
                        case.target_text.as_deref()
                    );
                }
                EvaluationObservation {
                    case_id: case.id.clone(),
                    emitted: candidate.is_some(),
                    confidence: if candidate.is_some() { 1.0 } else { 0.0 },
                }
            }));
        }
        let elapsed = started.elapsed();
        assert!(candidate_count <= manifest.resource_budget.maximum_candidates);
        assert!(
            elapsed.as_millis() <= u128::from(manifest.resource_budget.maximum_wall_time_ms),
            "release corpus run exceeded its fixed resource budget: {}ms > {}ms",
            elapsed.as_millis(),
            manifest.resource_budget.maximum_wall_time_ms
        );
        let report =
            evaluate_recipe_observations(&manifest, &observations, B7Thresholds::default())
                .unwrap();

        assert!(
            report.passed(),
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        assert_eq!(candidate_count, 1_000);
        assert_eq!(report.raw_totals.true_positive, 1_000);
        assert_eq!(report.raw_totals.false_positive, 0);
        assert_eq!(report.raw_totals.true_negative, 1_000);
        assert_eq!(report.raw_totals.false_negative, 0);
        let frozen_report: serde_json::Value = serde_json::from_str(include_str!(
            "../evaluation/unreachable_literal_rust_v1_report.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), frozen_report);
        eprintln!(
            "B2/B7 elapsed_ms={} report={}",
            elapsed.as_millis(),
            serde_json::to_string(&report).unwrap()
        );
    }
}
