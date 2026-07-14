use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use deslop_lang::{
    CapabilitySupport, ImportTraversalRule, LanguageResolutionRulePack, PrecedenceDimension,
    PrecedenceDirection, ResolutionInstruction, ResolutionRuleSectionKind, RuleNamespace,
};

use crate::{
    BindingTarget, BindingTiming, DeclarationModifier, ImportForm, NameNamespace, ScopeFactData,
    ScopeFactId, ScopeFactKey, ScopeFactKind, ScopeGraphProjection, VisibilityKind,
};

/// A transient M3.3 traversal result. M3.4 owns serializable paths and terminal outcomes.
///
/// ```compile_fail
/// fn assert_serializable<T: serde::Serialize>() {}
/// assert_serializable::<deslop_parse::ResolutionTraversal>();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionTraversal {
    reference: ScopeFactId,
    start_scope: ScopeFactId,
    lookup_root: String,
    remaining_segments: Vec<String>,
    scopes: Vec<LexicalScopeStep>,
    candidates: Vec<TraversalCandidate>,
    deferred_imports: Vec<DeferredImportTraversal>,
    dynamic_boundaries: Vec<DynamicBoundaryTraversal>,
    rule_gaps: Vec<RuleSectionGap>,
}

impl ResolutionTraversal {
    pub fn reference(&self) -> ScopeFactId {
        self.reference
    }

    pub fn start_scope(&self) -> ScopeFactId {
        self.start_scope
    }

    pub fn lookup_root(&self) -> &str {
        &self.lookup_root
    }

    pub fn remaining_segments(&self) -> &[String] {
        &self.remaining_segments
    }

    pub fn scopes(&self) -> &[LexicalScopeStep] {
        &self.scopes
    }

    pub fn candidates(&self) -> &[TraversalCandidate] {
        &self.candidates
    }

    pub fn deferred_imports(&self) -> &[DeferredImportTraversal] {
        &self.deferred_imports
    }

    pub fn dynamic_boundaries(&self) -> &[DynamicBoundaryTraversal] {
        &self.dynamic_boundaries
    }

    pub fn rule_gaps(&self) -> &[RuleSectionGap] {
        &self.rule_gaps
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexicalScopeStep {
    scope: ScopeFactId,
    lexical_distance: u32,
}

impl LexicalScopeStep {
    pub fn scope(self) -> ScopeFactId {
        self.scope
    }

    pub fn lexical_distance(self) -> u32 {
        self.lexical_distance
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraversalCandidate {
    declaration: ScopeFactId,
    definitions: Vec<ScopeFactId>,
    bindings: Vec<ScopeFactId>,
    lexical_distance: u32,
    namespace: NamespaceReachability,
    visibility: VisibilityObservation,
    timing: Vec<TimingObservation>,
    shadowed_by: Vec<ExplicitShadowing>,
    adapter_schema_matches: bool,
    precedence: Vec<PrecedenceComponent>,
}

impl TraversalCandidate {
    pub fn declaration(&self) -> ScopeFactId {
        self.declaration
    }

    pub fn definitions(&self) -> &[ScopeFactId] {
        &self.definitions
    }

    pub fn bindings(&self) -> &[ScopeFactId] {
        &self.bindings
    }

    pub fn lexical_distance(&self) -> u32 {
        self.lexical_distance
    }

    pub fn namespace(&self) -> NamespaceReachability {
        self.namespace
    }

    pub fn visibility(&self) -> VisibilityObservation {
        self.visibility
    }

    pub fn timing(&self) -> &[TimingObservation] {
        &self.timing
    }

    pub fn shadowed_by(&self) -> &[ExplicitShadowing] {
        &self.shadowed_by
    }

    pub fn adapter_schema_matches(&self) -> bool {
        self.adapter_schema_matches
    }

    pub fn precedence(&self) -> &[PrecedenceComponent] {
        &self.precedence
    }
}

/// Namespace reachability is directional from the reference namespace to the declaration namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceReachability {
    Exact,
    Unified,
    Transition,
    Unreachable,
    RuleUnavailable(CapabilitySupport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityObservation {
    Visible,
    WithinBoundary,
    OutsideBoundary,
    RuleRequired,
    RuleUnavailable(CapabilitySupport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingObservation {
    VisibleAtReference { binding: ScopeFactId },
    DeclaredAfterReference { binding: ScopeFactId },
    AdapterRuleRequired { binding: ScopeFactId },
    Unspecified,
    RuleUnavailable(CapabilitySupport),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitShadowing {
    fact: ScopeFactId,
    declaration: ScopeFactId,
    adapter_rule: String,
}

impl ExplicitShadowing {
    pub fn fact(&self) -> ScopeFactId {
        self.fact
    }

    pub fn declaration(&self) -> ScopeFactId {
        self.declaration
    }

    pub fn adapter_rule(&self) -> &str {
        &self.adapter_rule
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrecedenceComponent {
    dimension: PrecedenceDimension,
    direction: PrecedenceDirection,
    value: u64,
}

impl PrecedenceComponent {
    pub fn dimension(self) -> PrecedenceDimension {
        self.dimension
    }

    pub fn direction(self) -> PrecedenceDirection {
        self.direction
    }

    pub fn value(self) -> u64 {
        self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredImportTraversal {
    import: ScopeFactId,
    lexical_distance: u32,
    rule: ImportTraversalRule,
    rule_declared: bool,
    conditions: Vec<String>,
}

impl DeferredImportTraversal {
    pub fn import(&self) -> ScopeFactId {
        self.import
    }

    pub fn lexical_distance(&self) -> u32 {
        self.lexical_distance
    }

    pub fn rule(&self) -> ImportTraversalRule {
        self.rule
    }

    pub fn rule_declared(&self) -> bool {
        self.rule_declared
    }

    pub fn conditions(&self) -> &[String] {
        &self.conditions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicBoundaryTraversal {
    fact: ScopeFactId,
    lexical_distance: u32,
    construct_kind: String,
    reason: String,
}

impl DynamicBoundaryTraversal {
    pub fn fact(&self) -> ScopeFactId {
        self.fact
    }

    pub fn lexical_distance(&self) -> u32 {
        self.lexical_distance
    }

    pub fn construct_kind(&self) -> &str {
        &self.construct_kind
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSectionGap {
    section: ResolutionRuleSectionKind,
    support: CapabilitySupport,
}

impl RuleSectionGap {
    pub fn section(self) -> ResolutionRuleSectionKind {
        self.section
    }

    pub fn support(self) -> CapabilitySupport {
        self.support
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionTraversalError {
    Graph(String),
    MissingFactKey(String),
    WrongFactKind {
        expected: ScopeFactKind,
        actual: ScopeFactKind,
    },
    InvalidReference(String),
}

impl fmt::Display for ResolutionTraversalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Graph(message) => write!(formatter, "scope graph traversal failed: {message}"),
            Self::MissingFactKey(key) => write!(formatter, "scope graph fact key {key} is absent"),
            Self::WrongFactKind { expected, actual } => write!(
                formatter,
                "expected {} fact, found {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::InvalidReference(message) => write!(formatter, "invalid reference: {message}"),
        }
    }
}

impl Error for ResolutionTraversalError {}

#[derive(Debug)]
pub struct ResolutionTraversalEngine<'graph> {
    graph: &'graph ScopeGraphProjection,
    by_key: BTreeMap<ScopeFactKey, ScopeFactId>,
    declarations_by_scope: BTreeMap<ScopeFactKey, Vec<ScopeFactId>>,
    definitions_by_declaration: BTreeMap<ScopeFactKey, Vec<ScopeFactId>>,
    bindings_by_declaration: BTreeMap<ScopeFactKey, Vec<ScopeFactId>>,
    imports_by_scope: BTreeMap<ScopeFactKey, Vec<ScopeFactId>>,
    dynamic_by_scope: BTreeMap<ScopeFactKey, Vec<ScopeFactId>>,
    shadowing_by_declaration: BTreeMap<ScopeFactKey, Vec<ScopeFactId>>,
    fact_order: BTreeMap<ScopeFactId, u64>,
}

impl<'graph> ResolutionTraversalEngine<'graph> {
    pub fn new(graph: &'graph ScopeGraphProjection) -> Result<Self, ResolutionTraversalError> {
        let mut by_key = BTreeMap::new();
        let mut definitions_by_declaration: BTreeMap<ScopeFactKey, Vec<ScopeFactId>> =
            BTreeMap::new();
        let mut definition_to_declaration = BTreeMap::new();
        let mut fact_order = BTreeMap::new();
        for (ordinal, fact) in graph.facts().iter().enumerate() {
            by_key.insert(fact.key().clone(), fact.id());
            fact_order.insert(fact.id(), ordinal as u64);
            if let ScopeFactData::Definition { declaration, .. } = fact.data() {
                definitions_by_declaration
                    .entry(declaration.clone())
                    .or_default()
                    .push(fact.id());
                definition_to_declaration.insert(fact.key().clone(), declaration.clone());
            }
        }

        let mut declarations_by_scope: BTreeMap<ScopeFactKey, Vec<ScopeFactId>> = BTreeMap::new();
        let mut bindings_by_declaration: BTreeMap<ScopeFactKey, Vec<ScopeFactId>> = BTreeMap::new();
        let mut imports_by_scope: BTreeMap<ScopeFactKey, Vec<ScopeFactId>> = BTreeMap::new();
        let mut dynamic_by_scope: BTreeMap<ScopeFactKey, Vec<ScopeFactId>> = BTreeMap::new();
        let mut shadowing_by_declaration: BTreeMap<ScopeFactKey, Vec<ScopeFactId>> =
            BTreeMap::new();
        for fact in graph.facts() {
            match fact.data() {
                ScopeFactData::Declaration { scope, .. } => declarations_by_scope
                    .entry(scope.clone())
                    .or_default()
                    .push(fact.id()),
                ScopeFactData::Binding { target, .. } => {
                    let declaration = match target {
                        BindingTarget::Declaration(key) => Some(key),
                        BindingTarget::Definition(key) => definition_to_declaration.get(key),
                    };
                    if let Some(declaration) = declaration {
                        bindings_by_declaration
                            .entry(declaration.clone())
                            .or_default()
                            .push(fact.id());
                    }
                }
                ScopeFactData::Import { scope, .. } => imports_by_scope
                    .entry(scope.clone())
                    .or_default()
                    .push(fact.id()),
                ScopeFactData::DynamicBoundary { scopes, .. } => {
                    for scope in scopes {
                        dynamic_by_scope
                            .entry(scope.clone())
                            .or_default()
                            .push(fact.id());
                    }
                }
                ScopeFactData::Shadowing {
                    shadowed_declaration,
                    ..
                } => shadowing_by_declaration
                    .entry(shadowed_declaration.clone())
                    .or_default()
                    .push(fact.id()),
                _ => {}
            }
        }

        Ok(Self {
            graph,
            by_key,
            declarations_by_scope,
            definitions_by_declaration,
            bindings_by_declaration,
            imports_by_scope,
            dynamic_by_scope,
            shadowing_by_declaration,
            fact_order,
        })
    }

    pub fn traverse_reference(
        &self,
        reference: ScopeFactId,
    ) -> Result<ResolutionTraversal, ResolutionTraversalError> {
        let reference_fact = self
            .graph
            .fact(reference)
            .map_err(|error| ResolutionTraversalError::Graph(error.to_string()))?;
        let (segments, reference_namespace, start_scope_key) = match reference_fact.data() {
            ScopeFactData::Reference {
                segments,
                namespace,
                scope,
                ..
            } => (segments, namespace, scope),
            actual => {
                return Err(ResolutionTraversalError::WrongFactKind {
                    expected: ScopeFactKind::Reference,
                    actual: actual.kind(),
                });
            }
        };
        let Some((lookup_root, remaining_segments)) = segments.split_first() else {
            return Err(ResolutionTraversalError::InvalidReference(
                "reference has no lookup segments".into(),
            ));
        };
        let start_scope = self.id_for_key(start_scope_key)?;
        let scopes = self.scope_chain(start_scope)?;
        let scope_keys = scopes
            .iter()
            .map(|step| self.graph.fact(step.scope).map(|fact| fact.key().clone()))
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(|error| ResolutionTraversalError::Graph(error.to_string()))?;
        let pack = reference_fact.evidence().adapter.resolution_rules();
        let visibility_support = pack
            .section(ResolutionRuleSectionKind::VisibilityTiming)
            .support();

        let mut candidates = Vec::new();
        for step in &scopes {
            let scope_key = self.graph.fact(step.scope).unwrap().key();
            for declaration in self
                .declarations_by_scope
                .get(scope_key)
                .into_iter()
                .flatten()
            {
                let declaration_fact = self.graph.fact(*declaration).unwrap();
                let ScopeFactData::Declaration {
                    lookup_key,
                    namespace,
                    visibility,
                    modifiers,
                    ..
                } = declaration_fact.data()
                else {
                    unreachable!("declaration index contains only declarations")
                };
                if lookup_key != lookup_root {
                    continue;
                }
                let namespace = namespace_reachability(pack, reference_namespace, namespace);
                let bindings = self
                    .bindings_by_declaration
                    .get(declaration_fact.key())
                    .cloned()
                    .unwrap_or_default();
                let timing = self.timing_observations(
                    &bindings,
                    modifiers,
                    reference_fact.evidence().source_order,
                    visibility_support,
                );
                let visibility = visibility_observation(
                    visibility.kind,
                    visibility.boundary.as_ref(),
                    &scope_keys,
                    visibility_support,
                );
                let adapter_schema_matches = declaration_fact.evidence().adapter.schema()
                    == reference_fact.evidence().adapter.schema();
                let precedence = self.precedence_components(
                    pack,
                    step.lexical_distance,
                    namespace,
                    declaration_fact.evidence().source_order,
                    *declaration,
                );
                candidates.push(TraversalCandidate {
                    declaration: *declaration,
                    definitions: self
                        .definitions_by_declaration
                        .get(declaration_fact.key())
                        .cloned()
                        .unwrap_or_default(),
                    bindings,
                    lexical_distance: step.lexical_distance,
                    namespace,
                    visibility,
                    timing,
                    shadowed_by: Vec::new(),
                    adapter_schema_matches,
                    precedence,
                });
            }
        }

        let candidate_ids = candidates
            .iter()
            .map(|candidate| candidate.declaration)
            .collect::<BTreeSet<_>>();
        for candidate in &mut candidates {
            let key = self.graph.fact(candidate.declaration).unwrap().key();
            candidate.shadowed_by = self
                .shadowing_by_declaration
                .get(key)
                .into_iter()
                .flatten()
                .filter_map(|fact_id| {
                    let fact = self.graph.fact(*fact_id).ok()?;
                    let ScopeFactData::Shadowing {
                        shadowing_declaration,
                        adapter_rule,
                        ..
                    } = fact.data()
                    else {
                        return None;
                    };
                    let declaration = self.id_for_key(shadowing_declaration).ok()?;
                    candidate_ids
                        .contains(&declaration)
                        .then(|| ExplicitShadowing {
                            fact: *fact_id,
                            declaration,
                            adapter_rule: adapter_rule.clone(),
                        })
                })
                .collect();
        }

        let deferred_imports = self.deferred_imports(&scopes, lookup_root, pack);
        let dynamic_boundaries = self.dynamic_boundaries(&scopes, reference_namespace, pack);
        let rule_gaps = pack
            .sections()
            .iter()
            .filter(|section| section.support() != CapabilitySupport::Provided)
            .map(|section| RuleSectionGap {
                section: section.kind(),
                support: section.support(),
            })
            .collect();

        Ok(ResolutionTraversal {
            reference,
            start_scope,
            lookup_root: lookup_root.clone(),
            remaining_segments: remaining_segments.to_vec(),
            scopes,
            candidates,
            deferred_imports,
            dynamic_boundaries,
            rule_gaps,
        })
    }

    fn id_for_key(&self, key: &ScopeFactKey) -> Result<ScopeFactId, ResolutionTraversalError> {
        self.by_key
            .get(key)
            .copied()
            .ok_or_else(|| ResolutionTraversalError::MissingFactKey(key.as_str().into()))
    }

    fn scope_chain(
        &self,
        start_scope: ScopeFactId,
    ) -> Result<Vec<LexicalScopeStep>, ResolutionTraversalError> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(start_scope);
        while let Some(scope) = current {
            if !seen.insert(scope) {
                return Err(ResolutionTraversalError::Graph(
                    "scope parent relation contains a cycle".into(),
                ));
            }
            let fact = self
                .graph
                .fact(scope)
                .map_err(|error| ResolutionTraversalError::Graph(error.to_string()))?;
            let ScopeFactData::Scope { parent, .. } = fact.data() else {
                return Err(ResolutionTraversalError::WrongFactKind {
                    expected: ScopeFactKind::Scope,
                    actual: fact.data().kind(),
                });
            };
            chain.push(LexicalScopeStep {
                scope,
                lexical_distance: (chain.len() as u32),
            });
            current = parent
                .as_ref()
                .map(|key| self.id_for_key(key))
                .transpose()?;
        }
        Ok(chain)
    }

    fn timing_observations(
        &self,
        bindings: &[ScopeFactId],
        modifiers: &[DeclarationModifier],
        reference_order: u64,
        support: CapabilitySupport,
    ) -> Vec<TimingObservation> {
        if support != CapabilitySupport::Provided {
            return vec![TimingObservation::RuleUnavailable(support)];
        }
        if bindings.is_empty() {
            return vec![TimingObservation::Unspecified];
        }
        bindings
            .iter()
            .map(|binding| {
                let fact = self.graph.fact(*binding).unwrap();
                let ScopeFactData::Binding { timing, .. } = fact.data() else {
                    unreachable!("binding index contains only bindings")
                };
                match timing {
                    BindingTiming::ScopeEntry | BindingTiming::Hoisted => {
                        TimingObservation::VisibleAtReference { binding: *binding }
                    }
                    BindingTiming::AtDeclaration
                    | BindingTiming::BeforeInitializer
                    | BindingTiming::AfterInitializer => {
                        if fact.evidence().source_order <= reference_order
                            || modifiers.iter().any(|modifier| {
                                matches!(
                                    modifier,
                                    DeclarationModifier::Hoisted
                                        | DeclarationModifier::Forward
                                        | DeclarationModifier::Recursive
                                )
                            })
                        {
                            TimingObservation::VisibleAtReference { binding: *binding }
                        } else {
                            TimingObservation::DeclaredAfterReference { binding: *binding }
                        }
                    }
                    BindingTiming::AdapterDefined { .. } => {
                        TimingObservation::AdapterRuleRequired { binding: *binding }
                    }
                }
            })
            .collect()
    }

    fn precedence_components(
        &self,
        pack: &LanguageResolutionRulePack,
        lexical_distance: u32,
        namespace: NamespaceReachability,
        source_order: u64,
        declaration: ScopeFactId,
    ) -> Vec<PrecedenceComponent> {
        let section = pack.section(ResolutionRuleSectionKind::Precedence);
        let Some(ResolutionInstruction::Precedence { terms }) = section.instructions().first()
        else {
            return Vec::new();
        };
        terms
            .iter()
            .map(|term| PrecedenceComponent {
                dimension: term.dimension(),
                direction: term.direction(),
                value: match term.dimension() {
                    PrecedenceDimension::RuleStep => 0,
                    PrecedenceDimension::LexicalDistance => u64::from(lexical_distance),
                    PrecedenceDimension::Namespace => namespace_rank(namespace),
                    PrecedenceDimension::ImportSpecificity => 0,
                    PrecedenceDimension::SourceOrder => source_order,
                    PrecedenceDimension::AdapterOrder => self.fact_order[&declaration],
                },
            })
            .collect()
    }

    fn deferred_imports(
        &self,
        scopes: &[LexicalScopeStep],
        lookup_root: &str,
        pack: &LanguageResolutionRulePack,
    ) -> Vec<DeferredImportTraversal> {
        let section = pack.section(ResolutionRuleSectionKind::ImportsExports);
        let declared = section
            .instructions()
            .iter()
            .filter_map(|instruction| match instruction {
                ResolutionInstruction::ImportTraversal { rule } => Some(*rule),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut traversals = Vec::new();
        for step in scopes {
            let scope = self.graph.fact(step.scope).unwrap().key();
            for import in self.imports_by_scope.get(scope).into_iter().flatten() {
                let fact = self.graph.fact(*import).unwrap();
                let ScopeFactData::Import {
                    module_segments,
                    form,
                    alias,
                    selected_names,
                    conditions,
                    ..
                } = fact.data()
                else {
                    unreachable!("import index contains only imports")
                };
                let rule = if alias.as_deref() == Some(lookup_root) {
                    Some(ImportTraversalRule::Alias)
                } else if *form == ImportForm::Selective
                    && selected_names.iter().any(|name| name == lookup_root)
                {
                    Some(ImportTraversalRule::Selective)
                } else if *form == ImportForm::Glob {
                    Some(ImportTraversalRule::Glob)
                } else if *form == ImportForm::Module
                    && alias.is_none()
                    && module_segments
                        .last()
                        .is_some_and(|name| name == lookup_root)
                {
                    Some(ImportTraversalRule::Explicit)
                } else {
                    None
                };
                if let Some(rule) = rule {
                    traversals.push(DeferredImportTraversal {
                        import: *import,
                        lexical_distance: step.lexical_distance,
                        rule,
                        rule_declared: declared.contains(&rule),
                        conditions: conditions.clone(),
                    });
                }
            }
        }
        traversals
    }

    fn dynamic_boundaries(
        &self,
        scopes: &[LexicalScopeStep],
        namespace: &NameNamespace,
        pack: &LanguageResolutionRulePack,
    ) -> Vec<DynamicBoundaryTraversal> {
        let mut seen = BTreeSet::new();
        let mut boundaries = Vec::new();
        for step in scopes {
            let scope = self.graph.fact(step.scope).unwrap().key();
            for boundary in self.dynamic_by_scope.get(scope).into_iter().flatten() {
                if !seen.insert(*boundary) {
                    continue;
                }
                let fact = self.graph.fact(*boundary).unwrap();
                let ScopeFactData::DynamicBoundary {
                    construct_kind,
                    namespaces,
                    reason,
                    ..
                } = fact.data()
                else {
                    unreachable!("dynamic-boundary index contains only boundaries")
                };
                if namespaces.iter().any(|affected| {
                    !matches!(
                        namespace_reachability(pack, namespace, affected),
                        NamespaceReachability::Unreachable
                            | NamespaceReachability::RuleUnavailable(_)
                    )
                }) {
                    boundaries.push(DynamicBoundaryTraversal {
                        fact: *boundary,
                        lexical_distance: step.lexical_distance,
                        construct_kind: construct_kind.clone(),
                        reason: reason.clone(),
                    });
                }
            }
        }
        boundaries
    }
}

fn namespace_reachability(
    pack: &LanguageResolutionRulePack,
    reference: &NameNamespace,
    declaration: &NameNamespace,
) -> NamespaceReachability {
    if reference == declaration {
        return NamespaceReachability::Exact;
    }
    let section = pack.section(ResolutionRuleSectionKind::Namespaces);
    if section.support() != CapabilitySupport::Provided {
        return NamespaceReachability::RuleUnavailable(section.support());
    }
    let reference = rule_namespace(reference);
    let declaration = rule_namespace(declaration);
    for instruction in section.instructions() {
        match instruction {
            ResolutionInstruction::UnifyNamespaces { namespaces }
                if namespaces.contains(&reference) && namespaces.contains(&declaration) =>
            {
                return NamespaceReachability::Unified;
            }
            ResolutionInstruction::AllowNamespaceTransition { from, to, .. }
                if from == &reference && to == &declaration =>
            {
                return NamespaceReachability::Transition;
            }
            _ => {}
        }
    }
    NamespaceReachability::Unreachable
}

fn rule_namespace(namespace: &NameNamespace) -> RuleNamespace {
    match namespace {
        NameNamespace::Value => RuleNamespace::Value,
        NameNamespace::Type => RuleNamespace::Type,
        NameNamespace::Module => RuleNamespace::Module,
        NameNamespace::Macro => RuleNamespace::Macro,
        NameNamespace::Label => RuleNamespace::Label,
        NameNamespace::Member => RuleNamespace::Member,
        NameNamespace::AdapterDefined { schema, name } => RuleNamespace::AdapterDefined {
            schema: schema.clone(),
            name: name.clone(),
        },
    }
}

fn visibility_observation(
    kind: VisibilityKind,
    boundary: Option<&ScopeFactKey>,
    scope_chain: &BTreeSet<ScopeFactKey>,
    support: CapabilitySupport,
) -> VisibilityObservation {
    if support != CapabilitySupport::Provided {
        return VisibilityObservation::RuleUnavailable(support);
    }
    if let Some(boundary) = boundary {
        return if scope_chain.contains(boundary) {
            VisibilityObservation::WithinBoundary
        } else {
            VisibilityObservation::OutsideBoundary
        };
    }
    if kind == VisibilityKind::Public {
        VisibilityObservation::Visible
    } else {
        VisibilityObservation::RuleRequired
    }
}

const fn namespace_rank(namespace: NamespaceReachability) -> u64 {
    match namespace {
        NamespaceReachability::Exact => 0,
        NamespaceReachability::Unified => 1,
        NamespaceReachability::Transition => 2,
        NamespaceReachability::Unreachable => 3,
        NamespaceReachability::RuleUnavailable(_) => 4,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use crate::{
        BindingDraft, BindingForm, BindingTargetDraft, BuildContextId, CanonicalRoleSet,
        DeclarationDraft, DynamicBoundaryDraft, FactCoverageEvidence, ImportDraft, Mutability,
        NamespacePolicy, ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft, ReferenceRole,
        RepositoryId, ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind, ShadowingDraft,
        VisibilityDraft,
    };

    use super::*;

    const SOURCE: &str = r#"fn outer() {
    let target = 1;
    {
        target;
        let target = 2;
    }
}
fn sibling() {
    let target = 3;
}
"#;

    struct Fixture {
        graph: ScopeGraphProjection,
        reference: ScopeFactId,
        outer: ScopeFactId,
        later: ScopeFactId,
        wrong_namespace: ScopeFactId,
        sibling: ScopeFactId,
    }

    fn analysis() -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("traversal.rs"), SOURCE).unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("resolution-traversal-test-repository").unwrap(),
        )
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn nodes_by_text(analysis: &ProjectAnalysis, text: &str) -> Vec<crate::NodeId> {
        analysis
            .node_ids()
            .filter(|id| analysis.node(*id).unwrap().text() == text)
            .collect()
    }

    fn nodes_by_kind(analysis: &ProjectAnalysis, kind: &str) -> Vec<crate::NodeId> {
        analysis
            .node_ids()
            .filter(|id| analysis.node(*id).unwrap().raw_kind() == kind)
            .collect()
    }

    fn roles(analysis: &Arc<ProjectAnalysis>, node: crate::NodeId) -> CanonicalRoleSet {
        analysis
            .canonical_role_projection(Path::new("traversal.rs"))
            .unwrap()
            .facts()
            .iter()
            .find(|fact| fact.node() == node)
            .unwrap()
            .roles()
    }

    fn partial() -> FactCoverageEvidence {
        FactCoverageEvidence::partial("hand-labelled M3.3 traversal fixture").unwrap()
    }

    fn scoped(boundary: ScopeFactId) -> VisibilityDraft {
        VisibilityDraft {
            kind: VisibilityKind::Scope,
            boundary: Some(boundary),
            adapter_rule: None,
        }
    }

    fn fixture() -> Fixture {
        let analysis = analysis();
        let target_nodes = nodes_by_text(&analysis, "target");
        let functions = nodes_by_kind(&analysis, "function_item");
        let blocks = nodes_by_kind(&analysis, "block");
        assert_eq!(target_nodes.len(), 4);
        assert_eq!(functions.len(), 2);
        assert!(blocks.len() >= 3);
        let root_node = nodes_by_kind(&analysis, "source_file")[0];
        let namespaces = NamespacePolicy::new(
            vec![
                NameNamespace::Value,
                NameNamespace::Type,
                NameNamespace::Module,
                NameNamespace::Macro,
                NameNamespace::Label,
                NameNamespace::Member,
            ],
            vec![],
        )
        .unwrap();
        let mut builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"traversal-target"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"hand-labelled-traversal/1"]).unwrap(),
        )
        .unwrap();
        let root = builder
            .add_scope(
                root_node,
                roles(&analysis, root_node),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let outer_scope = builder
            .add_scope(
                functions[0],
                roles(&analysis, functions[0]),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(root),
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let inner_scope = builder
            .add_scope(
                blocks[1],
                roles(&analysis, blocks[1]),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::Block,
                    parent: Some(outer_scope),
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let sibling_scope = builder
            .add_scope(
                functions[1],
                roles(&analysis, functions[1]),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(root),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();
        let outer = builder
            .add_declaration(
                target_nodes[0],
                roles(&analysis, target_nodes[0]),
                partial(),
                DeclarationDraft {
                    original_name: "target".into(),
                    lookup_key: "target".into(),
                    namespace: NameNamespace::Value,
                    scope: outer_scope,
                    visibility: scoped(outer_scope),
                    modifiers: vec![],
                },
            )
            .unwrap();
        builder
            .add_binding(
                target_nodes[0],
                roles(&analysis, target_nodes[0]),
                partial(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(outer),
                    form: BindingForm::Declaration,
                    timing: BindingTiming::AtDeclaration,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let reference = builder
            .add_reference(
                target_nodes[1],
                roles(&analysis, target_nodes[1]),
                partial(),
                ReferenceDraft {
                    original_spelling: "target".into(),
                    segments: vec!["target".into()],
                    namespace: NameNamespace::Value,
                    scope: inner_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        let later = builder
            .add_declaration(
                target_nodes[2],
                roles(&analysis, target_nodes[2]),
                partial(),
                DeclarationDraft {
                    original_name: "target".into(),
                    lookup_key: "target".into(),
                    namespace: NameNamespace::Value,
                    scope: inner_scope,
                    visibility: scoped(inner_scope),
                    modifiers: vec![],
                },
            )
            .unwrap();
        builder
            .add_binding(
                target_nodes[2],
                roles(&analysis, target_nodes[2]),
                partial(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(later),
                    form: BindingForm::Declaration,
                    timing: BindingTiming::AfterInitializer,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let wrong_namespace = builder
            .add_declaration(
                target_nodes[0],
                roles(&analysis, target_nodes[0]),
                partial(),
                DeclarationDraft {
                    original_name: "target".into(),
                    lookup_key: "target".into(),
                    namespace: NameNamespace::Type,
                    scope: inner_scope,
                    visibility: scoped(inner_scope),
                    modifiers: vec![],
                },
            )
            .unwrap();
        let sibling = builder
            .add_declaration(
                target_nodes[3],
                roles(&analysis, target_nodes[3]),
                partial(),
                DeclarationDraft {
                    original_name: "target".into(),
                    lookup_key: "target".into(),
                    namespace: NameNamespace::Value,
                    scope: sibling_scope,
                    visibility: scoped(sibling_scope),
                    modifiers: vec![],
                },
            )
            .unwrap();
        builder
            .add_shadowing(
                target_nodes[2],
                roles(&analysis, target_nodes[2]),
                partial(),
                ShadowingDraft {
                    shadowing_declaration: later,
                    shadowed_declaration: outer,
                    namespace: NameNamespace::Value,
                    adapter_rule: "fixture-block-shadowing/1".into(),
                },
            )
            .unwrap();
        builder
            .add_import(
                root_node,
                roles(&analysis, root_node),
                partial(),
                ImportDraft {
                    scope: root,
                    module_segments: vec!["crate".into(), "dependency".into()],
                    form: ImportForm::Module,
                    alias: Some("target".into()),
                    selected_names: vec![],
                    conditions: vec!["default-target".into()],
                },
            )
            .unwrap();
        builder
            .add_dynamic_boundary(
                target_nodes[1],
                roles(&analysis, target_nodes[1]),
                partial(),
                DynamicBoundaryDraft {
                    construct_kind: "macro-invocation".into(),
                    scopes: vec![inner_scope],
                    namespaces: vec![NameNamespace::Value],
                    reason: "macro expansion is unavailable".into(),
                },
            )
            .unwrap();

        Fixture {
            graph: builder.build().unwrap(),
            reference,
            outer,
            later,
            wrong_namespace,
            sibling,
        }
    }

    #[test]
    fn lexical_traversal_retains_attempts_without_a_global_lookup_or_winner() {
        let fixture = fixture();
        let engine = ResolutionTraversalEngine::new(&fixture.graph).unwrap();
        let traversal = engine.traverse_reference(fixture.reference).unwrap();

        assert_eq!(traversal.lookup_root(), "target");
        assert!(traversal.remaining_segments().is_empty());
        assert_eq!(traversal.scopes().len(), 3);
        let ids = traversal
            .candidates()
            .iter()
            .map(TraversalCandidate::declaration)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids,
            [fixture.outer, fixture.later, fixture.wrong_namespace]
                .into_iter()
                .collect()
        );
        assert!(!ids.contains(&fixture.sibling));

        let outer = traversal
            .candidates()
            .iter()
            .find(|candidate| candidate.declaration() == fixture.outer)
            .unwrap();
        assert_eq!(outer.lexical_distance(), 1);
        assert_eq!(outer.namespace(), NamespaceReachability::Exact);
        assert!(matches!(
            outer.timing(),
            [TimingObservation::VisibleAtReference { .. }]
        ));
        assert_eq!(outer.shadowed_by()[0].declaration(), fixture.later);

        let later = traversal
            .candidates()
            .iter()
            .find(|candidate| candidate.declaration() == fixture.later)
            .unwrap();
        assert!(matches!(
            later.timing(),
            [TimingObservation::DeclaredAfterReference { .. }]
        ));
        let wrong = traversal
            .candidates()
            .iter()
            .find(|candidate| candidate.declaration() == fixture.wrong_namespace)
            .unwrap();
        assert_eq!(wrong.namespace(), NamespaceReachability::Unreachable);

        assert_eq!(traversal.deferred_imports().len(), 1);
        assert_eq!(
            traversal.deferred_imports()[0].rule(),
            ImportTraversalRule::Alias
        );
        assert!(traversal.deferred_imports()[0].rule_declared());
        assert_eq!(traversal.dynamic_boundaries().len(), 1);
        assert_eq!(
            traversal.dynamic_boundaries()[0].reason(),
            "macro expansion is unavailable"
        );
        assert_eq!(
            traversal.rule_gaps(),
            &[RuleSectionGap {
                section: ResolutionRuleSectionKind::Extraction,
                support: CapabilitySupport::Unknown,
            }]
        );
    }

    #[test]
    fn precedence_is_directional_structured_data_and_never_selects_a_candidate() {
        let fixture = fixture();
        let engine = ResolutionTraversalEngine::new(&fixture.graph).unwrap();
        let traversal = engine.traverse_reference(fixture.reference).unwrap();
        for candidate in traversal.candidates() {
            assert!(!candidate.precedence().is_empty());
            assert_eq!(
                candidate
                    .precedence()
                    .iter()
                    .map(|component| component.dimension())
                    .collect::<BTreeSet<_>>()
                    .len(),
                candidate.precedence().len()
            );
            assert!(candidate.precedence().iter().any(|component| {
                component.dimension() == PrecedenceDimension::LexicalDistance
                    && component.direction() == PrecedenceDirection::LowerFirst
            }));
        }
        assert_eq!(traversal.candidates().len(), 3);
    }

    #[test]
    fn traversal_rejects_non_reference_handles() {
        let fixture = fixture();
        let engine = ResolutionTraversalEngine::new(&fixture.graph).unwrap();
        assert!(matches!(
            engine.traverse_reference(fixture.outer),
            Err(ResolutionTraversalError::WrongFactKind {
                expected: ScopeFactKind::Reference,
                actual: ScopeFactKind::Declaration,
            })
        ));
    }
}
