use std::collections::{BTreeSet, VecDeque};

use deslop_core::{Lang, SafetyClass};
use deslop_parse::{
    AdapterCapability, CapabilitySupport, ControlPointKey, DataFlowAccessKind,
    DataFlowBoundaryKind, DataFlowEffectKind, FactCoverage, GraphEvidenceLayer,
    ProgramDependenceEdgeKind, ProgramDependenceGraph, ProgramDependenceNode,
    ProgramDependenceProjection, ProjectAnalysis, StructuredControlRegion,
    StructuredControlRegionKind, evaluate_program_graph_recipe_eligibility,
};

use crate::branch::{condition, fixture, flow_entity, graph_entity, graph_root, result, span};
use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactDirection, ImpactQueryError, ProofState, RecipeContractError, RecipeFixtureRole,
    RollbackPlan, RollbackStrategy, TransformationCandidate, TransformationCandidateDraft,
    TransformationEdit, TransformationFamily, TransformationRecipe, TransformationRecipeDraft,
    ValidationPlan, ValidationStep, ValidationStepKind, program_dependence_impact_cone,
};

const REQUIRED_SESE: &str = "complete-sese-region";
const REQUIRED_SLICE: &str = "complete-computation-object-state-slice";
const REQUIRED_SIGNATURE: &str = "exact-bounded-helper-signature";
const REQUIRED_INPUTS: &str = "exact-extraction-inputs";
const REQUIRED_OUTPUTS: &str = "exact-extraction-outputs";
const REQUIRED_MUTATIONS: &str = "exact-extraction-mutation-frontier";
const REQUIRED_EXITS: &str = "exact-extraction-exits";
const REQUIRED_EXCEPTIONS: &str = "exact-extraction-exceptions";
const REQUIRED_CAPTURES: &str = "exact-extraction-captures";
const REQUIRED_ASYNC_OWNERSHIP: &str = "exact-extraction-async-ownership";
const REQUIRED_EFFECTS: &str = "call-boundary-effects-reviewed";
const FORBIDDEN_ABRUPT: &str = "abrupt-or-suspending-exit";
const FORBIDDEN_SCOPE: &str = "unmodelled-local-or-owned-input";
const FORBIDDEN_NON_STRUCTURED: &str = "non-structured-control-intersection";
const FORBIDDEN_COLLISION: &str = "helper-name-collision";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionSliceEvidence {
    pub region: GraphEntityRef,
    pub computation_entities: Vec<GraphEntityRef>,
    pub object_state_entities: Vec<GraphEntityRef>,
    pub boundary_flow_entities: Vec<GraphEntityRef>,
    pub completeness: ProofState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionInputOrigin {
    Parameter,
    PriorLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionOwnershipMode {
    CopyValue,
    SharedBorrow,
    MutableReborrow,
    OwnedReturn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionInputEvidence {
    pub declaration: deslop_parse::NodeKey,
    pub name: String,
    pub type_text: String,
    pub origin: ExtractionInputOrigin,
    pub ownership: ExtractionOwnershipMode,
    pub direct_mutation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionOutputEvidence {
    pub binding: deslop_parse::NodeKey,
    pub name: String,
    pub type_text: String,
    pub ownership: ExtractionOwnershipMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionSignatureEvidence {
    pub inputs: Vec<ExtractionInputEvidence>,
    pub output: Option<ExtractionOutputEvidence>,
    pub exits: Vec<GraphEntityRef>,
    pub exceptions: Vec<GraphEntityRef>,
    pub captures: Vec<GraphEntityRef>,
    pub suspensions: Vec<GraphEntityRef>,
    pub mutation_completeness: ProofState,
    pub exception_completeness: ProofState,
    pub capture_completeness: ProofState,
    pub ownership_completeness: ProofState,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractMethodRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error(transparent)]
    Impact(#[from] ImpactQueryError),
    #[error("extract-method graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("extract-method recipe received an inconsistent projection: {0}")]
    Projection(String),
}

struct InputShape<'a> {
    declaration: deslop_parse::NodeView<'a>,
    value_type: deslop_parse::NodeView<'a>,
    name: String,
    origin: ExtractionInputOrigin,
    ownership: ExtractionOwnershipMode,
    direct_mutation: bool,
}

struct OutputShape<'a> {
    binding: deslop_parse::NodeView<'a>,
    value_type: deslop_parse::NodeView<'a>,
    name: String,
}

struct ExtractionShape<'a> {
    owner: deslop_parse::NodeView<'a>,
    replacement: deslop_parse::NodeView<'a>,
    branch: deslop_parse::NodeView<'a>,
    inputs: Vec<InputShape<'a>>,
    output: Option<OutputShape<'a>>,
    helper_name: String,
}

pub fn extract_method_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    TransformationRecipe::new(TransformationRecipeDraft {
        name: "rust-extract-sese-branch-method".into(),
        version: "2.0.0".into(),
        family: TransformationFamily::FunctionExpression,
        required_layers: vec![
            GraphEvidenceLayer::ControlFlow,
            GraphEvidenceLayer::ControlRegions,
            GraphEvidenceLayer::NonStructuredControl,
            GraphEvidenceLayer::DataFlow,
            GraphEvidenceLayer::ProgramDependence,
        ],
        required_conditions: vec![
            condition(
                REQUIRED_SESE,
                "The selected direct-body branch is a retained complete single-entry/single-exit region.",
                GraphEvidenceLayer::ControlRegions,
            ),
            condition(
                REQUIRED_SLICE,
                "The computation and object-state slice is closed under authoritative local dependence facts.",
                GraphEvidenceLayer::ProgramDependence,
            ),
            condition(
                REQUIRED_SIGNATURE,
                "The bounded helper signature contains exact used primitive/reference inputs and optional typed output.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_INPUTS,
                "Every used parameter or prior local crosses the helper boundary exactly once with its stored type.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_OUTPUTS,
                "The extraction has unit output or one exact explicitly typed returned initializer.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_MUTATIONS,
                "Mutable reborrows and direct syntactic writes are retained explicitly.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_EXITS,
                "No abrupt exit crosses the new callable boundary.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                REQUIRED_EXCEPTIONS,
                "Exceptional behavior is retained from complete typed effect evidence.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_CAPTURES,
                "The free helper has no implicit capture outside its explicit input frontier.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_ASYNC_OWNERSHIP,
                "The helper is synchronous and every value is copied, borrowed, reborrowed, or returned explicitly.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                REQUIRED_EFFECTS,
                "Moving the region across a call boundary is accepted only after effect review.",
                GraphEvidenceLayer::DataFlow,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                FORBIDDEN_ABRUPT,
                "The region contains a return, break, continue, try, await, yield, or other callable-boundary exit.",
                GraphEvidenceLayer::ControlFlow,
            ),
            condition(
                FORBIDDEN_SCOPE,
                "The region depends on a prior local, receiver, generic, capture, or owned value.",
                GraphEvidenceLayer::DataFlow,
            ),
            condition(
                FORBIDDEN_NON_STRUCTURED,
                "A retained non-structured control fact intersects the region.",
                GraphEvidenceLayer::NonStructuredControl,
            ),
            condition(
                FORBIDDEN_COLLISION,
                "The generated private helper name already exists in the source file.",
                GraphEvidenceLayer::ControlFlow,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: ValidationPlan {
            steps: vec![
                validation(
                    "build",
                    ValidationStepKind::Build,
                    "Build the extracted helper and call site.",
                ),
                validation(
                    "graph-delta",
                    ValidationStepKind::GraphDelta,
                    "Rebuild and compare the selected SESE region and dependence slice.",
                ),
                validation(
                    "parse",
                    ValidationStepKind::Parse,
                    "Parse the exact multi-function replacement.",
                ),
                validation(
                    "test",
                    ValidationStepKind::Test,
                    "Run project tests before accepting the extraction.",
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
                "direct-branch-with-reference-state",
                FixtureExpectation::Candidate,
                "A direct branch over primitive/reference parameters has an exact helper transaction.",
            ),
            fixture(
                RecipeFixtureRole::NoOp,
                "no-direct-branch-region",
                FixtureExpectation::NoCandidate,
                "A callable without a direct branch has no extraction unit.",
            ),
            fixture(
                RecipeFixtureRole::MinimalCounterexample,
                "production-defuse-unknown",
                FixtureExpectation::ReviewRequired,
                "The exact edit remains review-only while production slice authority is incomplete.",
            ),
            fixture(
                RecipeFixtureRole::AdversarialNearMiss,
                "branch-reads-untyped-prior-local",
                FixtureExpectation::NoCandidate,
                "A used prior local without an exact type cannot enter the helper signature.",
            ),
        ],
    })
}

pub fn detect_extract_method_candidates(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, ExtractMethodRecipeError> {
    let recipe = extract_method_recipe()?;
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
        .map_err(|error| ExtractMethodRecipeError::Eligibility(error.to_string()))?;
        let flow_graph = flow
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
        let owner = analysis
            .node_by_key(graph.owner())
            .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;

        for region in region_graph
            .regions()
            .iter()
            .filter(|region| region.kind() == StructuredControlRegionKind::Branch)
        {
            if non_structured_graph
                .facts()
                .iter()
                .any(|fact| intersects_sorted(fact.points(), region.points()))
            {
                continue;
            }
            let Some(dispatch) = flow_graph
                .points()
                .iter()
                .find(|point| point.key() == region.entry())
            else {
                return Err(missing(
                    "control-flow region entry",
                    region.entry().as_str(),
                ));
            };
            let Some(source) = dispatch.source() else {
                continue;
            };
            let branch = analysis
                .node_by_key(source)
                .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;
            let Some(shape) = extraction_shape(analysis, owner, branch)? else {
                continue;
            };
            let Some(root) = graph
                .nodes()
                .iter()
                .find(|node| node.point() == region.entry())
            else {
                return Err(missing(
                    "program-dependence region entry",
                    region.entry().as_str(),
                ));
            };
            let slice = extraction_slice(graph, region, data_graph);
            let signature = extraction_signature(region, data_graph, &shape);
            let replacement = render_extraction(&shape)?;
            let target_span = span(graph.owner());
            let signature_evidence = signature_evidence(data_graph, &signature);
            let slice_evidence = slice_condition_evidence(graph, data_graph, &slice);
            let region_state = if region_graph.coverage().status() == FactCoverage::Complete {
                ProofState::Proven
            } else {
                ProofState::Unknown
            };
            candidates.push(TransformationCandidate::new(TransformationCandidateDraft {
                recipe: recipe.clone(),
                source: CandidateSource {
                    project_snapshot: analysis.snapshot().id().as_str().into(),
                    analysis: analysis.id().as_str().into(),
                    program_dependence_projection: projection.id().as_str().into(),
                },
                target: CandidateTarget {
                    entity: graph_root(graph, root),
                    node: graph.owner().clone(),
                    span: target_span,
                },
                eligibility: eligibility.clone(),
                required_results: vec![
                    result(
                        REQUIRED_SESE,
                        region_state,
                        graph_entity(
                            GraphEvidenceLayer::ControlRegions,
                            region_graph.key().as_str(),
                            region.key().as_str(),
                        ),
                        if region_state == ProofState::Proven {
                            "Complete region coverage proves the exact branch SESE boundary."
                        } else {
                            "The retained branch is structured, but inherited control coverage is incomplete."
                        },
                    ),
                    ConditionResult {
                        condition: REQUIRED_SLICE.into(),
                        state: slice.completeness,
                        evidence: slice_evidence,
                    },
                    ConditionResult {
                        condition: REQUIRED_SIGNATURE.into(),
                        state: ProofState::Proven,
                        evidence: signature_evidence.clone(),
                    },
                    signature_inputs_result(data_graph, &signature),
                    signature_output_result(data_graph, &signature),
                    signature_mutation_result(data_graph, &signature),
                    result(
                        REQUIRED_EXITS,
                        ProofState::Proven,
                        flow_entity(flow_graph.key().as_str(), region.entry().as_str()),
                        "The selected exact CST has no return, break, continue, try, yield, or suspension boundary.",
                    ),
                    signature_effect_result(
                        REQUIRED_EXCEPTIONS,
                        signature.exception_completeness,
                        data_graph,
                        &signature.exceptions,
                        "No typed exceptional output is retained inside the selected region.",
                        "Typed exceptional output retained inside the selected region.",
                    ),
                    signature_capture_result(data_graph, &signature),
                    signature_async_ownership_result(data_graph, &signature),
                    capability_condition(
                        REQUIRED_EFFECTS,
                        ProofState::Unknown,
                        graph_entity(
                            GraphEvidenceLayer::DataFlow,
                            data_graph.key().as_str(),
                            data_graph.key().as_str(),
                        ),
                        "A new call frame can affect caller-location, panic, allocation, or hidden state observations and requires review.",
                        AdapterCapability::Effects,
                        data_graph.coverage().effects_support(),
                        data_graph.coverage().effects_authority(),
                    ),
                ],
                forbidden_results: vec![
                    result(
                        FORBIDDEN_ABRUPT,
                        ProofState::Disproven,
                        flow_entity(flow_graph.key().as_str(), region.entry().as_str()),
                        "The exact selected CST contains no abrupt, exceptional, or suspending exit.",
                    ),
                    result(
                        FORBIDDEN_SCOPE,
                        ProofState::Disproven,
                        graph_entity(
                            GraphEvidenceLayer::DataFlow,
                            data_graph.key().as_str(),
                            data_graph.key().as_str(),
                        ),
                        "Every used prior local or parameter has an exact primitive/reference type and explicit ownership mode.",
                    ),
                    result(
                        FORBIDDEN_NON_STRUCTURED,
                        ProofState::Disproven,
                        graph_entity(
                            GraphEvidenceLayer::NonStructuredControl,
                            non_structured_graph.key().as_str(),
                            non_structured_graph.key().as_str(),
                        ),
                        "No retained non-structured control fact intersects the selected region.",
                    ),
                    result(
                        FORBIDDEN_COLLISION,
                        ProofState::Disproven,
                        flow_entity(flow_graph.key().as_str(), region.entry().as_str()),
                        "The byte-derived helper name is absent from the exact source file.",
                    ),
                ],
                impact: program_dependence_impact_cone(
                    projection,
                    graph.key(),
                    root.key(),
                    ImpactDirection::Bidirectional,
                    12,
                )?,
                expected_delta: extraction_delta(graph, root, &slice),
                edits: vec![TransformationEdit::exact_node_replacement(
                    graph.owner().clone(),
                    target_span,
                    owner.text().into(),
                    replacement,
                )],
                safety: SafetyClass::SafeWithPrecondition,
                disposition: CandidateDisposition::ReviewRequired,
                validation_plan: recipe.validation_plan().clone(),
                rollback_plan: recipe.rollback_plan().clone(),
            })?);
        }
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

fn extraction_shape<'a>(
    analysis: &'a ProjectAnalysis,
    owner: deslop_parse::NodeView<'a>,
    branch: deslop_parse::NodeView<'a>,
) -> Result<Option<ExtractionShape<'a>>, ExtractMethodRecipeError> {
    if owner.grammar().lang() != Lang::Rust
        || owner.raw_grammar_kind() != "function_item"
        || owner.has_error()
        || branch.raw_grammar_kind() != "if_expression"
        || branch.has_error()
        || contains_forbidden_syntax(analysis, branch)?
    {
        return Ok(None);
    }
    let Some(parent) = owner.parent() else {
        return Ok(None);
    };
    let parent = analysis
        .node(parent)
        .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;
    if parent.raw_grammar_kind() != "source_file"
        || attached_outer_attribute(analysis, parent, owner)?
    {
        return Ok(None);
    }
    let prefix = owner
        .text()
        .split_once("fn ")
        .map(|(prefix, _)| prefix)
        .unwrap_or(owner.text());
    if ["async", "const", "unsafe", "extern"]
        .iter()
        .any(|word| prefix.split_whitespace().any(|part| part == *word))
        || child_by_field(analysis, owner, "type_parameters")?.is_some()
        || child_by_field(analysis, owner, "where_clause")?.is_some()
    {
        return Ok(None);
    }
    let Some(parameters) = child_by_field(analysis, owner, "parameters")? else {
        return Ok(None);
    };
    let Some(name) = child_by_field(analysis, owner, "name")? else {
        return Ok(None);
    };
    if name.text().starts_with("__deslop_extract_branch_") {
        return Ok(None);
    }
    let Some(body) = child_by_field(analysis, owner, "body")? else {
        return Ok(None);
    };
    let Some((statement, replacement, output)) = extraction_site(analysis, body, branch)? else {
        return Ok(None);
    };
    let statements = named_children(analysis, body)?;
    let Some(position) = statements
        .iter()
        .position(|candidate| candidate.id() == statement.id())
    else {
        return Ok(None);
    };
    if branch_action_count(analysis, branch)? < 2 {
        return Ok(None);
    }
    let used = used_identifier_names(analysis, branch)?;
    let mut inputs = Vec::new();
    for parameter in named_children(analysis, parameters)? {
        if parameter.raw_grammar_kind() != "parameter" {
            return Ok(None);
        }
        let Some(pattern) = child_by_field(analysis, parameter, "pattern")? else {
            return Ok(None);
        };
        let Some(value_type) = child_by_field(analysis, parameter, "type")? else {
            return Ok(None);
        };
        if pattern.raw_grammar_kind() != "identifier" {
            return Ok(None);
        }
        if used.contains(pattern.text()) {
            let Some(ownership) = supported_input_type(value_type) else {
                return Ok(None);
            };
            inputs.push(InputShape {
                declaration: parameter,
                value_type,
                name: pattern.text().into(),
                origin: ExtractionInputOrigin::Parameter,
                ownership,
                direct_mutation: direct_mutation(analysis, branch, pattern.text())?,
            });
        }
    }
    for local in statements[..position]
        .iter()
        .filter(|candidate| candidate.raw_grammar_kind() == "let_declaration")
    {
        let Some(pattern) = child_by_field(analysis, *local, "pattern")? else {
            return Ok(None);
        };
        if pattern.raw_grammar_kind() != "identifier" {
            return Ok(None);
        }
        if !used.contains(pattern.text()) {
            continue;
        }
        let Some(value_type) = child_by_field(analysis, *local, "type")? else {
            return Ok(None);
        };
        let Some(ownership) = supported_input_type(value_type) else {
            return Ok(None);
        };
        inputs.push(InputShape {
            declaration: *local,
            value_type,
            name: pattern.text().into(),
            origin: ExtractionInputOrigin::PriorLocal,
            ownership,
            direct_mutation: direct_mutation(analysis, branch, pattern.text())?,
        });
    }
    inputs.sort_by_key(|input| input.declaration.key().anchor().start_byte());
    let helper_name = format!(
        "__deslop_extract_branch_{}",
        branch.key().anchor().start_byte()
    );
    if parent.text().contains(&helper_name) {
        return Ok(None);
    }
    Ok(Some(ExtractionShape {
        owner,
        replacement,
        branch,
        inputs,
        output,
        helper_name,
    }))
}

fn extraction_site<'a>(
    analysis: &'a ProjectAnalysis,
    body: deslop_parse::NodeView<'a>,
    branch: deslop_parse::NodeView<'a>,
) -> Result<
    Option<(
        deslop_parse::NodeView<'a>,
        deslop_parse::NodeView<'a>,
        Option<OutputShape<'a>>,
    )>,
    ExtractMethodRecipeError,
> {
    let Some(parent_id) = branch.parent() else {
        return Ok(None);
    };
    let parent = analysis
        .node(parent_id)
        .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;
    if parent.raw_grammar_kind() == "expression_statement" && parent.parent() == Some(body.id()) {
        return Ok(Some((parent, parent, None)));
    }
    if parent.raw_grammar_kind() != "let_declaration" || parent.parent() != Some(body.id()) {
        return Ok(None);
    }
    let Some(value) = child_by_field(analysis, parent, "value")? else {
        return Ok(None);
    };
    let Some(binding) = child_by_field(analysis, parent, "pattern")? else {
        return Ok(None);
    };
    let Some(value_type) = child_by_field(analysis, parent, "type")? else {
        return Ok(None);
    };
    if value.id() != branch.id()
        || binding.raw_grammar_kind() != "identifier"
        || !supported_output_type(value_type)
    {
        return Ok(None);
    }
    Ok(Some((
        parent,
        branch,
        Some(OutputShape {
            binding,
            value_type,
            name: binding.text().into(),
        }),
    )))
}

fn supported_input_type(value_type: deslop_parse::NodeView<'_>) -> Option<ExtractionOwnershipMode> {
    if value_type.text().contains('\'')
        || value_type.text().contains("impl ")
        || value_type.text().contains("dyn ")
    {
        return None;
    }
    if value_type.raw_grammar_kind() == "reference_type" {
        return Some(if value_type.text().trim_start().starts_with("&mut ") {
            ExtractionOwnershipMode::MutableReborrow
        } else {
            ExtractionOwnershipMode::SharedBorrow
        });
    }
    primitive_type(value_type).then_some(ExtractionOwnershipMode::CopyValue)
}

fn supported_output_type(value_type: deslop_parse::NodeView<'_>) -> bool {
    primitive_type(value_type)
}

fn primitive_type(value_type: deslop_parse::NodeView<'_>) -> bool {
    value_type.raw_grammar_kind() == "primitive_type"
        || matches!(
            value_type.text(),
            "bool"
                | "char"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
        )
}

fn attached_outer_attribute(
    analysis: &ProjectAnalysis,
    source_file: deslop_parse::NodeView<'_>,
    owner: deslop_parse::NodeView<'_>,
) -> Result<bool, ExtractMethodRecipeError> {
    let children = named_children(analysis, source_file)?;
    let Some(index) = children
        .iter()
        .position(|candidate| candidate.id() == owner.id())
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
    branch: deslop_parse::NodeView<'_>,
) -> Result<bool, ExtractMethodRecipeError> {
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
                | "for_expression"
                | "match_expression"
                | "line_comment"
                | "block_comment"
        )
    };
    if forbidden(branch.raw_grammar_kind()) {
        return Ok(true);
    }
    Ok(analysis
        .descendant_node_ids(branch.id())
        .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?
        .any(|id| {
            analysis
                .node(id)
                .is_ok_and(|node| node.has_error() || forbidden(node.raw_grammar_kind()))
        }))
}

fn used_identifier_names(
    analysis: &ProjectAnalysis,
    branch: deslop_parse::NodeView<'_>,
) -> Result<BTreeSet<String>, ExtractMethodRecipeError> {
    let mut names = BTreeSet::new();
    for id in analysis
        .descendant_node_ids(branch.id())
        .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?
    {
        let node = analysis
            .node(id)
            .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;
        if node.raw_grammar_kind() == "identifier" && node.field() != Some("field") {
            names.insert(node.text().to_string());
        }
    }
    Ok(names)
}

fn direct_mutation(
    analysis: &ProjectAnalysis,
    branch: deslop_parse::NodeView<'_>,
    name: &str,
) -> Result<bool, ExtractMethodRecipeError> {
    for id in analysis
        .descendant_node_ids(branch.id())
        .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?
    {
        let assignment = analysis
            .node(id)
            .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;
        if !matches!(
            assignment.raw_grammar_kind(),
            "assignment_expression" | "compound_assignment_expr"
        ) {
            continue;
        }
        let Some(left) = child_by_field(analysis, assignment, "left")? else {
            continue;
        };
        if left.raw_grammar_kind() == "identifier" && left.text() == name {
            return Ok(true);
        }
        for descendant in analysis
            .descendant_node_ids(left.id())
            .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?
        {
            if analysis.node(descendant).is_ok_and(|node| {
                node.raw_grammar_kind() == "identifier"
                    && node.field() != Some("field")
                    && node.text() == name
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn branch_action_count(
    analysis: &ProjectAnalysis,
    branch: deslop_parse::NodeView<'_>,
) -> Result<usize, ExtractMethodRecipeError> {
    Ok(analysis
        .descendant_node_ids(branch.id())
        .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?
        .filter(|id| {
            analysis.node(*id).is_ok_and(|node| {
                matches!(
                    node.raw_grammar_kind(),
                    "expression_statement" | "let_declaration"
                )
            })
        })
        .count())
}

fn render_extraction(shape: &ExtractionShape<'_>) -> Result<String, ExtractMethodRecipeError> {
    let owner_start = shape.owner.key().anchor().start_byte() as usize;
    let replacement_start = shape.replacement.key().anchor().start_byte() as usize;
    let replacement_end = shape.replacement.key().anchor().end_byte() as usize;
    let relative_start = replacement_start.checked_sub(owner_start).ok_or_else(|| {
        ExtractMethodRecipeError::Projection("replacement begins before callable owner".into())
    })?;
    let relative_end = replacement_end.checked_sub(owner_start).ok_or_else(|| {
        ExtractMethodRecipeError::Projection("replacement ends before callable owner".into())
    })?;
    let mut original = shape.owner.text().to_string();
    if original.get(relative_start..relative_end) != Some(shape.replacement.text()) {
        return Err(ExtractMethodRecipeError::Projection(
            "replacement bytes do not match the retained callable source".into(),
        ));
    }
    let arguments = shape
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let declarations = shape
        .inputs
        .iter()
        .map(|input| format!("{}: {}", input.name, input.value_type.text()))
        .collect::<Vec<_>>()
        .join(", ");
    let (return_type, helper_body, call) = match &shape.output {
        Some(output) => (
            format!(" -> {}", output.value_type.text()),
            shape.branch.text().to_string(),
            format!("{}({arguments})", shape.helper_name),
        ),
        None => (
            String::new(),
            format!("{};", shape.branch.text()),
            format!("{}({arguments});", shape.helper_name),
        ),
    };
    original.replace_range(relative_start..relative_end, &call);
    Ok(format!(
        "fn {}({declarations}){return_type} {{\n    {helper_body}\n}}\n\n{}",
        shape.helper_name, original
    ))
}

fn extraction_slice(
    graph: &ProgramDependenceGraph,
    region: &StructuredControlRegion,
    data: &deslop_parse::DataFlowGraph,
) -> ExtractionSliceEvidence {
    let mut computation = graph
        .nodes()
        .iter()
        .filter(|node| region.points().binary_search(node.point()).is_ok())
        .map(|node| node.key().clone())
        .collect::<BTreeSet<_>>();
    let mut pending = computation.iter().cloned().collect::<VecDeque<_>>();
    while let Some(current) = pending.pop_front() {
        for adjacent in graph.edges().iter().filter_map(|edge| {
            if !matches!(edge.kind(), ProgramDependenceEdgeKind::Flow { .. }) {
                return None;
            }
            if edge.from() == &current {
                Some(edge.to())
            } else if edge.to() == &current {
                Some(edge.from())
            } else {
                None
            }
        }) {
            if computation.insert(adjacent.clone()) {
                pending.push_back(adjacent.clone());
            }
        }
    }
    let flow_edges = graph
        .edges()
        .iter()
        .filter(|edge| {
            matches!(edge.kind(), ProgramDependenceEdgeKind::Flow { .. })
                && (computation.contains(edge.from()) || computation.contains(edge.to()))
        })
        .map(|edge| {
            graph_entity(
                GraphEvidenceLayer::ProgramDependence,
                graph.key().as_str(),
                edge.key().as_str(),
            )
        })
        .collect::<Vec<_>>();
    let object_state = data
        .boundaries()
        .iter()
        .filter(|boundary| region.points().binary_search(boundary.point()).is_ok())
        .map(|boundary| {
            graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                boundary.key().as_str(),
            )
        })
        .chain(
            data.effects()
                .iter()
                .filter(|effect| region.points().binary_search(effect.point()).is_ok())
                .map(|effect| {
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data.key().as_str(),
                        effect.key().as_str(),
                    )
                }),
        )
        .collect::<Vec<_>>();
    let authoritative = data.coverage().status() == FactCoverage::Complete
        && graph.coverage().status() == FactCoverage::Complete
        && graph.gaps().is_empty()
        && data.coverage().def_use_support() == CapabilitySupport::Provided
        && data.coverage().effects_support() == CapabilitySupport::Provided
        && graph.coverage().local_pdg_support() == CapabilitySupport::Provided
        && data.coverage().def_use_authority().is_some()
        && data.coverage().effects_authority().is_some()
        && graph.coverage().local_pdg_authority().is_some();
    ExtractionSliceEvidence {
        region: graph_entity(
            GraphEvidenceLayer::ControlRegions,
            graph.control_region_graph().as_str(),
            region.key().as_str(),
        ),
        computation_entities: computation
            .iter()
            .map(|node| {
                graph_entity(
                    GraphEvidenceLayer::ProgramDependence,
                    graph.key().as_str(),
                    node.as_str(),
                )
            })
            .collect(),
        object_state_entities: object_state,
        boundary_flow_entities: flow_edges,
        completeness: if authoritative {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
    }
}

fn slice_condition_evidence(
    graph: &ProgramDependenceGraph,
    data: &deslop_parse::DataFlowGraph,
    slice: &ExtractionSliceEvidence,
) -> Vec<ConditionEvidence> {
    let mut evidence = slice
        .computation_entities
        .iter()
        .cloned()
        .map(|entity| ConditionEvidence {
            entity,
            detail: "Computation-slice node retained by flow closure.".into(),
            capability: Some(AdapterCapability::LocalPdg),
            support: Some(graph.coverage().local_pdg_support()),
            authority: graph.coverage().local_pdg_authority(),
        })
        .collect::<Vec<_>>();
    evidence.extend(
        slice
            .boundary_flow_entities
            .iter()
            .cloned()
            .map(|entity| ConditionEvidence {
                entity,
                detail: "Flow edge retained by the complete computation closure.".into(),
                capability: Some(AdapterCapability::DefUse),
                support: Some(data.coverage().def_use_support()),
                authority: data.coverage().def_use_authority(),
            }),
    );
    evidence
}

fn extraction_signature(
    region: &StructuredControlRegion,
    data: &deslop_parse::DataFlowGraph,
    shape: &ExtractionShape<'_>,
) -> ExtractionSignatureEvidence {
    let mut exceptions = data
        .boundaries()
        .iter()
        .filter(|boundary| {
            region.points().binary_search(boundary.point()).is_ok()
                && boundary.kind() == DataFlowBoundaryKind::ExceptionalOutput
        })
        .map(|boundary| {
            graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                boundary.key().as_str(),
            )
        })
        .chain(
            data.effects()
                .iter()
                .filter(|effect| {
                    region.points().binary_search(effect.point()).is_ok()
                        && effect.effects().contains(&DataFlowEffectKind::Throws)
                })
                .map(|effect| {
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data.key().as_str(),
                        effect.key().as_str(),
                    )
                }),
        )
        .collect::<Vec<_>>();
    let mut captures = data
        .accesses()
        .iter()
        .filter(|access| {
            region.points().binary_search(access.point()).is_ok()
                && access.kind() == DataFlowAccessKind::Capture
        })
        .map(|access| {
            graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                access.key().as_str(),
            )
        })
        .collect::<Vec<_>>();
    let mut suspensions = data
        .boundaries()
        .iter()
        .filter(|boundary| {
            region.points().binary_search(boundary.point()).is_ok()
                && boundary.kind() == DataFlowBoundaryKind::SuspensionOutput
        })
        .map(|boundary| {
            graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                boundary.key().as_str(),
            )
        })
        .chain(
            data.effects()
                .iter()
                .filter(|effect| {
                    region.points().binary_search(effect.point()).is_ok()
                        && effect.effects().contains(&DataFlowEffectKind::Suspends)
                })
                .map(|effect| {
                    graph_entity(
                        GraphEvidenceLayer::DataFlow,
                        data.key().as_str(),
                        effect.key().as_str(),
                    )
                }),
        )
        .collect::<Vec<_>>();
    for entities in [&mut exceptions, &mut captures, &mut suspensions] {
        entities.sort();
        entities.dedup();
    }
    let effect_complete = data.coverage().status() == FactCoverage::Complete
        && data.coverage().effects_support() == CapabilitySupport::Provided
        && data.coverage().effects_authority().is_some();
    let mutation_complete = effect_complete
        && data.coverage().def_use_support() == CapabilitySupport::Provided
        && data.coverage().def_use_authority().is_some();
    let no_captures = captures.is_empty();
    let no_suspensions = suspensions.is_empty();
    ExtractionSignatureEvidence {
        inputs: shape
            .inputs
            .iter()
            .map(|input| ExtractionInputEvidence {
                declaration: input.declaration.key().clone(),
                name: input.name.clone(),
                type_text: input.value_type.text().into(),
                origin: input.origin,
                ownership: input.ownership,
                direct_mutation: input.direct_mutation,
            })
            .collect(),
        output: shape
            .output
            .as_ref()
            .map(|output| ExtractionOutputEvidence {
                binding: output.binding.key().clone(),
                name: output.name.clone(),
                type_text: output.value_type.text().into(),
                ownership: ExtractionOwnershipMode::OwnedReturn,
            }),
        exits: Vec::new(),
        exceptions,
        captures,
        suspensions,
        mutation_completeness: if mutation_complete {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
        exception_completeness: if effect_complete {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
        capture_completeness: if no_captures {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
        ownership_completeness: if no_suspensions {
            ProofState::Proven
        } else {
            ProofState::Unknown
        },
    }
}

fn signature_evidence(
    data: &deslop_parse::DataFlowGraph,
    signature: &ExtractionSignatureEvidence,
) -> Vec<ConditionEvidence> {
    if signature.inputs.is_empty() && signature.output.is_none() {
        return vec![ConditionEvidence {
            entity: graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                data.key().as_str(),
            ),
            detail: "The exact helper signature has zero inputs and unit output.".into(),
            capability: None,
            support: None,
            authority: None,
        }];
    }
    let mut evidence = signature
        .inputs
        .iter()
        .map(|input| ConditionEvidence {
            entity: graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                data.key().as_str(),
            ),
            detail: format!(
                "Exact helper input `{}: {}` has {:?} origin and {:?} ownership.",
                input.name, input.type_text, input.origin, input.ownership
            ),
            capability: None,
            support: None,
            authority: None,
        })
        .collect::<Vec<_>>();
    if let Some(output) = &signature.output {
        evidence.push(ConditionEvidence {
            entity: graph_entity(
                GraphEvidenceLayer::DataFlow,
                data.key().as_str(),
                data.key().as_str(),
            ),
            detail: format!(
                "Exact helper output `{}: {}` crosses as {:?}.",
                output.name, output.type_text, output.ownership
            ),
            capability: None,
            support: None,
            authority: None,
        });
    }
    evidence
}

fn signature_inputs_result(
    data: &deslop_parse::DataFlowGraph,
    signature: &ExtractionSignatureEvidence,
) -> ConditionResult {
    let evidence = if signature.inputs.is_empty() {
        vec![plain_data_evidence(
            data,
            "The selected region has an exact empty explicit input frontier.",
        )]
    } else {
        signature
            .inputs
            .iter()
            .map(|input| {
                plain_data_evidence(
                    data,
                    &format!(
                        "Input `{}: {}` originates at {:?} and crosses as {:?}.",
                        input.name, input.type_text, input.origin, input.ownership
                    ),
                )
            })
            .collect()
    };
    ConditionResult {
        condition: REQUIRED_INPUTS.into(),
        state: ProofState::Proven,
        evidence,
    }
}

fn signature_output_result(
    data: &deslop_parse::DataFlowGraph,
    signature: &ExtractionSignatureEvidence,
) -> ConditionResult {
    let detail = signature.output.as_ref().map_or_else(
        || "The direct statement extraction has exact unit output.".to_string(),
        |output| {
            format!(
                "The helper returns exact output `{}: {}` into the retained let binding.",
                output.name, output.type_text
            )
        },
    );
    ConditionResult {
        condition: REQUIRED_OUTPUTS.into(),
        state: ProofState::Proven,
        evidence: vec![plain_data_evidence(data, &detail)],
    }
}

fn signature_mutation_result(
    data: &deslop_parse::DataFlowGraph,
    signature: &ExtractionSignatureEvidence,
) -> ConditionResult {
    let mutations = signature
        .inputs
        .iter()
        .filter(|input| input.ownership == ExtractionOwnershipMode::MutableReborrow)
        .collect::<Vec<_>>();
    let evidence = if mutations.is_empty() {
        vec![capability_data_evidence(
            data,
            "No mutable reborrow is present; hidden mutation absence follows retained DefUse/Effects authority.",
            AdapterCapability::DefUse,
            data.coverage().def_use_support(),
            data.coverage().def_use_authority(),
        )]
    } else {
        mutations
            .iter()
            .map(|input| {
                capability_data_evidence(
                    data,
                    &format!(
                        "Mutable reborrow `{}` crosses the boundary; direct syntactic write is {}.",
                        input.name, input.direct_mutation
                    ),
                    AdapterCapability::DefUse,
                    data.coverage().def_use_support(),
                    data.coverage().def_use_authority(),
                )
            })
            .collect()
    };
    ConditionResult {
        condition: REQUIRED_MUTATIONS.into(),
        state: signature.mutation_completeness,
        evidence,
    }
}

fn signature_effect_result(
    condition: &str,
    state: ProofState,
    data: &deslop_parse::DataFlowGraph,
    entities: &[GraphEntityRef],
    empty_detail: &str,
    retained_detail: &str,
) -> ConditionResult {
    let evidence = if entities.is_empty() {
        vec![capability_data_evidence(
            data,
            empty_detail,
            AdapterCapability::Effects,
            data.coverage().effects_support(),
            data.coverage().effects_authority(),
        )]
    } else {
        entities
            .iter()
            .cloned()
            .map(|entity| ConditionEvidence {
                entity,
                detail: retained_detail.into(),
                capability: Some(AdapterCapability::Effects),
                support: Some(data.coverage().effects_support()),
                authority: data.coverage().effects_authority(),
            })
            .collect()
    };
    ConditionResult {
        condition: condition.into(),
        state,
        evidence,
    }
}

fn signature_capture_result(
    data: &deslop_parse::DataFlowGraph,
    signature: &ExtractionSignatureEvidence,
) -> ConditionResult {
    let evidence = if signature.captures.is_empty() {
        vec![plain_data_evidence(
            data,
            "The generated free helper has no implicit capture; every local dependency is an explicit input.",
        )]
    } else {
        signature
            .captures
            .iter()
            .cloned()
            .map(|entity| ConditionEvidence {
                entity,
                detail: "Retained capture fact participates in the selected region.".into(),
                capability: Some(AdapterCapability::DefUse),
                support: Some(data.coverage().def_use_support()),
                authority: data.coverage().def_use_authority(),
            })
            .collect()
    };
    ConditionResult {
        condition: REQUIRED_CAPTURES.into(),
        state: signature.capture_completeness,
        evidence,
    }
}

fn signature_async_ownership_result(
    data: &deslop_parse::DataFlowGraph,
    signature: &ExtractionSignatureEvidence,
) -> ConditionResult {
    let modes = signature
        .inputs
        .iter()
        .map(|input| format!("{}={:?}", input.name, input.ownership))
        .chain(
            signature
                .output
                .iter()
                .map(|output| format!("{}={:?}", output.name, output.ownership)),
        )
        .collect::<Vec<_>>()
        .join(", ");
    let evidence = if signature.suspensions.is_empty() {
        vec![plain_data_evidence(
            data,
            &format!(
                "The helper is synchronous with zero suspension facts and explicit ownership modes [{}].",
                modes
            ),
        )]
    } else {
        signature
            .suspensions
            .iter()
            .cloned()
            .map(|entity| ConditionEvidence {
                entity,
                detail: format!(
                    "A retained suspension fact conflicts with the bounded synchronous ownership modes [{}].",
                    modes
                ),
                capability: Some(AdapterCapability::Effects),
                support: Some(data.coverage().effects_support()),
                authority: data.coverage().effects_authority(),
            })
            .collect()
    };
    ConditionResult {
        condition: REQUIRED_ASYNC_OWNERSHIP.into(),
        state: signature.ownership_completeness,
        evidence,
    }
}

fn plain_data_evidence(data: &deslop_parse::DataFlowGraph, detail: &str) -> ConditionEvidence {
    ConditionEvidence {
        entity: graph_entity(
            GraphEvidenceLayer::DataFlow,
            data.key().as_str(),
            data.key().as_str(),
        ),
        detail: detail.into(),
        capability: None,
        support: None,
        authority: None,
    }
}

fn capability_data_evidence(
    data: &deslop_parse::DataFlowGraph,
    detail: &str,
    capability: AdapterCapability,
    support: CapabilitySupport,
    authority: Option<deslop_parse::CapabilityAuthority>,
) -> ConditionEvidence {
    ConditionEvidence {
        entity: graph_entity(
            GraphEvidenceLayer::DataFlow,
            data.key().as_str(),
            data.key().as_str(),
        ),
        detail: detail.into(),
        capability: Some(capability),
        support: Some(support),
        authority,
    }
}

fn extraction_delta(
    graph: &ProgramDependenceGraph,
    root: &ProgramDependenceNode,
    slice: &ExtractionSliceEvidence,
) -> ExpectedGraphDelta {
    let mut changes = vec![ExpectedGraphChange {
        kind: GraphChangeKind::Modify,
        entity: graph_root(graph, root),
        rationale: "The selected branch dispatch moves behind one private helper call boundary."
            .into(),
    }];
    changes.extend(
        slice
            .computation_entities
            .iter()
            .chain(&slice.object_state_entities)
            .chain(&slice.boundary_flow_entities)
            .cloned()
            .map(|entity| ExpectedGraphChange {
                kind: GraphChangeKind::Preserve,
                entity,
                rationale:
                    "The extracted computation/object-state slice must be retained after rebuild."
                        .into(),
            }),
    );
    ExpectedGraphDelta { changes }
}

fn capability_condition(
    condition: &str,
    state: ProofState,
    entity: GraphEntityRef,
    detail: &str,
    capability: AdapterCapability,
    support: CapabilitySupport,
    authority: Option<deslop_parse::CapabilityAuthority>,
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

fn intersects_sorted(left: &[ControlPointKey], right: &[ControlPointKey]) -> bool {
    left.iter().any(|point| right.binary_search(point).is_ok())
}

fn child_by_field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    field: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, ExtractMethodRecipeError> {
    for child in node.children() {
        let view = analysis
            .node(child)
            .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))?;
        if view.field() == Some(field) {
            return Ok(Some(view));
        }
    }
    Ok(None)
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, ExtractMethodRecipeError> {
    node.children()
        .map(|child| {
            analysis
                .node(child)
                .map_err(|error| ExtractMethodRecipeError::Projection(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|children| {
            children
                .into_iter()
                .filter(|child| child.is_named())
                .collect()
        })
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

fn missing(kind: &str, identity: &str) -> ExtractMethodRecipeError {
    ExtractMethodRecipeError::Projection(format!("missing {kind} {identity}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use deslop_core::SafetyClass;

    use super::*;
    use crate::{build_rust_recipe_projection, detect_rust_recipes};

    const POSITIVE: &str = r#"fn run(flag: bool, value: &mut i32) {
    if flag {
        *value += 1;
        *value += 2;
    } else {
        *value -= 1;
        *value -= 2;
    }
    *value += 3;
}
"#;

    fn candidates(source: &str) -> (tempfile::TempDir, Vec<TransformationCandidate>) {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("extract.rs"), source).unwrap();
        let candidates = detect_rust_recipes(root.path(), &[PathBuf::from("extract.rs")])
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.recipe().name() == "rust-extract-sese-branch-method")
            .collect();
        (root, candidates)
    }

    #[test]
    fn recipe_declares_exact_four_role_contract() {
        let recipe = extract_method_recipe().unwrap();
        assert_eq!(recipe.family(), TransformationFamily::FunctionExpression);
        assert_eq!(recipe.fixtures().len(), 4);
        assert_eq!(
            recipe
                .fixtures()
                .iter()
                .map(|fixture| fixture.role)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                RecipeFixtureRole::Positive,
                RecipeFixtureRole::NoOp,
                RecipeFixtureRole::MinimalCounterexample,
                RecipeFixtureRole::AdversarialNearMiss,
            ])
        );
    }

    #[test]
    fn direct_sese_branch_emits_one_exact_review_candidate() {
        let (_root, found) = candidates(POSITIVE);
        assert_eq!(found.len(), 1);
        let candidate = &found[0];
        assert_eq!(
            candidate.disposition(),
            CandidateDisposition::ReviewRequired
        );
        assert_eq!(candidate.safety(), SafetyClass::SafeWithPrecondition);
        assert_eq!(candidate.edits().len(), 1);
        let edit = &candidate.edits()[0];
        assert_eq!(edit.before, POSITIVE.trim_end());
        assert!(edit.after.starts_with("fn __deslop_extract_branch_"));
        assert!(edit.after.contains("fn run(flag: bool, value: &mut i32)"));
        assert!(edit.after.contains("__deslop_extract_branch_"));
        assert!(edit.after.contains("(flag, value);"));
        assert_eq!(
            candidate
                .required_results()
                .iter()
                .find(|result| result.condition == REQUIRED_SIGNATURE)
                .unwrap()
                .state,
            ProofState::Proven
        );
        assert_eq!(
            candidate
                .required_results()
                .iter()
                .find(|result| result.condition == REQUIRED_SLICE)
                .unwrap()
                .state,
            ProofState::Unknown
        );
        let preserved = candidate
            .expected_delta()
            .changes
            .iter()
            .filter(|change| change.kind == GraphChangeKind::Preserve)
            .count();
        assert!(
            preserved >= 5,
            "expected a retained computation slice, got {preserved}"
        );
    }

    #[test]
    fn generated_extraction_compiles_and_does_not_reextract() {
        let (root, found) = candidates(POSITIVE);
        let replacement = &found[0].edits()[0].after;
        fs::write(root.path().join("extract.rs"), replacement).unwrap();
        let output = Command::new("rustc")
            .args([
                "--crate-type",
                "lib",
                "--edition",
                "2024",
                "extract.rs",
                "-o",
                "libextract.rlib",
            ])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "generated extraction failed to compile: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            detect_rust_recipes(root.path(), &[PathBuf::from("extract.rs")])
                .unwrap()
                .into_iter()
                .all(|candidate| candidate.recipe().name() != "rust-extract-sese-branch-method")
        );
    }

    #[test]
    fn typed_local_input_and_output_signature_preserve_behavior_matrix() {
        let source = r#"fn run(flag: bool, value: &mut i32, unused: u64) -> i32 {
    let amount: i32 = 3;
    let result: i32 = if flag {
        *value += amount;
        *value += 1;
        *value
    } else {
        *value -= amount;
        *value -= 1;
        *value
    };
    result * 2 + *value + unused as i32 - unused as i32
}

fn main() {
    for (flag, seed) in [(false, -8), (false, 0), (true, 0), (true, 11)] {
        let mut value = seed;
        println!("{flag}:{seed}:{}:{}", run(flag, &mut value, 9), value);
    }
}
"#;
        let (root, found) = candidates(source);
        assert_eq!(found.len(), 1);
        let candidate = &found[0];
        let edit = &candidate.edits()[0];
        let helper_signature = edit.after.lines().next().unwrap();
        assert!(helper_signature.contains("(flag: bool, value: &mut i32, amount: i32) -> i32"));
        assert!(!helper_signature.contains("unused"));
        assert!(
            edit.after
                .contains("let result: i32 = __deslop_extract_branch_")
        );
        assert!(edit.after.contains("(flag, value, amount);"));

        let conditions = candidate
            .required_results()
            .iter()
            .map(|result| (result.condition.as_str(), result.state))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(conditions[REQUIRED_INPUTS], ProofState::Proven);
        assert_eq!(conditions[REQUIRED_OUTPUTS], ProofState::Proven);
        assert_eq!(conditions[REQUIRED_MUTATIONS], ProofState::Unknown);
        assert_eq!(conditions[REQUIRED_EXITS], ProofState::Proven);
        assert_eq!(conditions[REQUIRED_EXCEPTIONS], ProofState::Unknown);
        assert_eq!(conditions[REQUIRED_CAPTURES], ProofState::Proven);
        assert_eq!(conditions[REQUIRED_ASYNC_OWNERSHIP], ProofState::Proven);
        let inputs = candidate
            .required_results()
            .iter()
            .find(|result| result.condition == REQUIRED_INPUTS)
            .unwrap();
        assert_eq!(inputs.evidence.len(), 3);
        assert!(
            inputs
                .evidence
                .iter()
                .any(|item| item.detail.contains("PriorLocal"))
        );
        assert!(
            inputs
                .evidence
                .iter()
                .all(|item| !item.detail.contains("unused"))
        );
        let mutations = candidate
            .required_results()
            .iter()
            .find(|result| result.condition == REQUIRED_MUTATIONS)
            .unwrap();
        assert!(mutations.evidence.iter().any(|item| {
            item.detail.contains("Mutable reborrow `value`")
                && item.detail.contains("direct syntactic write is true")
        }));

        let original_output = compile_and_run(root.path(), "before.rs", source);
        let mut rewritten = source.to_string();
        rewritten.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        let extracted_output = compile_and_run(root.path(), "after.rs", &rewritten);
        assert_eq!(original_output, extracted_output);
        assert_eq!(
            String::from_utf8(original_output).unwrap(),
            "false:-8:-36:-12\nfalse:0:-12:-4\ntrue:0:12:4\ntrue:11:45:15\n"
        );
    }

    fn compile_and_run(root: &std::path::Path, name: &str, source: &str) -> Vec<u8> {
        let path = root.join(name);
        let executable = root.join(format!("{name}.bin"));
        fs::write(&path, source).unwrap();
        let compiled = Command::new("rustc")
            .args(["--edition", "2024"])
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{name} failed to compile: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let output = Command::new(executable).output().unwrap();
        assert!(output.status.success());
        output.stdout
    }

    #[test]
    fn isolated_rebuild_preserves_candidate_identity_and_wire() {
        let (root, first) = candidates(POSITIVE);
        let projection = build_rust_recipe_projection(root.path(), &[PathBuf::from("extract.rs")])
            .unwrap()
            .unwrap();
        let second = detect_extract_method_candidates(&projection).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].id(), second[0].id());
        let wire = serde_json::to_vec(&first[0]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(value["recipe"]["name"], "rust-extract-sese-branch-method");
        assert_eq!(value["edits"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn exact_near_misses_are_rejected_numerically() {
        let cases = [
            // No direct branch.
            "fn run(value: &mut i32) { *value += 1; *value += 2; }\n",
            // Prior local would need full local-input inference.
            "fn run(flag: bool, value: &mut i32) { let amount = 2; if flag { *value += amount; *value += 1; } else { *value -= amount; *value -= 1; } }\n",
            // Owned input would be moved into the helper.
            "fn run(flag: bool, value: String) { if flag { drop(value); one(); } else { two(); three(); } }\n",
            // Generic signature belongs to M5.12.
            "fn run<T>(flag: bool, value: &mut T) { if flag { one(); two(); } else { three(); four(); } }\n",
            // Nested branch is not a direct function-body extraction unit.
            "fn run(flag: bool, outer: bool, value: &mut i32) { while outer { if flag { *value += 1; *value += 2; } else { *value -= 1; *value -= 2; } break; } }\n",
            // Return cannot cross the new callable boundary.
            "fn run(flag: bool, value: &mut i32) { if flag { *value += 1; return; } else { *value -= 1; *value -= 2; } }\n",
            // Macro expansion and effects are unavailable.
            "fn run(flag: bool, value: &mut i32) { if flag { println!(\"x\"); *value += 1; } else { *value -= 1; *value -= 2; } }\n",
            // Used prior local requires an explicit type.
            "fn run(flag: bool, value: &mut i32) { let amount = 2; if flag { *value += amount; *value += 1; } else { *value -= amount; *value -= 1; } }\n",
            // Owned prior local cannot cross without compiler ownership authority.
            "fn run(flag: bool, value: &mut i32) { let amount: String = String::new(); if flag { consume(&amount); *value += 1; } else { consume(&amount); *value -= 1; } }\nfn consume(_: &str) {}\n",
            // Internal binding creates another signature frontier.
            "fn run(flag: bool, value: &mut i32) { if flag { let amount = 1; *value += amount; } else { *value -= 1; *value -= 2; } }\n",
            // Returned initializer requires an explicit exact output type.
            "fn run(flag: bool, value: &mut i32) -> i32 { let result = if flag { *value += 1; *value } else { *value -= 1; *value }; result }\n",
            // Reference output lifetime inference is outside the bounded contract.
            "fn run(flag: bool, value: &i32) -> &i32 { let result: &i32 = if flag { touch(); value } else { touch(); value }; result }\nfn touch() {}\n",
            // Async callable and await boundary are rejected.
            "async fn run(flag: bool, value: &mut i32) { if flag { ping().await; *value += 1; } else { *value -= 1; *value -= 2; } }\nasync fn ping() {}\n",
            // Closure capture cannot become a free-helper input implicitly.
            "fn run(flag: bool, value: &mut i32) { if flag { let f = || *value; f(); } else { *value -= 1; *value -= 2; } }\n",
            // Try propagation cannot cross the helper boundary.
            "fn run(flag: bool, value: &mut i32) -> Result<(), ()> { if flag { fallible()?; *value += 1; } else { *value -= 1; *value -= 2; } Ok(()) }\nfn fallible() -> Result<(), ()> { Ok(()) }\n",
            // Named lifetime ownership requires generic helper inference.
            "fn run<'a>(flag: bool, value: &'a mut i32) { if flag { *value += 1; *value += 2; } else { *value -= 1; *value -= 2; } }\n",
        ];
        let counts = cases
            .iter()
            .map(|source| candidates(source).1.len())
            .collect::<Vec<_>>();
        assert_eq!(counts, vec![0; 16]);
    }
}
