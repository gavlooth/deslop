use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use deslop_lang::{AdapterCapability, CanonicalRoleSet, CapabilityAuthority, CapabilitySupport};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    GrammarSelection, LanguageAdapterIdentity, NodeId, NodeKey, ProjectAnalysis, ProjectionId,
};

pub const SCOPE_GRAPH_SCHEMA: &str = "deslop.scope-graph/1";
pub const BUILD_CONTEXT_SCHEMA: &str = "deslop.build-context/1";
pub const SCOPE_FACT_POLICY_SCHEMA: &str = "deslop.scope-fact-policy/1";

static NEXT_SCOPE_GRAPH_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BuildContextId(String);

impl BuildContextId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ScopeGraphBuildError> {
        derive_external_id(BUILD_CONTEXT_SCHEMA, "bc1_", parts).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BuildContextId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "bc1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ScopeFactPolicyId(String);

impl ScopeFactPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ScopeGraphBuildError> {
        derive_external_id(SCOPE_FACT_POLICY_SCHEMA, "sfp1_", parts).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ScopeFactPolicyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "sfp1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ScopeFactKey(String);

impl ScopeFactKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ScopeFactKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "sf1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

/// Dense identity for a fact in one live builder/projection. It is intentionally not serializable.
///
/// ```compile_fail
/// fn assert_serializable<T: serde::Serialize>() {}
/// assert_serializable::<deslop_parse::ScopeFactId>();
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeFactId {
    owner: u64,
    index: u32,
}

impl fmt::Debug for ScopeFactId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScopeFactId")
            .field("owner", &self.owner)
            .field("index", &self.index)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FactCoverage {
    Complete,
    Partial,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactCoverageEvidence {
    pub status: FactCoverage,
    pub reason: Option<String>,
}

impl FactCoverageEvidence {
    pub fn complete() -> Self {
        Self {
            status: FactCoverage::Complete,
            reason: None,
        }
    }

    pub fn partial(reason: impl Into<String>) -> Result<Self, ScopeGraphBuildError> {
        Self::incomplete(FactCoverage::Partial, reason)
    }

    pub fn unsupported(reason: impl Into<String>) -> Result<Self, ScopeGraphBuildError> {
        Self::incomplete(FactCoverage::Unsupported, reason)
    }

    pub fn failed(reason: impl Into<String>) -> Result<Self, ScopeGraphBuildError> {
        Self::incomplete(FactCoverage::Failed, reason)
    }

    fn incomplete(
        status: FactCoverage,
        reason: impl Into<String>,
    ) -> Result<Self, ScopeGraphBuildError> {
        let evidence = Self {
            status,
            reason: Some(reason.into()),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    fn validate(&self) -> Result<(), ScopeGraphBuildError> {
        match (self.status, self.reason.as_deref()) {
            (FactCoverage::Complete, None) => Ok(()),
            (FactCoverage::Complete, Some(_)) => Err(ScopeGraphBuildError::Invalid(
                "complete fact coverage cannot carry an incompleteness reason".into(),
            )),
            (_, Some(reason)) => validate_nonempty("fact coverage reason", reason),
            (_, None) => Err(ScopeGraphBuildError::Invalid(
                "incomplete fact coverage must retain an exact reason".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeKind {
    Project,
    Package,
    BuildTarget,
    Module,
    File,
    Namespace,
    Type,
    Callable,
    Block,
    Comprehension,
    Pattern,
    Handler,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NameNamespace {
    Value,
    Type,
    Module,
    Macro,
    Label,
    Member,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespacePolicy {
    pub namespaces: Vec<NameNamespace>,
    pub unified_groups: Vec<Vec<NameNamespace>>,
}

impl NamespacePolicy {
    pub fn new(
        namespaces: Vec<NameNamespace>,
        unified_groups: Vec<Vec<NameNamespace>>,
    ) -> Result<Self, ScopeGraphBuildError> {
        let policy = Self {
            namespaces,
            unified_groups,
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<(), ScopeGraphBuildError> {
        if self.namespaces.is_empty() {
            return Err(ScopeGraphBuildError::Invalid(
                "namespace policy must declare at least one namespace".to_string(),
            ));
        }
        let declared = self.namespaces.iter().collect::<BTreeSet<_>>();
        if declared.len() != self.namespaces.len() {
            return Err(ScopeGraphBuildError::Invalid(
                "namespace policy contains duplicate namespaces".to_string(),
            ));
        }
        let mut grouped = BTreeSet::new();
        for group in &self.unified_groups {
            if group.len() < 2 {
                return Err(ScopeGraphBuildError::Invalid(
                    "a unified namespace group must contain at least two namespaces".to_string(),
                ));
            }
            for namespace in group {
                validate_namespace(namespace)?;
                if !declared.contains(namespace) {
                    return Err(ScopeGraphBuildError::Invalid(
                        "unified namespace group names an undeclared namespace".to_string(),
                    ));
                }
                if !grouped.insert(namespace) {
                    return Err(ScopeGraphBuildError::Invalid(
                        "a namespace may occur in only one unified group".to_string(),
                    ));
                }
            }
        }
        for namespace in &self.namespaces {
            validate_namespace(namespace)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisibilityKind {
    Public,
    Package,
    Module,
    Scope,
    Private,
    AdapterDefined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visibility {
    pub kind: VisibilityKind,
    pub boundary: Option<ScopeFactKey>,
    pub adapter_rule: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationModifier {
    Hoisted,
    Recursive,
    Forward,
    Static,
    Async,
    Generator,
    AdapterDefined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Module,
    Namespace,
    Type,
    Trait,
    Interface,
    Function,
    Method,
    Constructor,
    Field,
    Property,
    Variable,
    Parameter,
    Constant,
    Macro,
    Label,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingForm {
    Declaration,
    Parameter,
    Destructure,
    Pattern,
    Import,
    Alias,
    Receiver,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingTiming {
    AtDeclaration,
    BeforeInitializer,
    AfterInitializer,
    ScopeEntry,
    Hoisted,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mutability {
    Mutable,
    Immutable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceRole {
    Read,
    Write,
    Call,
    TypeUse,
    ModuleUse,
    MacroUse,
    LabelUse,
    MemberUse,
    AdapterDefined { schema: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportForm {
    Module,
    Selective,
    Glob,
    SideEffect,
    AdapterDefined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingTarget {
    Declaration(ScopeFactKey),
    Definition(ScopeFactKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeFactKind {
    Scope,
    Declaration,
    Definition,
    Binding,
    Reference,
    Import,
    Export,
    BuildModule,
    DynamicBoundary,
    Shadowing,
}

impl ScopeFactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scope => "scope",
            Self::Declaration => "declaration",
            Self::Definition => "definition",
            Self::Binding => "binding",
            Self::Reference => "reference",
            Self::Import => "import",
            Self::Export => "export",
            Self::BuildModule => "build-module",
            Self::DynamicBoundary => "dynamic-boundary",
            Self::Shadowing => "shadowing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    content = "content",
    rename_all = "kebab-case"
)]
pub enum ScopeFactData {
    Scope {
        scope_kind: ScopeKind,
        parent: Option<ScopeFactKey>,
        namespace_policy: NamespacePolicy,
    },
    Declaration {
        original_name: String,
        lookup_key: String,
        namespace: NameNamespace,
        scope: ScopeFactKey,
        visibility: Visibility,
        modifiers: Vec<DeclarationModifier>,
    },
    Definition {
        declaration: ScopeFactKey,
        symbol_kind: SymbolKind,
        body_scope: Option<ScopeFactKey>,
        type_scope: Option<ScopeFactKey>,
    },
    Binding {
        target: BindingTarget,
        form: BindingForm,
        timing: BindingTiming,
        mutability: Mutability,
    },
    Reference {
        original_spelling: String,
        segments: Vec<String>,
        namespace: NameNamespace,
        scope: ScopeFactKey,
        role: ReferenceRole,
    },
    Import {
        scope: ScopeFactKey,
        module_segments: Vec<String>,
        form: ImportForm,
        alias: Option<String>,
        selected_names: Vec<String>,
        conditions: Vec<String>,
    },
    Export {
        scope: ScopeFactKey,
        local_target: Option<ScopeFactKey>,
        local_name: Option<String>,
        exported_name: String,
        reexport_segments: Vec<String>,
        visibility: Visibility,
        conditions: Vec<String>,
    },
    BuildModule {
        package_id: String,
        target_id: String,
        source_root: String,
        module_path: Vec<String>,
        file_scopes: Vec<ScopeFactKey>,
    },
    DynamicBoundary {
        construct_kind: String,
        scopes: Vec<ScopeFactKey>,
        namespaces: Vec<NameNamespace>,
        reason: String,
    },
    Shadowing {
        shadowing_declaration: ScopeFactKey,
        shadowed_declaration: ScopeFactKey,
        namespace: NameNamespace,
        adapter_rule: String,
    },
}

impl ScopeFactData {
    pub fn kind(&self) -> ScopeFactKind {
        match self {
            Self::Scope { .. } => ScopeFactKind::Scope,
            Self::Declaration { .. } => ScopeFactKind::Declaration,
            Self::Definition { .. } => ScopeFactKind::Definition,
            Self::Binding { .. } => ScopeFactKind::Binding,
            Self::Reference { .. } => ScopeFactKind::Reference,
            Self::Import { .. } => ScopeFactKind::Import,
            Self::Export { .. } => ScopeFactKind::Export,
            Self::BuildModule { .. } => ScopeFactKind::BuildModule,
            Self::DynamicBoundary { .. } => ScopeFactKind::DynamicBoundary,
            Self::Shadowing { .. } => ScopeFactKind::Shadowing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeFactEvidence {
    pub node_key: NodeKey,
    pub raw_kind: String,
    pub raw_kind_id: u16,
    pub raw_grammar_kind: String,
    pub raw_grammar_kind_id: u16,
    pub field: Option<String>,
    pub canonical_roles: CanonicalRoleSet,
    pub grammar: GrammarSelection,
    pub adapter: LanguageAdapterIdentity,
    pub capability: AdapterCapability,
    pub capability_support: CapabilitySupport,
    pub authority: Option<CapabilityAuthority>,
    pub coverage: FactCoverageEvidence,
    pub recovered: bool,
    pub source_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeFactWire {
    key: ScopeFactKey,
    evidence: ScopeFactEvidence,
    data: ScopeFactData,
}

impl ScopeFactWire {
    pub fn key(&self) -> &ScopeFactKey {
        &self.key
    }

    pub fn evidence(&self) -> &ScopeFactEvidence {
        &self.evidence
    }

    pub fn data(&self) -> &ScopeFactData {
        &self.data
    }
}

#[derive(Debug, Clone)]
pub struct ScopeFactRecord {
    id: ScopeFactId,
    node: NodeId,
    wire: ScopeFactWire,
}

impl ScopeFactRecord {
    pub fn id(&self) -> ScopeFactId {
        self.id
    }

    pub fn key(&self) -> &ScopeFactKey {
        &self.wire.key
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn evidence(&self) -> &ScopeFactEvidence {
        &self.wire.evidence
    }

    pub fn data(&self) -> &ScopeFactData {
        &self.wire.data
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeGraphDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    build_context: BuildContextId,
    fact_policy: ScopeFactPolicyId,
    facts: Vec<ScopeFactWire>,
}

impl ScopeGraphDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn build_context(&self) -> &BuildContextId {
        &self.build_context
    }

    pub fn fact_policy(&self) -> &ScopeFactPolicyId {
        &self.fact_policy
    }

    pub fn fact_count(&self) -> usize {
        self.facts.len()
    }

    pub fn facts(&self) -> &[ScopeFactWire] {
        &self.facts
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeGraphDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    build_context: BuildContextId,
    fact_policy: ScopeFactPolicyId,
    facts: Vec<ScopeFactWire>,
}

impl<'de> Deserialize<'de> for ScopeGraphDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ScopeGraphDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            build_context: wire.build_context,
            fact_policy: wire.fact_policy,
            facts: wire.facts,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct ScopeGraphProjection {
    id: ProjectionId,
    analysis: Arc<ProjectAnalysis>,
    build_context: BuildContextId,
    fact_policy: ScopeFactPolicyId,
    facts: Box<[ScopeFactRecord]>,
    document: ScopeGraphDocument,
}

impl ScopeGraphProjection {
    pub fn schema(&self) -> &'static str {
        SCOPE_GRAPH_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn analysis(&self) -> &Arc<ProjectAnalysis> {
        &self.analysis
    }

    pub fn build_context(&self) -> &BuildContextId {
        &self.build_context
    }

    pub fn fact_policy(&self) -> &ScopeFactPolicyId {
        &self.fact_policy
    }

    pub fn facts(&self) -> &[ScopeFactRecord] {
        &self.facts
    }

    pub fn document(&self) -> &ScopeGraphDocument {
        &self.document
    }

    pub fn fact(&self, id: ScopeFactId) -> Result<&ScopeFactRecord, ScopeGraphBuildError> {
        lookup_fact(&self.facts, id)
    }
}

#[derive(Debug, Clone)]
pub struct ScopeDraft {
    pub kind: ScopeKind,
    pub parent: Option<ScopeFactId>,
    pub namespace_policy: NamespacePolicy,
}

#[derive(Debug, Clone)]
pub struct DeclarationDraft {
    pub original_name: String,
    pub lookup_key: String,
    pub namespace: NameNamespace,
    pub scope: ScopeFactId,
    pub visibility: VisibilityDraft,
    pub modifiers: Vec<DeclarationModifier>,
}

#[derive(Debug, Clone)]
pub struct DefinitionDraft {
    pub declaration: ScopeFactId,
    pub symbol_kind: SymbolKind,
    pub body_scope: Option<ScopeFactId>,
    pub type_scope: Option<ScopeFactId>,
}

#[derive(Debug, Clone)]
pub enum BindingTargetDraft {
    Declaration(ScopeFactId),
    Definition(ScopeFactId),
}

#[derive(Debug, Clone)]
pub struct BindingDraft {
    pub target: BindingTargetDraft,
    pub form: BindingForm,
    pub timing: BindingTiming,
    pub mutability: Mutability,
}

#[derive(Debug, Clone)]
pub struct ReferenceDraft {
    pub original_spelling: String,
    pub segments: Vec<String>,
    pub namespace: NameNamespace,
    pub scope: ScopeFactId,
    pub role: ReferenceRole,
}

#[derive(Debug, Clone)]
pub struct ImportDraft {
    pub scope: ScopeFactId,
    pub module_segments: Vec<String>,
    pub form: ImportForm,
    pub alias: Option<String>,
    pub selected_names: Vec<String>,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExportDraft {
    pub scope: ScopeFactId,
    pub local_target: Option<ScopeFactId>,
    pub local_name: Option<String>,
    pub exported_name: String,
    pub reexport_segments: Vec<String>,
    pub visibility: VisibilityDraft,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BuildModuleDraft {
    pub package_id: String,
    pub target_id: String,
    pub source_root: String,
    pub module_path: Vec<String>,
    pub file_scopes: Vec<ScopeFactId>,
}

#[derive(Debug, Clone)]
pub struct DynamicBoundaryDraft {
    pub construct_kind: String,
    pub scopes: Vec<ScopeFactId>,
    pub namespaces: Vec<NameNamespace>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ShadowingDraft {
    pub shadowing_declaration: ScopeFactId,
    pub shadowed_declaration: ScopeFactId,
    pub namespace: NameNamespace,
    pub adapter_rule: String,
}

#[derive(Debug, Clone)]
pub struct VisibilityDraft {
    pub kind: VisibilityKind,
    pub boundary: Option<ScopeFactId>,
    pub adapter_rule: Option<String>,
}

#[derive(Debug)]
pub struct ScopeGraphBuilder {
    analysis: Arc<ProjectAnalysis>,
    build_context: BuildContextId,
    fact_policy: ScopeFactPolicyId,
    owner: u64,
    facts: Vec<ScopeFactRecord>,
    canonical_roles: BTreeMap<PathBuf, BTreeMap<NodeId, CanonicalRoleSet>>,
}

impl ScopeGraphBuilder {
    pub fn new(
        analysis: Arc<ProjectAnalysis>,
        build_context: BuildContextId,
        fact_policy: ScopeFactPolicyId,
    ) -> Result<Self, ScopeGraphBuildError> {
        let owner = NEXT_SCOPE_GRAPH_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                ScopeGraphBuildError::Invalid("scope fact owner space exhausted".into())
            })?;
        Ok(Self {
            analysis,
            build_context,
            fact_policy,
            owner,
            facts: Vec::new(),
            canonical_roles: BTreeMap::new(),
        })
    }

    pub fn add_scope(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: ScopeDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        draft.namespace_policy.validate()?;
        validate_scope_kind(&draft.kind)?;
        let parent = draft
            .parent
            .map(|id| self.require_key(id, ScopeFactKind::Scope))
            .transpose()?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::LexicalScopes,
            ScopeFactData::Scope {
                scope_kind: draft.kind,
                parent,
                namespace_policy: draft.namespace_policy,
            },
        )
    }

    pub fn add_declaration(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: DeclarationDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_nonempty("declaration original name", &draft.original_name)?;
        validate_nonempty("declaration lookup key", &draft.lookup_key)?;
        validate_namespace(&draft.namespace)?;
        let scope = self.require_key(draft.scope, ScopeFactKind::Scope)?;
        let visibility = self.visibility(draft.visibility)?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::LexicalScopes,
            ScopeFactData::Declaration {
                original_name: draft.original_name,
                lookup_key: draft.lookup_key,
                namespace: draft.namespace,
                scope,
                visibility,
                modifiers: draft.modifiers,
            },
        )
    }

    pub fn add_definition(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: DefinitionDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_symbol_kind(&draft.symbol_kind)?;
        let declaration = self.require_key(draft.declaration, ScopeFactKind::Declaration)?;
        let body_scope = draft
            .body_scope
            .map(|id| self.require_key(id, ScopeFactKind::Scope))
            .transpose()?;
        let type_scope = draft
            .type_scope
            .map(|id| self.require_key(id, ScopeFactKind::Scope))
            .transpose()?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::LexicalScopes,
            ScopeFactData::Definition {
                declaration,
                symbol_kind: draft.symbol_kind,
                body_scope,
                type_scope,
            },
        )
    }

    pub fn add_binding(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: BindingDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_binding_form(&draft.form)?;
        validate_binding_timing(&draft.timing)?;
        let target = match draft.target {
            BindingTargetDraft::Declaration(id) => {
                BindingTarget::Declaration(self.require_key(id, ScopeFactKind::Declaration)?)
            }
            BindingTargetDraft::Definition(id) => {
                BindingTarget::Definition(self.require_key(id, ScopeFactKind::Definition)?)
            }
        };
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::LexicalScopes,
            ScopeFactData::Binding {
                target,
                form: draft.form,
                timing: draft.timing,
                mutability: draft.mutability,
            },
        )
    }

    pub fn add_reference(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: ReferenceDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_nonempty("reference spelling", &draft.original_spelling)?;
        validate_segments("reference", &draft.segments)?;
        validate_namespace(&draft.namespace)?;
        validate_reference_role(&draft.role)?;
        let scope = self.require_key(draft.scope, ScopeFactKind::Scope)?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::NameResolution,
            ScopeFactData::Reference {
                original_spelling: draft.original_spelling,
                segments: draft.segments,
                namespace: draft.namespace,
                scope,
                role: draft.role,
            },
        )
    }

    pub fn add_import(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: ImportDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_segments("import module", &draft.module_segments)?;
        validate_optional_nonempty("import alias", draft.alias.as_deref())?;
        validate_strings("selected import", &draft.selected_names)?;
        validate_strings("import condition", &draft.conditions)?;
        if draft.form == ImportForm::Selective && draft.selected_names.is_empty() {
            return Err(ScopeGraphBuildError::Invalid(
                "selective import must retain at least one selected name".into(),
            ));
        }
        let scope = self.require_key(draft.scope, ScopeFactKind::Scope)?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::ImportsExports,
            ScopeFactData::Import {
                scope,
                module_segments: draft.module_segments,
                form: draft.form,
                alias: draft.alias,
                selected_names: draft.selected_names,
                conditions: draft.conditions,
            },
        )
    }

    pub fn add_export(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: ExportDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_nonempty("exported name", &draft.exported_name)?;
        validate_optional_nonempty("export local name", draft.local_name.as_deref())?;
        validate_strings("re-export", &draft.reexport_segments)?;
        validate_strings("export condition", &draft.conditions)?;
        let scope = self.require_key(draft.scope, ScopeFactKind::Scope)?;
        let local_target = draft
            .local_target
            .map(|id| {
                self.require_any_key(id, &[ScopeFactKind::Declaration, ScopeFactKind::Definition])
            })
            .transpose()?;
        if local_target.is_none()
            && draft.local_name.is_none()
            && draft.reexport_segments.is_empty()
        {
            return Err(ScopeGraphBuildError::Invalid(
                "export must retain a local target/name or a re-export path".into(),
            ));
        }
        let visibility = self.visibility(draft.visibility)?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::ImportsExports,
            ScopeFactData::Export {
                scope,
                local_target,
                local_name: draft.local_name,
                exported_name: draft.exported_name,
                reexport_segments: draft.reexport_segments,
                visibility,
                conditions: draft.conditions,
            },
        )
    }

    pub fn add_build_module(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: BuildModuleDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_nonempty("module package identity", &draft.package_id)?;
        validate_nonempty("module target identity", &draft.target_id)?;
        validate_nonempty("module source root", &draft.source_root)?;
        validate_segments("module path", &draft.module_path)?;
        if draft.file_scopes.is_empty() {
            return Err(ScopeGraphBuildError::Invalid(
                "build module must contain at least one file scope".into(),
            ));
        }
        let file_scopes = draft
            .file_scopes
            .into_iter()
            .map(|id| self.require_key(id, ScopeFactKind::Scope))
            .collect::<Result<Vec<_>, _>>()?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::DependencyGraph,
            ScopeFactData::BuildModule {
                package_id: draft.package_id,
                target_id: draft.target_id,
                source_root: draft.source_root,
                module_path: draft.module_path,
                file_scopes,
            },
        )
    }

    pub fn add_dynamic_boundary(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: DynamicBoundaryDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_nonempty("dynamic construct kind", &draft.construct_kind)?;
        validate_nonempty("dynamic boundary reason", &draft.reason)?;
        if draft.scopes.is_empty() || draft.namespaces.is_empty() {
            return Err(ScopeGraphBuildError::Invalid(
                "dynamic boundary must identify affected scopes and namespaces".into(),
            ));
        }
        for namespace in &draft.namespaces {
            validate_namespace(namespace)?;
        }
        let scopes = draft
            .scopes
            .into_iter()
            .map(|id| self.require_key(id, ScopeFactKind::Scope))
            .collect::<Result<Vec<_>, _>>()?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::NameResolution,
            ScopeFactData::DynamicBoundary {
                construct_kind: draft.construct_kind,
                scopes,
                namespaces: draft.namespaces,
                reason: draft.reason,
            },
        )
    }

    pub fn add_shadowing(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        draft: ShadowingDraft,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        validate_namespace(&draft.namespace)?;
        validate_nonempty("shadowing adapter rule", &draft.adapter_rule)?;
        if draft.shadowing_declaration == draft.shadowed_declaration {
            return Err(ScopeGraphBuildError::Invalid(
                "a declaration cannot shadow itself".into(),
            ));
        }
        let shadowing_declaration =
            self.require_key(draft.shadowing_declaration, ScopeFactKind::Declaration)?;
        let shadowed_declaration =
            self.require_key(draft.shadowed_declaration, ScopeFactKind::Declaration)?;
        self.push(
            node,
            canonical_roles,
            coverage,
            AdapterCapability::NameResolution,
            ScopeFactData::Shadowing {
                shadowing_declaration,
                shadowed_declaration,
                namespace: draft.namespace,
                adapter_rule: draft.adapter_rule,
            },
        )
    }

    pub fn build(self) -> Result<ScopeGraphProjection, ScopeGraphBuildError> {
        if self.facts.is_empty() {
            return Err(ScopeGraphBuildError::Invalid(
                "scope graph projection cannot be empty".into(),
            ));
        }
        let facts = self
            .facts
            .iter()
            .map(|fact| fact.wire.clone())
            .collect::<Vec<_>>();
        let payload = ScopeGraphProjectionPayload {
            schema: SCOPE_GRAPH_SCHEMA,
            analysis_id: self.analysis.id().as_str(),
            build_context: &self.build_context,
            fact_policy: &self.fact_policy,
            facts: &facts,
        };
        let policy = serde_json::to_vec(&payload)
            .map_err(|error| ScopeGraphBuildError::Identity(error.to_string()))?;
        let capabilities = declared_capability_bytes(&facts);
        let id = self
            .analysis
            .derive_projection_id(SCOPE_GRAPH_SCHEMA, &policy, &capabilities)
            .map_err(|error| ScopeGraphBuildError::Identity(error.to_string()))?;
        let document = ScopeGraphDocument {
            schema: SCOPE_GRAPH_SCHEMA.to_string(),
            projection_id: id.clone(),
            analysis_id: self.analysis.id().as_str().to_string(),
            build_context: self.build_context.clone(),
            fact_policy: self.fact_policy.clone(),
            facts,
        };
        document.validate()?;
        Ok(ScopeGraphProjection {
            id,
            analysis: self.analysis,
            build_context: self.build_context,
            fact_policy: self.fact_policy,
            facts: self.facts.into_boxed_slice(),
            document,
        })
    }

    fn push(
        &mut self,
        node: NodeId,
        canonical_roles: CanonicalRoleSet,
        coverage: FactCoverageEvidence,
        capability: AdapterCapability,
        data: ScopeFactData,
    ) -> Result<ScopeFactId, ScopeGraphBuildError> {
        let path = self
            .analysis
            .node(node)
            .map_err(|error| ScopeGraphBuildError::Node(error.to_string()))?
            .path()
            .to_path_buf();
        let expected_roles = self.canonical_roles_for_node(node, &path)?;
        if canonical_roles != expected_roles {
            return Err(ScopeGraphBuildError::Invalid(
                "scope fact canonical roles disagree with the owned M2 projection".into(),
            ));
        }
        let view = self
            .analysis
            .node(node)
            .map_err(|error| ScopeGraphBuildError::Node(error.to_string()))?;
        let entry =
            self.analysis.snapshot().entry(view.path()).ok_or_else(|| {
                ScopeGraphBuildError::Node("fact node file is not retained".into())
            })?;
        let adapter = entry
            .language_adapter_identity()
            .ok_or_else(|| ScopeGraphBuildError::Node("fact node has no stored adapter".into()))?;
        let declaration = adapter.capabilities().declaration(capability);
        coverage.validate()?;
        validate_coverage(coverage.status, declaration.support())?;
        let index = u32::try_from(self.facts.len()).map_err(|_| {
            ScopeGraphBuildError::Invalid("scope graph exceeds the local fact ID space".into())
        })?;
        let id = ScopeFactId {
            owner: self.owner,
            index,
        };
        let evidence = ScopeFactEvidence {
            node_key: view.key().clone(),
            raw_kind: view.raw_kind().to_string(),
            raw_kind_id: view.raw_kind_id(),
            raw_grammar_kind: view.raw_grammar_kind().to_string(),
            raw_grammar_kind_id: view.raw_grammar_kind_id(),
            field: view.field().map(str::to_string),
            canonical_roles,
            grammar: view.grammar().clone(),
            adapter: adapter.clone(),
            capability,
            capability_support: declaration.support(),
            authority: declaration.authority(),
            coverage,
            recovered: view.has_error() || view.is_error() || view.is_missing(),
            source_order: u64::try_from(view.span().start_byte()).map_err(|_| {
                ScopeGraphBuildError::Invalid("source byte order exceeds u64".into())
            })?,
        };
        let key = derive_fact_key(
            self.analysis.id().as_str(),
            &self.build_context,
            &self.fact_policy,
            index,
            &evidence,
            &data,
        )?;
        self.facts.push(ScopeFactRecord {
            id,
            node,
            wire: ScopeFactWire {
                key,
                evidence,
                data,
            },
        });
        Ok(id)
    }

    fn canonical_roles_for_node(
        &mut self,
        node: NodeId,
        path: &Path,
    ) -> Result<CanonicalRoleSet, ScopeGraphBuildError> {
        if !self.canonical_roles.contains_key(path) {
            let projection = self
                .analysis
                .canonical_role_projection(path)
                .map_err(|error| ScopeGraphBuildError::Node(error.to_string()))?;
            self.canonical_roles.insert(
                path.to_path_buf(),
                projection
                    .facts()
                    .iter()
                    .map(|fact| (fact.node(), fact.roles()))
                    .collect(),
            );
        }
        self.canonical_roles
            .get(path)
            .and_then(|facts| facts.get(&node))
            .copied()
            .ok_or_else(|| {
                ScopeGraphBuildError::Node(
                    "scope fact node is absent from the owned canonical-role projection".into(),
                )
            })
    }

    fn visibility(&self, draft: VisibilityDraft) -> Result<Visibility, ScopeGraphBuildError> {
        if draft.kind == VisibilityKind::AdapterDefined {
            validate_optional_nonempty("visibility adapter rule", draft.adapter_rule.as_deref())?;
            if draft.adapter_rule.is_none() {
                return Err(ScopeGraphBuildError::Invalid(
                    "adapter-defined visibility requires an adapter rule".into(),
                ));
            }
        }
        let boundary = draft
            .boundary
            .map(|id| self.require_key(id, ScopeFactKind::Scope))
            .transpose()?;
        Ok(Visibility {
            kind: draft.kind,
            boundary,
            adapter_rule: draft.adapter_rule,
        })
    }

    fn require_key(
        &self,
        id: ScopeFactId,
        expected: ScopeFactKind,
    ) -> Result<ScopeFactKey, ScopeGraphBuildError> {
        self.require_any_key(id, &[expected])
    }

    fn require_any_key(
        &self,
        id: ScopeFactId,
        expected: &[ScopeFactKind],
    ) -> Result<ScopeFactKey, ScopeGraphBuildError> {
        if id.owner != self.owner {
            return Err(ScopeGraphBuildError::ForeignFact);
        }
        let fact = lookup_fact(&self.facts, id)?;
        let actual = fact.data().kind();
        if !expected.contains(&actual) {
            return Err(ScopeGraphBuildError::WrongFactKind {
                expected: expected.to_vec(),
                actual,
            });
        }
        Ok(fact.key().clone())
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ScopeGraphProjectionPayload<'a> {
    schema: &'static str,
    analysis_id: &'a str,
    build_context: &'a BuildContextId,
    fact_policy: &'a ScopeFactPolicyId,
    facts: &'a [ScopeFactWire],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeGraphBuildError {
    Invalid(String),
    Node(String),
    ForeignFact,
    FactOutOfRange {
        requested: u32,
        fact_count: u32,
    },
    WrongFactKind {
        expected: Vec<ScopeFactKind>,
        actual: ScopeFactKind,
    },
    Identity(String),
}

impl fmt::Display for ScopeGraphBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid scope graph: {detail}"),
            Self::Node(detail) => write!(formatter, "invalid scope fact node: {detail}"),
            Self::ForeignFact => {
                formatter.write_str("scope fact belongs to a different projection")
            }
            Self::FactOutOfRange {
                requested,
                fact_count,
            } => write!(
                formatter,
                "scope fact {requested} is outside fact count {fact_count}"
            ),
            Self::WrongFactKind { expected, actual } => {
                write!(formatter, "expected one of {expected:?}, found {actual:?}")
            }
            Self::Identity(detail) => write!(formatter, "scope graph identity failed: {detail}"),
        }
    }
}

impl std::error::Error for ScopeGraphBuildError {}

fn lookup_fact(
    facts: &[ScopeFactRecord],
    id: ScopeFactId,
) -> Result<&ScopeFactRecord, ScopeGraphBuildError> {
    let owner = facts.first().map(|fact| fact.id.owner).unwrap_or(id.owner);
    if id.owner != owner {
        return Err(ScopeGraphBuildError::ForeignFact);
    }
    facts
        .get(id.index as usize)
        .ok_or_else(|| ScopeGraphBuildError::FactOutOfRange {
            requested: id.index,
            fact_count: u32::try_from(facts.len()).unwrap_or(u32::MAX),
        })
}

impl ScopeGraphDocument {
    fn validate(&self) -> Result<(), ScopeGraphBuildError> {
        if self.schema != SCOPE_GRAPH_SCHEMA {
            return Err(ScopeGraphBuildError::Invalid(format!(
                "unsupported scope graph schema {}",
                self.schema
            )));
        }
        validate_digest_id(self.projection_id.as_str(), "pj1_")?;
        validate_digest_id(&self.analysis_id, "pa1_")?;
        if self.facts.is_empty() {
            return Err(ScopeGraphBuildError::Invalid(
                "scope graph document cannot be empty".into(),
            ));
        }
        let mut kinds = BTreeMap::new();
        for (index, fact) in self.facts.iter().enumerate() {
            if kinds.insert(fact.key.clone(), fact.data.kind()).is_some() {
                return Err(ScopeGraphBuildError::Invalid(
                    "scope graph contains duplicate fact keys".into(),
                ));
            }
            validate_wire_fact(fact)?;
            let expected = derive_fact_key(
                &self.analysis_id,
                &self.build_context,
                &self.fact_policy,
                u32::try_from(index).map_err(|_| {
                    ScopeGraphBuildError::Invalid(
                        "scope graph exceeds the local fact ID space".into(),
                    )
                })?,
                &fact.evidence,
                &fact.data,
            )?;
            if fact.key != expected {
                return Err(ScopeGraphBuildError::Invalid(
                    "scope fact key does not bind its complete payload".into(),
                ));
            }
        }
        for fact in &self.facts {
            validate_wire_links(&fact.data, &kinds)?;
        }
        let facts = self
            .facts
            .iter()
            .map(|fact| (fact.key.clone(), fact))
            .collect::<BTreeMap<_, _>>();
        for fact in &self.facts {
            validate_cross_fact(fact, &facts)?;
        }
        validate_scope_parent_graph(&facts)?;
        Ok(())
    }
}

fn validate_wire_fact(fact: &ScopeFactWire) -> Result<(), ScopeGraphBuildError> {
    validate_digest_id(fact.key.as_str(), "sf1_")?;
    if !fact.evidence.node_key.is_supported() {
        return Err(ScopeGraphBuildError::Invalid(
            "scope fact has an unsupported node key".into(),
        ));
    }
    validate_nonempty("raw syntax kind", &fact.evidence.raw_kind)?;
    validate_nonempty("raw grammar kind", &fact.evidence.raw_grammar_kind)?;
    if fact.evidence.raw_grammar_kind != fact.evidence.node_key.raw_grammar_kind()
        || fact.evidence.raw_grammar_kind_id != fact.evidence.node_key.raw_grammar_kind_id()
    {
        return Err(ScopeGraphBuildError::Invalid(
            "scope fact raw grammar evidence disagrees with its node key".into(),
        ));
    }
    if fact.evidence.grammar != fact.evidence.node_key.file().grammar {
        return Err(ScopeGraphBuildError::Invalid(
            "scope fact grammar disagrees with its node key".into(),
        ));
    }
    let declaration = fact
        .evidence
        .adapter
        .capabilities()
        .declaration(fact.evidence.capability);
    if declaration.support() != fact.evidence.capability_support
        || declaration.authority() != fact.evidence.authority
    {
        return Err(ScopeGraphBuildError::Invalid(
            "scope fact capability evidence disagrees with its adapter manifest".into(),
        ));
    }
    if fact.evidence.adapter.schema() != fact.evidence.adapter.capabilities().adapter_schema() {
        return Err(ScopeGraphBuildError::Invalid(
            "scope fact adapter and capability schemas disagree".into(),
        ));
    }
    if fact.evidence.source_order != fact.evidence.node_key.anchor().start_byte() {
        return Err(ScopeGraphBuildError::Invalid(
            "scope fact source order disagrees with its node anchor".into(),
        ));
    }
    fact.evidence.coverage.validate()?;
    validate_coverage(
        fact.evidence.coverage.status,
        fact.evidence.capability_support,
    )?;
    match &fact.data {
        ScopeFactData::Scope {
            scope_kind,
            namespace_policy,
            ..
        } => {
            validate_scope_kind(scope_kind)?;
            namespace_policy.validate()?;
        }
        ScopeFactData::Declaration {
            original_name,
            lookup_key,
            namespace,
            visibility,
            ..
        } => {
            validate_nonempty("declaration original name", original_name)?;
            validate_nonempty("declaration lookup key", lookup_key)?;
            validate_namespace(namespace)?;
            validate_visibility(visibility)?;
        }
        ScopeFactData::Definition { symbol_kind, .. } => validate_symbol_kind(symbol_kind)?,
        ScopeFactData::Binding { form, timing, .. } => {
            validate_binding_form(form)?;
            validate_binding_timing(timing)?;
        }
        ScopeFactData::Reference {
            original_spelling,
            segments,
            namespace,
            role,
            ..
        } => {
            validate_nonempty("reference spelling", original_spelling)?;
            validate_segments("reference", segments)?;
            validate_namespace(namespace)?;
            validate_reference_role(role)?;
        }
        ScopeFactData::Import {
            module_segments,
            alias,
            selected_names,
            conditions,
            form,
            ..
        } => {
            validate_segments("import module", module_segments)?;
            validate_optional_nonempty("import alias", alias.as_deref())?;
            validate_strings("selected import", selected_names)?;
            validate_strings("import condition", conditions)?;
            if *form == ImportForm::Selective && selected_names.is_empty() {
                return Err(ScopeGraphBuildError::Invalid(
                    "selective import must retain at least one selected name".into(),
                ));
            }
        }
        ScopeFactData::Export {
            local_target,
            local_name,
            exported_name,
            reexport_segments,
            visibility,
            conditions,
            ..
        } => {
            validate_nonempty("exported name", exported_name)?;
            validate_optional_nonempty("export local name", local_name.as_deref())?;
            validate_strings("re-export", reexport_segments)?;
            validate_visibility(visibility)?;
            validate_strings("export condition", conditions)?;
            if local_target.is_none() && local_name.is_none() && reexport_segments.is_empty() {
                return Err(ScopeGraphBuildError::Invalid(
                    "export must retain a local target/name or a re-export path".into(),
                ));
            }
        }
        ScopeFactData::BuildModule {
            package_id,
            target_id,
            source_root,
            module_path,
            file_scopes,
        } => {
            validate_nonempty("module package identity", package_id)?;
            validate_nonempty("module target identity", target_id)?;
            validate_nonempty("module source root", source_root)?;
            validate_segments("module path", module_path)?;
            if file_scopes.is_empty() {
                return Err(ScopeGraphBuildError::Invalid(
                    "build module must contain a file scope".into(),
                ));
            }
        }
        ScopeFactData::DynamicBoundary {
            construct_kind,
            scopes,
            namespaces,
            reason,
        } => {
            validate_nonempty("dynamic construct kind", construct_kind)?;
            validate_nonempty("dynamic boundary reason", reason)?;
            if scopes.is_empty() || namespaces.is_empty() {
                return Err(ScopeGraphBuildError::Invalid(
                    "dynamic boundary must identify affected scopes and namespaces".into(),
                ));
            }
            for namespace in namespaces {
                validate_namespace(namespace)?;
            }
        }
        ScopeFactData::Shadowing {
            shadowing_declaration,
            shadowed_declaration,
            namespace,
            adapter_rule,
        } => {
            if shadowing_declaration == shadowed_declaration {
                return Err(ScopeGraphBuildError::Invalid(
                    "a declaration cannot shadow itself".into(),
                ));
            }
            validate_namespace(namespace)?;
            validate_nonempty("shadowing adapter rule", adapter_rule)?;
        }
    }
    Ok(())
}

fn validate_wire_links(
    data: &ScopeFactData,
    kinds: &BTreeMap<ScopeFactKey, ScopeFactKind>,
) -> Result<(), ScopeGraphBuildError> {
    let require = |key: &ScopeFactKey, expected: &[ScopeFactKind]| {
        let actual = kinds.get(key).ok_or_else(|| {
            ScopeGraphBuildError::Invalid(format!("dangling scope fact link {}", key.as_str()))
        })?;
        if !expected.contains(actual) {
            return Err(ScopeGraphBuildError::WrongFactKind {
                expected: expected.to_vec(),
                actual: *actual,
            });
        }
        Ok(())
    };
    match data {
        ScopeFactData::Scope { parent, .. } => {
            if let Some(parent) = parent {
                require(parent, &[ScopeFactKind::Scope])?;
            }
        }
        ScopeFactData::Declaration {
            scope, visibility, ..
        } => {
            require(scope, &[ScopeFactKind::Scope])?;
            validate_visibility_link(visibility, &require)?;
        }
        ScopeFactData::Definition {
            declaration,
            body_scope,
            type_scope,
            ..
        } => {
            require(declaration, &[ScopeFactKind::Declaration])?;
            for scope in [body_scope, type_scope].into_iter().flatten() {
                require(scope, &[ScopeFactKind::Scope])?;
            }
        }
        ScopeFactData::Binding { target, .. } => match target {
            BindingTarget::Declaration(key) => require(key, &[ScopeFactKind::Declaration])?,
            BindingTarget::Definition(key) => require(key, &[ScopeFactKind::Definition])?,
        },
        ScopeFactData::Reference { scope, .. } | ScopeFactData::Import { scope, .. } => {
            require(scope, &[ScopeFactKind::Scope])?;
        }
        ScopeFactData::Export {
            scope,
            local_target,
            visibility,
            ..
        } => {
            require(scope, &[ScopeFactKind::Scope])?;
            if let Some(target) = local_target {
                require(
                    target,
                    &[ScopeFactKind::Declaration, ScopeFactKind::Definition],
                )?;
            }
            validate_visibility_link(visibility, &require)?;
        }
        ScopeFactData::BuildModule { file_scopes, .. }
        | ScopeFactData::DynamicBoundary {
            scopes: file_scopes,
            ..
        } => {
            for scope in file_scopes {
                require(scope, &[ScopeFactKind::Scope])?;
            }
        }
        ScopeFactData::Shadowing {
            shadowing_declaration,
            shadowed_declaration,
            ..
        } => {
            require(shadowing_declaration, &[ScopeFactKind::Declaration])?;
            require(shadowed_declaration, &[ScopeFactKind::Declaration])?;
        }
    }
    Ok(())
}

fn validate_cross_fact(
    fact: &ScopeFactWire,
    facts: &BTreeMap<ScopeFactKey, &ScopeFactWire>,
) -> Result<(), ScopeGraphBuildError> {
    let scope_policy = |key: &ScopeFactKey| -> Result<&NamespacePolicy, ScopeGraphBuildError> {
        match &facts
            .get(key)
            .expect("wire link validation already proved presence")
            .data
        {
            ScopeFactData::Scope {
                namespace_policy, ..
            } => Ok(namespace_policy),
            _ => unreachable!("wire link validation already proved fact kind"),
        }
    };
    let declaration_namespace =
        |key: &ScopeFactKey| -> Result<&NameNamespace, ScopeGraphBuildError> {
            match &facts
                .get(key)
                .expect("wire link validation already proved presence")
                .data
            {
                ScopeFactData::Declaration { namespace, .. } => Ok(namespace),
                _ => unreachable!("wire link validation already proved fact kind"),
            }
        };
    match &fact.data {
        ScopeFactData::Declaration {
            namespace, scope, ..
        }
        | ScopeFactData::Reference {
            namespace, scope, ..
        } => {
            if !scope_policy(scope)?.namespaces.contains(namespace) {
                return Err(ScopeGraphBuildError::Invalid(
                    "fact namespace is not declared by its owning scope".into(),
                ));
            }
        }
        ScopeFactData::BuildModule { file_scopes, .. } => {
            for scope in file_scopes {
                let linked = facts
                    .get(scope)
                    .expect("wire link validation already proved presence");
                if !matches!(
                    linked.data,
                    ScopeFactData::Scope {
                        scope_kind: ScopeKind::File,
                        ..
                    }
                ) {
                    return Err(ScopeGraphBuildError::Invalid(
                        "build module constituents must be file scopes".into(),
                    ));
                }
            }
        }
        ScopeFactData::Shadowing {
            shadowing_declaration,
            shadowed_declaration,
            namespace,
            ..
        } => {
            if declaration_namespace(shadowing_declaration)? != namespace
                || declaration_namespace(shadowed_declaration)? != namespace
            {
                return Err(ScopeGraphBuildError::Invalid(
                    "shadowing namespace disagrees with a linked declaration".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_scope_parent_graph(
    facts: &BTreeMap<ScopeFactKey, &ScopeFactWire>,
) -> Result<(), ScopeGraphBuildError> {
    for (start, fact) in facts {
        if fact.data.kind() != ScopeFactKind::Scope {
            continue;
        }
        let mut current = Some(start);
        let mut seen = BTreeSet::new();
        while let Some(key) = current {
            if !seen.insert(key) {
                return Err(ScopeGraphBuildError::Invalid(
                    "scope parent graph contains a cycle".into(),
                ));
            }
            current = match &facts
                .get(key)
                .expect("wire link validation already proved presence")
                .data
            {
                ScopeFactData::Scope { parent, .. } => parent.as_ref(),
                _ => unreachable!("scope parent links target only scopes"),
            };
        }
    }
    Ok(())
}

fn validate_visibility_link(
    visibility: &Visibility,
    require: &impl Fn(&ScopeFactKey, &[ScopeFactKind]) -> Result<(), ScopeGraphBuildError>,
) -> Result<(), ScopeGraphBuildError> {
    if let Some(boundary) = &visibility.boundary {
        require(boundary, &[ScopeFactKind::Scope])?;
    }
    Ok(())
}

fn validate_visibility(visibility: &Visibility) -> Result<(), ScopeGraphBuildError> {
    if visibility.kind == VisibilityKind::AdapterDefined {
        validate_optional_nonempty(
            "visibility adapter rule",
            visibility.adapter_rule.as_deref(),
        )?;
        if visibility.adapter_rule.is_none() {
            return Err(ScopeGraphBuildError::Invalid(
                "adapter-defined visibility requires an adapter rule".into(),
            ));
        }
    }
    Ok(())
}

fn validate_coverage(
    coverage: FactCoverage,
    support: CapabilitySupport,
) -> Result<(), ScopeGraphBuildError> {
    match (coverage, support) {
        (FactCoverage::Complete, CapabilitySupport::Provided) => Ok(()),
        (FactCoverage::Complete, _) => Err(ScopeGraphBuildError::Invalid(
            "complete fact coverage requires a provided capability".into(),
        )),
        (
            FactCoverage::Unsupported,
            CapabilitySupport::Provided | CapabilitySupport::Unsupported,
        ) => Ok(()),
        (FactCoverage::Unsupported, CapabilitySupport::Unknown) => {
            Err(ScopeGraphBuildError::Invalid(
                "unsupported fact coverage requires an explicit adapter declaration".into(),
            ))
        }
        (FactCoverage::Partial, CapabilitySupport::Unsupported) => {
            Err(ScopeGraphBuildError::Invalid(
                "partial fact coverage contradicts an unsupported capability declaration".into(),
            ))
        }
        _ => Ok(()),
    }
}

fn validate_scope_kind(value: &ScopeKind) -> Result<(), ScopeGraphBuildError> {
    if let ScopeKind::AdapterDefined { schema, name } = value {
        validate_adapter_pair("scope kind", schema, name)?;
    }
    Ok(())
}

fn validate_namespace(value: &NameNamespace) -> Result<(), ScopeGraphBuildError> {
    if let NameNamespace::AdapterDefined { schema, name } = value {
        validate_adapter_pair("namespace", schema, name)?;
    }
    Ok(())
}

fn validate_symbol_kind(value: &SymbolKind) -> Result<(), ScopeGraphBuildError> {
    if let SymbolKind::AdapterDefined { schema, name } = value {
        validate_adapter_pair("symbol kind", schema, name)?;
    }
    Ok(())
}

fn validate_binding_form(value: &BindingForm) -> Result<(), ScopeGraphBuildError> {
    if let BindingForm::AdapterDefined { schema, name } = value {
        validate_adapter_pair("binding form", schema, name)?;
    }
    Ok(())
}

fn validate_binding_timing(value: &BindingTiming) -> Result<(), ScopeGraphBuildError> {
    if let BindingTiming::AdapterDefined { schema, name } = value {
        validate_adapter_pair("binding timing", schema, name)?;
    }
    Ok(())
}

fn validate_reference_role(value: &ReferenceRole) -> Result<(), ScopeGraphBuildError> {
    if let ReferenceRole::AdapterDefined { schema, name } = value {
        validate_adapter_pair("reference role", schema, name)?;
    }
    Ok(())
}

fn validate_adapter_pair(
    label: &str,
    schema: &str,
    name: &str,
) -> Result<(), ScopeGraphBuildError> {
    validate_nonempty(&format!("{label} schema"), schema)?;
    validate_nonempty(&format!("{label} name"), name)
}

fn validate_nonempty(label: &str, value: &str) -> Result<(), ScopeGraphBuildError> {
    if value.trim().is_empty() {
        return Err(ScopeGraphBuildError::Invalid(format!(
            "{label} must not be empty"
        )));
    }
    Ok(())
}

fn validate_optional_nonempty(
    label: &str,
    value: Option<&str>,
) -> Result<(), ScopeGraphBuildError> {
    if let Some(value) = value {
        validate_nonempty(label, value)?;
    }
    Ok(())
}

fn validate_strings(label: &str, values: &[String]) -> Result<(), ScopeGraphBuildError> {
    for value in values {
        validate_nonempty(label, value)?;
    }
    Ok(())
}

fn validate_segments(label: &str, values: &[String]) -> Result<(), ScopeGraphBuildError> {
    if values.is_empty() {
        return Err(ScopeGraphBuildError::Invalid(format!(
            "{label} must retain at least one segment"
        )));
    }
    validate_strings(label, values)
}

fn derive_external_id(
    schema: &str,
    prefix: &str,
    parts: &[&[u8]],
) -> Result<String, ScopeGraphBuildError> {
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(ScopeGraphBuildError::Invalid(format!(
            "{schema} identity requires non-empty parts"
        )));
    }
    let mut hasher = blake3::Hasher::new();
    hash_part(&mut hasher, schema.as_bytes());
    for part in parts {
        hash_part(&mut hasher, part);
    }
    Ok(format!("{prefix}{}", hasher.finalize().to_hex()))
}

fn derive_fact_key(
    analysis_id: &str,
    context: &BuildContextId,
    policy: &ScopeFactPolicyId,
    index: u32,
    evidence: &ScopeFactEvidence,
    data: &ScopeFactData,
) -> Result<ScopeFactKey, ScopeGraphBuildError> {
    #[derive(Serialize)]
    #[serde(deny_unknown_fields)]
    struct Payload<'a> {
        schema: &'static str,
        analysis_id: &'a str,
        build_context: &'a BuildContextId,
        fact_policy: &'a ScopeFactPolicyId,
        index: u32,
        evidence: &'a ScopeFactEvidence,
        data: &'a ScopeFactData,
    }
    let payload = serde_json::to_vec(&Payload {
        schema: SCOPE_GRAPH_SCHEMA,
        analysis_id,
        build_context: context,
        fact_policy: policy,
        index,
        evidence,
        data,
    })
    .map_err(|error| ScopeGraphBuildError::Identity(error.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    hash_part(&mut hasher, b"deslop.scope-fact-key/1");
    hash_part(&mut hasher, &payload);
    Ok(ScopeFactKey(format!("sf1_{}", hasher.finalize().to_hex())))
}

fn declared_capability_bytes(facts: &[ScopeFactWire]) -> Vec<u8> {
    let mut values = facts
        .iter()
        .map(|fact| {
            format!(
                "{}\0{}\0{}",
                fact.evidence.capability.as_str(),
                fact.evidence.capability_support.as_str(),
                fact.evidence
                    .authority
                    .map_or("", CapabilityAuthority::as_str)
            )
        })
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values.join("\n").into_bytes()
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), ScopeGraphBuildError> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return Err(ScopeGraphBuildError::Invalid(format!(
            "identity must start with {prefix}"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ScopeGraphBuildError::Invalid(format!(
            "{prefix} identity must contain 64 hexadecimal digits"
        )));
    }
    Ok(())
}

fn hash_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::Value;

    use super::*;
    use crate::{ProjectSnapshotBuilder, RepositoryId};

    const SOURCE: &str = r#"use crate::other::Thing;
fn outer(x: i32) {
    let x = x;
    println!("{x}");
}
"#;

    fn analysis() -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("scope.rs"), SOURCE).unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("scope-graph-test-repository").unwrap(),
        )
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn node_by_kind(analysis: &ProjectAnalysis, kind: &str) -> NodeId {
        analysis
            .node_ids()
            .find(|id| analysis.node(*id).unwrap().raw_kind() == kind)
            .unwrap_or_else(|| panic!("missing raw kind {kind}"))
    }

    fn nodes_by_text(analysis: &ProjectAnalysis, text: &str) -> Vec<NodeId> {
        analysis
            .node_ids()
            .filter(|id| analysis.node(*id).unwrap().text() == text)
            .collect()
    }

    fn roles(analysis: &Arc<ProjectAnalysis>, node: NodeId) -> CanonicalRoleSet {
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

    fn ids() -> (BuildContextId, ScopeFactPolicyId) {
        (
            BuildContextId::from_parts(&[b"test-target", b"features=default"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"hand-labelled-scope-facts/1"]).unwrap(),
        )
    }

    fn public_visibility() -> VisibilityDraft {
        VisibilityDraft {
            kind: VisibilityKind::Public,
            boundary: None,
            adapter_rule: None,
        }
    }

    fn partial() -> FactCoverageEvidence {
        FactCoverageEvidence::partial("M3.3 resolution rule pack is not installed").unwrap()
    }

    fn build_full(analysis: Arc<ProjectAnalysis>) -> ScopeGraphProjection {
        let (context, policy) = ids();
        let mut builder = ScopeGraphBuilder::new(Arc::clone(&analysis), context, policy).unwrap();
        let root_node = analysis.node_ids().next().unwrap();
        let function = node_by_kind(&analysis, "function_item");
        let use_node = node_by_kind(&analysis, "use_declaration");
        let macro_node = node_by_kind(&analysis, "macro_invocation");
        let outer = nodes_by_text(&analysis, "outer")[0];
        let xs = nodes_by_text(&analysis, "x");
        assert!(xs.len() >= 3);

        let namespaces = NamespacePolicy::new(
            vec![
                NameNamespace::Value,
                NameNamespace::Type,
                NameNamespace::Module,
                NameNamespace::Macro,
            ],
            vec![],
        )
        .unwrap();
        let root_scope = builder
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
        let function_scope = builder
            .add_scope(
                function,
                roles(&analysis, function),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(root_scope),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();
        let function_declaration = builder
            .add_declaration(
                outer,
                roles(&analysis, outer),
                partial(),
                DeclarationDraft {
                    original_name: "outer".into(),
                    lookup_key: "outer".into(),
                    namespace: NameNamespace::Value,
                    scope: root_scope,
                    visibility: public_visibility(),
                    modifiers: vec![],
                },
            )
            .unwrap();
        let function_definition = builder
            .add_definition(
                function,
                roles(&analysis, function),
                partial(),
                DefinitionDraft {
                    declaration: function_declaration,
                    symbol_kind: SymbolKind::Function,
                    body_scope: Some(function_scope),
                    type_scope: None,
                },
            )
            .unwrap();
        builder
            .add_binding(
                outer,
                roles(&analysis, outer),
                partial(),
                BindingDraft {
                    target: BindingTargetDraft::Definition(function_definition),
                    form: BindingForm::Declaration,
                    timing: BindingTiming::AtDeclaration,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        let parameter = builder
            .add_declaration(
                xs[0],
                roles(&analysis, xs[0]),
                partial(),
                DeclarationDraft {
                    original_name: "x".into(),
                    lookup_key: "x".into(),
                    namespace: NameNamespace::Value,
                    scope: function_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Scope,
                        boundary: Some(function_scope),
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        let local = builder
            .add_declaration(
                xs[1],
                roles(&analysis, xs[1]),
                partial(),
                DeclarationDraft {
                    original_name: "x".into(),
                    lookup_key: "x".into(),
                    namespace: NameNamespace::Value,
                    scope: function_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Scope,
                        boundary: Some(function_scope),
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        builder
            .add_binding(
                xs[1],
                roles(&analysis, xs[1]),
                partial(),
                BindingDraft {
                    target: BindingTargetDraft::Declaration(local),
                    form: BindingForm::Declaration,
                    timing: BindingTiming::AfterInitializer,
                    mutability: Mutability::Immutable,
                },
            )
            .unwrap();
        builder
            .add_reference(
                xs[2],
                roles(&analysis, xs[2]),
                partial(),
                ReferenceDraft {
                    original_spelling: "x".into(),
                    segments: vec!["x".into()],
                    namespace: NameNamespace::Value,
                    scope: function_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        builder
            .add_import(
                use_node,
                roles(&analysis, use_node),
                partial(),
                ImportDraft {
                    scope: root_scope,
                    module_segments: vec!["crate".into(), "other".into()],
                    form: ImportForm::Selective,
                    alias: None,
                    selected_names: vec!["Thing".into()],
                    conditions: vec!["default-target".into()],
                },
            )
            .unwrap();
        builder
            .add_export(
                outer,
                roles(&analysis, outer),
                partial(),
                ExportDraft {
                    scope: root_scope,
                    local_target: Some(function_declaration),
                    local_name: Some("outer".into()),
                    exported_name: "outer".into(),
                    reexport_segments: vec![],
                    visibility: public_visibility(),
                    conditions: vec![],
                },
            )
            .unwrap();
        builder
            .add_build_module(
                root_node,
                roles(&analysis, root_node),
                partial(),
                BuildModuleDraft {
                    package_id: "scope-test-package".into(),
                    target_id: "lib-default".into(),
                    source_root: "src".into(),
                    module_path: vec!["scope".into()],
                    file_scopes: vec![root_scope],
                },
            )
            .unwrap();
        builder
            .add_dynamic_boundary(
                macro_node,
                roles(&analysis, macro_node),
                partial(),
                DynamicBoundaryDraft {
                    construct_kind: "macro-invocation".into(),
                    scopes: vec![function_scope],
                    namespaces: vec![NameNamespace::Value, NameNamespace::Macro],
                    reason: "macro expansion is not retained".into(),
                },
            )
            .unwrap();
        builder
            .add_shadowing(
                xs[1],
                roles(&analysis, xs[1]),
                partial(),
                ShadowingDraft {
                    shadowing_declaration: local,
                    shadowed_declaration: parameter,
                    namespace: NameNamespace::Value,
                    adapter_rule: "rust-let-shadowing/1".into(),
                },
            )
            .unwrap();
        builder.build().unwrap()
    }

    #[test]
    fn complete_fact_catalog_is_owned_strict_and_deterministic() {
        let analysis = analysis();
        let first = build_full(Arc::clone(&analysis));
        let second = build_full(Arc::clone(&analysis));

        assert!(Arc::ptr_eq(first.analysis(), &analysis));
        assert_eq!(first.schema(), SCOPE_GRAPH_SCHEMA);
        assert_eq!(first.id(), second.id());
        assert_eq!(first.facts().len(), 14);
        assert_eq!(first.document().fact_count(), 14);
        assert_eq!(first.document().analysis_id(), analysis.id().as_str());
        assert_eq!(
            first
                .facts()
                .iter()
                .map(|fact| (fact.key(), fact.data().kind()))
                .collect::<Vec<_>>(),
            second
                .facts()
                .iter()
                .map(|fact| (fact.key(), fact.data().kind()))
                .collect::<Vec<_>>()
        );
        let kinds = first
            .facts()
            .iter()
            .map(|fact| fact.data().kind())
            .collect::<BTreeSet<_>>();
        assert_eq!(kinds, ScopeFactKind::ALL.into_iter().collect());
        assert!(first.facts().iter().all(|fact| {
            analysis.node(fact.node()).is_ok()
                && analysis.node_key(fact.node()).unwrap() == &fact.evidence().node_key
                && fact.evidence().coverage.status == FactCoverage::Partial
        }));

        let json = serde_json::to_value(first.document()).unwrap();
        let decoded: ScopeGraphDocument = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json);
    }

    #[test]
    fn build_context_and_policy_change_every_wire_identity() {
        let analysis = analysis();
        let baseline = build_full(Arc::clone(&analysis));
        let mut changed = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"other-target"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"other-policy/1"]).unwrap(),
        )
        .unwrap();
        let root = analysis.node_ids().next().unwrap();
        changed
            .add_scope(
                root,
                roles(&analysis, root),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: NamespacePolicy::new(vec![NameNamespace::Value], vec![])
                        .unwrap(),
                },
            )
            .unwrap();
        let changed = changed.build().unwrap();
        assert_ne!(baseline.id(), changed.id());
        assert_ne!(baseline.facts()[0].key(), changed.facts()[0].key());
    }

    #[test]
    fn builder_rejects_foreign_wrong_kind_and_forged_evidence() {
        let analysis = analysis();
        let root = analysis.node_ids().next().unwrap();
        let namespaces = NamespacePolicy::new(vec![NameNamespace::Value], vec![]).unwrap();
        let (context, policy) = ids();
        let mut first =
            ScopeGraphBuilder::new(Arc::clone(&analysis), context.clone(), policy.clone()).unwrap();
        let first_scope = first
            .add_scope(
                root,
                roles(&analysis, root),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let mut second = ScopeGraphBuilder::new(Arc::clone(&analysis), context, policy).unwrap();
        assert_eq!(
            second
                .add_scope(
                    root,
                    roles(&analysis, root),
                    partial(),
                    ScopeDraft {
                        kind: ScopeKind::Block,
                        parent: Some(first_scope),
                        namespace_policy: namespaces.clone(),
                    },
                )
                .unwrap_err(),
            ScopeGraphBuildError::ForeignFact
        );

        let scope = second
            .add_scope(
                root,
                roles(&analysis, root),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let declaration = second
            .add_declaration(
                root,
                roles(&analysis, root),
                partial(),
                DeclarationDraft {
                    original_name: "root".into(),
                    lookup_key: "root".into(),
                    namespace: NameNamespace::Value,
                    scope,
                    visibility: public_visibility(),
                    modifiers: vec![],
                },
            )
            .unwrap();
        assert!(matches!(
            second.add_scope(
                root,
                roles(&analysis, root),
                partial(),
                ScopeDraft {
                    kind: ScopeKind::Block,
                    parent: Some(declaration),
                    namespace_policy: namespaces,
                },
            ),
            Err(ScopeGraphBuildError::WrongFactKind { .. })
        ));

        let non_default = analysis
            .canonical_role_projection(Path::new("scope.rs"))
            .unwrap()
            .facts()
            .iter()
            .find(|fact| fact.roles() != CanonicalRoleSet::default())
            .unwrap()
            .node();
        assert!(
            second
                .add_scope(
                    non_default,
                    CanonicalRoleSet::default(),
                    partial(),
                    ScopeDraft {
                        kind: ScopeKind::Block,
                        parent: Some(scope),
                        namespace_policy: NamespacePolicy::new(vec![NameNamespace::Value], vec![],)
                            .unwrap(),
                    },
                )
                .unwrap_err()
                .to_string()
                .contains("canonical roles")
        );
    }

    #[test]
    fn incomplete_adapter_cannot_emit_complete_facts() {
        let analysis = analysis();
        let root = analysis.node_ids().next().unwrap();
        let (context, policy) = ids();
        let mut builder = ScopeGraphBuilder::new(Arc::clone(&analysis), context, policy).unwrap();
        let error = builder
            .add_scope(
                root,
                roles(&analysis, root),
                FactCoverageEvidence::complete(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: NamespacePolicy::new(vec![NameNamespace::Value], vec![])
                        .unwrap(),
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("requires a provided capability"));
    }

    #[test]
    fn coverage_requires_reasons_and_explicit_capability_alignment() {
        assert!(FactCoverageEvidence::partial(" ").is_err());
        assert!(FactCoverageEvidence::unsupported("").is_err());
        assert!(FactCoverageEvidence::failed("provider timeout").is_ok());
        assert!(validate_coverage(FactCoverage::Unsupported, CapabilitySupport::Provided).is_ok());
        assert!(validate_coverage(FactCoverage::Unsupported, CapabilitySupport::Unknown).is_err());
        assert!(validate_coverage(FactCoverage::Partial, CapabilitySupport::Unsupported).is_err());
    }

    #[test]
    fn strict_document_rejects_schema_unknown_fields_and_corrupted_links() {
        let projection = build_full(analysis());
        let json = serde_json::to_value(projection.document()).unwrap();

        let mut unknown = json.clone();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("winner".into(), Value::Bool(true));
        assert!(serde_json::from_value::<ScopeGraphDocument>(unknown).is_err());

        let mut schema = json.clone();
        schema["schema"] = Value::String("deslop.scope-graph/999".into());
        assert!(serde_json::from_value::<ScopeGraphDocument>(schema).is_err());

        let mut source_order = json.clone();
        source_order["facts"][0]["evidence"]["source_order"] = Value::from(99_999_u64);
        assert!(serde_json::from_value::<ScopeGraphDocument>(source_order).is_err());

        let mut unknown_content = json.clone();
        let reference = unknown_content["facts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|fact| fact["data"]["kind"] == "reference")
            .unwrap();
        reference["data"]["content"]["winner"] = Value::Bool(true);
        assert!(serde_json::from_value::<ScopeGraphDocument>(unknown_content).is_err());

        let mut wrong_namespace = json.clone();
        let declaration = wrong_namespace["facts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|fact| fact["data"]["kind"] == "declaration")
            .unwrap();
        declaration["data"]["content"]["namespace"] = Value::String("label".into());
        assert!(serde_json::from_value::<ScopeGraphDocument>(wrong_namespace).is_err());

        let mut cycle = json.clone();
        let scopes = cycle["facts"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, fact)| fact["data"]["kind"] == "scope")
            .map(|(index, fact)| (index, fact["key"].as_str().unwrap().to_string()))
            .collect::<Vec<_>>();
        cycle["facts"][scopes[0].0]["data"]["content"]["parent"] =
            Value::String(scopes[1].1.clone());
        assert!(serde_json::from_value::<ScopeGraphDocument>(cycle).is_err());

        let mut dangling = json;
        let reference = dangling["facts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|fact| fact["data"]["kind"] == "reference")
            .unwrap();
        reference["data"]["content"]["scope"] = Value::String(format!("sf1_{}", "0".repeat(64)));
        assert!(serde_json::from_value::<ScopeGraphDocument>(dangling).is_err());
    }

    impl ScopeFactKind {
        const ALL: [Self; 10] = [
            Self::Scope,
            Self::Declaration,
            Self::Definition,
            Self::Binding,
            Self::Reference,
            Self::Import,
            Self::Export,
            Self::BuildModule,
            Self::DynamicBoundary,
            Self::Shadowing,
        ];
    }
}
