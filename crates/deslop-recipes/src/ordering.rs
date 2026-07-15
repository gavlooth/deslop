use std::collections::BTreeSet;

use deslop_core::{Lang, SafetyClass, Span};
use deslop_parse::{
    AdapterCapability, BindingTarget, BindingTiming, CapabilityAuthority, CapabilitySupport,
    FactCoverage, GraphEligibilityDecision, GraphEvidenceLayer, ImportForm,
    ProgramDependenceProjection, ProjectAnalysis, ResolutionEndpoint, ResolutionResultRecord,
    ResolutionStatus, ScopeFactData, ScopeFactKey, ScopeFactRecord, ScopeGraphProjection,
    ScopeKind, SymbolKind, VisibilityKind, evaluate_graph_recipe_eligibility,
};

use crate::branch::{condition, fixture, graph_entity, span};
use crate::{
    CandidateDisposition, CandidateSource, CandidateTarget, ConditionEvidence, ConditionResult,
    ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind, GraphEntityRef,
    ImpactCone, ImpactConeQuery, ImpactDirection, ProofState, RecipeContractError,
    RecipeFixtureRole, RollbackPlan, RollbackStrategy, TransformationCandidate,
    TransformationCandidateDraft, TransformationEdit, TransformationFamily, TransformationRecipe,
    TransformationRecipeDraft, ValidationPlan, ValidationStep, ValidationStepKind,
};

const COMPLETE_AUTHORITY: &str = "complete-scope-and-resolution-authority";
const SIMPLE_IMPORTS: &str = "simple-order-independent-imports";
const HOISTED_PRIVATE: &str = "hoisted-private-function-declarations";
const CONTIGUOUS: &str = "contiguous-top-level-block";
const UNIQUE_KEYS: &str = "distinct-canonical-order-keys";
const PARTIAL: &str = "partial-or-non-unique-semantic-evidence";
const ORDER_SENSITIVE: &str = "order-sensitive-import-or-declaration";
const TRIVIA_OR_MACRO: &str = "attached-trivia-attribute-macro-or-recovered-syntax";

#[derive(Debug, thiserror::Error)]
pub enum OrderingRecipeError {
    #[error(transparent)]
    Contract(#[from] RecipeContractError),
    #[error("ordering graph eligibility failed: {0}")]
    Eligibility(String),
    #[error("ordering projection is inconsistent: {0}")]
    Projection(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrderingKind {
    Imports,
    Functions,
}

pub fn simple_import_order_recipe() -> Result<TransformationRecipe, RecipeContractError> {
    ordering_recipe(OrderingKind::Imports)
}

pub fn hoisted_private_function_order_recipe() -> Result<TransformationRecipe, RecipeContractError>
{
    ordering_recipe(OrderingKind::Functions)
}

pub fn detect_ordering_candidates(
    projection: &ProgramDependenceProjection,
) -> Result<Vec<TransformationCandidate>, OrderingRecipeError> {
    let imports = simple_import_order_recipe()?;
    let functions = hoisted_private_function_order_recipe()?;
    let import_eligibility = eligibility(projection, &imports)?;
    let function_eligibility = eligibility(projection, &functions)?;
    if !import_eligibility.eligible() && !function_eligibility.eligible() {
        return Ok(Vec::new());
    }

    let resolution = projection.data_flow().resolution();
    let scopes = resolution.scope_graph();
    let analysis = scopes.analysis();
    let mut candidates = Vec::new();
    for root in analysis.node_ids().filter_map(|id| {
        analysis
            .node(id)
            .ok()
            .filter(|node| node.raw_grammar_kind() == "source_file")
    }) {
        if root.grammar().lang() != Lang::Rust || root.has_error() {
            continue;
        }
        let Some(file_scope) = exact_file_scope(scopes, root.key()) else {
            continue;
        };
        let children = named_children(analysis, root)?;
        if import_eligibility.eligible() {
            for run in runs(&children, |node| simple_import_node(analysis, *node)) {
                if let Some(candidate) = import_candidate(
                    projection,
                    scopes,
                    file_scope,
                    root,
                    &run,
                    imports.clone(),
                    import_eligibility.clone(),
                )? {
                    candidates.push(candidate);
                }
            }
        }
        if function_eligibility.eligible() {
            for run in runs(&children, |node| simple_function_node(analysis, *node)) {
                if let Some(candidate) = function_candidate(
                    projection,
                    scopes,
                    file_scope,
                    root,
                    &run,
                    functions.clone(),
                    function_eligibility.clone(),
                )? {
                    candidates.push(candidate);
                }
            }
        }
    }
    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(candidates)
}

fn import_candidate(
    projection: &ProgramDependenceProjection,
    scopes: &ScopeGraphProjection,
    file_scope: &ScopeFactRecord,
    root: deslop_parse::NodeView<'_>,
    run: &[deslop_parse::NodeView<'_>],
    recipe: TransformationRecipe,
    eligibility: GraphEligibilityDecision,
) -> Result<Option<TransformationCandidate>, OrderingRecipeError> {
    let mut facts = Vec::new();
    for node in run {
        let matches = scopes
            .facts()
            .iter()
            .filter(|fact| {
                fact.evidence().node_key == *node.key()
                    && matches!(fact.data(), ScopeFactData::Import { .. })
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Ok(None);
        }
        let fact = matches[0];
        let ScopeFactData::Import {
            scope,
            form,
            conditions,
            ..
        } = fact.data()
        else {
            unreachable!();
        };
        if scope != file_scope.key()
            || !matches!(form, ImportForm::Module | ImportForm::Selective)
            || !conditions.is_empty()
            || !exact_fact(fact, AdapterCapability::ImportsExports)
            || scopes.facts().iter().any(|candidate| {
                candidate.evidence().node_key == *node.key()
                    && matches!(candidate.data(), ScopeFactData::Export { .. })
            })
        {
            return Ok(None);
        }
        facts.push(fact);
    }
    if order_sensitive_scope(scopes, file_scope.key(), &BTreeSet::new()) {
        return Ok(None);
    }
    build_candidate(
        projection,
        scopes,
        file_scope,
        root,
        run,
        &facts,
        OrderingKind::Imports,
        recipe,
        eligibility,
    )
}

fn function_candidate(
    projection: &ProgramDependenceProjection,
    scopes: &ScopeGraphProjection,
    file_scope: &ScopeFactRecord,
    root: deslop_parse::NodeView<'_>,
    run: &[deslop_parse::NodeView<'_>],
    recipe: TransformationRecipe,
    eligibility: GraphEligibilityDecision,
) -> Result<Option<TransformationCandidate>, OrderingRecipeError> {
    let analysis = scopes.analysis();
    let mut facts = Vec::new();
    let mut declarations = BTreeSet::new();
    for node in run {
        let Some(name) = field(analysis, *node, "name")? else {
            return Ok(None);
        };
        let declaration_matches = scopes
            .facts()
            .iter()
            .filter(|fact| {
                fact.evidence().node_key == *name.key()
                    && matches!(fact.data(), ScopeFactData::Declaration { .. })
            })
            .collect::<Vec<_>>();
        if declaration_matches.len() != 1 {
            return Ok(None);
        }
        let declaration = declaration_matches[0];
        let ScopeFactData::Declaration {
            scope,
            visibility,
            modifiers,
            ..
        } = declaration.data()
        else {
            unreachable!();
        };
        if scope != file_scope.key()
            || visibility.kind != VisibilityKind::Private
            || !modifiers.contains(&deslop_parse::DeclarationModifier::Hoisted)
            || !exact_fact(declaration, AdapterCapability::LexicalScopes)
        {
            return Ok(None);
        }
        let definition_matches = scopes
            .facts()
            .iter()
            .filter(|fact| {
                matches!(
                    fact.data(),
                    ScopeFactData::Definition {
                        declaration: key,
                        symbol_kind: SymbolKind::Function,
                        ..
                    } if key == declaration.key()
                )
            })
            .collect::<Vec<_>>();
        if definition_matches.len() != 1
            || !exact_fact(definition_matches[0], AdapterCapability::LexicalScopes)
        {
            return Ok(None);
        }
        let definition = definition_matches[0];
        let has_hoisted_binding = scopes.facts().iter().any(|fact| {
            matches!(
                fact.data(),
                ScopeFactData::Binding {
                    target: BindingTarget::Definition(key),
                    timing: BindingTiming::Hoisted,
                    ..
                } if key == definition.key()
            ) && exact_fact(fact, AdapterCapability::LexicalScopes)
        });
        if !has_hoisted_binding {
            return Ok(None);
        }
        declarations.insert(declaration.key().clone());
        declarations.insert(definition.key().clone());
        facts.push(declaration);
        facts.push(definition);
    }
    if order_sensitive_scope(scopes, file_scope.key(), &declarations) {
        return Ok(None);
    }
    let resolution = projection.data_flow().resolution();
    if resolution.results().iter().any(|record| {
        let result = record.wire();
        result.status() != ResolutionStatus::Unique
            || result.coverage().status() != FactCoverage::Complete
            || result.authority().is_none()
            || result.preferred().is_some_and(|preferred| {
                preferred.endpoints().iter().any(|endpoint| {
                    endpoint_key(endpoint).is_some_and(|key| {
                        declarations.contains(key)
                            && !matches!(result.status(), ResolutionStatus::Unique)
                    })
                })
            })
    }) {
        return Ok(None);
    }
    build_candidate(
        projection,
        scopes,
        file_scope,
        root,
        run,
        &facts,
        OrderingKind::Functions,
        recipe,
        eligibility,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_candidate(
    projection: &ProgramDependenceProjection,
    scopes: &ScopeGraphProjection,
    file_scope: &ScopeFactRecord,
    root: deslop_parse::NodeView<'_>,
    run: &[deslop_parse::NodeView<'_>],
    facts: &[&ScopeFactRecord],
    kind: OrderingKind,
    recipe: TransformationRecipe,
    eligibility: GraphEligibilityDecision,
) -> Result<Option<TransformationCandidate>, OrderingRecipeError> {
    if run.len() < 2 {
        return Ok(None);
    }
    let analysis = scopes.analysis();
    let mut keyed = run
        .iter()
        .map(|node| {
            let key = match kind {
                OrderingKind::Imports => node.text().trim().to_string(),
                OrderingKind::Functions => field(analysis, *node, "name")?
                    .ok_or_else(|| OrderingRecipeError::Projection("function has no name".into()))?
                    .text()
                    .to_string(),
            };
            Ok((key, *node))
        })
        .collect::<Result<Vec<_>, OrderingRecipeError>>()?;
    if keyed
        .iter()
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>()
        .len()
        != keyed.len()
    {
        return Ok(None);
    }
    let original_keys = keyed.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    if original_keys == keyed.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>() {
        return Ok(None);
    }
    let first = run[0];
    let last = run[run.len() - 1];
    let start = first.key().anchor().start_byte() as usize;
    let end = last.key().anchor().end_byte() as usize;
    let source = root.text();
    let before = source
        .get(start..end)
        .ok_or_else(|| OrderingRecipeError::Projection("ordering span escapes source".into()))?
        .to_string();
    let gaps = run
        .windows(2)
        .map(|pair| {
            source
                .get(
                    pair[0].key().anchor().end_byte() as usize
                        ..pair[1].key().anchor().start_byte() as usize,
                )
                .ok_or_else(|| {
                    OrderingRecipeError::Projection("ordering gap escapes source".into())
                })
                .map(str::to_string)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut after = String::new();
    for (index, (_, node)) in keyed.iter().enumerate() {
        after.push_str(node.text());
        if let Some(gap) = gaps.get(index) {
            after.push_str(gap);
        }
    }

    let resolution = projection.data_flow().resolution();
    let fact_keys = facts
        .iter()
        .map(|fact| fact.key().clone())
        .collect::<BTreeSet<_>>();
    let relevant_results = resolution
        .results()
        .iter()
        .filter(|record| {
            record
                .wire()
                .source_facts()
                .iter()
                .any(|key| fact_keys.contains(key))
                || record.wire().preferred().is_some_and(|preferred| {
                    preferred
                        .endpoints()
                        .iter()
                        .filter_map(endpoint_key)
                        .any(|key| fact_keys.contains(key))
                })
        })
        .collect::<Vec<_>>();
    let scope_root = scope_entity(scopes, file_scope.key());
    let resolution_root = graph_entity(
        GraphEvidenceLayer::Resolution,
        resolution.id().as_str(),
        resolution.id().as_str(),
    );
    let mut entities = vec![scope_root.clone(), resolution_root.clone()];
    entities.extend(facts.iter().map(|fact| scope_entity(scopes, fact.key())));
    entities.extend(
        relevant_results
            .iter()
            .map(|record| resolution_entity(resolution.id().as_str(), record)),
    );
    entities.sort();
    entities.dedup();
    let authority_evidence = facts
        .iter()
        .map(|fact| ConditionEvidence {
            entity: scope_entity(scopes, fact.key()),
            detail: "The retained fact has complete adapter-authoritative coverage.".into(),
            capability: Some(fact.evidence().capability),
            support: Some(fact.evidence().capability_support),
            authority: fact.evidence().authority,
        })
        .collect();
    let root_evidence = vec![ConditionEvidence {
        entity: scope_root.clone(),
        detail: "The exact top-level siblings form one trivia-free contiguous block.".into(),
        capability: None,
        support: None,
        authority: None,
    }];
    let resolution_evidence = vec![ConditionEvidence {
        entity: resolution_root.clone(),
        detail: "Every retained reference has complete terminal unique resolution.".into(),
        capability: None,
        support: None,
        authority: None,
    }];
    let kind_evidence = facts
        .iter()
        .map(|fact| ConditionEvidence {
            entity: scope_entity(scopes, fact.key()),
            detail: match kind {
                OrderingKind::Imports => {
                    "The import is explicit/selective, unconditional, and not side-effectful."
                }
                OrderingKind::Functions => {
                    "The private function declaration is hoisted with an exact Function definition."
                }
            }
            .into(),
            capability: Some(fact.evidence().capability),
            support: Some(fact.evidence().capability_support),
            authority: fact.evidence().authority,
        })
        .collect();
    let required_results = vec![
        condition_result(COMPLETE_AUTHORITY, ProofState::Proven, authority_evidence),
        condition_result(
            match kind {
                OrderingKind::Imports => SIMPLE_IMPORTS,
                OrderingKind::Functions => HOISTED_PRIVATE,
            },
            ProofState::Proven,
            kind_evidence,
        ),
        condition_result(CONTIGUOUS, ProofState::Proven, root_evidence.clone()),
        condition_result(UNIQUE_KEYS, ProofState::Proven, root_evidence.clone()),
    ];
    let forbidden_results = vec![
        condition_result(PARTIAL, ProofState::Disproven, resolution_evidence),
        condition_result(
            ORDER_SENSITIVE,
            ProofState::Disproven,
            root_evidence.clone(),
        ),
        condition_result(TRIVIA_OR_MACRO, ProofState::Disproven, root_evidence),
    ];
    let edit_span = Span {
        start_line: first.key().anchor().start_row() as usize + 1,
        end_line: last.key().anchor().end_row() as usize + 1,
        start_byte: start,
        end_byte: end,
    };
    let candidate = TransformationCandidate::new(TransformationCandidateDraft {
        recipe: recipe.clone(),
        source: CandidateSource {
            project_snapshot: analysis.snapshot().id().as_str().into(),
            analysis: analysis.id().as_str().into(),
            program_dependence_projection: projection.id().as_str().into(),
        },
        target: CandidateTarget {
            entity: scope_root.clone(),
            node: root.key().clone(),
            span: span(root.key()),
        },
        eligibility,
        required_results,
        forbidden_results,
        impact: ImpactCone {
            query: ImpactConeQuery {
                roots: vec![scope_root.clone()],
                direction: ImpactDirection::Bidirectional,
                layers: vec![
                    GraphEvidenceLayer::ScopeGraph,
                    GraphEvidenceLayer::Resolution,
                ],
                maximum_depth: 1,
            },
            entities,
            truncated: false,
        },
        expected_delta: ExpectedGraphDelta {
            changes: vec![
                ExpectedGraphChange {
                    kind: GraphChangeKind::Modify,
                    entity: scope_root,
                    rationale: "Rebuild source-order anchors while preserving scope membership."
                        .into(),
                },
                ExpectedGraphChange {
                    kind: GraphChangeKind::Preserve,
                    entity: resolution_root,
                    rationale: "Preserve every terminal binding status and endpoint.".into(),
                },
            ],
        },
        edits: vec![TransformationEdit::exact_node_replacement(
            root.key().clone(),
            edit_span,
            before,
            after,
        )],
        safety: SafetyClass::SafeWithPrecondition,
        disposition: CandidateDisposition::ReviewRequired,
        validation_plan: recipe.validation_plan().clone(),
        rollback_plan: recipe.rollback_plan().clone(),
    })?;
    Ok(Some(candidate))
}

fn ordering_recipe(kind: OrderingKind) -> Result<TransformationRecipe, RecipeContractError> {
    let (name, required, fixtures) = match kind {
        OrderingKind::Imports => (
            "rust-sort-simple-import-block",
            condition(
                SIMPLE_IMPORTS,
                "Every import is explicit/selective, unconditional, and non-side-effectful.",
                GraphEvidenceLayer::ScopeGraph,
            ),
            vec![
                fixture(
                    RecipeFixtureRole::Positive,
                    "unsorted-simple-imports",
                    FixtureExpectation::Candidate,
                    "Two exact simple imports are out of canonical order.",
                ),
                fixture(
                    RecipeFixtureRole::NoOp,
                    "already-ordered-imports",
                    FixtureExpectation::NoCandidate,
                    "Canonical import order is unchanged.",
                ),
                fixture(
                    RecipeFixtureRole::MinimalCounterexample,
                    "side-effect-or-conditional-import",
                    FixtureExpectation::NoCandidate,
                    "Order-sensitive import forms are not moved.",
                ),
                fixture(
                    RecipeFixtureRole::AdversarialNearMiss,
                    "partial-import-authority",
                    FixtureExpectation::NoCandidate,
                    "Spelling cannot replace complete import authority.",
                ),
            ],
        ),
        OrderingKind::Functions => (
            "rust-sort-hoisted-private-function-block",
            condition(
                HOISTED_PRIVATE,
                "Every declaration is a private hoisted plain Function with exact binding evidence.",
                GraphEvidenceLayer::ScopeGraph,
            ),
            vec![
                fixture(
                    RecipeFixtureRole::Positive,
                    "unsorted-hoisted-functions",
                    FixtureExpectation::Candidate,
                    "Two exact private hoisted functions are out of canonical order.",
                ),
                fixture(
                    RecipeFixtureRole::NoOp,
                    "already-ordered-functions",
                    FixtureExpectation::NoCandidate,
                    "Canonical declaration order is unchanged.",
                ),
                fixture(
                    RecipeFixtureRole::MinimalCounterexample,
                    "public-or-unhoisted-function",
                    FixtureExpectation::NoCandidate,
                    "API or order-sensitive declarations are not moved.",
                ),
                fixture(
                    RecipeFixtureRole::AdversarialNearMiss,
                    "macro-or-partial-resolution",
                    FixtureExpectation::NoCandidate,
                    "Macro/source-location behavior and partial bindings abstain.",
                ),
            ],
        ),
    };
    TransformationRecipe::new(TransformationRecipeDraft {
        name: name.into(),
        version: "1.0.0".into(),
        family: TransformationFamily::DependencyModule,
        required_layers: vec![
            GraphEvidenceLayer::ScopeGraph,
            GraphEvidenceLayer::Resolution,
        ],
        required_conditions: vec![
            condition(
                COMPLETE_AUTHORITY,
                "Scope and resolution evidence is complete and adapter-authoritative.",
                GraphEvidenceLayer::ScopeGraph,
            ),
            required,
            condition(
                CONTIGUOUS,
                "The selected nodes are direct contiguous top-level siblings.",
                GraphEvidenceLayer::ScopeGraph,
            ),
            condition(
                UNIQUE_KEYS,
                "Canonical order keys are distinct and change the block.",
                GraphEvidenceLayer::ScopeGraph,
            ),
        ],
        forbidden_conditions: vec![
            condition(
                PARTIAL,
                "Partial, unknown, ambiguous, unresolved, or conflicting evidence blocks ordering.",
                GraphEvidenceLayer::Resolution,
            ),
            condition(
                ORDER_SENSITIVE,
                "Side effects, conditions, exports, shadowing, dynamic scope, or non-hoisted timing block ordering.",
                GraphEvidenceLayer::ScopeGraph,
            ),
            condition(
                TRIVIA_OR_MACRO,
                "Comments, attributes, macros, recovery, or observable source-location constructs block ordering.",
                GraphEvidenceLayer::ScopeGraph,
            ),
        ],
        maximum_safety: SafetyClass::SafeWithPrecondition,
        validation_plan: validation_plan(),
        rollback_plan: rollback_plan(),
        fixtures,
    })
}

fn eligibility(
    projection: &ProgramDependenceProjection,
    recipe: &TransformationRecipe,
) -> Result<GraphEligibilityDecision, OrderingRecipeError> {
    evaluate_graph_recipe_eligibility(projection, None, &recipe.eligibility_requirement())
        .map_err(|error| OrderingRecipeError::Eligibility(error.to_string()))
}

fn exact_file_scope<'a>(
    scopes: &'a ScopeGraphProjection,
    root: &deslop_parse::NodeKey,
) -> Option<&'a ScopeFactRecord> {
    let matches = scopes
        .facts()
        .iter()
        .filter(|fact| {
            fact.evidence().node_key == *root
                && matches!(
                    fact.data(),
                    ScopeFactData::Scope {
                        scope_kind: ScopeKind::File,
                        parent: None,
                        ..
                    }
                )
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then_some(matches[0])
}

fn exact_fact(fact: &ScopeFactRecord, capability: AdapterCapability) -> bool {
    fact.evidence().capability == capability
        && fact.evidence().capability_support == CapabilitySupport::Provided
        && matches!(
            fact.evidence().authority,
            Some(CapabilityAuthority::Adapter | CapabilityAuthority::Compiler)
        )
        && fact.evidence().coverage.status == FactCoverage::Complete
        && fact.evidence().coverage.reason.is_none()
        && !fact.evidence().recovered
}

fn order_sensitive_scope(
    scopes: &ScopeGraphProjection,
    scope: &ScopeFactKey,
    selected: &BTreeSet<ScopeFactKey>,
) -> bool {
    scopes.facts().iter().any(|fact| match fact.data() {
        ScopeFactData::DynamicBoundary { scopes, .. } => scopes.contains(scope),
        ScopeFactData::Shadowing {
            shadowing_declaration,
            shadowed_declaration,
            ..
        } => selected.contains(shadowing_declaration) || selected.contains(shadowed_declaration),
        ScopeFactData::Export { local_target, .. } => local_target
            .as_ref()
            .is_some_and(|key| selected.contains(key)),
        _ => false,
    })
}

fn simple_import_node(analysis: &ProjectAnalysis, node: deslop_parse::NodeView<'_>) -> bool {
    node.raw_grammar_kind() == "use_declaration"
        && node.text().trim_start().starts_with("use ")
        && !bad_subtree(analysis, node)
}

fn simple_function_node(analysis: &ProjectAnalysis, node: deslop_parse::NodeView<'_>) -> bool {
    node.raw_grammar_kind() == "function_item"
        && node.text().trim_start().starts_with("fn ")
        && !bad_subtree(analysis, node)
}

fn bad_subtree(analysis: &ProjectAnalysis, node: deslop_parse::NodeView<'_>) -> bool {
    node.has_error()
        || analysis.descendant_node_ids(node.id()).is_err()
        || analysis
            .descendant_node_ids(node.id())
            .is_ok_and(|mut descendants| {
                descendants.any(|id| {
                    analysis.node(id).is_ok_and(|child| {
                        child.has_error()
                            || matches!(
                                child.raw_grammar_kind(),
                                "line_comment"
                                    | "block_comment"
                                    | "attribute_item"
                                    | "inner_attribute_item"
                                    | "macro_invocation"
                                    | "macro_definition"
                            )
                    })
                })
            })
}

fn runs<'a, F>(
    nodes: &[deslop_parse::NodeView<'a>],
    mut accepts: F,
) -> Vec<Vec<deslop_parse::NodeView<'a>>>
where
    F: FnMut(&deslop_parse::NodeView<'a>) -> bool,
{
    let mut output = Vec::new();
    let mut current = Vec::new();
    for node in nodes {
        if accepts(node) {
            current.push(*node);
        } else if !current.is_empty() {
            if current.len() >= 2 {
                output.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        output.push(current);
    }
    output
}

fn named_children<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
) -> Result<Vec<deslop_parse::NodeView<'a>>, OrderingRecipeError> {
    node.children()
        .filter_map(|id| match analysis.node(id) {
            Ok(child) if child.is_named() => Some(Ok(child)),
            Ok(_) => None,
            Err(error) => Some(Err(OrderingRecipeError::Projection(error.to_string()))),
        })
        .collect()
}

fn field<'a>(
    analysis: &'a ProjectAnalysis,
    node: deslop_parse::NodeView<'a>,
    name: &str,
) -> Result<Option<deslop_parse::NodeView<'a>>, OrderingRecipeError> {
    for child in node.children() {
        let child = analysis
            .node(child)
            .map_err(|error| OrderingRecipeError::Projection(error.to_string()))?;
        if child.field() == Some(name) {
            return Ok(Some(child));
        }
    }
    Ok(None)
}

fn endpoint_key(endpoint: &ResolutionEndpoint) -> Option<&ScopeFactKey> {
    match endpoint {
        ResolutionEndpoint::Declaration(key)
        | ResolutionEndpoint::Definition(key)
        | ResolutionEndpoint::Module(key) => Some(key),
        ResolutionEndpoint::MergedDeclarations(_) | ResolutionEndpoint::External(_) => None,
    }
}

fn condition_result(
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

fn scope_entity(scopes: &ScopeGraphProjection, fact: &ScopeFactKey) -> GraphEntityRef {
    graph_entity(
        GraphEvidenceLayer::ScopeGraph,
        scopes.id().as_str(),
        fact.as_str(),
    )
}

fn resolution_entity(graph: &str, result: &ResolutionResultRecord) -> GraphEntityRef {
    graph_entity(
        GraphEvidenceLayer::Resolution,
        graph,
        result.wire().key().as_str(),
    )
}

fn validation_plan() -> ValidationPlan {
    ValidationPlan {
        steps: vec![
            validation(
                "build",
                ValidationStepKind::Build,
                "Build the reordered Rust source.",
            ),
            validation(
                "format",
                ValidationStepKind::Format,
                "Format without changing the semantic block contract.",
            ),
            validation(
                "parse",
                ValidationStepKind::Parse,
                "Parse the exact reordered source.",
            ),
            validation(
                "resolution-delta",
                ValidationStepKind::GraphDelta,
                "Rebuild and preserve terminal resolution endpoints.",
            ),
            validation(
                "test",
                ValidationStepKind::Test,
                "Run project tests after ordering.",
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
            "format".into(),
            "parse".into(),
            "resolution-delta".into(),
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
        DataFlowBuilder, DataFlowEffectDraft, DataFlowGraphDraft, DataFlowPolicyId,
        DeclarationDraft, DeclarationModifier, DefinitionDraft, FactCoverageEvidence, ImportDraft,
        Mutability, NameNamespace, NamespacePolicy, NonStructuredControlPolicyId,
        ProgramDependencePolicyId, ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft,
        ReferenceRole, RepositoryId, ResolutionPolicyId, ScopeDraft, ScopeFactPolicyId,
        ScopeGraphBuilder, VisibilityDraft, derive_control_regions,
        derive_non_structured_control_regions, derive_program_dependence,
    };
    use serde_json::Value;

    use super::*;
    use crate::inline_helper::tests::INLINE_TEST_PACK;

    #[derive(Clone, Copy)]
    struct FixtureOptions {
        ordered: bool,
        partial_import: bool,
        side_effect_import: bool,
        conditional_import: bool,
        glob_import: bool,
        hoisted_functions: bool,
        public_function: bool,
        macro_function: bool,
        separating_comments: bool,
    }

    impl Default for FixtureOptions {
        fn default() -> Self {
            Self {
                ordered: false,
                partial_import: false,
                side_effect_import: false,
                conditional_import: false,
                glob_import: false,
                hoisted_functions: true,
                public_function: false,
                macro_function: false,
                separating_comments: false,
            }
        }
    }

    struct Fixture {
        source: String,
        projection: ProgramDependenceProjection,
    }

    fn fixture(options: FixtureOptions) -> Fixture {
        let imports = if options.ordered {
            ["use std::collections::BTreeMap;", "use std::vec::Vec;"]
        } else {
            ["use std::vec::Vec;", "use std::collections::BTreeMap;"]
        };
        let zebra = if options.macro_function {
            "fn zebra() -> i32 { line!() as i32 }"
        } else if options.public_function {
            "pub fn zebra() -> i32 { alpha() + 1 }"
        } else {
            "fn zebra() -> i32 { alpha() + 1 }"
        };
        let functions = if options.ordered {
            ["fn alpha() -> i32 { 1 }", zebra]
        } else {
            [zebra, "fn alpha() -> i32 { 1 }"]
        };
        let separator = if options.separating_comments {
            "\n// retained separator\n"
        } else {
            "\n"
        };
        let source = format!(
            "{}{}{}\n\n{}{}{}\n\nfn main() {{ let _: Option<Vec<i32>> = None; let _: Option<BTreeMap<i32, i32>> = None; println!(\"{{}}\", zebra()); }}\n",
            imports[0], separator, imports[1], functions[0], separator, functions[1]
        );
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::default();
        registry.register(&INLINE_TEST_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("ordering-test").unwrap(),
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
        let source_root = nodes("source_file")[0];
        let use_nodes = nodes("use_declaration");
        let function_nodes = nodes("function_item");
        assert_eq!(use_nodes.len(), 2);
        assert_eq!(function_nodes.len(), 3);
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
        let namespaces =
            NamespacePolicy::new(vec![NameNamespace::Value, NameNamespace::Type], vec![]).unwrap();
        let mut scopes = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"ordering-build"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"ordering-scope"]).unwrap(),
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
        for (index, node) in use_nodes.iter().enumerate() {
            let coverage = if options.partial_import && index == 0 {
                FactCoverageEvidence::partial("fixture import authority is partial").unwrap()
            } else {
                complete.clone()
            };
            scopes
                .add_import(
                    *node,
                    roles(*node),
                    coverage,
                    ImportDraft {
                        scope: file_scope,
                        module_segments: if analysis.node(*node).unwrap().text().contains("vec") {
                            vec!["std".into(), "vec".into(), "Vec".into()]
                        } else {
                            vec!["std".into(), "collections".into(), "BTreeMap".into()]
                        },
                        form: if options.side_effect_import && index == 0 {
                            ImportForm::SideEffect
                        } else if options.glob_import && index == 0 {
                            ImportForm::Glob
                        } else {
                            ImportForm::Module
                        },
                        alias: None,
                        selected_names: vec![],
                        conditions: if options.conditional_import && index == 0 {
                            vec!["cfg(test)".into()]
                        } else {
                            vec![]
                        },
                    },
                )
                .unwrap();
        }

        let mut callable_scopes = BTreeMap::new();
        for function in &function_nodes {
            let name = child_field(&analysis, *function, "name");
            let scope = scopes
                .add_scope(
                    *function,
                    roles(*function),
                    complete.clone(),
                    ScopeDraft {
                        kind: ScopeKind::Callable,
                        parent: Some(file_scope),
                        namespace_policy: namespaces.clone(),
                    },
                )
                .unwrap();
            callable_scopes.insert(analysis.node(name).unwrap().text().to_string(), scope);
        }
        let mut definitions = BTreeMap::new();
        for function in &function_nodes {
            let name_node = child_field(&analysis, *function, "name");
            let name = analysis.node(name_node).unwrap().text().to_string();
            let declaration = scopes
                .add_declaration(
                    name_node,
                    roles(name_node),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: name.clone(),
                        lookup_key: name.clone(),
                        namespace: NameNamespace::Value,
                        scope: file_scope,
                        visibility: VisibilityDraft {
                            kind: if options.public_function && name == "zebra" {
                                VisibilityKind::Public
                            } else {
                                VisibilityKind::Private
                            },
                            boundary: Some(file_scope),
                            adapter_rule: None,
                        },
                        modifiers: options
                            .hoisted_functions
                            .then_some(DeclarationModifier::Hoisted)
                            .into_iter()
                            .collect(),
                    },
                )
                .unwrap();
            let definition = scopes
                .add_definition(
                    *function,
                    roles(*function),
                    complete.clone(),
                    DefinitionDraft {
                        declaration,
                        symbol_kind: SymbolKind::Function,
                        body_scope: Some(callable_scopes[&name]),
                        type_scope: None,
                    },
                )
                .unwrap();
            scopes
                .add_binding(
                    name_node,
                    roles(name_node),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Definition(definition),
                        form: BindingForm::Declaration,
                        timing: if options.hoisted_functions {
                            BindingTiming::Hoisted
                        } else {
                            BindingTiming::AtDeclaration
                        },
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            definitions.insert(name, definition);
        }
        for name in ["alpha", "zebra"] {
            let mut identifiers = analysis
                .node_ids()
                .filter(|id| {
                    let node = analysis.node(*id).unwrap();
                    node.raw_grammar_kind() == "identifier" && node.text() == name
                })
                .collect::<Vec<_>>();
            identifiers.sort_by_key(|id| analysis.node(*id).unwrap().span().start_byte());
            for identifier in identifiers.into_iter().skip(1) {
                let function = enclosing_function(&analysis, identifier);
                let function_name = analysis
                    .node(child_field(&analysis, function, "name"))
                    .unwrap()
                    .text()
                    .to_string();
                scopes
                    .add_reference(
                        identifier,
                        roles(identifier),
                        complete.clone(),
                        ReferenceDraft {
                            original_spelling: name.into(),
                            segments: vec![name.into()],
                            namespace: NameNamespace::Value,
                            scope: callable_scopes[&function_name],
                            role: ReferenceRole::Call,
                        },
                    )
                    .unwrap();
            }
        }
        let scopes = Arc::new(scopes.build().unwrap());
        let resolution = Arc::new(
            deslop_parse::ResolutionProjection::build(
                scopes,
                ResolutionPolicyId::from_parts(&[b"ordering-resolution"]).unwrap(),
            )
            .unwrap(),
        );
        if !options.conditional_import && !options.glob_import {
            assert!(resolution.results().iter().all(|result| {
                result.wire().status() == ResolutionStatus::Unique
                    && result.wire().coverage().status() == FactCoverage::Complete
            }));
        }
        let main = function_nodes
            .iter()
            .copied()
            .find(|function| {
                analysis
                    .node(child_field(&analysis, *function, "name"))
                    .unwrap()
                    .text()
                    == "main"
            })
            .unwrap();
        let main_body = child_field(&analysis, main, "body");
        let mut flow_builder = ControlFlowBuilder::new(
            Arc::clone(&analysis),
            ControlFlowPolicyId::from_parts(&[b"ordering-flow"]).unwrap(),
        );
        flow_builder
            .add_graph(ControlFlowGraphDraft {
                owner: main,
                owner_kind: ControlFlowOwnerKind::Callable,
                coverage: ControlFlowCoverageEvidence::complete(),
                points: vec![
                    ControlPointDraft {
                        kind: ControlPointKind::Entry,
                        source: None,
                        ordinal: 0,
                    },
                    ControlPointDraft {
                        kind: ControlPointKind::Synthetic(ControlSyntheticPointKind::ExitDispatch),
                        source: Some(main_body),
                        ordinal: 0,
                    },
                    ControlPointDraft {
                        kind: ControlPointKind::Exit,
                        source: None,
                        ordinal: 0,
                    },
                ],
                edges: vec![
                    ControlEdgeDraft {
                        from: 0,
                        to: 1,
                        kind: ControlEdgeKind::Entry,
                        source: main,
                        predicate: None,
                        precision: ControlEdgePrecision::Exact,
                    },
                    ControlEdgeDraft {
                        from: 1,
                        to: 2,
                        kind: ControlEdgeKind::Exit(ControlExitOutcome::Normal),
                        source: main,
                        predicate: None,
                        precision: ControlEdgePrecision::Exact,
                    },
                ],
            })
            .unwrap();
        let flow = Arc::new(flow_builder.build().unwrap());
        let regions = Arc::new(
            derive_control_regions(
                Arc::clone(&flow),
                deslop_parse::ControlRegionPolicyId::from_parts(&[b"ordering-regions"]).unwrap(),
            )
            .unwrap(),
        );
        let mut data_builder = DataFlowBuilder::new(
            Arc::clone(&regions),
            resolution,
            DataFlowPolicyId::from_parts(&[b"ordering-data"]).unwrap(),
        )
        .unwrap();
        let flow_graph = &flow.document().graphs()[0];
        data_builder
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
        let data = Arc::new(data_builder.build().unwrap());
        let non_structured = Arc::new(
            derive_non_structured_control_regions(
                regions,
                NonStructuredControlPolicyId::from_parts(&[b"ordering-non-structured"]).unwrap(),
            )
            .unwrap(),
        );
        let projection = derive_program_dependence(
            data,
            non_structured,
            ProgramDependencePolicyId::from_parts(&[b"ordering-pdg"]).unwrap(),
        )
        .unwrap();
        assert_eq!(definitions.len(), 3);
        Fixture { source, projection }
    }

    fn child_field(
        analysis: &ProjectAnalysis,
        node: deslop_parse::NodeId,
        name: &str,
    ) -> deslop_parse::NodeId {
        analysis
            .node(node)
            .unwrap()
            .children()
            .find(|child| analysis.node(*child).unwrap().field() == Some(name))
            .unwrap()
    }

    fn enclosing_function(
        analysis: &ProjectAnalysis,
        mut node: deslop_parse::NodeId,
    ) -> deslop_parse::NodeId {
        loop {
            let view = analysis.node(node).unwrap();
            if view.raw_grammar_kind() == "function_item" {
                return node;
            }
            node = view.parent().unwrap();
        }
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
    fn recipe_contracts_are_review_gated_and_scope_resolution_bound() {
        for recipe in [
            simple_import_order_recipe().unwrap(),
            hoisted_private_function_order_recipe().unwrap(),
        ] {
            assert_eq!(recipe.family(), TransformationFamily::DependencyModule);
            assert_eq!(recipe.maximum_safety(), SafetyClass::SafeWithPrecondition);
            assert_eq!(
                recipe.required_layers(),
                [
                    GraphEvidenceLayer::ScopeGraph,
                    GraphEvidenceLayer::Resolution
                ]
            );
            assert_eq!(recipe.fixtures().len(), 4);
        }
    }

    #[test]
    fn complete_fixture_emits_two_exact_behavior_preserving_orderings() {
        let fixture = fixture(FixtureOptions::default());
        let candidates = detect_ordering_candidates(&fixture.projection).unwrap();
        assert_eq!(candidates.len(), 2);
        let names = candidates
            .iter()
            .map(|candidate| candidate.recipe().name())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "rust-sort-hoisted-private-function-block",
                "rust-sort-simple-import-block",
            ])
        );
        assert!(candidates.iter().all(|candidate| {
            candidate.disposition() == CandidateDisposition::ReviewRequired
                && candidate.safety() == SafetyClass::SafeWithPrecondition
                && candidate.edits().len() == 1
                && candidate.eligibility().eligible()
        }));
        let rewritten = apply_all(&fixture.source, &candidates);
        assert!(rewritten.find("BTreeMap").unwrap() < rewritten.find("Vec;").unwrap());
        assert!(rewritten.find("fn alpha").unwrap() < rewritten.find("fn zebra").unwrap());
        assert_eq!(run_rust(&fixture.source), run_rust(&rewritten));
        assert_eq!(run_rust(&rewritten).trim(), "2");
    }

    #[test]
    fn already_ordered_fixture_emits_nothing() {
        let fixture = fixture(FixtureOptions {
            ordered: true,
            ..FixtureOptions::default()
        });
        assert!(
            detect_ordering_candidates(&fixture.projection)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn partial_scope_authority_blocks_every_ordering() {
        let fixture = fixture(FixtureOptions {
            partial_import: true,
            ..FixtureOptions::default()
        });
        assert!(
            detect_ordering_candidates(&fixture.projection)
                .unwrap()
                .is_empty()
        );
        let recipe = simple_import_order_recipe().unwrap();
        let decision = eligibility(&fixture.projection, &recipe).unwrap();
        assert!(!decision.eligible());
        assert!(decision.blocks().iter().any(|block| matches!(
            block,
            deslop_parse::GraphEligibilityBlock::IncompleteCoverage {
                layer: GraphEvidenceLayer::ScopeGraph,
                ..
            }
        )));
    }

    #[test]
    fn side_effect_import_and_unhoisted_function_each_block_only_their_recipe() {
        let side_effect = fixture(FixtureOptions {
            side_effect_import: true,
            ..FixtureOptions::default()
        });
        for recipe in [
            simple_import_order_recipe().unwrap(),
            hoisted_private_function_order_recipe().unwrap(),
        ] {
            let decision = eligibility(&side_effect.projection, &recipe).unwrap();
            assert!(decision.eligible(), "{:?}", decision.blocks());
        }
        let candidates = detect_ordering_candidates(&side_effect.projection).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].recipe().name(),
            "rust-sort-hoisted-private-function-block"
        );

        let unhoisted = fixture(FixtureOptions {
            hoisted_functions: false,
            ..FixtureOptions::default()
        });
        let candidates = detect_ordering_candidates(&unhoisted.projection).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].recipe().name(),
            "rust-sort-simple-import-block"
        );
    }

    #[test]
    fn comments_split_runs_and_prevent_detached_trivia() {
        let fixture = fixture(FixtureOptions {
            separating_comments: true,
            ..FixtureOptions::default()
        });
        assert!(
            detect_ordering_candidates(&fixture.projection)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn conditional_glob_public_and_macro_near_misses_abstain_per_recipe() {
        for options in [
            FixtureOptions {
                conditional_import: true,
                ..FixtureOptions::default()
            },
            FixtureOptions {
                glob_import: true,
                ..FixtureOptions::default()
            },
        ] {
            let fixture = fixture(options);
            let candidates = detect_ordering_candidates(&fixture.projection).unwrap();
            assert!(
                candidates.iter().all(|candidate| {
                    candidate.recipe().name() != "rust-sort-simple-import-block"
                })
            );
        }
        for options in [
            FixtureOptions {
                public_function: true,
                ..FixtureOptions::default()
            },
            FixtureOptions {
                macro_function: true,
                ..FixtureOptions::default()
            },
        ] {
            let fixture = fixture(options);
            let candidates = detect_ordering_candidates(&fixture.projection).unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].recipe().name(),
                "rust-sort-simple-import-block"
            );
        }
    }

    #[test]
    fn candidate_wire_roundtrip_is_strict_and_content_bound() {
        let fixture = fixture(FixtureOptions::default());
        let candidates = detect_ordering_candidates(&fixture.projection).unwrap();
        for candidate in candidates {
            let value = serde_json::to_value(&candidate).unwrap();
            let rebuilt: TransformationCandidate = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(rebuilt, candidate);
            let mut tampered = value;
            tampered["edits"][0]["after"] = Value::String("use tampered::value;".into());
            assert!(serde_json::from_value::<TransformationCandidate>(tampered).is_err());
        }
    }
}
