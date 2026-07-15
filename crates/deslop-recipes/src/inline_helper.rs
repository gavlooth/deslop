use std::collections::BTreeSet;

use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    DataFlowAccessKind, FactCoverage, GraphEvidenceLayer, ProgramDependenceGraph, ProjectAnalysis,
    ResolutionEndpoint, ResolutionStatus, SystemDependenceProjection,
    evaluate_graph_recipe_eligibility,
};

use crate::branch::{condition, fixture, graph_entity, result, span};
use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactDirection, ImpactQueryError, ProofState, RecipeContractError, RecipeFixtureRole,
    RollbackPlan, RollbackStrategy, TransformationCandidate, TransformationCandidateDraft,
    TransformationEdit, TransformationFamily, TransformationRecipe, TransformationRecipeDraft,
    ValidationPlan, ValidationStep, ValidationStepKind, program_dependence_impact_cone,
};

const REQUIRED_BINDING: &str = "exact-local-call-binding";
const REQUIRED_SINGLE_USE: &str = "complete-single-use-reference-frontier";
const REQUIRED_SHAPE: &str = "exact-zero-parameter-unit-helper";
const REQUIRED_SCOPE: &str = "block-scope-and-evaluation-order-preserved";
const REQUIRED_EFFECTS: &str = "complete-inline-effect-frontier";
const REQUIRED_FRAME_REVIEW: &str = "call-frame-observations-reviewed";
const FORBIDDEN_EXTRA_USE: &str = "additional-call-or-value-reference";
const FORBIDDEN_SUBSTITUTION: &str = "parameter-output-or-ownership-substitution";
const FORBIDDEN_BOUNDARY: &str = "public-generic-async-abrupt-or-opaque-boundary";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineHelperEvidence {
    pub call_site: GraphEntityRef,
    pub caller: GraphEntityRef,
    pub callee: GraphEntityRef,
    pub call_reference: GraphEntityRef,
    pub callee_effects: Vec<GraphEntityRef>,
}

#[derive(Debug, thiserror::Error)]
pub enum InlineHelperRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("inline-helper graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("inline-helper recipe received an inconsistent projection: {0}")]
    Projection(String),
}

pub fn inline_single_use_helper_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-inline-exact-single-use-helper".into(),
        version: "1.0.0".into(),
        family: TransformationFamily::FunctionExpression,
        required_layers: vec![
            GraphEvidenceLayer::ControlFlow,
            GraphEvidenceLayer::ControlRegions,
            GraphEvidenceLayer::NonStructuredControl,
            GraphEvidenceLayer::DataFlow,
            GraphEvidenceLayer::ProgramDependence,
            GraphEvidenceLayer::SystemDependence,
        ],
        required_conditions: vec![
            condition(
                REQUIRED_BINDING,
                "Complete Unique semantic resolution binds the call to one exact local callable owner.",
                GraphEvidenceLayer::SystemDependence,
            ),
            condition(
                REQUIRED_SINGLE_USE,
                "Complete call and reference enumeration proves the helper has exactly one use.",
                GraphEvidenceLayer::SystemDependence,
            ),
            condition(
                REQUIRED_SHAPE,
                "The helper is private, synchronous, unit-returning, and needs no substitution.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_SCOPE,
                "Replacing the call statement with the exact helper block preserves order and temporary scope.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                REQUIRED_EFFECTS,
                "Complete retained effects and outputs cross no parameter, return, or hidden state binding.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_FRAME_REVIEW,
                "Removal of a call frame, caller location, and panic/backtrace observations requires review.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_EXTRA_USE,
                "The helper has another call or function-value reference.",
                GraphEvidenceLayer::SystemDependence,
            ),
            condition(
                FORBIDDEN_SUBSTITUTION,
                "Inlining requires parameter, output, ownership, or evaluation substitution.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                FORBIDDEN_BOUNDARY,
                "The helper crosses a public, generic, async, unsafe, abrupt, macro, closure, or recovered boundary.",
                GraphEvidenceLayer::ControlFlow,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: ValidationPlan {
            steps: vec![
                validation(
                    "build",
                    ValidationStepKind::Build,
                    "Build after deleting the helper and inlining its exact block.",
                ),
                validation(
                    "graph-delta",
                    ValidationStepKind::GraphDelta,
                    "Rebuild and verify removal of the call/callee boundary with retained body effects.",
                ),
                validation(
                    "parse",
                    ValidationStepKind::Parse,
                    "Parse both exact edits as one atomic transaction.",
                ),
                validation(
                    "test",
                    ValidationStepKind::Test,
                    "Run project tests before accepting the inline.",
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
                "one-exact-local-call",
                FixtureExpectation::Candidate,
                "One private zero-parameter unit helper has one exact call and reference.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "two-exact-calls",
                FixtureExpectation::NoCandidate,
                "A second call keeps the helper abstraction live.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "function-value-reference",
                FixtureExpectation::NoCandidate,
                "A non-call reference makes the use count greater than one.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "same-spelling-without-unique-binding",
                FixtureExpectation::NoCandidate,
                "Spelling without Complete Unique resolution cannot bind a callee.",
            ),
        ],
    })
}

pub fn detect_inline_single_use_helpers(
    system: &SystemDependenceProjection,
) -> Result<Vec<TransformationCandidate>, InlineHelperRecipeError> {
    let recipe = inline_single_use_helper_recipe()?;
    let pdg = system.program_dependence();
    let data = pdg.data_flow();
    let resolution = data.resolution();
    let analysis = data.control_regions().control_flow().analysis();
    let eligibility =
        evaluate_graph_recipe_eligibility(pdg, Some(system), &recipe.eligibility_requirement())
            .map_err(|error| InlineHelperRecipeError::Eligibility(error.to_string()))?;
    if system.document().coverage().status() != FactCoverage::Complete
        || !system.document().gaps().is_empty()
        || resolution
            .results()
            .iter()
            .any(|result| result.wire().coverage().status() != FactCoverage::Complete)
    {
        return Ok(Vec::new());
    }
    for graph in data.document().graphs() {
        let Some(program_graph) = graph_for_data(pdg, graph) else {
            return Err(missing("PDG for data-flow graph", graph.key().as_str()));
        };
        let call_accesses = graph
            .accesses()
            .iter()
            .filter(|access| access.kind() == DataFlowAccessKind::Call)
            .count();
        let retained_calls = system
            .document()
            .calls()
            .iter()
            .filter(|call| call.caller() == program_graph.key())
            .count();
        if graph.coverage().status() != FactCoverage::Complete || call_accesses != retained_calls {
            return Ok(Vec::new());
        }
    }

    let mut candidates = Vec::new();
    for call in system.document().calls() {
        let Some(callee_key) = call.callee() else {
            continue;
        };
        if call.uncertainty().is_some()
            || !call.parameter_bindings().is_empty()
            || !call.output_bindings().is_empty()
            || call.caller() == callee_key
            || system
                .document()
                .calls()
                .iter()
                .filter(|candidate| candidate.callee() == Some(callee_key))
                .count()
                != 1
        {
            continue;
        }
        let Some(caller_graph) = pdg
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == call.caller())
        else {
            return Err(missing("caller PDG", call.caller().as_str()));
        };
        let Some(callee_graph) = pdg
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == callee_key)
        else {
            return Err(missing("callee PDG", callee_key.as_str()));
        };
        if caller_graph.owner().file().path != callee_graph.owner().file().path {
            continue;
        }
        let Some(caller_data) = data
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == caller_graph.data_flow_graph())
        else {
            return Err(missing(
                "caller data-flow graph",
                caller_graph.data_flow_graph().as_str(),
            ));
        };
        let Some(callee_data) = data
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.key() == callee_graph.data_flow_graph())
        else {
            return Err(missing(
                "callee data-flow graph",
                callee_graph.data_flow_graph().as_str(),
            ));
        };
        let Some(access) = caller_data
            .accesses()
            .iter()
            .find(|access| access.key() == call.call())
        else {
            return Err(missing("call access", call.call().as_str()));
        };
        let matching_references =
            complete_unique_references_to_owner(analysis, resolution, callee_graph.owner());
        if matching_references.len() != 1 || matching_references[0] != access.resolution() {
            continue;
        }
        let Some(call_node) = caller_graph
            .nodes()
            .iter()
            .find(|node| node.key() == call.call_node())
        else {
            return Err(missing("call PDG node", call.call_node().as_str()));
        };
        let Some(call_source) = call_node.source() else {
            continue;
        };
        let call_expression = analysis
            .node_by_key(call_source)
            .map_err(|error| InlineHelperRecipeError::Projection(error.to_string()))?;
        let callee = analysis
            .node_by_key(callee_graph.owner())
            .map_err(|error| InlineHelperRecipeError::Projection(error.to_string()))?;
        let Some(shape) = inline_shape(analysis, call_expression, callee)? else {
            continue;
        };
        if !callee_data.definitions().is_empty()
            || !callee_data.accesses().is_empty()
            || !callee_data.boundaries().is_empty()
            || callee_data.effects().iter().any(|effect| {
                effect
                    .effects()
                    .iter()
                    .any(|kind| !matches!(kind, deslop_parse::DataFlowEffectKind::Returns))
            })
        {
            continue;
        }
        let evidence = inline_evidence(
            system,
            caller_graph,
            callee_graph,
            access.resolution(),
            callee_data,
        );
        let call_site_entity = evidence.call_site.clone();
        let target_entity = graph_entity(
            GraphEvidenceLayer::ProgramDependence,
            caller_graph.key().as_str(),
            call.call_node().as_str(),
        );
        let mut edits = vec![
            TransformationEdit::exact_node_deletion(
                callee.key().clone(),
                span(callee.key()),
                callee.text().into(),
            ),
            TransformationEdit::exact_node_replacement(
                shape.statement.key().clone(),
                span(shape.statement.key()),
                shape.statement.text().into(),
                shape.body.text().into(),
            ),
        ];
        edits.sort_by(|left, right| {
            (
                &left.target.file().path,
                left.span.start_byte,
                left.span.end_byte,
            )
                .cmp(&(
                    &right.target.file().path,
                    right.span.start_byte,
                    right.span.end_byte,
                ))
        });
        let effect_evidence = if evidence.callee_effects.is_empty() {
            vec![plain(
                graph_entity(
                    GraphEvidenceLayer::DataFlow,
                    callee_data.key().as_str(),
                    callee_data.key().as_str(),
                ),
                "Complete callee data flow retains zero non-return effects and zero outputs.",
            )]
        } else {
            evidence
                .callee_effects
                .iter()
                .cloned()
                .map(|entity| plain(entity, "Retained callee effect moves with the exact block."))
                .collect()
        };
        candidates.push(TransformationCandidate::new(TransformationCandidateDraft {
            recipe: recipe.clone(),
            source: CandidateSource {
                project_snapshot: analysis.snapshot().id().as_str().into(),
                analysis: analysis.id().as_str().into(),
                program_dependence_projection: pdg.id().as_str().into(),
            },
            target: CandidateTarget {
                entity: target_entity,
                node: shape.statement.key().clone(),
                span: span(shape.statement.key()),
            },
            eligibility: eligibility.clone(),
            required_results: vec![
                ConditionResult { condition: REQUIRED_BINDING.into(), state: ProofState::Proven, evidence: vec![plain(call_site_entity.clone(), "Complete Unique resolution binds this call site to the exact local callee owner.")] },
                ConditionResult { condition: REQUIRED_SINGLE_USE.into(), state: ProofState::Proven, evidence: vec![plain(call_site_entity.clone(), "Complete call/reference enumeration contains exactly this one use.")] },
                ConditionResult { condition: REQUIRED_SHAPE.into(), state: ProofState::Proven, evidence: vec![plain(graph_entity(GraphEvidenceLayer::DataFlow, callee_data.key().as_str(), callee_data.key().as_str()), "Private zero-parameter unit helper requires no substitution or output binding.")] },
                result(REQUIRED_SCOPE, ProofState::Proven, graph_entity(GraphEvidenceLayer::ControlFlow, caller_graph.control_flow_graph().as_str(), access.point().as_str()), "The exact helper block replaces one direct call statement without flattening its scope."),
                ConditionResult { condition: REQUIRED_EFFECTS.into(), state: ProofState::Proven, evidence: effect_evidence },
                ConditionResult { condition: REQUIRED_FRAME_REVIEW.into(), state: ProofState::Unknown, evidence: vec![plain(graph_entity(GraphEvidenceLayer::DataFlow, callee_data.key().as_str(), callee_data.key().as_str()), "Inlining removes one call frame; caller-location, panic, and backtrace observations require review.")] },
            ],
            forbidden_results: vec![
                ConditionResult { condition: FORBIDDEN_EXTRA_USE.into(), state: ProofState::Disproven, evidence: vec![plain(call_site_entity.clone(), "Complete enumeration has no second call or value reference.")] },
                ConditionResult { condition: FORBIDDEN_SUBSTITUTION.into(), state: ProofState::Disproven, evidence: vec![plain(graph_entity(GraphEvidenceLayer::DataFlow, callee_data.key().as_str(), callee_data.key().as_str()), "Callee summaries contain no formal input, output, mutation, or global binding.")] },
                ConditionResult { condition: FORBIDDEN_BOUNDARY.into(), state: ProofState::Disproven, evidence: vec![plain(graph_entity(GraphEvidenceLayer::ControlFlow, callee_graph.control_flow_graph().as_str(), callee_data.control_flow_graph().as_str()), "Exact CST and complete control flow contain no forbidden helper boundary.")] },
            ],
            impact: program_dependence_impact_cone(pdg, caller_graph.key(), call.call_node(), ImpactDirection::Bidirectional, 12)?,
            expected_delta: ExpectedGraphDelta { changes: vec![
                ExpectedGraphChange { kind: GraphChangeKind::Remove, entity: call_site_entity, rationale: "The sole interprocedural call boundary is removed.".into() },
                ExpectedGraphChange { kind: GraphChangeKind::Remove, entity: evidence.callee, rationale: "The now-unused local helper callable is deleted.".into() },
                ExpectedGraphChange { kind: GraphChangeKind::Modify, entity: evidence.caller, rationale: "The caller contains the exact helper block at the former call site.".into() },
            ] },
            edits,
            safety: SafetyClass::SafeWithPrecondition,
            disposition: CandidateDisposition::ReviewRequired,
            validation_plan: recipe.validation_plan().clone(),
            rollback_plan: recipe.rollback_plan().clone(),
        })?);
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

struct InlineShape<'a> {
    statement: deslop_parse::NodeView<'a>,
    body: deslop_parse::NodeView<'a>,
}

fn inline_shape<'a>(
    analysis: &'a ProjectAnalysis,
    call: deslop_parse::NodeView<'a>,
    callee: deslop_parse::NodeView<'a>,
) -> Result<Option<InlineShape<'a>>, InlineHelperRecipeError> {
    if call.raw_grammar_kind() != "call_expression"
        || callee.grammar().lang() != Lang::Rust
        || callee.raw_grammar_kind() != "function_item"
        || call.has_error()
        || callee.has_error()
        || contains_forbidden_syntax(analysis, callee)?
    {
        return Ok(None);
    }
    let Some(function) = child_by_field(analysis, call, "function")? else {
        return Ok(None);
    };
    let Some(arguments) = child_by_field(analysis, call, "arguments")? else {
        return Ok(None);
    };
    let Some(parameters) = child_by_field(analysis, callee, "parameters")? else {
        return Ok(None);
    };
    let Some(body) = child_by_field(analysis, callee, "body")? else {
        return Ok(None);
    };
    let Some(source_file) = callee.parent().and_then(|id| analysis.node(id).ok()) else {
        return Ok(None);
    };
    let prefix = callee
        .text()
        .split_once("fn ")
        .map_or(callee.text(), |(prefix, _)| prefix);
    if source_file.raw_grammar_kind() != "source_file"
        || attached_outer_attribute(analysis, source_file, callee)?
        || ["async", "const", "unsafe", "extern"]
            .iter()
            .any(|word| prefix.split_whitespace().any(|part| part == *word))
        || function.raw_grammar_kind() != "identifier"
        || !named_children(analysis, arguments)?.is_empty()
        || !named_children(analysis, parameters)?.is_empty()
        || child_by_field(analysis, callee, "return_type")?.is_some()
        || child_by_field(analysis, callee, "type_parameters")?.is_some()
        || child_by_field(analysis, callee, "where_clause")?.is_some()
        || callee.children().any(|id| {
            analysis.node(id).is_ok_and(|node| {
                matches!(
                    node.raw_grammar_kind(),
                    "visibility_modifier" | "attribute_item"
                )
            })
        })
    {
        return Ok(None);
    }
    let statements = named_children(analysis, body)?;
    if !(1..=4).contains(&statements.len())
        || statements.iter().any(|statement| {
            statement.raw_grammar_kind() != "expression_statement" || statement.has_error()
        })
    {
        return Ok(None);
    }
    let Some(parent) = call.parent().and_then(|id| analysis.node(id).ok()) else {
        return Ok(None);
    };
    let statement_children = named_children(analysis, parent)?;
    if parent.raw_grammar_kind() != "expression_statement"
        || statement_children.len() != 1
        || statement_children[0].id() != call.id()
    {
        return Ok(None);
    }
    let Some(caller_body) = parent.parent().and_then(|id| analysis.node(id).ok()) else {
        return Ok(None);
    };
    let Some(caller) = caller_body.parent().and_then(|id| analysis.node(id).ok()) else {
        return Ok(None);
    };
    if caller_body.raw_grammar_kind() != "block"
        || caller.raw_grammar_kind() != "function_item"
        || callee.parent() != caller.parent()
    {
        return Ok(None);
    }
    Ok(Some(InlineShape {
        statement: parent,
        body,
    }))
}

fn complete_unique_references_to_owner<'a>(
    analysis: &ProjectAnalysis,
    resolution: &'a deslop_parse::ResolutionProjection,
    owner: &deslop_parse::NodeKey,
) -> Vec<&'a deslop_parse::ResolutionResultKey> {
    let scope = resolution.scope_graph();
    resolution
        .results()
        .iter()
        .filter_map(|result| {
            let wire = result.wire();
            if wire.status() != ResolutionStatus::Unique
                || wire.coverage().status() != FactCoverage::Complete
            {
                return None;
            }
            let preferred = wire.preferred()?;
            if preferred.status() != ResolutionStatus::Unique || preferred.endpoints().len() != 1 {
                return None;
            }
            let fact_key = match &preferred.endpoints()[0] {
                ResolutionEndpoint::Declaration(key) | ResolutionEndpoint::Definition(key) => key,
                _ => return None,
            };
            let fact = scope.facts().iter().find(|fact| fact.key() == fact_key)?;
            (analysis.node_key(fact.node()).ok()? == owner).then_some(wire.key())
        })
        .collect()
}

fn inline_evidence(
    system: &SystemDependenceProjection,
    caller: &ProgramDependenceGraph,
    callee: &ProgramDependenceGraph,
    reference: &deslop_parse::ResolutionResultKey,
    data: &deslop_parse::DataFlowGraph,
) -> InlineHelperEvidence {
    let call = system
        .document()
        .calls()
        .iter()
        .find(|site| site.caller() == caller.key() && site.callee() == Some(callee.key()))
        .expect("selected exact call remains present");
    InlineHelperEvidence {
        call_site: graph_entity(
            GraphEvidenceLayer::SystemDependence,
            system.id().as_str(),
            call.key().as_str(),
        ),
        caller: graph_entity(
            GraphEvidenceLayer::ProgramDependence,
            caller.key().as_str(),
            caller.key().as_str(),
        ),
        callee: graph_entity(
            GraphEvidenceLayer::ProgramDependence,
            callee.key().as_str(),
            callee.key().as_str(),
        ),
        call_reference: graph_entity(
            GraphEvidenceLayer::DataFlow,
            data.key().as_str(),
            reference.as_str(),
        ),
        callee_effects: data
            .effects()
            .iter()
            .flat_map(|effect| {
                effect
                    .effects()
                    .iter()
                    .filter(|kind| !matches!(kind, deslop_parse::DataFlowEffectKind::Returns))
                    .map(|_| {
                        graph_entity(
                            GraphEvidenceLayer::DataFlow,
                            data.key().as_str(),
                            effect.key().as_str(),
                        )
                    })
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    }
}

fn graph_for_data<'a>(
    pdg: &'a deslop_parse::ProgramDependenceProjection,
    data: &deslop_parse::DataFlowGraph,
) -> Option<&'a ProgramDependenceGraph> {
    pdg.document()
        .graphs()
        .iter()
        .find(|graph| graph.data_flow_graph() == data.key())
}

fn attached_outer_attribute(
    analysis: &ProjectAnalysis,
    source_file: deslop_parse::NodeView<'_>,
    callee: deslop_parse::NodeView<'_>,
) -> Result<bool, InlineHelperRecipeError> {
    let children = named_children(analysis, source_file)?;
    let Some(index) = children
        .iter()
        .position(|candidate| candidate.id() == callee.id())
    else {
        return Ok(true);
    };
    Ok(index.checked_sub(1).is_some_and(|previous| {
        matches!(
            children[previous].raw_grammar_kind(),
            "attribute_item" | "inner_attribute_item" | "line_comment" | "block_comment"
        )
    }))
}

fn contains_forbidden_syntax(
    analysis: &ProjectAnalysis,
    callee: deslop_parse::NodeView<'_>,
) -> Result<bool, InlineHelperRecipeError> {
    let forbidden = |kind: &str| {
        matches!(
            kind,
            "return_expression"
                | "break_expression"
                | "continue_expression"
                | "try_expression"
                | "await_expression"
                | "yield_expression"
                | "macro_invocation"
                | "attribute_item"
                | "inner_attribute_item"
                | "unsafe_block"
                | "closure_expression"
                | "let_declaration"
                | "let_condition"
                | "let_chain"
                | "line_comment"
                | "block_comment"
        )
    };
    if forbidden(callee.raw_grammar_kind()) {
        return Ok(true);
    }
    Ok(analysis
        .descendant_node_ids(callee.id())
        .map_err(|error| InlineHelperRecipeError::Projection(error.to_string()))?
        .any(|id| {
            analysis
                .node(id)
                .is_ok_and(|node| node.has_error() || forbidden(node.raw_grammar_kind()))
        }))
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, InlineHelperRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| InlineHelperRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, InlineHelperRecipeError> {
    node.children()
        .map(|id| {
            analysis
                .node(id)
                .map_err(|error| InlineHelperRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|nodes| nodes.into_iter().filter(|node| node.is_named()).collect())
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

fn plain(entity: GraphEntityRef, detail: &str) -> ConditionEvidence {
    ConditionEvidence {
        entity,
        detail: detail.into(),
        capability: None,
        support: None,
        authority: None,
    }
}

fn missing(kind: &str, identity: &str) -> InlineHelperRecipeError {
    InlineHelperRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    use deslop_core::Lang;
    use deslop_lang::{GrammarDescriptor, LangPack, RUST_PACK, Registry};
    use deslop_parse::{
        AdapterCapability, BindingDraft, BindingForm, BindingTargetDraft, BuildContextId,
        CallSiteDraft, CallableSummaryDraft, CanonicalRoleSet, CapabilityAuthority,
        CapabilityDeclaration, ControlEdgeDraft, ControlEdgeKind, ControlEdgePrecision,
        ControlExitOutcome, ControlFlowBuilder, ControlFlowCoverageEvidence, ControlFlowGraph,
        ControlFlowGraphDraft, ControlFlowOwnerKind, ControlFlowPolicyId, ControlPointDraft,
        ControlPointKind, ControlSyntheticPointKind, DataFlowAccessDraft, DataFlowAccessKind,
        DataFlowBuilder, DataFlowEffectDraft, DataFlowGraphDraft, DataFlowPolicyId,
        DeclarationDraft, DuplicateDefinitionRule, ExtractionFactKind, FactCoverage,
        FactCoverageEvidence, ImportTraversalRule, LanguageAdapterCapabilityManifest,
        LanguageConstructPolicy, LanguageControlFlowRulePack, LanguageLexicalPolicy,
        LanguageQueryPack, LanguageResolutionRulePack, Mutability, NameNamespace, NamespacePolicy,
        NonStructuredControlPolicyId, PrecedenceDimension, PrecedenceDirection,
        ProgramDependencePolicyId, ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft,
        ReferenceRole, RegionSpan, RepositoryId, ResolutionInstruction, ResolutionPolicyId,
        ResolutionRuleSection, ResolutionRuleSectionKind, ResolutionSyntaxSelector, RuleNamespace,
        ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind, SystemDependenceBuilder,
        SystemDependencePolicyId, VisibilityDraft, VisibilityKind, derive_control_regions,
        derive_non_structured_control_regions, derive_program_dependence,
    };

    use super::*;

    struct InlineTestPack;
    static INLINE_TEST_PACK: InlineTestPack = InlineTestPack;

    impl LangPack for InlineTestPack {
        fn name(&self) -> &'static str {
            "inline-test-rust"
        }

        fn adapter_schema(&self) -> &'static str {
            RUST_PACK.adapter_schema()
        }

        fn capability_manifest(&self) -> LanguageAdapterCapabilityManifest {
            let mut manifest = RUST_PACK.capability_manifest();
            for capability in [
                AdapterCapability::LexicalScopes,
                AdapterCapability::NameResolution,
                AdapterCapability::DefUse,
                AdapterCapability::Effects,
                AdapterCapability::LocalPdg,
                AdapterCapability::CallGraph,
                AdapterCapability::Sdg,
            ] {
                manifest = manifest
                    .with_declaration(CapabilityDeclaration::provided(
                        capability,
                        CapabilityAuthority::Adapter,
                    ))
                    .unwrap();
            }
            manifest
        }

        fn query_pack(&self) -> LanguageQueryPack {
            RUST_PACK.query_pack()
        }

        fn lexical_policy(&self) -> LanguageLexicalPolicy {
            RUST_PACK.lexical_policy()
        }

        fn construct_policy(&self) -> LanguageConstructPolicy {
            RUST_PACK.construct_policy()
        }

        fn control_flow_rule_pack(&self) -> LanguageControlFlowRulePack {
            RUST_PACK.control_flow_rule_pack()
        }

        fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
            let source = RUST_PACK.resolution_rule_pack();
            let mut sections = source.sections().to_vec();
            let index = |kind| {
                ResolutionRuleSectionKind::ALL
                    .iter()
                    .position(|candidate| *candidate == kind)
                    .unwrap()
            };
            sections[index(ResolutionRuleSectionKind::Extraction)] =
                ResolutionRuleSection::provided(
                    ResolutionRuleSectionKind::Extraction,
                    [
                        ExtractionFactKind::Declaration,
                        ExtractionFactKind::Definition,
                        ExtractionFactKind::Binding,
                        ExtractionFactKind::Reference,
                    ]
                    .into_iter()
                    .map(|fact_kind| ResolutionInstruction::ExtractFact {
                        selector: ResolutionSyntaxSelector::new("identifier", None, None).unwrap(),
                        name_field: None,
                        namespace: matches!(
                            fact_kind,
                            ExtractionFactKind::Declaration | ExtractionFactKind::Reference
                        )
                        .then_some(RuleNamespace::Value),
                        fact_kind,
                    })
                    .collect(),
                )
                .unwrap();
            sections[index(ResolutionRuleSectionKind::ImportsExports)] =
                ResolutionRuleSection::provided(
                    ResolutionRuleSectionKind::ImportsExports,
                    [
                        ImportTraversalRule::Explicit,
                        ImportTraversalRule::Selective,
                        ImportTraversalRule::Alias,
                        ImportTraversalRule::Glob,
                        ImportTraversalRule::Prelude,
                        ImportTraversalRule::Export,
                        ImportTraversalRule::ReExport,
                    ]
                    .into_iter()
                    .map(|rule| ResolutionInstruction::ImportTraversal { rule })
                    .collect(),
                )
                .unwrap();
            let duplicates = index(ResolutionRuleSectionKind::ShadowingDuplicates);
            sections[duplicates] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::ShadowingDuplicates,
                sections[duplicates]
                    .instructions()
                    .iter()
                    .cloned()
                    .map(|instruction| match instruction {
                        ResolutionInstruction::DuplicateDefinitions { namespace, .. } => {
                            ResolutionInstruction::DuplicateDefinitions {
                                namespace,
                                rule: DuplicateDefinitionRule::Ambiguous,
                            }
                        }
                        other => other,
                    })
                    .collect(),
            )
            .unwrap();
            sections[index(ResolutionRuleSectionKind::Precedence)] =
                ResolutionRuleSection::provided(
                    ResolutionRuleSectionKind::Precedence,
                    vec![ResolutionInstruction::Precedence {
                        terms: vec![
                            deslop_parse::PrecedenceTerm::new(
                                PrecedenceDimension::RuleStep,
                                PrecedenceDirection::LowerFirst,
                            ),
                            deslop_parse::PrecedenceTerm::new(
                                PrecedenceDimension::LexicalDistance,
                                PrecedenceDirection::LowerFirst,
                            ),
                            deslop_parse::PrecedenceTerm::new(
                                PrecedenceDimension::Namespace,
                                PrecedenceDirection::LowerFirst,
                            ),
                        ],
                    }],
                )
                .unwrap();
            LanguageResolutionRulePack::new(
                self.adapter_schema(),
                source.dialects().to_vec(),
                sections,
            )
            .unwrap()
        }

        fn canonical_roles(&self, node: tree_sitter::Node<'_>, text: &str) -> CanonicalRoleSet {
            RUST_PACK.canonical_roles(node, text)
        }

        fn lang(&self) -> Lang {
            Lang::Rust
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["inliners"]
        }

        fn grammar(&self) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar()
        }

        fn grammar_for_path(&self, path: &Path) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar_for_path(path)
        }

        fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
            RUST_PACK.grammar_descriptor_for_path(Path::new("fixture.rs"))
        }

        fn line_comments(&self) -> &'static [&'static str] {
            RUST_PACK.line_comments()
        }

        fn metrics_regions(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_regions()
        }

        fn metrics_branches(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_branches()
        }

        fn metrics_nesting(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_nesting()
        }

        fn metrics_flow_breaks(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_flow_breaks()
        }

        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            RUST_PACK.halstead_operator_tokens()
        }

        fn enclosing_region(&self, node: tree_sitter::Node<'_>, text: &str) -> Option<RegionSpan> {
            RUST_PACK.enclosing_region(node, text)
        }
    }

    struct Fixture {
        source: String,
        system: deslop_parse::SystemDependenceProjection,
    }

    fn fixture(call_count: usize, public: bool, value_reference: bool) -> Fixture {
        assert!((1..=2).contains(&call_count));
        let calls = "    helper();\n".repeat(call_count);
        let value_use = if value_reference {
            "    let _function_value = helper;\n"
        } else {
            ""
        };
        let source = format!(
            "{}fn helper() {{\n    1 + 2;\n}}\nfn run() {{\n{value_use}{calls}}}\nfn main() {{ run(); println!(\"ok\"); }}\n",
            if public { "pub " } else { "" }
        );
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::default();
        registry.register(&INLINE_TEST_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("inline-helper-test").unwrap(),
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
        let functions = nodes("function_item");
        let blocks = nodes("block");
        let call_nodes = nodes("call_expression");
        let binary = nodes("binary_expression")[0];
        let helper_calls = call_nodes
            .into_iter()
            .filter(|id| analysis.node(*id).unwrap().text() == "helper()")
            .collect::<Vec<_>>();
        assert_eq!(helper_calls.len(), call_count);
        let source_root = nodes("source_file")[0];
        let helper = functions[0];
        let run = functions[1];
        let helper_body = blocks[0];
        let run_body = blocks[1];
        let mut helper_identifiers = analysis
            .node_ids()
            .filter(|id| {
                let node = analysis.node(*id).unwrap();
                node.raw_grammar_kind() == "identifier" && node.text() == "helper"
            })
            .collect::<Vec<_>>();
        helper_identifiers.sort_by_key(|id| analysis.node(*id).unwrap().span().start_byte());
        assert_eq!(
            helper_identifiers.len(),
            call_count + 1 + usize::from(value_reference)
        );
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
        let namespace = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let mut scopes = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"inline-helper-build"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"inline-helper-scope"]).unwrap(),
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
                    namespace_policy: namespace.clone(),
                },
            )
            .unwrap();
        let _helper_scope = scopes
            .add_scope(
                helper,
                roles(helper),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(file_scope),
                    namespace_policy: namespace.clone(),
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
                    namespace_policy: namespace,
                },
            )
            .unwrap();
        let helper_declaration = scopes
            .add_declaration(
                helper,
                roles(helper),
                complete.clone(),
                DeclarationDraft {
                    original_name: "helper".into(),
                    lookup_key: "helper".into(),
                    namespace: NameNamespace::Value,
                    scope: file_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Scope,
                        boundary: Some(file_scope),
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        scopes
            .add_binding(
                helper,
                roles(helper),
                complete.clone(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(helper_declaration),
                    form: BindingForm::Declaration,
                    timing: deslop_parse::BindingTiming::AtDeclaration,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let references = helper_identifiers
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, identifier)| {
                let identifier = analysis.node(*identifier).unwrap();
                let parent = analysis.node(identifier.parent().unwrap()).unwrap();
                let (role, syntax) = if parent.raw_grammar_kind() == "call_expression" {
                    (ReferenceRole::Call, parent.id())
                } else {
                    (ReferenceRole::Read, identifier.id())
                };
                scopes
                    .add_reference(
                        identifier.id(),
                        roles(identifier.id()),
                        complete.clone(),
                        ReferenceDraft {
                            original_spelling: "helper".into(),
                            segments: vec!["helper".into()],
                            namespace: NameNamespace::Value,
                            scope: run_scope,
                            role: role.clone(),
                        },
                    )
                    .map(|fact| (index, fact, role, syntax))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let scopes = Arc::new(scopes.build().unwrap());
        let reference_keys = references
            .iter()
            .map(|(_, fact, role, syntax)| {
                (
                    scopes.fact(*fact).unwrap().key().clone(),
                    role.clone(),
                    *syntax,
                )
            })
            .collect::<Vec<_>>();
        let resolution = Arc::new(
            deslop_parse::ResolutionProjection::build(
                scopes,
                ResolutionPolicyId::from_parts(&[b"inline-helper-resolution"]).unwrap(),
            )
            .unwrap(),
        );
        assert!(resolution.results().iter().all(|result| {
            result.wire().status() == deslop_parse::ResolutionStatus::Unique
                && result.wire().coverage().status() == FactCoverage::Complete
        }));

        let mut flow = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"inline-helper-flow"]).unwrap(),
        );
        add_linear_flow(&mut flow, helper, helper_body, &[binary]);
        let mut run_syntax = reference_keys
            .iter()
            .map(|(_, _, syntax)| *syntax)
            .collect::<Vec<_>>();
        run_syntax.sort_by_key(|id| analysis.node(*id).unwrap().span().start_byte());
        add_linear_flow(&mut flow, run, run_body, &run_syntax);
        let flow = Arc::new(flow.build().unwrap());
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                deslop_parse::ControlRegionPolicyId::from_parts(&[b"inline-helper-regions"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let flow_for = |owner| {
            flow.document()
                .graphs()
                .iter()
                .find(|graph| graph.owner() == analysis.node_key(owner).unwrap())
                .unwrap()
        };
        let helper_flow = flow_for(helper);
        let run_flow = flow_for(run);
        let mut data = DataFlowBuilder::new(
            Arc::clone(&regions),
            resolution,
            DataFlowPolicyId::from_parts(&[b"inline-helper-data"]).unwrap(),
        )
        .unwrap();
        data.add_graph(DataFlowGraphDraft {
            control_flow_graph: helper_flow.key().clone(),
            definitions: vec![],
            accesses: vec![],
            boundaries: vec![],
            effects: empty_effects(helper_flow),
        })
        .unwrap();
        let access_points = reference_keys
            .iter()
            .map(|(_, _, syntax)| {
                run_flow
                    .points()
                    .iter()
                    .find(|point| point.source() == Some(analysis.node_key(*syntax).unwrap()))
                    .unwrap()
                    .key()
                    .clone()
            })
            .collect::<Vec<_>>();
        data.add_graph(DataFlowGraphDraft {
            control_flow_graph: run_flow.key().clone(),
            definitions: vec![],
            accesses: access_points
                .iter()
                .zip(&reference_keys)
                .enumerate()
                .map(
                    |(ordinal, (point, (reference, role, _)))| DataFlowAccessDraft {
                        point: point.clone(),
                        reference: reference.clone(),
                        kind: if *role == ReferenceRole::Call {
                            DataFlowAccessKind::Call
                        } else {
                            DataFlowAccessKind::Read
                        },
                        ordinal: ordinal as u32,
                    },
                )
                .collect(),
            boundaries: vec![],
            effects: empty_effects(run_flow),
        })
        .unwrap();
        let data = Arc::new(data.build().unwrap());
        let non_structured = Arc::new(
            derive_non_structured_control_regions(
                regions,
                NonStructuredControlPolicyId::from_parts(&[b"inline-helper-non-structured"])
                    .unwrap(),
            )
            .unwrap(),
        );
        let pdg = Arc::new(
            derive_program_dependence(
                Arc::clone(&data),
                non_structured,
                ProgramDependencePolicyId::from_parts(&[b"inline-helper-pdg"]).unwrap(),
            )
            .unwrap(),
        );
        assert!(
            pdg.document()
                .graphs()
                .iter()
                .all(|graph| { graph.coverage().status() == FactCoverage::Complete })
        );
        let pdg_for = |flow: &ControlFlowGraph| {
            pdg.document()
                .graphs()
                .iter()
                .find(|graph| graph.control_flow_graph() == flow.key())
                .unwrap()
        };
        let helper_pdg = pdg_for(helper_flow);
        let run_pdg = pdg_for(run_flow);
        let run_data = data
            .document()
            .graphs()
            .iter()
            .find(|graph| graph.control_flow_graph() == run_flow.key())
            .unwrap();
        let mut system = SystemDependenceBuilder::new(
            Arc::clone(&pdg),
            SystemDependencePolicyId::from_parts(&[b"inline-helper-system"]).unwrap(),
        );
        for graph in [helper_pdg, run_pdg] {
            system
                .add_summary(CallableSummaryDraft {
                    program_dependence_graph: graph.key().clone(),
                    formal_inputs: vec![],
                    outputs: vec![],
                    globals: vec![],
                })
                .unwrap();
        }
        for access in run_data
            .accesses()
            .iter()
            .filter(|access| access.kind() == DataFlowAccessKind::Call)
        {
            system.add_call_site(CallSiteDraft {
                caller: run_pdg.key().clone(),
                call: access.key().clone(),
                parameter_bindings: vec![],
                output_bindings: vec![],
            });
        }
        let system = system.build().unwrap();
        assert_eq!(
            system.document().coverage().status(),
            FactCoverage::Complete
        );
        Fixture { source, system }
    }

    fn add_linear_flow(
        builder: &mut ControlFlowBuilder,
        owner: deslop_parse::NodeId,
        body: deslop_parse::NodeId,
        syntax: &[deslop_parse::NodeId],
    ) {
        let mut points = vec![ControlPointDraft {
            kind: ControlPointKind::Entry,
            source: None,
            ordinal: 0,
        }];
        points.extend(
            syntax
                .iter()
                .enumerate()
                .map(|(ordinal, source)| ControlPointDraft {
                    kind: ControlPointKind::Syntax,
                    source: Some(*source),
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
                source: owner,
                predicate: None,
                precision: ControlEdgePrecision::Exact,
            })
            .collect();
        builder
            .add_graph(ControlFlowGraphDraft {
                owner,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points,
                edges,
            })
            .unwrap();
    }

    fn empty_effects(graph: &ControlFlowGraph) -> Vec<DataFlowEffectDraft> {
        graph
            .points()
            .iter()
            .map(|point| DataFlowEffectDraft {
                point: point.key().clone(),
                effects: vec![],
                uncertainty: None,
            })
            .collect()
    }

    fn apply_edits(source: &str, candidate: &TransformationCandidate) -> String {
        let mut output = source.to_string();
        let mut edits = candidate.edits().iter().collect::<Vec<_>>();
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
    fn complete_exact_single_use_emits_atomic_behavior_preserving_inline() {
        let fixture = fixture(1, false, false);
        let candidates = detect_inline_single_use_helpers(&fixture.system).unwrap();
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(
            candidate.recipe().name(),
            "rust-inline-exact-single-use-helper"
        );
        assert_eq!(candidate.edits().len(), 2);
        assert_eq!(
            candidate.disposition(),
            CandidateDisposition::ReviewRequired
        );
        let rewritten = apply_edits(&fixture.source, candidate);
        assert!(!rewritten.contains("fn helper"));
        assert!(rewritten.contains("{\n    1 + 2;\n}"));
        assert_eq!(run_rust(&fixture.source), run_rust(&rewritten));
    }

    #[test]
    fn second_exact_call_and_public_boundary_fail_closed() {
        assert!(
            detect_inline_single_use_helpers(&fixture(2, false, false).system)
                .unwrap()
                .is_empty()
        );
        assert!(
            detect_inline_single_use_helpers(&fixture(1, true, false).system)
                .unwrap()
                .is_empty()
        );
        assert!(
            detect_inline_single_use_helpers(&fixture(1, false, true).system)
                .unwrap()
                .is_empty()
        );
    }
}
